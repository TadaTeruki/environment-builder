use libnoise::{Generator, Simplex, Source};

pub struct ValueWithNormalized {
    pub value: f64,
    pub normalized: f64,
}

pub struct EnvironmentFactors {
    pub virtual_latitude: f64,
    pub temperature: f64,

    pub primitive_shelf: f64,
    pub primitive_persistence: f64,
    pub primitive_elevation: ValueWithNormalized,

    pub ocean_current_angle: f64,
    pub ocean_current_speed: f64,
}

pub trait EnvironmentProvider {
    fn get_factors(&self, x: f64, y: f64) -> Option<EnvironmentFactors>;
}

const NOISE_PRIMITIVE_CONTINENT: usize = 0;
const NOISE_PRIMITIVE_PERSISTENCE: usize = 1;
const NOISE_PRIMITIVE_LAND: usize = 2;
const NOISE_OCEAN_CURRENT: usize = 3;
const NOISE_END: usize = 10;

pub struct ReferenceEnvironmentParameters {
    pub primitive_shelf_scale: f64,
    pub primitive_shelf_power: f64,
    pub primitive_persistence_scale: f64,
    pub primitive_elevation_scale: f64,

    pub ocean_current_scale: f64,
    pub ocean_current_elevation_effect_distance: f64,
}

impl Default for ReferenceEnvironmentParameters {
    fn default() -> Self {
        Self {
            primitive_shelf_scale: 1.0,
            primitive_shelf_power: 0.75,
            primitive_persistence_scale: 0.07,
            primitive_elevation_scale: 0.5,

            ocean_current_scale: 0.8,
            ocean_current_elevation_effect_distance: 0.02,
        }
    }
}

pub struct ReferenceEnvironmentProvider {
    noises: Vec<Simplex<2>>,
    virtual_latitude_fn: Box<dyn Fn(f64, f64) -> f64>,
    valid_fn: Box<dyn Fn(f64, f64) -> bool>,
    params: ReferenceEnvironmentParameters,
}

impl ReferenceEnvironmentProvider {
    pub fn new(
        seeds: Option<[u64; NOISE_END]>,
        virtual_latitude_fn: Box<dyn Fn(f64, f64) -> f64>,
        valid_fn: Box<dyn Fn(f64, f64) -> bool>,
        params: ReferenceEnvironmentParameters,
    ) -> Self {
        let noises = if let Some(seeds) = seeds {
            (0..NOISE_END)
                .map(|i| Source::simplex(seeds[i]))
                .collect::<Vec<_>>()
        } else {
            (0..NOISE_END)
                .map(|i| Source::simplex(i as u64))
                .collect::<Vec<_>>()
        };

        Self {
            noises,
            virtual_latitude_fn,
            valid_fn,
            params,
        }
    }

    fn get_noise(&self, x: f64, y: f64, octaves: u32, persistence: f64, idx: usize) -> f64 {
        if idx >= self.noises.len() {
            return 0.0;
        }
        let mut value = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        let mut max_value = 0.0;
        for _ in 0..octaves {
            value += self.noises[idx].sample([x * frequency, y * frequency]) * amplitude;
            max_value += amplitude;
            amplitude *= persistence;
            frequency *= 2.0;
        }
        value / max_value
    }

    fn get_gradient(
        &self,
        sample_num: i32,
        iteration: u32,
        x: f64,
        y: f64,
        d: f64,
        noise_fn: impl Fn(f64, f64) -> f64,
    ) -> (f64, f64) {
        let mut final_angle = 0.0;
        let mut final_value = 0.0;
        let mut range = (0., std::f64::consts::PI * 2.);
        for _ in 0..iteration {
            let mut min_value = f64::MAX;
            let mut min_angle = 0.0;
            let stp = (range.1 - range.0) / (sample_num - 1) as f64;
            for i in 0..sample_num {
                let angle = range.0 + stp * (i as f64);
                let dx = angle.cos() * d;
                let dy = angle.sin() * d;
                let value = noise_fn(x + dx, y + dy);
                if value < min_value {
                    min_value = value;
                    min_angle = angle;
                }
            }
            range = (min_angle - stp * 0.5, min_angle + stp * 0.5);
            final_angle = min_angle;
            final_value = min_value;
        }

        let value = noise_fn(x, y);
        let diff = (final_value - value) / d;

        (final_angle, diff)
    }
}

