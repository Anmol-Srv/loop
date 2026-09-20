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
    /// Listed bottom-up: the first action sits nearest the avatar.
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
/// Clicking opens a list of actions *above* the avatar, springing up out of
/// it. Upward because the avatar lives at the foot of the sidebar, and a menu
/// that opens off-screen is no menu at all.
///
/// The list floats in an `Area` rather than taking layout space, so opening it
/// never reflows the sidebar underneath.
///
/// Open/closed state lives in egui's temp store keyed on the widget id, so a
/// caller does not have to thread a bool through its own state.
pub fn show(ui: &mut Ui, avatar: &Avatar<'_>) -> Option<usize> {
    let id = ui.make_persistent_id(("avatar", avatar.seed));
    let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));

    let (area, face_response) =
        ui.allocate_exact_size(Vec2::splat(avatar.size), Sense::click());
    let centre = area.center();

    // 0 closed, 1 fully open. egui drives the easing and the repaints.
    let t = ui.ctx().animate_bool_with_time(id, open, 0.16);

    let mut clicked = None;

    if t > 0.001 {
        let row_h = 30.0;
        let gap = 6.0;
        let width = 152.0;
        let count = avatar.actions.len() as f32;
        let stack_h = count * row_h + (count - 1.0).max(0.0) * gap;
        let travel = 8.0; // how far each row slides as it fades in

        let top_left = egui::pos2(
            centre.x - width / 2.0,
            area.top() - gap - stack_h + (1.0 - t) * travel,
        );

        egui::Area::new(id.with("menu"))
            .fixed_pos(top_left)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ui.ctx(), |ui| {
                ui.set_width(width);
                // Rendered bottom-up: index 0 ends nearest the avatar, so the
                // first action is closest to the cursor and anything
                // destructive sits furthest from an accidental click.
                for (i, action) in avatar.actions.iter().enumerate().rev() {
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(width, row_h), Sense::click());
                    let hovered = response.hovered();

                    let fill = if hovered { colour::GLASS_ACTIVE } else { colour::GLASS_HOVER };
                    let p = ui.painter();
                    p.rect_filled(rect, 8.0, fill.gamma_multiply(t));
                    p.rect_stroke(
                        rect,
                        8.0,
                        egui::Stroke::new(
                            1.0,
                            if hovered { colour::EDGE_HI_HOVER } else { colour::EDGE_MID }
                                .gamma_multiply(t),
                        ),
                        egui::StrokeKind::Inside,
                    );

                    let fg = action.tint.unwrap_or(colour::TEXT).gamma_multiply(t);
                    p.text(
                        egui::pos2(rect.left() + 11.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        action.icon,
                        FontId::proportional(14.0),
                        fg,
                    );
                    p.text(
                        egui::pos2(rect.left() + 32.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        action.label,
                        FontId::proportional(text::BODY),
                        fg,
                    );

                    if hovered {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if response.clicked() {
                        clicked = Some(i);
                        open = false;
                    }
                    if i > 0 {
                        ui.add_space(gap);
                    }
                }
            });
    }

    // The avatar itself.
    let face = Rect::from_center_size(centre, Vec2::splat(avatar.size));
    let hue = hue_of(avatar.seed);
    let top = from_hue(hue, 0.42, 0.82);
    let bottom = from_hue((hue + 40.0) % 360.0, 0.52, 0.62);

    // A two-triangle mesh clipped to the circle: egui has no radial fill, and
    // a flat disc next to glass looks like a sticker.
    let p = ui.painter().with_clip_rect(face);
    p.circle_filled(centre, avatar.size / 2.0, top);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(face.left_top(), top);
    mesh.colored_vertex(face.right_top(), top);
    mesh.colored_vertex(face.left_bottom(), bottom);
    mesh.colored_vertex(face.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    p.add(egui::Shape::mesh(mesh));

    let p = ui.painter();
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

    if face_response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if face_response.clicked() {
        open = !open;
    }
    // A click anywhere else closes it, which is what every menu does.
    if open && clicked.is_none() && ui.input(|i| i.pointer.any_click()) && !face_response.clicked()
    {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            let menu = Rect::from_min_size(
                egui::pos2(centre.x - 80.0, area.top() - 160.0),
                Vec2::new(160.0, 160.0),
            );
            if !menu.contains(pos) && !face.contains(pos) {
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
