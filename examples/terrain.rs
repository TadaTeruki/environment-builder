use std::{cell::RefCell, rc::Rc};

use environment_builder::{
    EnvironmentProvider, ReferenceEnvironmentParameters, ReferenceEnvironmentProvider,
};
use gtk4::{cairo::Context, prelude::WidgetExt, DrawingArea};
use vislayers::{
    colormap::SimpleColorMap,
    geometry::FocusRange,
    window::{Layer, Visualizer},
};
use worley_particle::map::{Band, IDWStrategy, InterpolationMethod, ParticleMap};

struct TerrainMap {
    particle_map: ParticleMap<f64>,
    bands: Vec<Band>,
}

impl TerrainMap {
    fn new(file_path: &str, sea_level: f64) -> Self {
        let particle_map =
            ParticleMap::<f64>::read_from_file(file_path).expect("Error reading terrain map");
        let num_thresholds = 80;

        let thresholds = (0..num_thresholds)
            .map(|i| i as f64 * 0.9 / (num_thresholds - 1) as f64 + sea_level)
            .collect::<Vec<_>>();

        let bands = particle_map
            .isobands(
                particle_map.corners(),
                300000.0,
                &thresholds,
                &InterpolationMethod::IDW(IDWStrategy::default_from_params(particle_map.params())),
                true,
            )
            .expect("Error generating bands");

        Self {
            particle_map,
            bands,
        }
    }
}

fn bands_step(focus_range: &FocusRange) -> usize {
    (2.0_f64.powi((focus_range.radius() * 8.0).ceil() as i32) as usize).min(16) - 1
}

impl Layer for TerrainMap {
    fn draw(&self, drawing_area: &DrawingArea, cr: &Context, focus_range: &FocusRange) {
        let color_map = SimpleColorMap::new(
            vec![
                [100.0, 150.0, 70.0],
                [60.0, 90.0, 55.0],
                [210.0, 210.0, 210.0],
            ],
            vec![0.0, 0.35, 0.6],
        );

        let area_width = drawing_area.width();
        let area_height = drawing_area.height();

        let rect = focus_range.to_rect(area_width as f64, area_height as f64);

        let bands_step = bands_step(focus_range);

        for threshold in self.bands.iter().step_by(bands_step) {
            cr.new_path();
            for polygon in &threshold.polygons {
                for (i, point) in polygon.iter().enumerate().step_by(bands_step) {
                    let x = rect.map_coord_x(
                        point.0 - self.particle_map.params().scale / 2.0,
                        0.0,
                        area_width as f64,
                    );
                    let y = rect.map_coord_y(
                        point.1 - self.particle_map.params().scale / 2.0,
                        0.0,
                        area_height as f64,
                    );

                    if i == 0 {
                        cr.move_to(x, y);
                    } else {
                        cr.line_to(x, y);
                    }
                }

                cr.close_path();
            }
            let color = color_map.get_color(threshold.threshold);
            cr.set_source_rgb(color[0] / 255.0, color[1] / 255.0, color[2] / 255.0);
            cr.fill().expect("Failed to fill polygon");
        }

        cr.set_source_rgb(1.0, 0.0, 0.0);
        cr.arc(
            rect.map_coord_x(0.0, 0.0, area_width as f64),
            rect.map_coord_y(0.0, 0.0, area_height as f64),
            2.0,
            0.0,
            2.0 * std::f64::consts::PI,
        );
        cr.fill().expect("Failed to draw center point");
    }
}

struct EnvironmentProviderWrapped<T: EnvironmentProvider>(T, Vec<(String, f64)>);

