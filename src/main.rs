mod fetch;
mod gui;
mod model;
mod solver;

use gui::SekigaeApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();

    eframe::run_native(
        "席替えアプリ",
        options,
        Box::new(|cc| Ok(Box::new(SekigaeApp::new(cc)))),
    )
}