//! The agent's face, and the few shapes that say what it is doing.
//!
//! An agent is a rounded square with the robot mark, tinted in its owner's
//! colour — a person is a disc, an agent is a square, and whose agent it is
//! reads from the hue. Presence rides on the face rather than beside it:
//!
//! * working — one slow arc travelling round the edge. The only continuous
//!   motion in the app, and it runs only while an avatar that shows it is on
//!   screen: it asks for a frame at 30 fps, never for "as soon as possible",
//!   and stops asking the moment it scrolls away or the work stops.
//! * waiting for first contact — the edge breathes, for the connect flow.
//! * needs input — an amber dot, the same amber as every "waiting on you".
//! * offline — the tint fades to grey after ten minutes without a word.
//!
//! With `AIRTRIBE_REDUCE_MOTION` (egui's `animation_time` at zero) the arc is
//! a still ring and nothing here asks for a repaint.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::time::Duration;

use chrono::{DateTime, Utc};
use egui::{pos2, vec2, Color32, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};

use super::tokens::{colour, radius, size, space, text};
use super::{avatar, glyph, motion, theme, widgets};

/// The four avatar sizes: a table cell, a rail row, a page header, a card.
pub const XS: f32 = 16.0;
pub const SM: f32 = 20.0;
pub const MD: f32 = 28.0;
pub const LG: f32 = 40.0;

/// One turn of the working arc. Slow on purpose: it says "still going", not
/// "hurry".
const PERIOD: f64 = 1.6;
/// How often a moving ring asks to be drawn again. A slow arc at 30 fps is
/// indistinguishable from 60 at half the cost.
const TICK: Duration = Duration::from_millis(33);
/// Ten minutes without a word and an agent reads as away.
const OFFLINE_AFTER_SECS: i64 = 600;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Presence {
    Idle,
    Working,
    /// Created, not yet heard from: the connect flow's last step.
    Waiting,
    NeedsInput,
    Offline,
}

impl Presence {
    /// From a delegated task's `agentState` and the agent's `lastSeenAt`.
    /// Waiting on you beats everything — the agent is parked either way —
    /// and a silent agent is offline whatever it last claimed to be doing.
    pub fn of(state: &str, last_seen: Option<&str>) -> Self {
        if state == "needs_input" {
            return Presence::NeedsInput;
        }
        let away = last_seen
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .is_none_or(|t| (Utc::now() - t.with_timezone(&Utc)).num_seconds() > OFFLINE_AFTER_SECS);
        if away {
            Presence::Offline
        } else if matches!(state, "working" | "acknowledged") {
            Presence::Working
        } else {
            Presence::Idle
        }
    }

    fn words(self) -> &'static str {
        match self {
            Presence::Idle => "idle",
            Presence::Working => "working",
            Presence::Waiting => "waiting for first contact",
            Presence::NeedsInput => "needs input",
            Presence::Offline => "offline",
        }
    }
}

/// The avatar. `name` is what a screen reader hears, with the presence.
pub fn avatar(ui: &mut Ui, seed: &str, side: f32, presence: Presence, name: &str) -> Response {
    face(ui, seed, side, presence, name, true)
}

/// The same face with a still ring, for the second copy of an agent on a page
/// that already carries the moving one — there is one working ring per thing
/// that is working.
pub fn avatar_still(ui: &mut Ui, seed: &str, side: f32, presence: Presence, name: &str) -> Response {
    face(ui, seed, side, presence, name, false)
}

fn face(ui: &mut Ui, seed: &str, side: f32, presence: Presence, name: &str, animate: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let label = format!("{name}, {}", presence.words());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &label));

    let mut tint = avatar::tint(seed);
    if presence == Presence::Offline {
        tint = tint.lerp_to_gamma(colour::IDLE, 0.7);
    }
    let r = corner(side);
    let p = ui.painter();
    p.rect_filled(rect, r, tint.gamma_multiply(0.16));
    p.rect_stroke(rect, r, Stroke::new(1.0, tint.gamma_multiply(0.45)), StrokeKind::Inside);
    widgets::paint_agent_mark(p, rect.shrink(side * 0.2), tint);

    match presence {
        Presence::Working => ring(ui, rect, r, animate),
        Presence::Waiting => breathe(ui, rect, r, animate),
        Presence::NeedsInput => {
            let rad = (side * 0.15).clamp(3.0, 5.0);
            let c = rect.right_top() + vec2(-rad * 0.3, rad * 0.3);
            p.circle_filled(c, rad + 1.5, colour::CANVAS);
            p.circle_filled(c, rad, colour::WARN);
        }
        Presence::Idle | Presence::Offline => {}
    }
    response.on_hover_text(label)
}

