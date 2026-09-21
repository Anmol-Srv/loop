//! Visualisations.
//!
//! Four shapes, all hand-drawn: egui has no chart primitive and a plotting
//! crate would be a dependency for four static figures. Each takes counts the
//! API already returns — none of them can be fed a number we do not have,
//! which is deliberate after a capacity chart was once drawn from nothing.

use egui::{Color32, Pos2, Response, RichText, Sense, Ui, Vec2};

use super::tokens::{colour, radius, space, text};
use super::{motion, theme};

/// Every figure on the dashboard draws into a box this tall. Four cards of
/// four different heights read as four unrelated things; one height makes the
/// row a row. Sized to the tallest figure (the headline plus its columns),
/// and the card clips rather than grows, so no future figure can break the
/// alignment by being one row longer.
pub const BODY_H: f32 = 102.0;

/// How many bars a figure shows before it collapses the rest into a count.
/// Four rows is what fits in `BODY_H` with a line left for the overflow.
pub const MAX_BARS: usize = 4;

/// One slice of a donut, or one row of a legend.
pub struct Slice<'a> {
    pub label: &'a str,
    pub count: usize,
    pub colour: Color32,
}

/// A ring with a legend beside it.
///
/// The centre carries the one number worth reading at a glance; the legend
/// carries the rest. A pie with labels on the slices is unreadable at this
/// size, which is why the legend is a column and not callouts.
pub fn donut(ui: &mut Ui, slices: &[Slice<'_>], centre_value: &str, centre_label: &str) {
    let total: usize = slices.iter().map(|s| s.count).sum();
    let d = 82.0;

    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(d), Sense::hover());
        let centre = rect.center();
        let radius_outer = d / 2.0;
        let thickness = 7.0;
        let r = radius_outer - thickness / 2.0;
        let p = ui.painter();

        // The track, so an empty or partial ring still reads as a ring.
        ring_arc(p, centre, r, thickness, 0.0, std::f32::consts::TAU, colour::SURFACE_HOVER);

        if total > 0 {
            let mut start = -std::f32::consts::FRAC_PI_2;
            for s in slices {
                if s.count == 0 {
                    continue;
                }
                let sweep = s.count as f32 / total as f32 * std::f32::consts::TAU;
                ring_arc(p, centre, r, thickness, start, sweep, s.colour);
                start += sweep;
            }
        }

        p.text(
            centre - Vec2::new(0.0, 4.0),
            egui::Align2::CENTER_CENTER,
            centre_value,
            egui::FontId::new(text::HEADING, egui::FontFamily::Name(theme::BOLD.into())),
            colour::TEXT,
        );
        p.text(
            centre + Vec2::new(0.0, 9.0),
            egui::Align2::CENTER_CENTER,
            centre_label,
            egui::FontId::proportional(text::CAPTION - 1.5),
            colour::TEXT_MUTED,
        );

        ui.add_space(space::MD);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::XXS;
            for s in slices {
                legend_row(ui, s);
            }
        });
    });
}

/// An arc drawn as a triangle strip. egui can stroke a path, but a stroked
/// polyline leaves mitre gaps at this thickness; two rings of vertices do not.
fn ring_arc(
    p: &egui::Painter,
    centre: Pos2,
    r: f32,
    thickness: f32,
    start: f32,
    sweep: f32,
    fill: Color32,
) {
    let steps = ((sweep.abs() / 0.12).ceil() as usize).max(2);
    let inner = r - thickness / 2.0;
    let outer = r + thickness / 2.0;
    let mut mesh = egui::Mesh::default();

    for i in 0..=steps {
        let a = start + sweep * (i as f32 / steps as f32);
        let (sin, cos) = a.sin_cos();
        mesh.colored_vertex(centre + Vec2::new(cos * inner, sin * inner), fill);
        mesh.colored_vertex(centre + Vec2::new(cos * outer, sin * outer), fill);
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b, b + 1);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

fn legend_row(ui: &mut Ui, s: &Slice<'_>) {
    ui.horizontal(|ui| {
        let (sw, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
        ui.painter().rect_filled(sw, 2.0, s.colour);
        ui.add_space(space::XXS);
        ui.label(
            RichText::new(s.label)
                .size(text::CAPTION)
                .color(colour::TEXT_2),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(s.count.to_string())
                    .size(text::CAPTION)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
        });
    });
}

/// A labelled horizontal bar: name, track, count. Used for done-per-discipline
/// and for team load, which are the same shape.
pub fn bar_row(ui: &mut Ui, name: &str, fill: f32, tint: Color32, right: &str, name_w: f32) {
    ui.horizontal(|ui| {
        ui.set_height(14.0);
        let (n, _) = ui.allocate_exact_size(Vec2::new(name_w, 14.0), Sense::hover());
        ui.painter().text(
            egui::pos2(n.left(), n.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(text::CAPTION),
            colour::TEXT_2,
        );

        let right_w = 40.0;
        let track_w = (ui.available_width() - right_w - space::SM).max(20.0);
        let (track, _) = ui.allocate_exact_size(Vec2::new(track_w, 12.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(track, 3.0, colour::INSET);
        if fill > 0.0 {
            p.rect_filled(
                egui::Rect::from_min_size(
                    track.min,
                    Vec2::new(track.width() * fill.clamp(0.0, 1.0), track.height()),
                ),
                3.0,
                tint,
            );
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(right)
                    .size(text::CAPTION)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
        });
    });
    ui.add_space(space::XS + 1.0);
}

/// A column chart with day labels. Seven bars is a week; more than about
/// fourteen and this wants to be a line.
pub fn columns(ui: &mut Ui, values: &[f32], labels: &[&str], tint: Color32) {
    if values.is_empty() {
        return;
    }
    let height = 44.0;
    let peak = values.iter().cloned().fold(1.0_f32, f32::max);
    let gap = 5.0;
    let n = values.len() as f32;

    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let w = (rect.width() - gap * (n - 1.0)) / n;
    let p = ui.painter();
    for (i, v) in values.iter().enumerate() {
        let h = (v / peak * height).max(2.0);
        let x = rect.left() + i as f32 * (w + gap);
        p.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, rect.bottom() - h), Vec2::new(w, h)),
            3.0,
            tint,
        );
    }

    let (lrect, _) = ui.allocate_exact_size(Vec2::new(rect.width(), 12.0), Sense::hover());
    let p = ui.painter();
    for (i, label) in labels.iter().enumerate().take(values.len()) {
        let x = lrect.left() + i as f32 * (w + gap) + w / 2.0;
        p.text(
            egui::pos2(x, lrect.center().y),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(text::CAPTION - 1.5),
            colour::TEXT_MUTED,
        );
    }
}

