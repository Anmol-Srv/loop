//! An avatar that opens a ring of actions when clicked.
//!
//! Modelled on the radial cluster on bencho.dev: one round avatar, with
//! satellite buttons that spring out around it. The satellites animate from
//! the centre rather than appearing in place, so the ring reads as *coming
//! from* the avatar — that motion is the whole idea.

use egui::{Align2, Color32, FontId, Rect, Response, Sense, Ui, Vec2};

use super::tokens::{colour, text};

/// One satellite.
pub struct Action<'a> {
    /// A Phosphor glyph.
    pub icon: &'a str,
    /// Shown on hover; also the accessible name.
    pub label: &'a str,
    /// Tint for destructive or otherwise notable actions.
    pub tint: Option<Color32>,
}

pub struct Avatar<'a> {
    /// Drives both the initials and the gradient, so one person is always the
    /// same colour everywhere in the app.
    pub seed: &'a str,
    pub size: f32,
    pub actions: &'a [Action<'a>],
}

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

/// Draw the avatar. Returns the index of a clicked action, if any.
///
/// Open/closed state lives in egui's temp store keyed on the widget id, so a
/// caller does not have to thread a bool through its own state.
pub fn show(ui: &mut Ui, avatar: &Avatar<'_>) -> Option<usize> {
    let id = ui.make_persistent_id(("avatar", avatar.seed));
    let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));

    // The ring needs room even when closed, or opening would reflow the page.
    let ring = avatar.size * 0.95;
    let satellite = avatar.size * 0.62;
    let extent = ring + satellite / 2.0 + 4.0;
    let (area, _) = ui.allocate_exact_size(Vec2::splat(extent * 2.0), Sense::hover());
    let centre = area.center();

    // 0 closed, 1 fully open. egui drives the easing and repaints for us.
    let t = ui.ctx().animate_bool_with_time(id, open, 0.18);

    let mut clicked = None;

    if t > 0.001 {
        let count = avatar.actions.len().max(1) as f32;
        for (i, action) in avatar.actions.iter().enumerate() {
            // Start at twelve o'clock and go clockwise.
            let angle = -std::f32::consts::FRAC_PI_2
                + (i as f32 / count) * std::f32::consts::TAU;
            let distance = ring * t;
            let pos = centre + Vec2::new(angle.cos(), angle.sin()) * distance;
            let size = satellite * t;
            let rect = Rect::from_center_size(pos, Vec2::splat(size));

            let response = ui
                .interact(rect, id.with(i), Sense::click())
                .on_hover_text(action.label);
            let hovered = response.hovered();

            let fg = action.tint.unwrap_or(colour::TEXT);
            let fill = if hovered { colour::GLASS_ACTIVE } else { colour::GLASS_HOVER };

            let p = ui.painter();
            p.circle_filled(pos, size / 2.0, fill);
            p.circle_stroke(
                pos,
                size / 2.0,
                egui::Stroke::new(1.0, if hovered { colour::EDGE_HI_HOVER } else { colour::EDGE_MID }),
            );
            // Satellites fade in with the spring, so early frames are ghosts.
            p.text(
                pos,
                Align2::CENTER_CENTER,
                action.icon,
                FontId::proportional(size * 0.42),
                fg.gamma_multiply(t),
            );

            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                clicked = Some(i);
                open = false;
            }
        }
    }

    // The avatar itself, painted last so it sits above the ring.
    let face = Rect::from_center_size(centre, Vec2::splat(avatar.size));
    let response = ui.interact(face, id.with("face"), Sense::click());

    let hue = hue_of(avatar.seed);
    let top = from_hue(hue, 0.42, 0.82);
    let bottom = from_hue((hue + 40.0) % 360.0, 0.52, 0.62);

    // A two-triangle mesh clipped to the circle: egui has no radial fill, and
    // a flat disc next to glass looks like a sticker.
    let p = ui.painter().with_clip_rect(face);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(face.left_top(), top);
    mesh.colored_vertex(face.right_top(), top);
    mesh.colored_vertex(face.left_bottom(), bottom);
    mesh.colored_vertex(face.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    p.circle_filled(centre, avatar.size / 2.0, top);
    p.add(egui::Shape::mesh(mesh));

    let p = ui.painter();
    p.circle_filled(centre, avatar.size / 2.0 - 0.0, Color32::TRANSPARENT);
    p.circle_stroke(
        centre,
        avatar.size / 2.0,
        egui::Stroke::new(1.0, if open { colour::EDGE_HI_HOVER } else { colour::EDGE_MID }),
    );
    p.text(
        centre,
        Align2::CENTER_CENTER,
        initials(avatar.seed),
        FontId::proportional(avatar.size * 0.34),
        colour::ON_ACCENT,
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        open = !open;
    }
    // Clicking anywhere else closes the ring, which is what every menu does.
    if open && ui.input(|i| i.pointer.any_click()) && !response.clicked() && clicked.is_none() {
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if let Some(pos) = pointer {
            if centre.distance(pos) > ring + satellite {
                open = false;
            }
        }
    }

    ui.ctx().data_mut(|d| d.insert_temp(id, open));
    clicked
}

/// A plain avatar with no actions — for list rows, where a ring would be noise.
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
