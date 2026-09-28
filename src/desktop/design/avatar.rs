//! Avatars.
//!
//! A gradient disc with initials, and nothing else. An earlier version opened
//! a ring of actions on click; it was more component than the job needed, and
//! the sidebar already has room for plain controls.
//!
//! The hue is a hash of the seed, so one person is the same colour in every
//! list, forever, without anyone assigning colours.

use egui::{Align2, Color32, FontId, Response, Sense, Ui, Vec2};

use super::tokens::text;

/// Two initials from a name or an email local-part.
fn initials(seed: &str) -> String {
    let base = seed.split('@').next().unwrap_or(seed);
    let mut parts = base
        .split(|c: char| c == ' ' || c == '.' || c == '_' || c == '-')
        .filter(|p| !p.is_empty());

    let first = parts.next().unwrap_or(base);
    match parts.next() {
        Some(second) => format!(
            "{}{}",
            first.chars().next().unwrap_or('?'),
            second.chars().next().unwrap_or(' ')
        )
        .to_uppercase(),
        None => first.chars().take(2).collect::<String>().to_uppercase(),
    }
}

/// A stable hue per person. Same seed, same colour, forever.
///
/// FNV-1a alone leaves short, similar seeds close together in hash space —
/// "Airtribe Agent" and "Slack Agent" landed two degrees apart before the
/// finalizer below, indistinguishable at a glance. A Murmur3-style mix
/// scatters neighbouring inputs across the full circle before the modulo.
fn hue_of(seed: &str) -> f32 {
    let mut h = seed
        .bytes()
        .fold(2166136261u32, |acc, b| (acc ^ b as u32).wrapping_mul(16777619));
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h % 360) as f32
}

fn from_hue(hue: f32, sat: f32, val: f32) -> Color32 {
    let c = val * sat;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = val - c;
    let (r, g, b) = match hue as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// A person's colour. An agent wears its owner's, so whose it is reads at a
/// glance.
pub fn tint(seed: &str) -> Color32 {
    tint_pair(seed).0
}

/// The same colour, plus a second hue for the agent globe's gradient — rotated
/// warmer rather than picked separately, so the pair always reads as one
/// person's two-tone rather than two people's single tones.
pub fn tint_pair(seed: &str) -> (Color32, Color32) {
    let hue = hue_of(seed);
    (from_hue(hue, 0.42, 0.72), from_hue((hue + 55.0) % 360.0, 0.48, 0.78))
}

/// Initials ink. The disc is a pale tint in either palette, so this stays
/// dark whatever the accent's own ink is.
const INK: Color32 = Color32::from_rgb(0x06, 0x14, 0x1E);

/// The avatar.
pub fn small(ui: &mut Ui, seed: &str, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint(ui.painter(), rect, seed);
    response.on_hover_text(seed)
}

/// The disc and initials into `rect`, for a caller that has placed it already.
pub fn paint(p: &egui::Painter, rect: egui::Rect, seed: &str) {
    let size = rect.width();
    p.circle_filled(rect.center(), size / 2.0, tint(seed));
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials(seed),
        FontId::proportional((size * 0.36).max(text::CAPTION)),
        INK,
    );
}
