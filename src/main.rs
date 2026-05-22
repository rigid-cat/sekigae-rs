#[cfg(feature = "google-fetch")]
mod fetch;
mod gui;
mod model;
mod solver;

use gui::SekigaeApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1200.0, 900.0]),
        ..Default::default()
    };

    eframe::run_native(
        "sekigae-rs",
        options,
        Box::new(|cc| Ok(Box::new(SekigaeApp::new(cc)))),
    )
}
