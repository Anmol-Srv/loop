//! Visualisations.
//!
//! Four shapes, all hand-drawn: egui has no chart primitive and a plotting
//! crate would be a dependency for four static figures. Each takes counts the
//! API already returns — none of them can be fed a number we do not have,
//! which is deliberate after a capacity chart was once drawn from nothing.

use egui::{Color32, Pos2, Response, RichText, Sense, Ui, Vec2};

use super::tokens::{colour, radius, size, space, text};
use super::widgets::truncated;
use super::{motion, theme};

/// How many bars a figure shows before it collapses the rest into a count.
/// A density choice, not a fitting one — the row sizes itself to whatever it
/// is given, so this is only about how much of a long list is worth showing.
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
        let galley =
            truncated(ui, name, egui::FontId::proportional(text::CAPTION), colour::TEXT_2, name_w);
        ui.painter().galley(
            egui::pos2(n.left(), n.center().y - galley.size().y / 2.0),
            galley,
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
/// figure.
///
/// Four cards of four different heights read as four unrelated things, so
/// they all take the height of the tallest. `min_body` is that height, and
/// the return value is what this figure actually needed — `row` below feeds
/// one into the other. A fixed height was the first attempt and it was wrong:
/// it cropped the legend and half the column chart. A figure is allowed to be
/// as tall as it is; the row is what adapts.
pub fn card(
    ui: &mut Ui,
    label: &str,
    right: &str,
    min_body: f32,
    body: impl FnOnce(&mut Ui),
) -> f32 {
    let mut used = 0.0;
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
                    // The subtitle is the expendable half of this row: when
                    // the card is narrow it clips rather than running into
                    // the title, which is the half you actually need.
                    ui.add(
                        egui::Label::new(
                            RichText::new(right)
                                .size(text::CAPTION)
                                .color(colour::TEXT_MUTED),
                        )
                        .truncate(),
                    );
                });
            }
        });
        ui.add_space(space::MD);

        used = ui.scope(body).response.rect.height();
        if used < min_body {
            ui.add_space(min_body - used);
        }
    });
    used
}

/// A row of figures, all the same height.
///
/// egui lays out in one pass, so the tallest figure is not known until every
/// figure has been drawn. This keeps last frame's tallest in the temp store,
/// hands it to each card as a floor, and asks for a repaint on the frame the
/// answer changes. Content has to change for that to happen, and when it does
/// the correction lands in the same frame the user sees.
pub fn row(ui: &mut Ui, id: egui::Id, cards: &mut [&mut dyn FnMut(&mut Ui, f32) -> f32]) {
    let floor: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    let mut tallest = 0.0_f32;

    // Wrap rather than squeeze. Below the threshold a quarter of the page is
    // narrower than the donut and its legend, and the figures start clipping
    // — a card that cannot show its own legend is worse than a shorter row.
    let per_row = if ui.available_width() >= CARDS_FOUR_AT { cards.len() } else { 2 };

    let rows = cards.len().div_ceil(per_row);
    for (r, chunk) in cards.chunks_mut(per_row).enumerate() {
        // The last row of an odd split must not stretch its cards to fill the
        // page: `columns` divides by the count it is given, so it is given the
        // full count and the empty slots are simply not drawn.
        ui.columns(per_row, |cols| {
            for (col, card) in cols.iter_mut().zip(chunk.iter_mut()) {
                tallest = tallest.max(card(col, floor));
            }
        });
        // Between the rows only. What follows the block is the caller's gap.
        if r + 1 < rows {
            ui.add_space(space::MD);
        }
    }

    if (tallest - floor).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, tallest));
        ui.ctx().request_repaint();
    }
}

/// Content width at which the figures sit four across. Below it they go two
/// up: the donut plus its five-row legend is the widest of them and needs
/// about this much of a quarter-page to render without clipping.
const CARDS_FOUR_AT: f32 = 880.0;

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

