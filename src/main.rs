mod gui;
mod model;
mod solver;
mod fetch;

use gui::SekigaeApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1200.0, 900.0]),
        ..Default::default()
    };

    eframe::run_native(
        "sejigae-rs",
        options,
        Box::new(|cc| Ok(Box::new(SekigaeApp::new(cc)))),
    )
}
