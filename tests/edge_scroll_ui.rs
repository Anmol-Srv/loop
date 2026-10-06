#![cfg(feature = "app")]

use std::cell::Cell;
use std::rc::Rc;

use acp_server::desktop::design::shell;
use egui::text_selection::LabelSelectionState;
use egui::{Event, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2};
use egui_kittest::Harness;

#[test]
fn drag_selecting_past_the_bottom_keeps_scrolling_and_keeps_the_selection() {
    let offset = Rc::new(Cell::new(0.0));
    let seen = offset.clone();
    let mut harness = Harness::builder().with_size(Vec2::new(300.0, 240.0)).build_ui(move |ui| {
        let out = egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
            for i in 0..200 {
                shell::selectable(ui, egui::Label::new(format!("line {i}")));
            }
            shell::edge_scroll(ui);
        });
        seen.set(out.state.offset.y);
    });
    harness.run_steps(2);

    let start = Pos2::new(20.0, 10.0);
    harness.event(Event::PointerMoved(start));
    harness.event(Event::PointerButton {
        pos: start,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    harness.event(Event::PointerMoved(Pos2::new(40.0, 60.0)));
    harness.run_steps(2);

    harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: Vec2::new(0.0, -40.0),
        phase: TouchPhase::Move,
        modifiers: Default::default(),
    });
    harness.run_steps(4);
    let wheeled = offset.get();
    assert!(wheeled > 0.0, "the trackpad scrolls during a drag");

    harness.event(Event::PointerMoved(Pos2::new(40.0, 300.0)));
    harness.run_steps(60);
    assert!(offset.get() > wheeled + 200.0, "held past the edge, it keeps scrolling");

    let selected = harness
        .ctx
        .plugin_opt::<LabelSelectionState>()
        .is_some_and(|p| p.lock().has_selection());
    assert!(selected, "the selection outlives its first line scrolling away");
}
