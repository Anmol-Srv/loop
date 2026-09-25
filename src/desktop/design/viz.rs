//! Visualisations.
//!
//! Four shapes, all hand-drawn: egui has no chart primitive and a plotting
//! crate would be a dependency for four static figures. Each takes counts the
//! API already returns — none of them can be fed a number we do not have,
//! which is deliberate after a capacity chart was once drawn from nothing.

use chrono::{Datelike, NaiveDate};
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
        p.rect_filled(track, 2.0, colour::INSET);
        if fill > 0.0 {
            p.rect_filled(
                egui::Rect::from_min_size(
                    track.min,
                    Vec2::new(track.width() * fill.clamp(0.0, 1.0), track.height()),
                ),
                2.0,
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
            2.0,
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
    // Wrap rather than squeeze. Below the threshold a quarter of the page is
    // narrower than the donut and its legend, and the figures start clipping
    // — a card that cannot show its own legend is worse than a shorter row.
    let per_row = if ui.available_width() >= CARDS_FOUR_AT { cards.len() } else { 2 };

    let rows = cards.len().div_ceil(per_row);
    for (r, chunk) in cards.chunks_mut(per_row).enumerate() {
        // Each visual row agrees on its own height. One floor for the whole
        // block meant that once the cards wrapped two-up, the second row was
        // padded out to the first row's tallest card and read as half empty.
        let row_id = id.with(("row", per_row, r));
        let floor: f32 = ui.ctx().data(|d| d.get_temp(row_id)).unwrap_or(0.0);
        let mut tallest = 0.0_f32;

        // The last row of an odd split must not stretch its cards to fill the
        // page: `columns` divides by the count it is given, so it is given the
        // full count and the empty slots are simply not drawn.
        ui.columns(per_row, |cols| {
            for (col, card) in cols.iter_mut().zip(chunk.iter_mut()) {
                tallest = tallest.max(card(col, floor));
            }
        });
        if (tallest - floor).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(row_id, tallest));
            ui.ctx().request_repaint();
        }

        // Between the rows only. What follows the block is the caller's gap.
        if r + 1 < rows {
            ui.add_space(space::MD);
        }
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
/// Three states worth telling apart: idle is the surface, hover eases a step
/// lighter, and active is a raised neutral with full-strength semibold ink
/// and a stronger hairline. A filtered view should never be mistakable for an
/// empty one — but not by way of a blue box: the owner asked for the blue
/// selected border to go everywhere, and in a properties rail a control that
/// merely *has a value* was reading as focused.
///
/// `caret` is for the ones that open a menu. A toggle that draws a caret is
/// promising a popup it does not have.
pub fn filter(ui: &mut Ui, label: &str, active: bool, caret: bool) -> Response {
    let font = egui::FontId::new(
        text::SMALL,
        egui::FontFamily::Name(if active { theme::SEMIBOLD } else { theme::MEDIUM }.into()),
    );
    let ink = if active { colour::TEXT } else { colour::TEXT_2 };
    let galley = truncated(ui, label, font, ink, MAX_LABEL_W);
    let caret_w = if caret { CARET_COL } else { 0.0 };
    // `interact_size.x` is egui's own "narrowest an interactive widget may
    // be". Honouring it is what lets a form make every control in a row one
    // width by setting a single value in its scope, instead of each control
    // sizing to whatever its label happens to say.
    let width = (galley.size().x + space::MD * 2.0 + caret_w).max(ui.spacing().interact_size.x);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, HEIGHT), Sense::click());
    // Painted by hand, so it says what it is for the accessibility tree
    // itself: a screen reader, and the test harness that clicks by label,
    // both find it by its text.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), label)
    });
    let response = motion::operable(ui, response, radius::SM as f32);

    let fill = if active {
        colour::SURFACE_HOVER
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
            if active || response.hovered() {
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
            if active { colour::TEXT } else { colour::TEXT_MUTED },
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
fn menu_row(ui: &mut Ui, label: &str, selected: bool, check: bool, lit: bool) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), size::CONTROL), Sense::click());
    // Painted, so it names itself: a screen reader, and a test picking a row.
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    let response = motion::operable(ui, response, radius::SM as f32);

    // A tick box already says a multi-select row is on; a fill as well reads
    // as a second, blue selection. Single-choice rows keep the soft fill.
    let fill = if selected && !check {
        colour::ACCENT_SOFT
    } else {
        motion::hover_fill(
            ui,
            response.id.with("fill"),
            response.hovered() || lit,
            colour::TRANSPARENT,
            colour::SURFACE_HOVER,
        )
    };
    let ink = if selected || lit || response.hovered() {
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
    select_styled(ui, any, options, slot, false)
}

