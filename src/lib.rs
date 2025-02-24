use libnoise::{Generator, Simplex, Source};

#[derive(Debug, Clone, Copy)]
pub struct ValueRange {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ValueWithNormalized {
    pub value: f64,
    pub normalized: f64,
}

impl ValueWithNormalized {
    pub fn from_normalized(normalized: f64, range: ValueRange) -> Self {
        Self {
            value: range.min + normalized * (range.max - range.min),
            normalized,
        }
    }
}

/// primitive_elevation = primitive_land_base + primitive_shelf
/// (if primitive_elevation > 0.0, primitive_elevation = primitive_elevation.powf(primitive_land_power))
#[derive(Debug, Clone, Copy)]
pub struct PrimitiveElevationFactors {
    /// [-primitive_shelf_depth,0.0] (primitive_shelf_power applied)
    pub shelf: f64,
    /// [0.0,1.0] (normalized)
    pub persistence: ValueWithNormalized,
    /// [0.0,1.0]
    pub land_base: f64,
    /// [-1.0, 1.0] (normalized)
    pub elevation: ValueWithNormalized,
}

#[derive(Debug, Clone, Copy)]
pub struct EnvironmentFactors {
    /// [-PI/2, PI/2] (radian) (calculated by virtual_latitude_fn)
    pub virtual_latitude: f64,
    /// (degree) (calculated by temperature_surface_fn)
    pub temperature_surface: f64,

    /// [-1.0, 1.0]
    pub atmosphere_pressure_normalized: f64,
    pub atmosphere_current_angle: f64,
    pub atmosphere_current_magnitude: f64,

    /// [PrimitiveElevationFactors]
    pub primitive_elevation_factors: PrimitiveElevationFactors,
    /// (radian)
    pub ocean_current_angle: f64,
    /// [0.0, 1.0]
    pub ocean_current_magnitude: f64,
}

pub trait EnvironmentProvider {
    fn get_parameters(&self) -> &ReferenceEnvironmentParameters;
    fn get_factors(&self, x: f64, y: f64) -> Option<EnvironmentFactors>;
}

const NOISE_PRIMITIVE_CONTINENT: usize = 0;
const NOISE_PRIMITIVE_PERSISTENCE: usize = 1;
const NOISE_PRIMITIVE_LAND: usize = 2;
const NOISE_OCEAN_CURRENT: usize = 3;
const NOISE_ATMOSPHERE_PRESSURE: usize = 4;
const NOISE_END: usize = 10;

pub struct ReferenceEnvironmentParameters {
    pub primitive_shelf_scale: f64,
    pub primitive_shelf_power: f64,
    pub primitive_shelf_depth: f64,

    /// Acceptable range of persistence
    pub primitive_persistence_range: ValueRange,
    pub primitive_persistence_scale: f64,

    pub primitive_land_scale: f64,
    pub primitive_land_power: f64,

    /// Real elevation range (m)
    pub primitive_elevation_range: ValueRange,

    pub ocean_current_scale: f64,
    /// Max distance of ocean current effect (particulary for temperature)
    pub ocean_current_elevation_effect_distance: f64,

    /// (x, y) -> virtual_latitude [-PI/2, PI/2]
    pub virtual_latitude_fn: Box<dyn Fn(f64, f64) -> f64>,
    /// (x, y) -> valid or not
    pub valid_fn: Box<dyn Fn(f64, f64) -> bool>,
    /// latitude -> temperature_surface (degree)
    pub temperature_surface_fn: Box<dyn Fn(f64) -> f64>,

    /// Number of samples for gradient calculation (shared)
    pub gradient_sample_num: i32,
    /// Number of iterations for gradient calculation (shared)
    pub gradient_iteration: u32,

