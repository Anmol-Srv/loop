//! The table.
//!
//! Every list in the app is one of these: the dashboard, the projects, a
//! project's tasks, my tasks. They used to each carry their own frame, header
//! band, hover tracking and keyboard wiring, and each one made its own small
//! decisions about alignment — which is how a right-aligned age ended up
//! under a left-aligned "Created". One implementation, one set of decisions.
//!
//! A column declares its width and its alignment once, in `Col`, and both
//! the header label and `cell` honour it, so a header cannot disagree with
//! the values beneath it.

use egui::{Align, Layout, RichText, Ui};
use egui_extras::{Column, TableBuilder, TableRow};

use super::tokens::{colour, radius, size, space, text};
use super::{motion, theme};

/// The height every table's rows share. Tall enough for an avatar and two
/// chips; short enough that twenty rows fit on a laptop.
pub const ROW_H: f32 = 38.0;

#[derive(Clone, Copy)]
pub enum Width {
    Exact(f32),
    /// Takes what is left, but never less than this.
    Remainder(f32),
}

#[derive(Clone, Copy)]
pub struct Col {
    pub label: &'static str,
    pub width: Width,
    pub align: Align,
}

impl Col {
    pub const fn left(label: &'static str, w: f32) -> Self {
        Self { label, width: Width::Exact(w), align: Align::LEFT }
    }
    pub const fn right(label: &'static str, w: f32) -> Self {
        Self { label, width: Width::Exact(w), align: Align::RIGHT }
    }
    pub const fn fill(label: &'static str, min: f32) -> Self {
        Self { label, width: Width::Remainder(min), align: Align::LEFT }
    }
}

/// A cell laid out to its column's alignment. Every text cell goes through
/// this so the alignment decision is made once, in the `Col`.
pub fn cell(ui: &mut Ui, col: &Col, add: impl FnOnce(&mut Ui)) {
    let layout = match col.align {
        Align::RIGHT => Layout::right_to_left(Align::Center),
        _ => Layout::left_to_right(Align::Center),
    };
    ui.with_layout(layout, add);
}

/// A plain text cell: body size, full ink unless told otherwise.
pub fn text_cell(ui: &mut Ui, col: &Col, s: &str, ink: egui::Color32) {
    cell(ui, col, |ui| {
        ui.add(
            egui::Label::new(RichText::new(s).size(text::BODY).color(ink))
                .truncate()
                .selectable(false),
        );
    });
}

/// A muted secondary text cell, "—" when empty.
pub fn muted_cell(ui: &mut Ui, col: &Col, s: &str) {
    let (s, ink) = if s.is_empty() {
        ("—", colour::TEXT_FAINT)
    } else {
        (s, colour::TEXT_MUTED)
    };
    cell(ui, col, |ui| {
        ui.add(
            egui::Label::new(RichText::new(s).size(text::SMALL).color(ink))
                .truncate()
                .selectable(false),
        );
    });
}

/// Draw a table. `row` fills one row's cells with `TableRow::col`, in column
/// order. Returns the index of the row that was clicked this frame, if any.
///
/// Hover is tracked one frame behind in the temp store, because a row's own
/// response only exists after its last cell — too late to tint its first.
/// Keyboard reach is wired after the builder releases the `Ui`, since the
/// body closure holds the only one the focus ring can be drawn on.
pub fn show(
    ui: &mut Ui,
    id: &str,
    cols: &[Col],
    n: usize,
    mut row: impl FnMut(&mut TableRow<'_, '_>, usize),
) -> Option<usize> {
    let hover_id = egui::Id::new(id).with("hover");
    let was: Option<usize> = ui.ctx().data(|d| d.get_temp(hover_id)).flatten();
    let mut now: Option<usize> = None;
    let mut clicked: Option<usize> = None;
    let mut responses: Vec<egui::Response> = Vec::with_capacity(n);

    egui::Frame::new()
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::LINE))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::symmetric(space::MD as i8, 0))
        .show(ui, |ui| {
            // Reserved now, painted once the header's extent is known: the
            // band has to sit under the header text, not over it.
            let band = ui.painter().add(egui::Shape::Noop);
            let top = ui.cursor().top();
            ui.spacing_mut().item_spacing = egui::Vec2::new(space::MD, 0.0);

            let mut builder = TableBuilder::new(ui)
                .id_salt(id)
                .vscroll(false)
                .sense(egui::Sense::click())
                .cell_layout(Layout::left_to_right(Align::Center));
            for c in cols {
                builder = builder.column(match c.width {
                    Width::Exact(w) => Column::exact(w).clip(true),
                    Width::Remainder(min) => Column::remainder().at_least(min).clip(true),
                });
            }

            builder
                .header(size::CONTROL, |mut header| {
                    for c in cols {
                        header.col(|ui| {
                            cell(ui, c, |ui| {
                                ui.label(
                                    RichText::new(c.label)
                                        .size(text::CAPTION)
                                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                                        .color(colour::TEXT_MUTED),
                                );
                            });
                        });
                    }
                })
                .body(|mut body| {
                    for i in 0..n {
                        body.row(ROW_H, |mut r| {
                            r.set_hovered(was == Some(i));
                            r.set_overline(i > 0);
                            row(&mut r, i);
                            responses.push(r.response());
                        });
                    }
                });

            for (i, response) in responses.into_iter().enumerate() {
                let response = motion::operable_sm(ui, response);
                if response.hovered() {
                    now = Some(i);
                    response.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    clicked = Some(i);
                }
            }

            let band_rect = egui::Rect::from_min_max(
                egui::pos2(ui.max_rect().left() - space::MD, top),
                egui::pos2(ui.max_rect().right() + space::MD, top + size::CONTROL),
            );
            ui.painter().set(
                band,
                egui::Shape::rect_filled(
                    band_rect,
                    egui::CornerRadius { nw: radius::LG, ne: radius::LG, sw: 0, se: 0 },
                    colour::CHROME,
                ),
            );
            ui.painter().hline(
                band_rect.x_range(),
                band_rect.bottom(),
                egui::Stroke::new(1.0, colour::LINE),
            );
        });

    ui.ctx().data_mut(|d| d.insert_temp(hover_id, now));
    clicked
}
