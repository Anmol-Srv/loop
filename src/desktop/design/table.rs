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
    /// What to give up first when the window is too narrow. `0` never drops —
    /// it is the column the table exists to show. Higher numbers go first.
    ///
    /// Fixed columns that simply run off the right edge were the bug this
    /// fixes: a table that cannot fit should show less, not show it cut in
    /// half.
    pub rank: u8,
}

impl Col {
    pub const fn left(label: &'static str, w: f32) -> Self {
        Self { label, width: Width::Exact(w), align: Align::LEFT, rank: 0 }
    }
    pub const fn right(label: &'static str, w: f32) -> Self {
        Self { label, width: Width::Exact(w), align: Align::RIGHT, rank: 0 }
    }
    pub const fn fill(label: &'static str, min: f32) -> Self {
        Self { label, width: Width::Remainder(min), align: Align::LEFT, rank: 0 }
    }
    /// Mark this column droppable. Same column, one call later in the chain.
    pub const fn rank(mut self, rank: u8) -> Self {
        self.rank = rank;
        self
    }
}

/// The cells of one row, addressed by their column's index in the full set.
///
/// The caller names the column it is filling, so a table that has dropped a
/// column for width still puts every remaining value under the right header.
/// Writing `row.col(…)` in order could not survive a missing column.
pub struct Cells<'a, 'r, 'c> {
    row: &'a mut TableRow<'r, 'c>,
    cols: &'a [Col],
    visible: &'a [bool],
}

