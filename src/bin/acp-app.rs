//! Airtribe Control Plane — native macOS client.

use acp_server::desktop::App;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Content runs under the title bar and out to every edge. The
            // traffic lights still float top-left, so the sidebar leaves room
            // for them rather than the window reserving a whole strip.
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([820.0, 520.0])
            // Explicit, though it is the default: a maximized window on macOS
            // resists being dragged smaller, and starting zoomed made the app
            // feel fixed-size. Open at a sensible size and let people set it.
            .with_resizable(true)
            .with_title("Loop")
            // eframe replaces the Dock icon with its own at launch unless the
            // window names one, so the bundle's icon.icns alone never shows.
            // Same tile `scripts/make-icon.py` writes into the bundle.
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png"))
                    .expect("assets/icon.png is a valid PNG"),
            ),
        ..Default::default()
    };

    eframe::run_native(
        "Loop",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