/// How wide a control's label is allowed to get before it truncates. A filter
/// bar of five controls only fits if no one label can eat the row.
const MAX_LABEL_W: f32 = 160.0;
/// The caret's footprint. Hand-painted, because the vendored Nunito has no
/// glyph for ▾ and a missing glyph renders as a hollow box.
const CARET_W: f32 = 7.0;
const CARET_H: f32 = 4.0;
/// The gutter the caret sits in, so a label never crowds it.
const CARET_COL: f32 = 14.0;
/// A popup is at least this wide whatever the control measured — a two-word
/// filter should not open a two-word menu.
const MENU_MIN_W: f32 = 180.0;
/// The multi-select checkbox. 14 reads as a checkbox at this type size; 12
/// reads as a dot and 16 as a button. Its corner is half the control radius,
/// because `radius::SM` on a 14pt square is a lozenge, not a box.
const BOX: f32 = 14.0;
const BOX_R: f32 = radius::SM as f32 / 2.0;
/// The tick's width, and the weight it is drawn at. Two segments, no glyph.
const TICK_W: f32 = 9.0;
const TICK_STROKE: f32 = 2.0;

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
    let ink = if active { colour::ACCENT } else { colour::TEXT_2 };
    let galley = truncated(ui, label, font, ink, MAX_LABEL_W);
    let caret_w = if caret { CARET_COL } else { 0.0 };
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
    p.galley(
        egui::pos2(rect.left() + space::MD, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    if caret {
        caret_at(
            p,
            egui::pos2(rect.right() - space::SM - CARET_W / 2.0, rect.center().y),
            if active { colour::ACCENT } else { colour::TEXT_MUTED },
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// The disclosure triangle, as geometry. See `CARET_W`.
fn caret_at(p: &egui::Painter, centre: Pos2, ink: Color32) {
    p.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(centre.x - CARET_W / 2.0, centre.y - CARET_H / 2.0),
            egui::pos2(centre.x + CARET_W / 2.0, centre.y - CARET_H / 2.0),
            egui::pos2(centre.x, centre.y + CARET_H / 2.0),
        ],
        ink,
        egui::Stroke::NONE,
    ));
}

/// A tick, as two segments. Same reason as the caret: no glyph to borrow.
fn tick_at(p: &egui::Painter, centre: Pos2, ink: Color32) {
    let stroke = egui::Stroke::new(TICK_STROKE, ink);
    let elbow = egui::pos2(centre.x - TICK_W * 0.14, centre.y + TICK_W * 0.30);
    p.line_segment(
        [egui::pos2(centre.x - TICK_W / 2.0, centre.y + TICK_W * 0.02), elbow],
        stroke,
    );
    p.line_segment(
        [elbow, egui::pos2(centre.x + TICK_W / 2.0, centre.y - TICK_W * 0.34)],
        stroke,
    );
}

/// The popup's own surface. egui's stock menu frame is a different radius and
/// a different fill from every card in the app; this is the app's.
fn menu_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::LINE))
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::same(space::XS as i8))
}

/// One row of a menu.
///
/// `check` picks which indicator the row carries: a checkbox on the left for a
/// set you are building up, a tick on the right for a choice that replaces the
/// last one. Both are painted, both are the same row otherwise — a menu whose
/// rows differ in height between the two pickers reads as two components.
fn menu_row(ui: &mut Ui, label: &str, selected: bool, check: bool) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), size::CONTROL), Sense::click());
    let response = motion::operable(ui, response, radius::SM as f32);

    let fill = if selected {
        colour::ACCENT_SOFT
    } else {
        motion::hover_fill(
            ui,
            response.id.with("fill"),
            response.hovered(),
            colour::TRANSPARENT,
            colour::SURFACE_HOVER,
        )
    };
    let ink = if selected || response.hovered() {
        colour::TEXT
    } else {
        colour::TEXT_2
    };
    // The gutter the indicator occupies: a box on the left, a tick on the
    // right. Text is inset past whichever one this row has.
    let left = rect.left() + space::SM + if check { BOX + space::SM } else { 0.0 };
    let right_gutter = if check { space::SM } else { space::SM + TICK_W };
    let galley = truncated(
        ui,
        label,
        egui::FontId::proportional(text::BODY),
        ink,
        (rect.right() - right_gutter - left).max(1.0),
    );

    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, fill);
    p.galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );

    if check {
        let box_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + space::SM + BOX / 2.0, rect.center().y),
            Vec2::splat(BOX),
        );
        if selected {
            p.rect_filled(box_rect, BOX_R, colour::ACCENT);
            tick_at(p, box_rect.center(), colour::ON_ACCENT);
        } else {
            p.rect_stroke(
                box_rect,
                BOX_R,
                egui::Stroke::new(1.0, colour::LINE_STRONG),
                egui::StrokeKind::Inside,
            );
        }
    } else if selected {
        tick_at(
            p,
            egui::pos2(rect.right() - space::SM - TICK_W / 2.0, rect.center().y),
            colour::ACCENT,
        );
    }

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A `filter` and the menu it opens.
///
/// `None` is the leading "any" entry, so clearing a choice is the same gesture
/// as making one. The dashboard's filters and the project form's lead picker
/// are the same control — one popup implementation, so they cannot drift.
pub fn select(
    ui: &mut Ui,
    any: &str,
    options: &[(String, String)],
    slot: &mut Option<String>,
) -> Response {
    let shown = slot
        .as_deref()
        .and_then(|v| options.iter().find(|(value, _)| value == v))
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| any.to_owned());

    let response = filter(ui, &shown, slot.is_some(), true);
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .frame(menu_frame())
        .width(response.rect.width().max(MENU_MIN_W))
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if menu_row(ui, any, slot.is_none(), false).clicked() {
                *slot = None;
            }
            for (value, label) in options {
                if menu_row(ui, label, slot.as_deref() == Some(value), false).clicked() {
                    *slot = Some(value.clone());
                }
            }
        });
    response
}

