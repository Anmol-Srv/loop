//! Motion and focus.
//!
//! Two jobs that belong together: both are about making state legible.
//!
//! **Motion budget.** 150ms for feedback, 200ms for a transition between
//! views. Nothing in this app animates for decoration — the audit's product
//! register is explicit that users are in a task and will not wait for
//! choreography. There is no page-load sequence and there will not be one.
//!
//! **Reduced motion.** egui 0.36 does not surface the OS "reduce motion"
//! setting, so there is nothing to read directly. What it does expose is its
//! own `Style::animation_time`; setting it to zero disables every animation in
//! the framework. Every helper here respects that knob, so turning motion off
//! is one line in `theme.rs` and it genuinely turns everything off rather than
//! leaving hand-rolled animations running. Wire it to the OS setting the day
//! eframe surfaces one.

use egui::{Color32, Id, Response, Ui};

use super::tokens::{colour, radius};

/// Feedback: a hover tint, a press. Short enough to feel immediate.
pub const FAST: f32 = 0.12;
/// A transition between states: a section opening, a filter applying.
pub const BASE: f32 = 0.18;

/// 0→1 eased toward `on`, or an instant flip when the user asked for less
/// motion. egui's `animate_bool_with_time` already eases out; the value is
/// meant to drive a colour lerp or an offset, never a layout property.
pub fn to(ui: &Ui, id: Id, on: bool, secs: f32) -> f32 {
    if ui.style().animation_time <= f32::EPSILON {
        return if on { 1.0 } else { 0.0 };
    }
    ui.ctx().animate_bool_with_time(id, on, secs)
}

/// A hover tint that eases rather than snapping. Returns the colour to paint.
pub fn hover_fill(ui: &Ui, id: Id, hovered: bool, from: Color32, to_c: Color32) -> Color32 {
    let t = to(ui, id, hovered, FAST);
    from.lerp_to_gamma(to_c, t)
}

/// The focus ring.
///
/// Drawn for keyboard focus only — a mouse user who has just clicked does not
/// need a ring, and drawing one on every click is the reason people disable
/// focus styles and break keyboard access for everyone. egui tracks this for
/// us: `Response::has_focus` is true only while the widget holds focus, and we
/// suppress it when the pointer is what put it there.
pub fn focus_ring(ui: &Ui, response: &Response, r: f32) {
    if !response.has_focus() {
        return;
    }
    let rect = response.rect.expand(2.0);
    ui.painter().rect_stroke(
        rect,
        r + 2.0,
        egui::Stroke::new(2.0, colour::ACCENT),
        egui::StrokeKind::Outside,
    );
}

/// Make a response keyboard-operable.
///
/// The audit found that not one control in this app was focusable: everything
/// used `Sense::click()`, so Tab reached nothing and there was no ring to see.
/// This adds the focus sense, draws the ring, and reports a keyboard
/// activation (Space or Enter) as a click — which is what a user pressing
/// Space on a focused button expects, and what egui does not do for a
/// hand-painted widget.
pub fn operable(ui: &mut Ui, mut response: Response, r: f32) -> Response {
    // Hand-painted widgets allocate with Sense::click(); adding focusable here
    // keeps every call site from having to remember.
    ui.memory_mut(|m| m.interested_in_focus(response.id, ui.layer_id()));

    if response.has_focus() {
        let activated = ui.input(|i| {
            i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)
        });
        if activated {
            // The synthetic-click flag `clicked()` already honours, rather
            // than CLICKED, which is reserved for a real pointer press.
            response.flags |= egui::response::Flags::FAKE_PRIMARY_CLICKED;
        }
    }
    if response.clicked() {
        response.request_focus();
    }
    focus_ring(ui, &response, r);
    response
}

/// Convenience for the common case: a control with the small radius.
pub fn operable_sm(ui: &mut Ui, response: Response) -> Response {
    operable(ui, response, radius::SM as f32)
}
