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

/// A glass card: a translucent lift off the canvas rather than an opaque
/// block, so the sidebar wash shows through it.
///
/// egui cannot blur what is behind a surface, so "glass" here is three cheap
/// cues that read the same way: a weak white fill, a brighter-than-usual
/// hairline doing the work of the missing shadow, and a one-pixel sheen along
/// the top edge. The sheen is what sells it — without it this is just a
/// translucent rectangle.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let out = egui::Frame::new()
        .fill(colour::GLASS)
        .stroke(egui::Stroke::new(1.0, colour::GLASS_LINE))
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(pad::CARD.0 as i8, pad::CARD.1 as i8))
        .show(ui, add);

    sheen(ui, out.response.rect);
    out
}

/// The highlight along a surface's top edge, inset so it follows the corner.
pub fn sheen(ui: &Ui, rect: egui::Rect) {
    let r = radius::MD as f32;
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + r, rect.top() + 0.5),
            egui::pos2(rect.right() - r, rect.top() + 0.5),
        ],
        egui::Stroke::new(1.0, colour::GLASS_SHEEN),
    );
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
pub fn link(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(
        RichText::new(label).size(text::SMALL).color(colour::ACCENT),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(egui::Stroke::NONE))
}

/// A labelled input. The label sits above in caption type, which keeps forms
/// scannable without a second column.
pub fn field(ui: &mut Ui, label: &str, value: &mut String, secret: bool) -> Response {
    ui.vertical(|ui| {
        caption(ui, label);
        ui.add_space(space::XXS);
        ui.add_sized(
            [ui.available_width(), size::CONTROL],
            egui::TextEdit::singleline(value)
                .password(secret)
                .margin(egui::Margin::symmetric(pad::INPUT.0 as i8, pad::INPUT.1 as i8)),
        )
    })
    .inner
}

// ---------------------------------------------------------------- states

/// Empty, loading and error are the three states every fetched thing has.
/// Naming them here means no view forgets one.
pub fn empty(ui: &mut Ui, message: &str) {
    ui.add_space(space::LG);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(message).size(text::SMALL).color(colour::TEXT_FAINT));
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