impl<T: EnvironmentProvider> Layer for EnvironmentProviderWrapped<T> {
    fn draw(&self, drawing_area: &DrawingArea, cr: &Context, focus_range: &FocusRange) {
        let img_width = drawing_area.width();
        let img_height = drawing_area.height();

        let area_width = drawing_area.width();
        let area_height = drawing_area.height();

        let rect = focus_range.to_rect(area_width as f64, area_height as f64);

        let temperature_colormap = SimpleColorMap::new(
            vec![[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            vec![-30.0, 0.0, 30.0],
        );

        let grayscale_colormap =
            SimpleColorMap::new(vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]], vec![0.0, 1.0]);

        let size = 3;

        let draw_dot = |ix: i32, iy: i32, color: [f64; 3], alpha: f64| {
            cr.set_source_rgba(color[0], color[1], color[2], alpha);
            cr.rectangle(
                ix as f64 - size as f64 / 2.0,
                iy as f64 - size as f64 / 2.0,
                size as f64,
                size as f64,
            );
            cr.fill().expect("Failed to fill rectangle");
        };

        let draw_arrow =
            |ix: i32, iy: i32, angle: f64, color: [f64; 3], alpha: f64, length: f64| {
                let multiple = 5;
                if iy % (size * multiple) != 0 || ix % (size * multiple) != 0 {
                    return;
                }

                let x = ix as f64;
                let y = iy as f64;
                let x1 = x + (size * multiple) as f64 * angle.cos() * length;
                let y1 = y + (size * multiple) as f64 * angle.sin() * length;

                cr.set_line_width(2.0);

                cr.set_source_rgba(color[0], color[1], color[2], alpha);
                cr.move_to(x, y);
                cr.line_to(x1, y1);
                cr.stroke().expect("Failed to draw arrow");

                cr.arc(x, y, size as f64 * 0.5, 0.0, 2.0 * std::f64::consts::PI);
                cr.fill().expect("Failed to fill circle");
            };

        let domains = &self.1;

        for (domain, alpha) in domains {
            for iy in (0..img_height).step_by(size as usize) {
                for ix in (0..img_width).step_by(size as usize) {
                    let prop_x = (ix as f64) / img_width as f64;
                    let prop_y = (iy as f64) / img_height as f64;

                    let x = rect.min_x + prop_x * rect.width();
                    let y = rect.min_y + prop_y * rect.height();

                    let environment = self.0.get_factors(x, y);
                    if let Some(environment) = environment {
                        match domain.as_str() {
                            "temperature" => {
                                let temperature = environment.temperature;
                                let color = temperature_colormap.get_color(temperature);
                                draw_dot(ix, iy, color, *alpha);
                            }
                            "primitive_shelf" => {
                                let primitive_shelf = environment.primitive_shelf;
                                let color = grayscale_colormap.get_color(primitive_shelf);
                                draw_dot(ix, iy, color, *alpha);
                            }
                            "primitive_persistence" => {
                                let primitive_persistence = environment.primitive_persistence;
                                let color = grayscale_colormap.get_color(primitive_persistence);
                                draw_dot(ix, iy, color, *alpha);
                            }
                            "primitive_elevation" => {
                                let primitive_elevation = environment.primitive_elevation;
                                let color =
                                    grayscale_colormap.get_color(primitive_elevation.normalized);
                                draw_dot(ix, iy, color, *alpha);
                            }
                            "ocean_current" => {
                                let ocean_current_angle = environment.ocean_current_angle;
                                draw_arrow(
                                    ix,
                                    iy,
                                    ocean_current_angle,
                                    [1.0, 0.0, 0.0],
                                    *alpha,
                                    environment.ocean_current_speed * 0.2,
                                );
                            }
                            _ => break,
                        };
                    }
                }
            }
        }
    }
}

fn main() {
    let mut visualizer = Visualizer::new(800, 600);
    let environment_provider = ReferenceEnvironmentProvider::new(
        None,
        Box::new(|_, y| (y * std::f64::consts::PI / 4.0).sin()),
        Box::new(|_, y| y.abs() < 1.0),
        ReferenceEnvironmentParameters::default(),
    );
    visualizer.add_layer(
        Rc::new(RefCell::new(EnvironmentProviderWrapped(
            environment_provider,
            vec![
                ("primitive_elevation".to_string(), 1.0),
                ("temperature".to_string(), 0.4),
            ],
        ))),
        0,
    );
    visualizer.run();
}
