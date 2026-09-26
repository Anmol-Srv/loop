//! The agent's face, and the few shapes that say what it is doing.
//!
//! An agent is a dotted globe, tinted in its owner's colour — a person is a
//! solid disc with initials, an agent is a sphere of dots with none, and whose
//! agent it is reads from the gradient (`avatar::tint_pair`, the owner's hue
//! plus a second one rotated warmer). Presence is the globe's own animation
//! rather than a ring drawn beside it:
//!
//! * working — the sphere turns and two particles orbit it. The only
//!   continuous motion in most lists, and it runs only while an avatar that
//!   shows it is on screen: it asks for a frame at 30 fps, never for "as soon
//!   as possible", and stops asking the moment it scrolls away or the work
//!   stops.
//! * waiting for first contact — a meridian sweeps the sphere as it turns, for
//!   the connect flow.
//! * needs input — the sphere holds still; an amber dot, the same amber as
//!   every "waiting on you", sits at the corner.
//! * idle — a very slow turn at MD and above, still below that. Unlike
//!   working and waiting it never asks for its own frame — the page sleeps
//!   unless the agent is doing something — so it only advances on whatever
//!   repaint something else on the page causes anyway.
//! * offline — still, and the gradient fades toward grey after ten minutes
//!   without a word.
//!
//! With `AIRTRIBE_REDUCE_MOTION` (egui's `animation_time` at zero), every
//! state is its still frame and nothing here asks for a repaint.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use chrono::{DateTime, Utc};
use egui::{pos2, vec2, Color32, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

use super::tokens::{colour, radius, size, space, text};
use super::{avatar, glyph, motion, orb, theme};

/// The avatar sizes: a table cell, a rail row, a page header, a card, and
/// the agent's own page.
pub const XS: f32 = 16.0;
pub const SM: f32 = 20.0;
pub const MD: f32 = 28.0;
pub const LG: f32 = 40.0;
/// An agent's own page: the face that heads it.
pub const XL: f32 = 56.0;
/// The connect flow's last step: the one place the globe is the whole screen
/// rather than a mark beside other content.
pub const XXL: f32 = 72.0;

/// One turn of the now-line spinner. Slow on purpose: it says "still going",
/// not "hurry".
const PERIOD: f64 = 1.6;
/// How often a moving shape asks to be drawn again. 30 fps is
/// indistinguishable from 60 here, at half the cost.
const TICK: Duration = Duration::from_millis(33);
/// One full turn of an idle or working globe. Slow enough to read as ambient
/// rather than a loader.
const SPIN_PERIOD: f64 = 14.0;
/// One lap of the searching meridian — quick enough to read as a scan.
const SWEEP_PERIOD: f64 = 3.2;
/// One lap of the working orbit, deliberately faster than the sphere's own
/// spin so the two motions read as separate layers.
const ORBIT_PERIOD: f64 = 2.0;
/// The globe's fixed viewing angle: enough tilt that it reads as a sphere,
/// not a flat disc face-on.
const TILT: f32 = 0.42;
/// Ten minutes without a word and an agent reads as away.
const OFFLINE_AFTER_SECS: i64 = 600;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Presence {
    Idle,
    Working,
    /// Created, not yet heard from: the connect flow's last step.
    Waiting,
    /// Waiting on you: a question, or a plan for your review.
    NeedsInput,
    Offline,
}

impl Presence {
    /// From a delegated task's `agentState` and the agent's `lastSeenAt`.
    /// Waiting on you beats everything — the agent is parked either way —
    /// and a silent agent is offline whatever it last claimed to be doing.
    pub fn of(state: &str, last_seen: Option<&str>) -> Self {
        if matches!(state, "needs_input" | "plan_review") {
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

/// The same face, still, for the second copy of an agent on a page that
/// already carries the moving one — there is one turning globe per thing that
/// is working, everywhere else shows its still frame.
pub fn avatar_still(ui: &mut Ui, seed: &str, side: f32, presence: Presence, name: &str) -> Response {
    face(ui, seed, side, presence, name, false)
}

fn face(ui: &mut Ui, seed: &str, side: f32, presence: Presence, name: &str, animate: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let label = format!("{name}, {}", presence.words());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &label));

    let (mut c1, mut c2) = avatar::tint_pair(seed);
    if presence == Presence::Offline {
        c1 = c1.lerp_to_gamma(colour::IDLE, 0.7);
        c2 = c2.lerp_to_gamma(colour::IDLE, 0.7);
    }

    let center = rect.center();
    let globe_r = side * 0.5 * orb::INSET;
    let dynamic = animate && ui.style().animation_time > f32::EPSILON;
    let p = ui.painter();
    match presence {
        Presence::Working if dynamic => {
            orb::paint(p, center, globe_r, side, (c1, c2), spin_phase(ui, SPIN_PERIOD), TILT, None);
            orb::orbit(p, center, globe_r, side, c2, spin_phase(ui, ORBIT_PERIOD) / TAU);
            keep_moving(ui, rect);
        }
        Presence::Waiting if dynamic => {
            let spin = spin_phase(ui, SWEEP_PERIOD);
            orb::paint(p, center, globe_r, side, (c1, c2), spin, TILT, Some(0.0));
            keep_moving(ui, rect);
        }
        // Idle never asks for its own frame — "the page sleeps unless the
        // agent is working" is load-bearing elsewhere. At MD and up it still
        // reads the clock, so it drifts a little across whatever repaints the
        // rest of the page causes anyway; below that it would only ever be
        // caught mid-turn by accident, so it takes the fixed still frame.
        Presence::Idle if dynamic && side >= MD => {
            orb::paint(p, center, globe_r, side, (c1, c2), spin_phase(ui, SPIN_PERIOD), TILT, None);
        }
        _ => {
            let (spin, tilt) = still_phase(seed);
            orb::paint(p, center, globe_r, side, (c1, c2), spin, tilt, None);
        }
    }
    orb::hairline(p, center, globe_r, c1);

    if presence == Presence::NeedsInput {
        let rad = (side * 0.15).clamp(3.0, 5.0);
        let c = rect.right_top() + vec2(-rad * 0.3, rad * 0.3);
        p.circle_filled(c, rad + 1.5, colour::CANVAS);
        p.circle_filled(c, rad, colour::WARN);
    }
    response.on_hover_text(label)
}

/// A continuous turn, in radians, bounded to `[0, TAU)` so it never loses
/// precision over a long session the way an ever-growing angle would.
fn spin_phase(ui: &Ui, period: f64) -> f32 {
    (((ui.input(|i| i.time) / period).rem_euclid(1.0)) * TAU as f64) as f32
}

/// A fixed spin and tilt for a globe that isn't turning — a small per-seed
/// offset so a list of still agents doesn't read as one shape repeated, the
/// way distinct hues already keep a row of initials discs from doing.
fn still_phase(seed: &str) -> (f32, f32) {
    let h = seed.bytes().fold(2166136261u32, |acc, b| (acc ^ b as u32).wrapping_mul(16777619));
    let spin = (h % 1000) as f32 / 1000.0 * TAU;
    let tilt = 0.32 + ((h / 1000) % 100) as f32 / 100.0 * 0.22;
    (spin, tilt)
}

/// Where the now-line spinner's clock is, 0→1 through a turn, or `None` with
/// motion off.
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
}