/// A `filter` that opens a menu of checkable people.
///
/// `chosen` holds person ids in the order they were picked. The control's
/// label summarises the set: the placeholder when empty, the one name when
/// there is one, "Anmol +2" past that.
pub fn multi_select(
    ui: &mut Ui,
    placeholder: &str,
    options: &[(String, String)],
    chosen: &mut Vec<String>,
) -> Response {
    let name = |id: &String| {
        options
            .iter()
            .find(|(v, _)| v == id)
            .map(|(_, l)| l.as_str())
            .unwrap_or("?")
    };
    let shown = match chosen.as_slice() {
        [] => placeholder.to_owned(),
        [one] => name(one).to_owned(),
        [first, rest @ ..] => format!("{} +{}", name(first), rest.len()),
    };

    let response = filter(ui, &shown, !chosen.is_empty(), true);
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(menu_frame())
        .width(response.rect.width().max(MENU_MIN_W))
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (value, label) in options {
                let on = chosen.contains(value);
                if menu_row(ui, label, on, true).clicked() {
                    if on {
                        chosen.retain(|c| c != value);
                    } else {
                        chosen.push(value.clone());
                    }
                }
            }
            // Picking a set leaves the menu open, so it needs a way out that is
            // not "click somewhere harmless".
            ui.add_space(space::XXS);
            let (rule, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(rule, 0.0, colour::LINE);
            ui.add_space(space::XXS);
            if menu_row(ui, "Done", false, false).clicked() {
                ui.close();
            }
        });
    response
}

/// Every control on the filter bar is this tall, including the clear button,
/// so the bar reads as one strip rather than as controls of two heights. It is
/// `size::CONTROL` rather than a number of its own: a form row that mixes a
/// picker with a button was mixing 30 with 28.
pub const HEIGHT: f32 = size::CONTROL;

/// A row of filter controls that wraps onto a second line when the window is
/// narrow, rather than running its last control into whatever sits at the
/// right edge. The count of matching rows does not live here — it rides on
/// the table's own heading, where it cannot collide with anything.
pub fn toolbar(ui: &mut Ui, controls: impl FnOnce(&mut Ui)) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::SM);
        controls(ui);
    });
    ui.add_space(space::MD);
}

/// The filter bar's search box.
///
/// Sized and shaped like the controls beside it so the bar reads as one
/// strip. A magnifier would be a glyph Nunito does not have, so the hint text
/// carries the affordance instead — it says what it searches, which a
/// magnifier never did.
pub fn search(ui: &mut Ui, hint: &str, value: &mut String) -> Response {
    // Painted here rather than left to egui's own frame: a `TextEdit`'s
    // default fill, corner radius and focus stroke are none of the three the
    // filter controls beside it use, and the mismatch is the whole reason the
    // bar looked like two different toolbars pushed together.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SEARCH_W, HEIGHT), Sense::hover());
    let id = ui.make_persistent_id(("search", rect.left() as i32));
    let focused = ui.memory(|m| m.has_focus(id));

    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, colour::SURFACE);
    p.rect_stroke(
        rect,
        radius::SM as f32,
        egui::Stroke::new(1.0, if focused { colour::ACCENT } else { colour::LINE }),
        egui::StrokeKind::Inside,
    );

    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(space::MD, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    inner.add(
        egui::TextEdit::singleline(value)
            .id(id)
            .frame(egui::Frame::NONE)
            .desired_width(f32::INFINITY)
            .hint_text(RichText::new(hint).size(text::SMALL).color(colour::TEXT_FAINT))
            .font(egui::FontId::proportional(text::SMALL))
            .text_color(colour::TEXT)
            .margin(egui::Margin::ZERO),
    )
}

/// Wide enough for a few words of a task title. Fixed, because a search box
/// that grows with the window makes the controls beside it move.
pub const SEARCH_W: f32 = 200.0;

/// Whether `needle` appears in any of `haystacks`, case-insensitively.
///
/// Lives here so every list matches the same way: lowercase, substring, no
/// tokenising. A person typing "rat" wants "Lead Rating" and does not want to
/// think about it.
pub fn matches(needle: &str, haystacks: &[&str]) -> bool {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    haystacks.iter().any(|h| h.to_lowercase().contains(&needle))
}

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
