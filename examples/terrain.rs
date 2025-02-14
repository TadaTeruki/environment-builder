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

struct EnvironmentProviderWrapped<T: EnvironmentProvider> {
    provider: T,
    layers: Vec<(String, f64)>,
}

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

        for (layer, alpha) in &self.layers {
            for iy in (0..img_height).step_by(size as usize) {
                for ix in (0..img_width).step_by(size as usize) {
                    let prop_x = (ix as f64) / img_width as f64;
                    let prop_y = (iy as f64) / img_height as f64;

                    let x = rect.min_x + prop_x * rect.width();
                    let y = rect.min_y + prop_y * rect.height();

                    let environment = self.provider.get_factors(x, y);
                    if let Some(environment) = environment {
                        match layer.as_str() {
                            "temperature_surface" => {
                                let temperature = environment.temperature_surface;
                                let color = temperature_colormap.get_color(temperature);
                                draw_dot(ix, iy, color, *alpha);
                            }
                            "primitive_shelf" => {
                                let primitive_shelf = environment.primitive_elevation_factors.shelf;
                                let color = grayscale_colormap.get_color(primitive_shelf + 1.0);
                                draw_dot(ix, iy, color, *alpha);
                            }
                            "primitive_persistence" => {
                                let primitive_persistence =
                                    environment.primitive_elevation_factors.persistence;
                                let color =
                                    grayscale_colormap.get_color(primitive_persistence.normalized);
                                draw_dot(ix, iy, color, *alpha);
                            }
                            "primitive_elevation" => {
                                let primitive_elevation =
                                    environment.primitive_elevation_factors.elevation;
                                let color =
                                    grayscale_colormap.get_color(primitive_elevation.normalized);
                                draw_dot(ix, iy, color, *alpha);
                            }
                            "atmosphere_pressure_normalized" => {
                                let atmosphere_pressure_normalized =
                                    environment.atmosphere_pressure_normalized;
                                let color =
                                    grayscale_colormap.get_color(atmosphere_pressure_normalized);
                                draw_dot(ix, iy, color, *alpha);
                            }
                            "atmosphere_pressure_current" => {
                                draw_arrow(
                                    ix,
                                    iy,
                                    environment.atmosphere_current_angle,
                                    [1.0, 0.0, 0.0],
                                    *alpha,
                                    environment.atmosphere_current_magnitude,
                                );
                            }
                            "ocean_current" => {
                                draw_arrow(
                                    ix,
                                    iy,
                                    environment.ocean_current_angle,
                                    [1.0, 0.0, 0.0],
                                    *alpha,
                                    environment.ocean_current_magnitude,
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
    let parameters = ReferenceEnvironmentParameters::default();
    let environment_provider = ReferenceEnvironmentProvider::new(None, parameters);
    visualizer.add_layer(
        Rc::new(RefCell::new(EnvironmentProviderWrapped {
            provider: environment_provider,
            layers: vec![
                ("primitive_elevation".to_string(), 1.0),
                // ("ocean_current".to_string(), 0.5),
                // ("temperature_surface".to_string(), 0.5),
                //("atmosphere_pressure_normalized".to_string(), 0.5),
                ("atmosphere_pressure_current".to_string(), 0.5),
            ],
        })),
        0,
    );
    visualizer.run();
}