    pub atmosphere_pressure_scale: f64,
    pub atmosphere_pressure_noise_prop: f64,
}

impl Default for ReferenceEnvironmentParameters {
    fn default() -> Self {
        Self {
            primitive_shelf_scale: 1.0,
            primitive_shelf_power: 0.5,
            primitive_shelf_depth: 0.3,

            primitive_persistence_range: ValueRange { min: 0.2, max: 0.8 },
            primitive_persistence_scale: 0.3,

            primitive_land_scale: 1.0,
            primitive_land_power: 2.0,

            primitive_elevation_range: ValueRange {
                min: -5000.0,
                max: 5000.0,
            },

            ocean_current_scale: 0.8,
            ocean_current_elevation_effect_distance: 0.3,

            virtual_latitude_fn: Box::new(|_, y| (y * std::f64::consts::PI / 4.0).sin()),
            valid_fn: Box::new(|_, y| y.abs() < 1.0),
            temperature_surface_fn: Box::new(|lat| 30.0 * (1.0 - lat.abs().sin() * 3.0)),

            gradient_sample_num: 16,
            gradient_iteration: 2,

            atmosphere_pressure_scale: 1.0,
            atmosphere_pressure_noise_prop: 0.2,
        }
    }
}

pub struct ReferenceEnvironmentProvider {
    noises: Vec<Simplex<2>>,