/// A visualisation card: a quiet label, something on the right, then the
/// figure in a fixed-height box.
///
/// The box is exactly `BODY_H` tall and clipped, which is the whole point —
/// four cards in a row must be one height, and the only way to guarantee that
/// is to stop the figure deciding. A figure that needs more room shows fewer
/// rows (see `MAX_BARS`) rather than stretching its card.
pub fn card<R>(ui: &mut Ui, label: &str, right: &str, body: impl FnOnce(&mut Ui) -> R) -> Response {
    super::cards::surface(ui, false, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(label)
                    .size(text::SMALL)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT_2),
            );
            if !right.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(right)
                            .size(text::CAPTION)
                            .color(colour::TEXT_MUTED),
                    );
                });
            }
        });
        ui.add_space(space::MD);

        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), BODY_H), Sense::hover());
        let mut box_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        box_ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        body(&mut box_ui)
    })
    .response
}

/// A big numeral with a quiet qualifier beside it.
pub fn headline(ui: &mut Ui, value: &str, note: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(value)
                .size(text::HERO)
                .family(egui::FontFamily::Name(theme::BOLD.into()))
                .color(colour::TEXT),
        );
        if !note.is_empty() {
            ui.add_space(space::XS);
            ui.label(
                RichText::new(note)
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            );
        }
    });
}

/// A filter control that shows whether it is set.
///
/// Three states worth telling apart, so all three get their own fill rather
/// than sharing one and differing by border: idle is the surface, hover eases
/// a step lighter, and active is accent-tinted with accent text. A filtered
/// view should never be mistakable for an empty one.
///
/// `caret` is for the ones that open a menu. A toggle that draws a caret is
/// promising a popup it does not have.
pub fn filter(ui: &mut Ui, label: &str, active: bool, caret: bool) -> Response {
    let font = egui::FontId::new(
        text::SMALL,
        egui::FontFamily::Name(if active { theme::SEMIBOLD } else { theme::MEDIUM }.into()),
    );
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, colour::TEXT);
    let caret_w = if caret { 14.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(galley.size().x + space::MD * 2.0 + caret_w, HEIGHT),
        Sense::click(),
    );
    let response = motion::operable(ui, response, radius::SM as f32);

    let fill = if active {
        colour::ACCENT_SOFT
    } else {
        motion::hover_fill(
            ui,
            response.id.with("fill"),
            response.hovered(),
            colour::SURFACE,
            colour::SURFACE_HOVER,
        )
    };
    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, fill);
    p.rect_stroke(
        rect,
        radius::SM as f32,
        egui::Stroke::new(
            1.0,
            if active {
                colour::ACCENT
            } else if response.hovered() {
                colour::LINE_STRONG
            } else {
                colour::LINE
            },
        ),
        egui::StrokeKind::Inside,
    );
    let ink = if active { colour::ACCENT } else { colour::TEXT_2 };
    p.galley(
        egui::pos2(rect.left() + space::MD, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    if caret {
        p.text(
            egui::pos2(rect.right() - space::SM, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "\u{25BE}",
            egui::FontId::proportional(text::CAPTION),
            if active { colour::ACCENT } else { colour::TEXT_MUTED },
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// Every control on the filter bar is this tall, including the clear button,
/// so the bar reads as one strip rather than as controls of two heights.
pub const HEIGHT: f32 = 30.0;

/// The escape hatch: only drawn when something is filtered, because a clear
/// button next to four unset filters is a control that does nothing.
pub fn clear(ui: &mut Ui) -> Response {
    let font = egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::MEDIUM.into()));
    let galley = ui.painter().layout_no_wrap("Clear".to_owned(), font, colour::TEXT);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(galley.size().x + space::MD * 2.0, HEIGHT),
        Sense::click(),
    );
    let response = motion::operable(ui, response, radius::SM as f32);

    let ink = if response.hovered() { colour::TEXT } else { colour::TEXT_MUTED };
    ui.painter().galley(
        egui::pos2(rect.left() + space::MD, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}
