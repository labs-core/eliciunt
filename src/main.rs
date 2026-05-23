mod analysis;
mod app;
mod constants;
mod export;
mod math;
mod metrics;
mod models;
mod palette;
mod ui;

use app::App;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ELICIUNT")
            .with_inner_size([1440.0, 880.0])
            .with_min_inner_size([900.0, 600.0]),
        follow_system_theme: false,
        default_theme:       eframe::Theme::Light,
        ..Default::default()
    };
    eframe::run_native(
        "ELICIUNT",
        native_options,
        Box::new(|_cc| Box::new(App::default())),
    )
}