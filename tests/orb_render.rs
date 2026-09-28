//! Every agent presence at every avatar size, drawn at a few moments so the
//! motion can be judged frame by frame. Writes PNGs, so it is ignored:
//!
//!   cargo test --features app --test orb_render -- --ignored
//!
//! Output: docs/design-mocks/render/orbs/orbs-<palette>-<frame>.png
#![cfg(feature = "app")]

use acp_server::desktop::design::agent::{self as face, Presence};
use acp_server::desktop::design::{colour, theme};
use egui_kittest::Harness;

const STATES: [(Presence, &str); 6] = [
    (Presence::Idle, "Idle"),
    (Presence::Planning, "Planning"),
    (Presence::Working, "Working"),
    (Presence::NeedsInput, "Needs input"),
    (Presence::Waiting, "Connecting"),
    (Presence::Offline, "Offline"),
];
const SIZES: [f32; 5] = [face::XS, face::SM, face::MD, face::XL, face::XXL];
/// Seconds between frames: enough to see each state move.
const STEP: f32 = 0.45;
const FRAMES: usize = 4;

#[test]
#[ignore = "writes PNGs: cargo test --features app --test orb_render -- --ignored"]
fn orbs() {
    for appearance in ["dark", "light"] {
        std::env::set_var("ACP_APPEARANCE", appearance);
        let mut installed = false;
        let mut harness = Harness::builder()
            .with_size(egui::vec2(560.0, 620.0))
            .with_pixels_per_point(2.0)
            .with_step_dt(STEP)
            .build_ui(|ui| {
                if !installed {
                    theme::install(ui.ctx());
                    // kittest zeroes this for stable snapshots, which is the
                    // reduced-motion path: every orb would draw its still frame.
                    ui.ctx().all_styles_mut(|s| s.animation_time = egui::Style::default().animation_time);
                    installed = true;
                }
                egui::Frame::new().fill(colour::CANVAS()).inner_margin(16).show(ui, |ui| {
                    for (presence, name) in STATES {
                        ui.horizontal(|ui| {
                            ui.set_min_height(face::XXL + 8.0);
                            ui.add_sized([96.0, face::XXL], egui::Label::new(egui::RichText::new(name).color(colour::TEXT_2())));
                            for side in SIZES {
                                face::avatar(ui, "anmol.srivastava@airtribe.live", side, presence, name);
                                ui.add_space(12.0);
                            }
                        });
                    }
                });
            });
        std::fs::create_dir_all("docs/design-mocks/render/orbs").unwrap();
        for f in 0..FRAMES {
            harness.step();
            let path = format!("docs/design-mocks/render/orbs/orbs-{appearance}-{f}.png");
            harness.render().expect("render").save(&path).expect("write png");
        }
    }
}