fn corner(side: f32) -> f32 {
    (side * 0.26).min(radius::LG as f32)
}

/// Where the ring's clock is, 0→1 through a turn, or `None` with motion off.
fn clock(ui: &Ui) -> Option<f32> {
    if ui.style().animation_time <= f32::EPSILON {
        return None;
    }
    Some(((ui.input(|i| i.time) % PERIOD) / PERIOD) as f32)
}

/// Ask for the next frame of a moving shape — only while it can be seen.
fn keep_moving(ui: &Ui, rect: Rect) {
    if ui.is_rect_visible(rect) {
        ui.ctx().request_repaint_after(TICK);
    }
}

fn ring_geometry(rect: Rect, r: f32) -> (Rect, f32, f32) {
    let gap = (rect.width() * 0.1).clamp(2.0, 3.0);
    let width = if rect.width() >= MD { 1.6 } else { 1.3 };
    (rect.expand(gap), r + gap, width)
}

/// The working ring: a faint track and one arc travelling round it, easing in
/// and out each turn so it reads as breathing work rather than a loader.
fn ring(ui: &Ui, rect: Rect, r: f32, animate: bool) {
    let (outer, rr, width) = ring_geometry(rect, r);
    let p = ui.painter();
    let Some(t) = clock(ui).filter(|_| animate) else {
        p.rect_stroke(outer, rr, Stroke::new(width, colour::INFO.gamma_multiply(0.7)), StrokeKind::Middle);
        return;
    };
    p.rect_stroke(outer, rr, Stroke::new(width, colour::INFO.gamma_multiply(0.14)), StrokeKind::Middle);
    let pts = outline(outer, rr);
    let n = pts.len() - 1;
    let head = egui::emath::easing::cubic_in_out(t) * n as f32;
    let tail = n as f32 * 0.32;
    for i in 0..n {
        let rel = (i as f32 + 0.5 - (head - tail)).rem_euclid(n as f32);
        if rel < tail {
            let a = rel / tail;
            p.line_segment([pts[i], pts[i + 1]], Stroke::new(width, colour::INFO.gamma_multiply(a)));
        }
    }
    keep_moving(ui, outer);
}

/// Waiting for first contact: the edge brightens and dims.
fn breathe(ui: &Ui, rect: Rect, r: f32, animate: bool) {
    let (outer, rr, width) = ring_geometry(rect, r);
    let alpha = match clock(ui).filter(|_| animate) {
        Some(t) => {
            keep_moving(ui, outer);
            0.2 + 0.6 * (0.5 - 0.5 * (TAU * t).cos())
        }
        None => 0.5,
    };
    ui.painter().rect_stroke(outer, rr, Stroke::new(width, colour::INFO.gamma_multiply(alpha)), StrokeKind::Middle);
}

/// A rounded rectangle's edge as evenly spaced points, clockwise from the top
/// of the right-hand corner, closed. Even spacing is what lets the arc's fade
/// run smoothly along a straight edge as well as round a corner.
fn outline(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -FRAC_PI_2),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), FRAC_PI_2),
        (pos2(rect.left() + r, rect.top() + r), PI),
    ];
    let mut raw = Vec::with_capacity(29);
    for (c, a0) in corners {
        for i in 0..=6 {
            let a = a0 + FRAC_PI_2 * i as f32 / 6.0;
            raw.push(c + vec2(a.cos(), a.sin()) * r);
        }
    }
    raw.push(raw[0]);

    const STEP: f32 = 1.5;
    let mut out = vec![raw[0]];
    let mut carry = 0.0;
    for w in raw.windows(2) {
        let d = w[0].distance(w[1]);
        if d <= f32::EPSILON {
            continue;
        }
        let mut s = STEP - carry;
        while s <= d {
            out.push(w[0] + (w[1] - w[0]) * (s / d));
            s += STEP;
        }
        carry = d - (s - STEP);
    }
    out.push(raw[0]);
    out
}