impl Cells<'_, '_, '_> {
    /// Anything hand-drawn: a chip, an avatar, a bar.
    pub fn at(&mut self, i: usize, add: impl FnOnce(&mut Ui)) {
        if !self.visible[i] {
            return;
        }
        let col = self.cols[i];
        self.row.col(|ui| cell(ui, &col, add));
    }

    /// The row's own name: body size, semibold.
    pub fn strong(&mut self, i: usize, s: &str, ink: egui::Color32) {
        self.at(i, |ui| strong_label(ui, s, ink));
    }

    /// A plain value at body size.
    pub fn text(&mut self, i: usize, s: &str, ink: egui::Color32) {
        self.at(i, |ui| {
            ui.add(
                egui::Label::new(RichText::new(s).size(text::BODY).color(ink))
                    .truncate()
                    .selectable(false),
            );
        });
    }

    /// Secondary text, "—" when there is none.
    pub fn muted(&mut self, i: usize, s: &str) {
        let (s, ink) =
            if s.is_empty() { ("\u{2014}", colour::TEXT_FAINT) } else { (s, colour::TEXT_MUTED) };
        self.at(i, |ui| {
            ui.add(
                egui::Label::new(RichText::new(s).size(text::SMALL).color(ink))
                    .truncate()
                    .selectable(false),
            );
        });
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

/// A row's own name: body size, semibold. The one text in a row that carries
/// weight, and four tables spelled it out identically. Separate from
/// `strong_cell` because three of them sit beside a chip in the same cell.
pub fn strong_label(ui: &mut Ui, s: &str, ink: egui::Color32) {
    ui.add(
        egui::Label::new(
            RichText::new(s)
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(ink),
        )
        .truncate()
        .selectable(false),
    );
}

/// `strong_label` in a cell of its own, for a title with nothing beside it.
pub fn strong_cell(ui: &mut Ui, col: &Col, s: &str, ink: egui::Color32) {
    cell(ui, col, |ui| strong_label(ui, s, ink));
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
    mut row: impl FnMut(&mut Cells<'_, '_, '_>, usize),
) -> Option<usize> {
    // Drop the least important column until the rest fit. The gaps go with
    // them, so this converges rather than shaving one column short.
    let mut visible = vec![true; cols.len()];
    let budget = ui.available_width() - space::MD * 2.0;
    loop {
        let shown: Vec<&Col> =
            cols.iter().zip(&visible).filter(|(_, v)| **v).map(|(c, _)| c).collect();
        let needed: f32 = shown
            .iter()
            .map(|c| match c.width {
                Width::Exact(w) => w,
                Width::Remainder(min) => min,
            })
            .sum::<f32>()
            + space::MD * (shown.len().saturating_sub(1)) as f32;
        if needed <= budget {
            break;
        }
        // The highest rank still showing is the next to go; rank 0 stays even
        // if that means a table wider than its window.
        let worst = cols
            .iter()
            .enumerate()
            .filter(|(i, c)| visible[*i] && c.rank > 0)
            .max_by_key(|(_, c)| c.rank);
        match worst {
            Some((i, _)) => visible[i] = false,
            None => break,
        }
    }

    let hover_id = egui::Id::new(id).with("hover");
    let was: Option<usize> = ui.ctx().data(|d| d.get_temp(hover_id)).flatten();
    let mut now: Option<usize> = None;
    let mut clicked: Option<usize> = None;
    let mut responses: Vec<(usize, egui::Response)> = Vec::with_capacity(n);

    egui::Frame::new()
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::LINE))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::symmetric(space::MD as i8, 0))
        .show(ui, |ui| {
            // Reserved now, painted once the header's extent is known: the
            // band has to sit under the header text, not over it. The same
            // for the row hover and the separators: egui_extras would paint
            // them itself, but only across the table's own rect, which stops
            // `space::MD` short of the frame on either side — a hover that
            // does not reach the edge reads as a box inside the row.
            let band = ui.painter().add(egui::Shape::Noop);
            let rows_paint = ui.painter().add(egui::Shape::Noop);
            let top = ui.cursor().top();
            let x_range = egui::Rangef::new(
                ui.max_rect().left() - space::MD,
                ui.max_rect().right() + space::MD,
            );
            ui.spacing_mut().item_spacing = egui::Vec2::new(space::MD, 0.0);

            let mut builder = TableBuilder::new(ui)
                .id_salt(id)
                .vscroll(false)
                .sense(egui::Sense::click())
                .cell_layout(Layout::left_to_right(Align::Center));
            // Once the remainder column has been dropped every column left is
            // fixed, so the table stops short of the frame it sits in — and
            // the band and the row hover, which span the frame, hang past it.
            // The last column takes the slack instead.
            let fills = cols
                .iter()
                .zip(&visible)
                .any(|(c, v)| *v && matches!(c.width, Width::Remainder(_)));
            let last = visible.iter().rposition(|v| *v);
            for (i, c) in cols.iter().enumerate().filter(|(i, _)| visible[*i]) {
                let stretch = !fills && Some(i) == last;
                builder = builder.column(match c.width {
                    Width::Exact(w) if stretch => Column::remainder().at_least(w).clip(true),
                    Width::Exact(w) => Column::exact(w).clip(true),
                    Width::Remainder(min) => Column::remainder().at_least(min).clip(true),
                });
            }

            builder
                .header(size::CONTROL, |mut header| {
                    for c in cols.iter().zip(&visible).filter(|(_, v)| **v).map(|(c, _)| c) {
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
                .body(|body| {
                    // `rows` culls to what is on screen; a loop of `row` would
                    // build all of them, and these lists are unbounded.
                    body.rows(ROW_H, n, |mut r| {
                        let i = r.index();
                        row(&mut Cells { row: &mut r, cols, visible: &visible }, i);
                        responses.push((i, r.response()));
                    });
                });

            for (i, response) in responses {
                let response = motion::operable_sm(ui, response);
                if response.hovered() {
                    now = Some(i);
                    response.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    clicked = Some(i);
                }
            }

            // Rows sit directly under the header at a fixed pitch, so their
            // rects are known without asking the table.
            let mut shapes: Vec<egui::Shape> = Vec::with_capacity(n + 1);
            let first_row = top + size::CONTROL;
            let clip = ui.clip_rect();
            for i in 0..n {
                let row_top = first_row + i as f32 * ROW_H;
                // Off-screen rows are not built, so they do not need a rule
                // or a hover fill either.
                if row_top + ROW_H < clip.top() || row_top > clip.bottom() {
                    continue;
                }
                if i > 0 {
                    shapes.push(egui::Shape::hline(
                        x_range,
                        row_top,
                        egui::Stroke::new(1.0, colour::LINE),
                    ));
                }
                if was == Some(i) {
                    // The last row shares the frame's rounded bottom; a square
                    // fill there would poke out of the corners.
                    let corners = if i + 1 == n {
                        egui::CornerRadius { nw: 0, ne: 0, sw: radius::LG, se: radius::LG }
                    } else {
                        egui::CornerRadius::ZERO
                    };
                    shapes.push(egui::Shape::rect_filled(
                        egui::Rect::from_x_y_ranges(x_range, row_top..=row_top + ROW_H),
                        corners,
                        colour::SURFACE_HOVER,
                    ));
                }
            }
            ui.painter().set(rows_paint, egui::Shape::Vec(shapes));

            let band_rect = egui::Rect::from_x_y_ranges(x_range, top..=top + size::CONTROL);
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

/// Which row the pointer was over last frame, for a cell that is only offered
/// on hover. Reads the slot `show` writes, with the same one-frame lag.
pub fn hovered(ui: &Ui, id: &str) -> Option<usize> {
    ui.ctx().data(|d| d.get_temp(egui::Id::new(id).with("hover"))).flatten()
}
