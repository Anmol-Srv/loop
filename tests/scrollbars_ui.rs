#![cfg(feature = "app")]

use acp_server::desktop::design::theme;
use egui::{Event, MouseWheelUnit, TouchPhase, Vec2};
use egui_kittest::Harness;

fn shown(harness: &Harness) -> f32 {
    harness.ctx.global_style().spacing.scroll.active_handle_opacity
}

#[test]
fn the_overlay_bar_shows_while_scrolling_then_fades() {
    let mut harness = Harness::builder()
        .with_size(Vec2::new(300.0, 200.0))
        .build_ui(|ui| theme::follow(ui.ctx()));
    harness.run_steps(2);

    let scroll = harness.ctx.global_style().spacing.scroll;
    assert_eq!(scroll.floating_allocated_width, 0.0, "overlay bars take no gutter");
    assert!(scroll.interact_handle_opacity > 0.0, "hovering the bar's strip shows it");
    assert_eq!(shown(&harness), 0.0, "at rest the bar is hidden");

    harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: Vec2::new(0.0, -30.0),
        phase: TouchPhase::Move,
        modifiers: Default::default(),
    });
    harness.run_steps(3);
    assert!(shown(&harness) > 0.0, "scrolling shows the bar");

    harness.run_steps(90);
    assert_eq!(shown(&harness), 0.0, "it fades once scrolling stops");
}