/// The now line's mark: a short arc turning on a faint circle, in step with
/// the ring on the same page. Still, it is a dot.
pub fn spinner(ui: &mut Ui, side: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let c = rect.center();
    let rad = side * 0.34;
    let p = ui.painter();
    let Some(t) = clock(ui) else {
        p.circle_filled(c, rad * 0.6, colour::INFO);
        return response;
    };
    p.circle_stroke(c, rad, Stroke::new(1.4, colour::INFO.gamma_multiply(0.2)));
    let start = t * TAU - FRAC_PI_2;
    let arc: Vec<Pos2> = (0..=10).map(|i| c + Vec2::angled(start + i as f32 / 10.0 * FRAC_PI_2 * 1.2) * rad).collect();
    p.add(egui::Shape::line(arc, Stroke::new(1.4, colour::INFO)));
    keep_moving(ui, rect);
    response
}

// ------------------------------------------------------------------ stepper

/// One stage of a stepper: its name, and when it was reached, for the hover.
pub struct Step {
    pub label: String,
    pub when: Option<String>,
}

const NODE: f32 = 14.0;
const LINK_W: f32 = 20.0;

/// Stages in a row joined by hairlines: past ones ticked, the current one
/// ringed in `tone` with its name in full ink, the rest hollow and faint.
/// `complete` ticks the current one too, in green — the end was reached.
pub fn stepper(ui: &mut Ui, steps: &[Step], current: usize, complete: bool, tone: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        for (i, step) in steps.iter().enumerate() {
            let done = i < current || (complete && i == current);
            let now = i == current && !complete;
            let (font, ink) = if now {
                (egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::SEMIBOLD.into())), colour::TEXT)
            } else if done {
                (egui::FontId::proportional(text::SMALL), colour::TEXT_2)
            } else {
                (egui::FontId::proportional(text::SMALL), colour::TEXT_FAINT)
            };
            let galley = ui.painter().layout_no_wrap(step.label.clone(), font, ink);
            let (rect, response) = ui.allocate_exact_size(
                vec2(NODE + space::XS + galley.size().x, NODE.max(galley.size().y)),
                Sense::hover(),
            );
            let state = if done { "done" } else if now { "current" } else { "next" };
            let name = format!("{}: {state}", step.label);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));

            let c = pos2(rect.left() + NODE / 2.0, rect.center().y);
            let p = ui.painter();
            if done {
                let (fill, mark) = if complete && i == current {
                    (colour::OK_BG, colour::OK)
                } else {
                    (colour::SURFACE_ACTIVE, colour::TEXT_2)
                };
                p.circle_filled(c, NODE / 2.0, fill);
                glyph::tick(p, c, NODE * 0.62, mark);
            } else if now {
                p.circle_stroke(c, NODE / 2.0 - 0.75, Stroke::new(1.5, tone));
                p.circle_filled(c, NODE * 0.2, tone);
            } else {
                p.circle_stroke(c, NODE / 2.0 - 0.5, Stroke::new(1.0, colour::LINE_STRONG));
            }
            p.galley(pos2(rect.left() + NODE + space::XS, rect.center().y - galley.size().y / 2.0), galley, ink);
            if let Some(when) = &step.when {
                response.on_hover_text(when);
            }

            if i + 1 < steps.len() {
                let (link, _) = ui.allocate_exact_size(vec2(LINK_W, NODE), Sense::hover());
                let ink = if i < current { colour::LINE_STRONG } else { colour::LINE };
                ui.painter().hline(link.x_range(), link.center().y, Stroke::new(1.0, ink));
            }
        }
    });
}

// ------------------------------------------------------------------- pieces

/// A placeholder block where content is on its way: a still shape of roughly
/// the right size, so the page does not jump when it lands.
pub fn skeleton(ui: &mut Ui, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width.min(ui.available_width()), height), Sense::hover());
    ui.painter().rect_filled(rect, radius::SM as f32, colour::SURFACE_HOVER);
}

/// Fourteen days of activity as tiny bars, oldest first. An empty day is a
/// stub, so a quiet fortnight still reads as a fortnight.
pub fn activity(ui: &mut Ui, days: &[f32]) -> Response {
    let (bar, gap, h) = (4.0, 2.0, 16.0);
    let width = days.len() as f32 * (bar + gap) - gap;
    let (rect, response) = ui.allocate_exact_size(vec2(width.max(0.0), h), Sense::hover());
    let total: f32 = days.iter().sum();
    let words = format!("{} updates in the last {} days", total as i64, days.len());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &words));
    let peak = days.iter().copied().fold(1.0_f32, f32::max);
    let p = ui.painter();
    for (i, v) in days.iter().enumerate() {
        let x = rect.left() + i as f32 * (bar + gap);
        let (hh, ink) = if *v > 0.0 { ((v / peak * h).max(3.0), colour::AGENT.gamma_multiply(0.85)) } else { (2.0, colour::LINE_STRONG) };
        p.rect_filled(Rect::from_min_size(pos2(x, rect.bottom() - hh), vec2(bar, hh)), 1.0, ink);
    }
    response.on_hover_text(words)
}