impl EnvironmentProvider for ReferenceEnvironmentProvider {
    fn get_factors(&self, x: f64, y: f64) -> Option<EnvironmentFactors> {
        if !(self.valid_fn)(x, y) {
            return None;
        }
        let primitive_shelf_fn = |x: f64, y: f64| {
            let x = x / self.params.primitive_shelf_scale;
            let y = y / self.params.primitive_shelf_scale;
            self.get_noise(x, y, 3, 0.5, NOISE_PRIMITIVE_CONTINENT)
                .max(0.0)
                .powf(self.params.primitive_shelf_power)
        };

        let primitive_shelf = primitive_shelf_fn(x, y);

        let primitive_persistence = {
            let x = x / self.params.primitive_persistence_scale;
            let y = y / self.params.primitive_persistence_scale;
            (self.get_noise(x, y, 3, 0.5, NOISE_PRIMITIVE_PERSISTENCE) * 0.5 + 0.5) * 0.7 + 0.2
        };

        let primitive_land = {
            let x = x / self.params.primitive_elevation_scale;
            let y = y / self.params.primitive_elevation_scale;
            let p = self
                .get_noise(x, y, 8, primitive_persistence, NOISE_PRIMITIVE_LAND)
                .abs();
            primitive_shelf * p
        };

        let primitive_elevation_normalized = primitive_land + primitive_shelf - 1.0;

        let primitive_elevation = ValueWithNormalized {
            value: primitive_elevation_normalized * 5000.0,
            normalized: primitive_elevation_normalized,
        };

        let (ocean_current_angle, ocean_current_speed) = {
            let x = x / self.params.ocean_current_scale;
            let y = y / self.params.ocean_current_scale;
            let ocean_current_noise =
                |x: f64, y: f64| self.get_noise(x, y, 1, 0.5, NOISE_OCEAN_CURRENT);
            let dist_grad = self.params.ocean_current_scale * 1e-5;
            let (gradient, diff) = self.get_gradient(16, 2, x, y, dist_grad, ocean_current_noise);
            let angle = gradient + std::f64::consts::PI / 4.0;
            let speed = diff;

            let primitive_shelf_forward = {
                let dx = angle.cos() * self.params.ocean_current_elevation_effect_distance * speed;
                let dy = angle.sin() * self.params.ocean_current_elevation_effect_distance * speed;
                primitive_shelf_fn(x + dx, y + dy)
            };

            let speed = speed * (1.0 - primitive_shelf_forward);

            let grad_elevation = (primitive_shelf_forward - primitive_shelf)
                / (self.params.ocean_current_elevation_effect_distance * speed);
            let angle = if grad_elevation < 0.0 {
                angle
            } else {
                angle + std::f64::consts::PI / 4.0 * grad_elevation
            };

            (angle, speed)
        };

        let latitude = (self.virtual_latitude_fn)(x, y);

        let temperature = {
            let dx = ocean_current_angle.cos()
                * self.params.ocean_current_elevation_effect_distance
                * ocean_current_speed;
            let dy = ocean_current_angle.sin()
                * self.params.ocean_current_elevation_effect_distance
                * ocean_current_speed;
            let temperature_latitude = (self.virtual_latitude_fn)(x + dx, y + dy);

            let temperature_surface = 30.0 * (1.0 - temperature_latitude.abs().sin() * 3.0);
            let temperature_with_foehn = temperature_surface
                * (1.0 - primitive_elevation_normalized.max(0.0))
                - (primitive_elevation.value.max(0.0) * 0.01) * 0.6;
            temperature_with_foehn
        };

        Some(EnvironmentFactors {
            virtual_latitude: latitude,
            temperature,
            primitive_shelf,
            primitive_persistence,
            primitive_elevation,
            ocean_current_angle,
            ocean_current_speed,
        })
    }
}