/// A `select` whose resting label is the thing's current value, not a
/// placeholder — a properties rail's Status or Priority, where `None` in the
/// slot means "unchanged", not "unset".
///
/// It draws as a set value, because it is one. Through plain `select` the
/// rail showed Status and Priority in placeholder ink beside dates and labels
/// in full ink, and five controls holding real values read as two kinds.
pub fn value_select(
    ui: &mut Ui,
    current: &str,
    options: &[(String, String)],
    slot: &mut Option<String>,
) -> Response {
    select_styled(ui, current, options, slot, true)
}

fn select_styled(
    ui: &mut Ui,
    any: &str,
    options: &[(String, String)],
    slot: &mut Option<String>,
    resting_is_value: bool,
) -> Response {
    let shown = slot
        .as_deref()
        .and_then(|v| options.iter().find(|(value, _)| value == v))
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| any.to_owned());

    let response = filter(ui, &shown, slot.is_some() || resting_is_value, true);
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .frame(menu_frame())
        .width(response.rect.width().max(MENU_MIN_W))
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if menu_row(ui, any, slot.is_none(), false, false).clicked() {
                *slot = None;
            }
            for (value, label) in options {
                if menu_row(ui, label, slot.as_deref() == Some(value), false, false).clicked() {
                    *slot = Some(value.clone());
                }
            }
        });
    response
}

/// One entry in a `tag_picker`: an id, what it is called, and its badge
/// colours (a hue for the fill, an ink for the name).
pub struct Tag<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub hue: Color32,
    pub ink: Color32,
}

/// What a `tag_picker` was asked to make: a name, and which of the
/// `swatches` it should wear.
pub struct NewTag {
    pub name: String,
    pub swatch: usize,
}

/// The picker popup's own memory: what is typed, which row the arrows are on,
/// the colour picked for a new one, and whether focus has been placed yet.
#[derive(Clone, Default)]
struct TagSearch {
    query: String,
    lit: usize,
    swatch: Option<usize>,
    focused: bool,
}

const TAG_MENU_W: f32 = 240.0;
const SWATCH: f32 = 14.0;