/// One line of a checklist: a painted tick, cross or dash, then what it is.
/// `None` is "not reported" — an older agent kit that says nothing about it.
pub fn check(ui: &mut Ui, label: &str, ok: Option<bool>, detail: &str) -> Response {
    let (mark, ink) = match ok {
        Some(true) => (1, colour::OK),
        Some(false) => (2, colour::DANGER),
        None => (0, colour::TEXT_FAINT),
    };
    let r = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(text::BODY), Sense::hover());
            let p = ui.painter();
            match mark {
                1 => glyph::tick(p, rect.center(), rect.width() * 0.8, ink),
                2 => glyph::cross(p, rect.center(), rect.width(), ink),
                _ => glyph::dash(p, rect.center(), rect.width(), ink),
            }
            ui.label(RichText::new(label).size(text::SMALL).color(if ok.is_some() { colour::TEXT_2 } else { colour::TEXT_MUTED }));
            if !detail.is_empty() {
                ui.label(RichText::new(detail).size(text::SMALL).color(colour::TEXT_MUTED));
            }
        })
        .response;
    let state = match ok {
        Some(true) => "yes",
        Some(false) => "no",
        None => "not reported",
    };
    let name = format!("{label}: {state}");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
    r
}

/// A quiet "Only you can see this", with a painted lock.
pub fn private_label(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XXS;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(text::CAPTION + 1.0), Sense::hover());
        glyph::lock(ui.painter(), rect.center(), rect.width(), colour::TEXT_MUTED);
        ui.label(RichText::new("Only you can see this").size(text::CAPTION).color(colour::TEXT_MUTED));
    });
}

/// A row that opens and closes what is under it: an eased caret, a label, a
/// count. Keyboard-operable; says whether it is open.
pub fn disclosure(ui: &mut Ui, id: egui::Id, label: &str, count: Option<usize>, open: &mut bool) -> Response {
    let font = egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::MEDIUM.into()));
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, colour::TEXT_MUTED);
    let count_g = count.map(|n| {
        ui.painter().layout_no_wrap(n.to_string(), egui::FontId::proportional(text::SMALL), colour::TEXT_FAINT)
    });
    let w = space::LG + galley.size().x + count_g.as_ref().map_or(0.0, |g| space::XS + g.size().x) + space::SM;
    let (rect, response) = ui.allocate_exact_size(vec2(w, size::CONTROL), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, *open, label));
    let response = motion::operable(ui, response, radius::SM as f32);
    if response.clicked() {
        *open = !*open;
    }
    let hot = response.hovered() || response.has_focus();
    let ink = if hot { colour::TEXT } else { colour::TEXT_MUTED };
    let turn = motion::to(ui, id.with("caret"), *open, motion::FAST);
    let p = ui.painter();
    glyph::caret(p, pos2(rect.left() + space::XS + 4.0, rect.center().y), text::SMALL, turn, ink);
    let x = rect.left() + space::LG;
    p.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley.clone(), ink);
    if let Some(g) = count_g {
        p.galley(pos2(x + galley.size().x + space::XS, rect.center().y - g.size().y / 2.0), g, colour::TEXT_FAINT);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_reads_state_and_silence() {
        let now = Utc::now().to_rfc3339();
        let old = (Utc::now() - chrono::Duration::minutes(30)).to_rfc3339();
        assert_eq!(Presence::of("working", Some(&now)), Presence::Working);
        assert_eq!(Presence::of("acknowledged", Some(&now)), Presence::Working);
        assert_eq!(Presence::of("working", Some(&old)), Presence::Offline);
        assert_eq!(Presence::of("needs_input", Some(&old)), Presence::NeedsInput);
        assert_eq!(Presence::of("in_review", Some(&now)), Presence::Idle);
        assert_eq!(Presence::of("working", None), Presence::Offline);
    }

    #[test]
    fn outline_is_even_and_closed() {
        let pts = outline(Rect::from_min_size(Pos2::ZERO, vec2(30.0, 30.0)), 6.0);
        assert_eq!(pts.first(), pts.last());
        for w in pts.windows(2).take(pts.len() - 2) {
            assert!(w[0].distance(w[1]) < 1.6, "{:?}", w);
        }
    }
}
