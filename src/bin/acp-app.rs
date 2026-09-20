//! Airtribe Control Plane — native macOS client.

use acp_server::desktop::App;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([760.0, 480.0])
            .with_title("Airtribe Control Plane"),
        ..Default::default()
    };

    eframe::run_native(
        "Airtribe Control Plane",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