/// A searchable multi-select whose picks show as removable badges.
///
/// The control comes first and the picked badges follow it in the flow.
/// Its popup is a search box over the options: type to filter, arrows move,
/// Enter ticks or unticks, Backspace in an empty box takes the last badge
/// off, Escape closes. When nothing is named exactly what was typed, the last
/// row offers to make it, with a colour from `swatches` (`default_swatch`
/// until another is picked) — returned for the caller to create and then
/// add to `chosen`.
pub fn tag_picker(
    ui: &mut Ui,
    add_label: &str,
    options: &[Tag<'_>],
    chosen: &mut Vec<String>,
    swatches: &[(&str, Color32)],
    default_swatch: usize,
) -> Option<NewTag> {
    let mut create = None;
    let mut drop: Option<usize> = None;
    // Keyed to the picker rather than the control's auto id, so the popup
    // survives the row reflowing as badges come and go.
    let popup_id = ui.make_persistent_id(("tag-picker", add_label));
    let trigger = ui
        .horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::XS, space::XS);
            // The control first: badges after it can wrap without moving the
            // popup out from under the pointer.
            let trigger = filter(ui, add_label, false, true);
            for (i, id) in chosen.iter().enumerate() {
                if let Some(t) = options.iter().find(|t| t.id == id) {
                    if super::cards::removable_badge(ui, t.name, t.hue, t.ink) {
                        drop = Some(i);
                    }
                }
            }
            trigger
        })
        .inner;
    if let Some(i) = drop {
        chosen.remove(i);
    }

    let state_id = popup_id.with("search");
    let shown = egui::Popup::menu(&trigger)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(menu_frame())
        .width(TAG_MENU_W)
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut st: TagSearch = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
            let needle = st.query.trim().to_lowercase();
            let hits: Vec<&Tag<'_>> =
                options.iter().filter(|t| t.name.to_lowercase().contains(&needle)).collect();
            let offer = !needle.is_empty() && !options.iter().any(|t| t.name.to_lowercase() == needle);
            let rows = hits.len() + usize::from(offer);

            // Taken before the box sees them: a single-line edit would
            // otherwise hand the arrows to egui's focus walk, and Enter would
            // drop focus out of the box.
            let (down, up, enter, back, escape) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                    st.query.is_empty() && i.key_pressed(egui::Key::Backspace),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if rows > 0 {
                if down {
                    st.lit = (st.lit + 1) % rows;
                }
                if up {
                    st.lit = (st.lit + rows - 1) % rows;
                }
            }
            st.lit = st.lit.min(rows.saturating_sub(1));

            let search_id = state_id.with("box");
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
            let focused = ui.memory(|m| m.has_focus(search_id));
            ui.painter().rect_filled(rect, radius::SM as f32, colour::INSET);
            ui.painter().rect_stroke(
                rect,
                radius::SM as f32,
                egui::Stroke::new(1.0, if focused { colour::LINE_STRONG } else { colour::LINE }),
                egui::StrokeKind::Inside,
            );
            let mut inner = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(egui::vec2(space::SM, 0.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let before = st.query.clone();
            let edit = inner.add(
                egui::TextEdit::singleline(&mut st.query)
                    .id(search_id)
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .hint_text(RichText::new("Search or create\u{2026}").size(text::SMALL).color(colour::TEXT_FAINT))
                    .font(egui::FontId::proportional(text::SMALL))
                    .text_color(colour::TEXT)
                    .margin(egui::Margin::ZERO),
            );
            edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search labels"));
            if !st.focused {
                edit.request_focus();
                st.focused = true;
            }
            if st.query != before {
                st.lit = 0;
            }
            ui.add_space(space::XS);

            let toggle = |id: &str, chosen: &mut Vec<String>| {
                if let Some(i) = chosen.iter().position(|c| c == id) {
                    chosen.remove(i);
                } else {
                    chosen.push(id.to_owned());
                }
            };
            // A click on a row takes focus off the box; typing goes on there.
            let mut clicked = false;
            for (i, t) in hits.iter().enumerate() {
                let on = chosen.iter().any(|c| c == t.id);
                let row = menu_row(ui, t.name, on, true, st.lit == i).clicked();
                clicked |= row;
                if row || (enter && st.lit == i) {
                    toggle(t.id, chosen);
                }
            }
            if hits.is_empty() && !offer {
                ui.add_space(space::XS);
                ui.label(RichText::new("No labels yet \u{2014} type a name to make one.").size(text::SMALL).color(colour::TEXT_MUTED));
                ui.add_space(space::XS);
            }
            if offer {
                if !hits.is_empty() {
                    menu_rule(ui);
                }
                let name = st.query.trim().to_owned();
                let label = format!("Create label \u{201c}{name}\u{201d}");
                let swatch = st.swatch.unwrap_or(default_swatch).min(swatches.len().saturating_sub(1));
                if menu_row(ui, &label, false, false, st.lit == hits.len()).clicked() || (enter && st.lit == hits.len()) {
                    create = Some(NewTag { name, swatch });
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    ui.add_space(space::SM);
                    ui.label(RichText::new("Colour").size(text::CAPTION).color(colour::TEXT_MUTED));
                    ui.add_space(space::XS);
                    for (i, (name, hue)) in swatches.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(SWATCH + space::XS), Sense::click());
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, i == swatch, *name)
                        });
                        let resp = motion::operable(ui, resp, radius::PILL as f32);
                        let p = ui.painter();
                        p.circle_filled(r.center(), SWATCH / 2.0 - 1.0, *hue);
                        if i == swatch {
                            p.circle_stroke(r.center(), SWATCH / 2.0 + 1.5, egui::Stroke::new(1.5, colour::TEXT));
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.on_hover_text(*name).clicked() {
                            st.swatch = Some(i);
                        }
                    }
                });
                ui.add_space(space::XS);
            }

            if back {
                chosen.pop();
            }
            if create.is_some() {
                st = TagSearch { focused: true, ..TagSearch::default() };
            }
            if enter || clicked || create.is_some() {
                edit.request_focus();
            }
            ui.data_mut(|d| d.insert_temp(state_id, st));
            if escape {
                ui.close();
            }
        });
    if shown.is_none() {
        ui.data_mut(|d| d.remove::<TagSearch>(state_id));
    }
    create
}

/// A hairline between groups of menu rows.
pub fn menu_rule(ui: &mut Ui) {
    ui.add_space(space::XXS);
    let (rule, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rule, 0.0, colour::LINE);
    ui.add_space(space::XXS);
}