    params: ReferenceEnvironmentParameters,
}

impl ReferenceEnvironmentProvider {
    pub fn new(seeds: Option<[u64; NOISE_END]>, params: ReferenceEnvironmentParameters) -> Self {
        let noises = if let Some(seeds) = seeds {
            (0..NOISE_END)
                .map(|i| Source::simplex(seeds[i]))
                .collect::<Vec<_>>()
        } else {
            (0..NOISE_END)
                .map(|i| Source::simplex(i as u64))
                .collect::<Vec<_>>()
        };

        Self { noises, params }
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

    fn get_primitive_elevation_factors(&self, x: f64, y: f64) -> PrimitiveElevationFactors {
        let primitive_shelf = {
            let x = x / self.params.primitive_shelf_scale;
            let y = y / self.params.primitive_shelf_scale;
            let n = self.get_noise(x, y, 3, 0.5, NOISE_PRIMITIVE_CONTINENT);
            let sign = n.signum();
            (n.abs().powf(self.params.primitive_shelf_power) * sign - 1.0)
                * self.params.primitive_shelf_depth
        };

        let primitive_persistence = {
            let x = x / self.params.primitive_persistence_scale;
            let y = y / self.params.primitive_persistence_scale;

            ValueWithNormalized::from_normalized(
                self.get_noise(x, y, 3, 0.5, NOISE_PRIMITIVE_PERSISTENCE) * 0.5 + 0.5,
                self.params.primitive_persistence_range,
            )
        };

        let primitive_land_base = {
            let x = x / self.params.primitive_land_scale;
            let y = y / self.params.primitive_land_scale;
            self.get_noise(x, y, 8, primitive_persistence.value, NOISE_PRIMITIVE_LAND)
                .abs()
        };

        let mut primitive_elevation_normalized = primitive_land_base + primitive_shelf;
        if primitive_elevation_normalized > 0.0 {
            primitive_elevation_normalized =
                primitive_elevation_normalized.powf(self.params.primitive_land_power);
        }

        let primitive_elevation = ValueWithNormalized::from_normalized(
            primitive_elevation_normalized,
            self.params.primitive_elevation_range,
        );

        PrimitiveElevationFactors {
            shelf: primitive_shelf,
            persistence: primitive_persistence,
            land_base: primitive_land_base,
            elevation: primitive_elevation,
        }
    }

    fn get_gradient(
        &self,
        x: f64,
        y: f64,
        d: f64,
        noise_fn: impl Fn(f64, f64) -> f64,
    ) -> (f64, f64) {
        let mut final_angle = 0.0;
        let mut final_value = 0.0;
        let mut range = (0., std::f64::consts::PI * 2.);
        for _ in 0..self.params.gradient_iteration {
            let mut min_value = f64::MAX;
            let mut min_angle = 0.0;
            let stp = (range.1 - range.0) / (self.params.gradient_sample_num - 1) as f64;
            for i in 0..self.params.gradient_sample_num {
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

    fn create_vector_field_noise(
        &self,
        x: f64,
        y: f64,
        noise_fn: impl Fn(f64, f64) -> f64,
        angle_offset: f64,
        dist_grad: f64,
    ) -> (f64, f64) {
        let (gradient, diff) = self.get_gradient(x, y, dist_grad, noise_fn);
        let angle = gradient + angle_offset;
        (angle, diff)
    }
}

impl EnvironmentProvider for ReferenceEnvironmentProvider {
    fn get_parameters(&self) -> &ReferenceEnvironmentParameters {
        &self.params
    }

    fn get_factors(&self, x: f64, y: f64) -> Option<EnvironmentFactors> {
        if !(self.params.valid_fn)(x, y) {
            return None;
        }

        let primitive_elevation_factors = self.get_primitive_elevation_factors(x, y);

        let ocean_current_noise = |x: f64, y: f64| {
            self.get_noise(
                x / self.params.ocean_current_scale,
                y / self.params.ocean_current_scale,
                1,
                0.0,
                NOISE_OCEAN_CURRENT,
            )
        };
        let (ocean_current_angle, ocean_current_diff) = self.create_vector_field_noise(
            x,
            y,
            ocean_current_noise,
            std::f64::consts::PI / 4.0,
            1e-9,
        );

        // this maximum value is not accurate, but enough for this purpose
        let perlin_noise_max_gradient = {
            let pnmg1d = 30.0 / 8.0;
            2.0 * (pnmg1d * pnmg1d * 2.0_f64).sqrt()
        };

        let ocean_current_magnitude = ocean_current_diff
            * (1.0 - primitive_elevation_factors.elevation.normalized.max(0.0))
            / perlin_noise_max_gradient
            * self.params.ocean_current_scale;

        let latitude = (self.params.virtual_latitude_fn)(x, y);

        let temperature_surface = {
            let dx = ocean_current_angle.cos()
                * self.params.ocean_current_elevation_effect_distance
                * ocean_current_magnitude;
            let dy = ocean_current_angle.sin()
                * self.params.ocean_current_elevation_effect_distance
                * ocean_current_magnitude;
            let temperature_latitude = (self.params.virtual_latitude_fn)(x + dx, y + dy);

            (self.params.temperature_surface_fn)(temperature_latitude)
        };

        let atmosphere_pressure_normalized_func = |x: f64, y: f64| {
            let base = -(y * std::f64::consts::PI * 2.0).cos() * 0.5 + 0.5;
            let x = x / self.params.atmosphere_pressure_scale;
            let y = y / self.params.atmosphere_pressure_scale;
            let noise = self.get_noise(x, y, 1, 0.5, NOISE_ATMOSPHERE_PRESSURE);
            base * (1.0 - self.params.atmosphere_pressure_noise_prop)
                + noise * self.params.atmosphere_pressure_noise_prop
        };
        let atmosphere_pressure_normalized = atmosphere_pressure_normalized_func(x, y);

        let (atmosphere_current_angle, atmsphere_current_diff) = self.create_vector_field_noise(
            x,
            y,
            |x, y| atmosphere_pressure_normalized_func(x, y),
            -(((y + 0.5) * std::f64::consts::PI).tan().abs() * (y * std::f64::consts::PI).sin())
                .atan(),
            1e-5,
        );
        let atmosphere_current_magnitude = atmsphere_current_diff / std::f64::consts::PI;

        Some(EnvironmentFactors {
            virtual_latitude: latitude,
            temperature_surface,

            atmosphere_pressure_normalized,
            atmosphere_current_angle,
            atmosphere_current_magnitude,

            primitive_elevation_factors,
            ocean_current_angle,
            ocean_current_magnitude,
        })
    }
}
