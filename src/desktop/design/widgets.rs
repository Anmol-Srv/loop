//! The component layer.
//!
//! egui has no shadcn, so this is ours. Every widget here takes tokens and
//! nothing else — no view should be reaching for a raw colour or a bare number.

use egui::{Color32, Response, RichText, Sense, Ui, Vec2};

use super::tokens::{colour, pad, radius, size, space, text};

// ---------------------------------------------------------------- text

pub fn title(ui: &mut Ui, s: &str) {
    ui.label(RichText::new(s).size(text::TITLE).color(colour::TEXT));
}

pub fn heading(ui: &mut Ui, s: &str) {
    ui.label(RichText::new(s).size(text::HEADING).color(colour::TEXT));
}

pub fn body(ui: &mut Ui, s: &str) {
    ui.label(RichText::new(s).size(text::BODY).color(colour::TEXT));
}

pub fn muted(ui: &mut Ui, s: &str) {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT_MUTED));
}

/// A monospaced caption: a server URL, an ordinal, anything where the glyphs
/// should line up. Both the sign-in screen and the board hand-painted this.
pub fn mono_caption(ui: &mut Ui, s: &str) {
    ui.label(
        RichText::new(s)
            .monospace()
            .size(text::CAPTION)
            .color(colour::TEXT_FAINT),
    );
}

pub fn caption(ui: &mut Ui, s: &str) {
    ui.label(RichText::new(s).size(text::CAPTION).color(colour::TEXT_MUTED));
}

/// A shortened id, monospaced, full value on hover. Full uuids are noise on
/// screen but people still need to copy them.
pub fn id(ui: &mut Ui, value: &str) -> Response {
    let short = value.get(..8).unwrap_or(value);
    ui.add(egui::Label::new(
        RichText::new(short).monospace().size(text::CAPTION).color(colour::TEXT_FAINT),
    ))
    .on_hover_text(value)
}

// ---------------------------------------------------------------- status

/// A filled dot. The densest possible way to show state — used in list rows
/// where a pill would be too loud repeated forty times.
pub fn dot(ui: &mut Ui, c: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, c);
}

/// A tinted pill. For one-off state, not for every row.
pub fn pill(ui: &mut Ui, label: &str, c: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), egui::FontId::proportional(text::CAPTION), c);
    let pad = Vec2::new(space::SM, 2.5);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::hover());
    ui.painter()
        .rect_filled(rect, radius::SM as f32, c.gamma_multiply(0.10));
    ui.painter().galley(rect.min + pad, galley, c);
}

// ---------------------------------------------------------------- surfaces

/// The glass edge: three strokes, brightest on top.
///
/// Real glass catches light along its upper rim and loses it toward the
/// bottom. One flat border cannot say that, which is why a translucent
/// rectangle never quite reads as glass. bencho.dev encodes the same idea as
/// `--edge-hi .75 / --edge-far .42 / --edge-lo .18`; this is that, adapted for
/// a dark surface where the light is weaker.
pub fn glass_edge(ui: &Ui, rect: egui::Rect, r: f32, hovered: bool) {
    let (hi, mid, lo) = if hovered {
        (colour::EDGE_HI_HOVER, colour::EDGE_MID_HOVER, colour::EDGE_MID)
    } else {
        (colour::EDGE_HI, colour::EDGE_MID, colour::EDGE_LO)
    };
    let p = ui.painter();

    // Sides and bottom carry the dim tone; the rounded rect gives the corners.
    p.rect_stroke(rect, r, egui::Stroke::new(1.0, mid), egui::StrokeKind::Inside);
    p.line_segment(
        [
            egui::pos2(rect.left() + r, rect.bottom() - 0.5),
            egui::pos2(rect.right() - r, rect.bottom() - 0.5),
        ],
        egui::Stroke::new(1.0, lo),
    );
    // The top rim, catching the light. This is the line that sells it.
    p.line_segment(
        [
            egui::pos2(rect.left() + r, rect.top() + 0.5),
            egui::pos2(rect.right() - r, rect.top() + 0.5),
        ],
        egui::Stroke::new(1.0, hi),
    );
}

/// A glass card. Translucent, so the sidebar wash shows through, with the
/// gradient rim above doing the work a shadow would do elsewhere.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    glass_panel(ui, false, add)
}

/// A glass card that responds to the pointer. Hover lifts the fill a little
/// and brightens the rim — felt, not announced.
pub fn glass_panel<R>(
    ui: &mut Ui,
    hovered: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    let fill = if hovered { colour::GLASS_HOVER } else { colour::GLASS };
    let out = egui::Frame::new()
        .fill(fill)
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(pad::CARD.0 as i8, pad::CARD.1 as i8))
        .show(ui, add);

    glass_edge(ui, out.response.rect, radius::MD as f32, hovered);
    out
}

/// A card whose whole surface is clickable, hover included.
pub fn card_button<R>(
    ui: &mut Ui,
    add: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    let id = ui.next_auto_id();
    // Hover state comes from the previous frame: we must know it before
    // painting, but the rect only exists after. One frame of lag is invisible.
    let hovered = ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));

    let out = glass_panel(ui, hovered, add);
    let response = out.response.interact(egui::Sense::click());

    ui.ctx().data_mut(|d| d.insert_temp(id, response.hovered()));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (response, out.inner)
}

