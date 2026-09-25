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
use egui_kittest::kittest::Queryable;
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

/// The agent globe: every `Presence` at every tuned size, for three seeds (so
/// the two-hue gradient's variety shows), plus the connect flow's 72px
/// waiting orb on its own. Not a snapshot test — open the PNG and look.
#[test]
fn agent_globes() {
    use acp_server::desktop::design::agent::{self as face, Presence};
    use acp_server::desktop::design::colour;

    let seeds = ["anmol@airtribe.live", "hermes-mac", "codex-cli"];
    let sizes = [face::XS, face::SM, face::MD, face::LG, face::XL];
    let presences = [
        ("idle", Presence::Idle),
        ("working", Presence::Working),
        ("waiting", Presence::Waiting),
        ("needs input", Presence::NeedsInput),
        ("offline", Presence::Offline),
    ];

    let installed = std::cell::Cell::new(false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(620.0, 1220.0))
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui(|ui| {
            if !installed.replace(true) {
                theme::install(ui.ctx());
                // Motion is off by default in the harness (every render test
                // before this one is of a still UI); turn it on so the
                // working orbit and the searching sweep actually show motion
                // instead of their reduced-motion still frame.
                ui.ctx().all_styles_mut(|s| s.animation_time = 1.0);
                return;
            }
            egui::Frame::new().fill(colour::CANVAS).inner_margin(16).show(ui, |ui| {
                egui::Grid::new("agent-globes").spacing(egui::vec2(18.0, 14.0)).show(ui, |ui| {
                    for seed in seeds {
                        for (label, presence) in presences {
                            ui.label(
                                egui::RichText::new(format!("{seed} \u{b7} {label}"))
                                    .color(colour::TEXT_MUTED)
                                    .size(10.0),
                            );
                            for side in sizes {
                                face::avatar(ui, seed, side, presence, "Agent");
                            }
                            ui.end_row();
                        }
                    }
                    ui.label(egui::RichText::new("72px waiting").color(colour::TEXT_MUTED).size(10.0));
                    face::avatar(ui, seeds[0], face::XXL, Presence::Waiting, "Agent");
                    ui.end_row();
                });
            });
        });
    // Not `.run()`: the working/waiting/idle globes ask for a repaint every
    // tick by design, which trips `.run()`'s "did this ever settle" check.
    harness.step(); // installs fonts
    harness.step(); // first real draw, at time 0
    // Mid-animation, not frame zero, so the sweep/orbit actually show motion.
    harness.input_mut().time = Some(1.1);
    harness.step();
    save(&mut harness, "agent-globes");
}

// ------------------------------------------------------------------- my tasks

#[test]
fn filter_bar() {
    use acp_server::desktop::design::viz;
    let installed = std::cell::Cell::new(false);
    let opts: Vec<(String, String)> = ["Open", "In progress", "In review", "Done"]
        .iter()
        .map(|s| (s.to_string(), s.to_string()))
        .collect();
    let mut status = Some("In review".to_owned());
    let mut owner: Option<String> = None;
    let mut q = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(760.0, 260.0))
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui(move |ui| {
            if !installed.replace(true) {
                theme::install(ui.ctx());
                return;
            }
            egui::Frame::new()
                .fill(acp_server::desktop::design::colour::CANVAS)
                .inner_margin(16)
                .show(ui, |ui| {
                    viz::toolbar(ui, |ui| {
                        viz::search(ui, "Search tasks", &mut q);
                        viz::select(ui, "Status", &opts, &mut status);
                        viz::select(ui, "Owner", &opts, &mut owner);
                        viz::filter(ui, "Mine", true, false);
                        viz::filter(ui, "Archived", false, false);
                        viz::clear(ui);
                    });
                    ui.add_space(200.0);
                });
        });
    harness.run();
    harness.get_by_label("In review").click();
    harness.run();
    save(&mut harness, "filter-bar");
}
