//! Offscreen renders of the app's tables, for looking at.
//!
//! Not a snapshot test: nothing is compared, nothing fails on a pixel. The
//! app itself cannot be screenshotted by the tooling that builds it, and
//! "the alignment is off" is not a claim anyone can check by reading Rust.
//! `cargo test --features app --test ui_render` writes PNGs under
//! `docs/design-mocks/render/`; open them.
#![cfg(feature = "app")]

use std::collections::HashMap;

use acp_server::desktop::design::theme;
use acp_server::desktop::views::projects;
use egui_kittest::Harness;
use serde_json::{json, Value};

fn save(harness: &mut Harness<'_>, name: &str) {
    let image = harness.render().expect("render");
    let path = format!("docs/design-mocks/render/{name}.png");
    image.save(&path).expect("write png");
    eprintln!("wrote {path}");
}

#[test]
fn projects_table() {
    let rows: Vec<Value> = vec![
        json!({"id":"a","name":"Lead Rating","description":"Backend-engineering leads on the weighted-rubric path now get their role bucket from Jev.","status":"active","createdAt":"2026-09-22T07:34:21Z"}),
        json!({"id":"b","name":"Checkout redesign","description":"","status":"paused","createdAt":"2026-09-01T07:34:21Z"}),
        json!({"id":"c","name":"A project with a very long name that has to truncate before the next column","description":"Short.","status":"done","createdAt":"2026-06-22T07:34:21Z"}),
    ];
    let mut flows: HashMap<String, Value> = HashMap::new();
    flows.insert("a".into(), json!({"done": 3, "total": 8}));
    flows.insert("b".into(), json!({"done": 0, "total": 0}));
    flows.insert("c".into(), json!({"done": 12, "total": 12}));

    // The harness runs a frame while it is built and fonts only bind on the
    // frame after `set_fonts`, so the first frame installs and draws nothing.
    let installed = std::cell::Cell::new(false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1040.0, 220.0))
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui(|ui| {
            if !installed.replace(true) {
                theme::install(ui.ctx());
                return;
            }
            egui::Frame::new()
                .fill(acp_server::desktop::design::colour::CANVAS)
                .inner_margin(16)
                .show(ui, |ui| {
                    let mut open = None;
                    projects::table(ui, &rows, &flows, true, &mut open);
                });
        });
    harness.run();
    // Pretend the pointer sat on the last row last frame, so the render shows
    // the hover state — the one that is easy to get subtly wrong.
    harness.ctx.data_mut(|d| {
        d.insert_temp(egui::Id::new("projects:table").with("hover"), Some(2usize))
    });
    harness.step();
    save(&mut harness, "projects-table");
}
