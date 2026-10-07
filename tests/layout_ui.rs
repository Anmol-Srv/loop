#![cfg(feature = "app")]

use std::cell::RefCell;

use acp_server::desktop::design::{theme, viz};
use egui_kittest::Harness;

#[test]
fn cards_in_a_row_come_out_the_same_height() {
    let rects = RefCell::new(Vec::new());
    let rects = &rects;
    let installed = std::cell::Cell::new(false);
    let mut harness = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_ui(move |ui| {
        if !installed.replace(true) {
            theme::install(ui.ctx());
            return;
        }
        rects.borrow_mut().clear();
        let card = |body: f32| {
            move |ui: &mut egui::Ui, floor: f32| {
                let used = viz::card(ui, "Figure", "", floor, |ui| {
                    ui.add_space(body);
                });
                rects.borrow_mut().push(ui.min_rect());
                used
            }
        };
        let (mut short, mut tall) = (card(40.0), card(160.0));
        viz::row(ui, egui::Id::new("cards"), &mut [&mut short, &mut tall]);
    });
    harness.run_steps(6);
    let rects = rects.borrow();
    assert_eq!(rects.len(), 2);
    assert!((rects[0].height() - rects[1].height()).abs() < 0.5, "{} vs {}", rects[0].height(), rects[1].height());
}