/// An action menu's width. The longest item, "Hand off to" and an agent's
/// name, truncates rather than widening the menu from row to row.
const ACTION_MENU_W: f32 = 220.0;

/// "More actions": a secondary button with three painted dots, opening the
/// same menu a right-click on the thing's row opens. The dots are painted —
/// the text face has no ellipsis on the midline, and the icon font's is a
/// hairline at this size.
pub fn more(ui: &mut Ui, items: impl FnOnce(&mut Ui)) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(HEIGHT), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More actions"));
    let response = motion::operable(ui, response, radius::SM as f32);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    // `w::secondary`'s three states, so it sits beside Edit as one of a set.
    let hot = response.hovered() || response.has_focus() || open;
    let fill = if open || response.is_pointer_button_down_on() {
        colour::GLASS_ACTIVE
    } else if hot {
        colour::GLASS_HOVER
    } else {
        colour::GLASS
    };
    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, fill);
    p.rect_stroke(
        rect,
        radius::SM as f32,
        egui::Stroke::new(1.0, if hot { colour::EDGE_HI_HOVER } else { colour::EDGE_MID }),
        egui::StrokeKind::Inside,
    );
    for dx in [-5.0, 0.0, 5.0] {
        p.circle_filled(rect.center() + Vec2::new(dx, 0.0), 1.6, colour::TEXT);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let response =
        if open { response } else { response.on_hover_text("More actions \u{2014} or right-click any row") };

    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(space::XS)
        .frame(menu_frame())
        .width(ACTION_MENU_W)
        .show(|ui| menu_body(ui, items));
    response
}