/// A vertical gradient. egui has no gradient primitive, so this is a two-
/// triangle mesh with per-vertex colour.
pub fn gradient_v(ui: &Ui, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// A hairline. Prefer whitespace; reach for this only when a boundary is
/// genuinely ambiguous without it.
pub fn rule(ui: &mut Ui) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, colour::LINE_SOFT));
}

/// A row with a coloured spine down its left edge — used to mark a row as
/// delegated to an agent. Restored as a widget after the board port dropped it
/// for being hand-painted geometry: the signal was worth keeping, the literals
/// were not. Delegated work should be visible in a scan, not read for.
pub fn row_marked<R>(ui: &mut Ui, spine: Color32, add: impl FnOnce(&mut Ui) -> R) -> Response {
    let top = ui.cursor().top();
    let response = row(ui, add);
    let rect = egui::Rect::from_min_size(
        egui::pos2(response.rect.left(), top),
        egui::vec2(2.0, response.rect.height()),
    );
    ui.painter().rect_filled(rect, radius::SM as f32, spine);
    response
}

/// A clickable list row of fixed height, so columns align down the page.
/// Hover tints the background rather than moving anything.
pub fn row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> Response {
    let w = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, size::ROW), Sense::click());

    if response.hovered() {
        ui.painter().rect_filled(rect, radius::SM as f32, colour::GLASS_HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(space::SM, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    add(&mut inner);

    response
}

// ---------------------------------------------------------------- controls

/// The one filled button on a screen. More than one and the eye has nowhere
/// to land.
pub fn primary(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let text = RichText::new(label).size(text::BODY).color(colour::ON_ACCENT);
    let button = egui::Button::new(text)
        .fill(if enabled { colour::ACCENT } else { colour::ACCENT.gamma_multiply(0.4) })
        .corner_radius(radius::SM)
        .min_size(Vec2::new(0.0, size::CONTROL));
    ui.add_enabled(enabled, button)
}

pub fn secondary(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let button = egui::Button::new(RichText::new(label).size(text::BODY).color(colour::TEXT))
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::LINE))
        .corner_radius(radius::SM)
        .min_size(Vec2::new(0.0, size::CONTROL));
    ui.add_enabled(enabled, button)
}

/// Destructive, and deliberately not a filled red block — outlined, so it
/// reads as available rather than urged.
pub fn danger(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let button = egui::Button::new(RichText::new(label).size(text::BODY).color(colour::DANGER))
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::DANGER.gamma_multiply(0.5)))
        .corner_radius(radius::SM)
        .min_size(Vec2::new(0.0, size::CONTROL));
    ui.add_enabled(enabled, button)
}

/// Text-only, for tertiary navigation like "back".
///
/// Deliberately not accent-coloured. The accent is a fill colour here — pink
/// text on a dark ground reads as decoration and competes with the status
/// colours that actually carry meaning.
pub fn link(ui: &mut Ui, label: &str) -> Response {
    let response = ui.add(
        egui::Button::new(RichText::new(label).size(text::SMALL).color(colour::TEXT_MUTED))
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE),
    );
    if response.hovered() {
        // Brighten rather than recolour.
        ui.painter().text(
            response.rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(text::SMALL),
            colour::TEXT,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A labelled input. The label sits above in caption type, which keeps forms
/// scannable without a second column.
///
/// `hint` is the placeholder shown while empty; pass `""` for none. It is a
/// required argument rather than a builder because the first port of this
/// widget silently dropped every placeholder in the sign-in form — an example
/// address and a sample setup code — and nobody noticed until the form was
/// read back. Making it explicit means forgetting it is a decision.
pub fn field(ui: &mut Ui, label: &str, value: &mut String, secret: bool, hint: &str) -> Response {
    ui.vertical(|ui| {
        caption(ui, label);
        ui.add_space(space::XXS);
        ui.add_sized(
            [ui.available_width(), size::CONTROL],
            egui::TextEdit::singleline(value)
                .password(secret)
                .hint_text(RichText::new(hint).size(text::BODY).color(colour::TEXT_DISABLED))
                .margin(egui::Margin::symmetric(pad::INPUT.0 as i8, pad::INPUT.1 as i8)),
        )
    })
    .inner
}

// ---------------------------------------------------------------- states

/// Empty, loading and error are the three states every fetched thing has.
/// Naming them here means no view forgets one.
/// `detail` is a quieter second line; pass `""` for none. Empty states almost
/// always want to say what the thing is as well as that there is none of it.
pub fn empty(ui: &mut Ui, message: &str, detail: &str) {
    ui.add_space(space::LG);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(message).size(text::SMALL).color(colour::TEXT_FAINT));
        if !detail.is_empty() {
            ui.add_space(space::XXS);
            ui.label(
                RichText::new(detail)
                    .size(text::CAPTION)
                    .color(colour::TEXT_DISABLED),
            );
        }
    });
    ui.add_space(space::LG);
}

pub fn loading(ui: &mut Ui, what: &str) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(text::BODY));
        ui.add_space(space::XS);
        muted(ui, what);
    });
}

pub fn error(ui: &mut Ui, message: &str) {
    egui::Frame::new()
        .fill(colour::DANGER.gamma_multiply(0.06))
        .stroke(egui::Stroke::new(1.0, colour::DANGER.gamma_multiply(0.30)))
        .corner_radius(radius::SM)
        .inner_margin(egui::Margin::symmetric(pad::BUTTON.0 as i8, pad::BUTTON.1 as i8))
        .show(ui, |ui| {
            ui.label(RichText::new(message).size(text::SMALL).color(colour::DANGER));
        });
}
