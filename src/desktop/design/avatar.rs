//! Avatars.
//!
//! A gradient disc with initials, and nothing else. An earlier version opened
//! a ring of actions on click; it was more component than the job needed, and
//! the sidebar already has room for plain controls.
//!
//! The hue is a hash of the seed, so one person is the same colour in every
//! list, forever, without anyone assigning colours.

use egui::{Align2, Color32, FontId, Response, Sense, Ui, Vec2};

use super::tokens::{colour, text};

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
fn hue_of(seed: &str) -> f32 {
    let h = seed
        .bytes()
        .fold(2166136261u32, |acc, b| (acc ^ b as u32).wrapping_mul(16777619));
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

/// The avatar.
pub fn small(ui: &mut Ui, seed: &str, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let hue = hue_of(seed);
    let p = ui.painter();
    p.circle_filled(rect.center(), size / 2.0, from_hue(hue, 0.42, 0.72));
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials(seed),
        FontId::proportional((size * 0.36).max(text::CAPTION)),
        colour::ON_ACCENT,
    );
    response.on_hover_text(seed)
}

/// A stack of overlapping avatars, as used for "who is on this".
pub fn stack(ui: &mut Ui, seeds: &[&str], size: f32) {
    let overlap = size * 0.32;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = -overlap;
        for seed in seeds.iter().take(4) {
            small(ui, seed, size);
        }
        if seeds.len() > 4 {
            ui.add_space(overlap + 4.0);
            ui.label(
                egui::RichText::new(format!("+{}", seeds.len() - 4))
                    .size(text::CAPTION)
                    .color(colour::TEXT_MUTED),
            );
        }
    });
}