/// The action menu, opened at the pointer by a right-click on `response`.
/// True while it is open, so a row can stay lit under it.
pub fn context_menu(response: &Response, items: impl FnOnce(&mut Ui)) -> bool {
    egui::Popup::context_menu(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(menu_frame())
        .width(ACTION_MENU_W)
        .show(|ui| menu_body(ui, items))
        .is_some()
}

fn menu_body(ui: &mut Ui, items: impl FnOnce(&mut Ui)) {
    ui.spacing_mut().item_spacing.y = 0.0;
    // Arrow keys walk focus spatially; without this they walk off the bottom
    // of the menu into the page under it.
    ui.ctx().memory_mut(|m| m.set_modal_layer(ui.layer_id()));
    items(ui);
}

/// One action in a menu. True when it was picked, which closes the menu.
///
/// `why` disables it and says why on hover: an item that is missing teaches
/// nobody that it exists, one that is greyed with a reason does. `danger` is
/// for the one that cannot be undone.
pub fn menu_item(ui: &mut Ui, label: &str, danger: bool, why: Option<&str>) -> bool {
    let response = action_row(ui, label, danger, why, false);
    if response.clicked() {
        ui.close();
        return true;
    }
    false
}

/// A row that opens `items` to its side, with a painted chevron — Nunito
/// has no arrow glyph.
pub fn submenu(ui: &mut Ui, label: &str, why: Option<&str>, items: impl FnOnce(&mut Ui)) {
    let response = action_row(ui, label, false, why, true);
    if why.is_none() {
        // egui draws a submenu in its stock menu frame; the theme's window
        // fill and stroke are the app's, and this is its radius.
        ui.scope(|ui| {
            ui.visuals_mut().menu_corner_radius = radius::MD.into();
            egui::containers::menu::SubMenu::new().show(ui, &response, |ui| {
                ui.set_width(MENU_MIN_W);
                menu_body(ui, items);
            });
        });
    }
}

/// A submenu's pick-one row: ticked when it is the current value.
pub fn menu_choice(ui: &mut Ui, label: &str, current: bool) -> bool {
    if menu_row(ui, label, current, false, false).clicked() {
        ui.close();
        return true;
    }
    false
}

fn action_row(ui: &mut Ui, label: &str, danger: bool, why: Option<&str>, chevron: bool) -> Response {
    let enabled = why.is_none();
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), size::CONTROL),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let response = if enabled { motion::operable(ui, response, radius::SM as f32) } else { response };
    // Focus is outside a menu when it opens — on the row it came from, or
    // nowhere; the first Down arrow lands on its first live row, and egui
    // walks focus from there.
    let focus_outside = ui.memory(|m| m.focused()).is_none_or(|id| {
        ui.ctx().read_response(id).is_none_or(|r| r.layer_id != ui.layer_id())
    });
    if enabled && focus_outside && ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
        let claim = ui.id().with("arrow");
        let frame = ui.ctx().cumulative_pass_nr();
        if ui.data(|d| d.get_temp::<u64>(claim)) != Some(frame) {
            ui.data_mut(|d| d.insert_temp(claim, frame));
            response.request_focus();
            // Or egui walks this same press on, to the second row.
            ui.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        }
    }

    let hot = enabled && (response.hovered() || response.has_focus());
    let fill = motion::hover_fill(ui, response.id.with("fill"), hot, colour::TRANSPARENT, colour::SURFACE_HOVER);
    let ink = match (enabled, danger) {
        (false, _) => colour::TEXT_DISABLED,
        (true, true) => colour::DANGER,
        (true, false) if hot => colour::TEXT,
        _ => colour::TEXT_2,
    };
    let right = if chevron { space::SM + CHEVRON * 2.0 + space::SM } else { space::SM };
    let left = rect.left() + space::SM;
    let galley =
        truncated(ui, label, egui::FontId::proportional(text::BODY), ink, (rect.right() - right - left).max(1.0));
    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, fill);
    p.galley(egui::pos2(left, rect.center().y - galley.size().y / 2.0), galley, ink);
    if chevron {
        let x = rect.right() - space::SM - CHEVRON;
        let stroke = egui::Stroke::new(1.5, if enabled { colour::TEXT_MUTED } else { colour::TEXT_DISABLED });
        p.line_segment([egui::pos2(x - CHEVRON / 2.0, rect.center().y - CHEVRON), egui::pos2(x + CHEVRON / 2.0, rect.center().y)], stroke);
        p.line_segment([egui::pos2(x + CHEVRON / 2.0, rect.center().y), egui::pos2(x - CHEVRON / 2.0, rect.center().y + CHEVRON)], stroke);
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    match why {
        Some(why) => response.on_hover_text(why),
        None => response,
    }
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

/// A date, picked from a month.
///
/// The control is a `filter` like every other picker, so a row of them is one
/// height and one vocabulary; the popup is the menus' own frame. egui_extras
/// ships a date picker, but it is year/month/day combo boxes in egui's stock
/// dress — a second visual language in the middle of a form.
///
/// Returns the control's response, marked changed on the frame a date is
/// picked or cleared, so a caller that saves on change can ask `changed()`.
pub fn date_picker(ui: &mut Ui, placeholder: &str, value: &mut Option<NaiveDate>) -> Response {
    let shown = value.map(|d| d.format("%-d %b %Y").to_string());
    let mut response = filter(ui, shown.as_deref().unwrap_or(placeholder), value.is_some(), true);

    // Which month the popup is showing. Reset to the value (or today) each
    // time the control is opened, so it never opens on a month you paged to
    // last time and forgot about.
    let month_id = response.id.with("month");
    let today = chrono::Local::now().date_naive();
    if response.clicked() {
        let anchor = value.unwrap_or(today);
        ui.ctx().data_mut(|d| d.insert_temp(month_id, first_of(anchor)));
    }
    let mut month: NaiveDate =
        ui.ctx().data(|d| d.get_temp(month_id)).unwrap_or_else(|| first_of(value.unwrap_or(today)));

    let mut picked: Option<Option<NaiveDate>> = None;
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(menu_frame())
        .width(DAY * 7.0)
        .show(|ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            // Month and year, with a painted chevron either side.
            ui.horizontal(|ui| {
                if chevron(ui, false).clicked() {
                    month = shift_month(month, -1);
                }
                let (title, _) =
                    ui.allocate_exact_size(Vec2::new(DAY * 5.0, DAY), Sense::hover());
                ui.painter().text(
                    title.center(),
                    egui::Align2::CENTER_CENTER,
                    month.format("%B %Y").to_string(),
                    egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::SEMIBOLD.into())),
                    colour::TEXT,
                );
                if chevron(ui, true).clicked() {
                    month = shift_month(month, 1);
                }
            });

            // Weekday initials, Monday first.
            ui.horizontal(|ui| {
                for initial in ["M", "T", "W", "T", "F", "S", "S"] {
                    let (cell, _) = ui.allocate_exact_size(Vec2::splat(DAY), Sense::hover());
                    ui.painter().text(
                        cell.center(),
                        egui::Align2::CENTER_CENTER,
                        initial,
                        egui::FontId::proportional(text::CAPTION),
                        colour::TEXT_MUTED,
                    );
                }
            });

            // Six weeks always, so the popup does not change height as you
            // page between a four-row February and a six-row month.
            let lead = month.weekday().num_days_from_monday() as i64;
            let start = month - chrono::Duration::days(lead);
            for week in 0..6 {
                ui.horizontal(|ui| {
                    for d in 0..7 {
                        let day = start + chrono::Duration::days(week * 7 + d);
                        let state = DayState {
                            in_month: day.month() == month.month(),
                            selected: *value == Some(day),
                            today: day == today,
                        };
                        if day_cell(ui, day, state).clicked() {
                            picked = Some(Some(day));
                        }
                    }
                });
            }

            ui.add_space(space::XXS);
            let (rule, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(rule, 0.0, colour::LINE);
            ui.add_space(space::XXS);
            if menu_row(ui, "Today", false, false, false).clicked() {
                picked = Some(Some(today));
            }
            if value.is_some() && menu_row(ui, "Clear", false, false, false).clicked() {
                picked = Some(None);
            }
            if picked.is_some() {
                ui.close();
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(month_id, month));

    if let Some(new) = picked {
        if new != *value {
            *value = new;
            response.mark_changed();
        }
    }
    response
}

/// One day cell, and the popup's column width: a day is exactly as wide as a
/// control is tall, so the grid is square and a week is seven controls wide.
const DAY: f32 = size::CONTROL;

#[derive(Clone, Copy)]
struct DayState {
    in_month: bool,
    selected: bool,
    today: bool,
}

fn day_cell(ui: &mut Ui, day: NaiveDate, state: DayState) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(DAY), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, day.format("%-d %B %Y"))
    });
    let response = motion::operable(ui, response, radius::SM as f32);
    let inner = rect.shrink(1.0);
    let p = ui.painter();

    // The chosen day is filled, not outlined: the owner asked for no blue
    // selection borders, and a fill says "this one" without borrowing the
    // focus ring's shape.
    let ink = if state.selected {
        p.rect_filled(inner, radius::SM as f32, colour::ACCENT);
        colour::ON_ACCENT
    } else {
        if response.hovered() {
            p.rect_filled(inner, radius::SM as f32, colour::SURFACE_HOVER);
        }
        if state.today {
            p.rect_stroke(
                inner,
                radius::SM as f32,
                egui::Stroke::new(1.0, colour::LINE_STRONG),
                egui::StrokeKind::Inside,
            );
        }
        if state.in_month { colour::TEXT_2 } else { colour::TEXT_FAINT }
    };
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        day.day().to_string(),
        egui::FontId::proportional(text::SMALL),
        ink,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.on_hover_text(day.format("%A %-d %B %Y").to_string())
}

/// A month-paging button: a chevron drawn as two strokes, since Nunito has no
/// arrow to borrow.
fn chevron(ui: &mut Ui, forward: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(DAY), Sense::click());
    let response = motion::operable(ui, response, radius::SM as f32);
    if response.hovered() {
        ui.painter().rect_filled(rect.shrink(1.0), radius::SM as f32, colour::SURFACE_HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let c = rect.center();
    let (tip, back) = if forward { (CHEVRON, -CHEVRON) } else { (-CHEVRON, CHEVRON) };
    let stroke = egui::Stroke::new(TICK_STROKE, colour::TEXT_2);
    ui.painter().line_segment([egui::pos2(c.x + back / 2.0, c.y - CHEVRON), egui::pos2(c.x + tip / 2.0, c.y)], stroke);
    ui.painter().line_segment([egui::pos2(c.x + tip / 2.0, c.y), egui::pos2(c.x + back / 2.0, c.y + CHEVRON)], stroke);
    response.on_hover_text(if forward { "Next month" } else { "Previous month" })
}

/// Half the chevron's height; small enough to read as a glyph, not a shape.
const CHEVRON: f32 = 4.0;

fn first_of(d: NaiveDate) -> NaiveDate {
    d.with_day(1).expect("every month has a first")
}

fn shift_month(first: NaiveDate, by: i32) -> NaiveDate {
    let months = first.year() * 12 + first.month0() as i32 + by;
    NaiveDate::from_ymd_opt(months.div_euclid(12), months.rem_euclid(12) as u32 + 1, 1)
        .expect("the first of a month always exists")
}

#[cfg(test)]
mod date_tests {
    use super::*;

    #[test]
    fn months_page_across_years() {
        let jan = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(shift_month(jan, -1), NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
        assert_eq!(shift_month(jan, 12), NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
        let dec = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
        assert_eq!(shift_month(dec, 1), NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
    }
}
