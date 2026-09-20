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

/// The discipline column. Fixed width and monospaced so the titles beside it
/// line up down the page; quiet, because it appears on every single row.
pub fn discipline(ui: &mut Ui, value: &str) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(super::tokens::DISCIPLINE_W, size::ROW),
        Sense::hover(),
    );
    if value.is_empty() {
        return;
    }
    ui.painter().text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        value,
        egui::FontId::monospace(text::CAPTION),
        super::tokens::discipline_colour(value),
    );
}

/// "waiting on X" — what a blocked row says instead of a status.
///
/// A blocked task is not in a *state*, it is in a *relationship*, and naming
/// the thing it waits on is what makes the row actionable. A "blocked" pill
/// tells you to go and find out; this tells you.
pub fn blocked_by(ui: &mut Ui, what: &str) {
    ui.label(
        RichText::new(format!("\u{2933} waiting on \u{201c}{what}\u{201d}"))
            .size(text::SMALL)
            .color(colour::TEXT_FAINT),
    );
}

/// A thin progress track. Used for phase and discipline completion.
pub fn progress(ui: &mut Ui, fraction: f32, width: f32, tint: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 4.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, colour::INSET);
    if fraction > 0.0 {
        let filled = egui::Rect::from_min_size(
            rect.min,
            Vec2::new(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
        );
        p.rect_filled(filled, 2.0, tint);
    }
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

/// A glass card holding a list of `row`s. Tighter than `card`, because rows
/// already pad themselves and a prose card's margin double-indents them. Both
/// page agents hit this gap independently, which is how it earned a token.
pub fn card_list<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let out = egui::Frame::new()
        .fill(colour::GLASS)
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(pad::LIST.0 as i8, pad::LIST.1 as i8))
        .show(ui, add);
    glass_edge(ui, out.response.rect, radius::MD as f32, false);
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

// ---------------------------------------------------------------- buttons
//
// One implementation, five variants. They all allocate, read their own hover
// and press state, then paint ONCE.
//
// The previous `link` painted its label, then painted it again in a brighter
// colour on hover — two overlapping draws that read as doubled, smeared text.
// Deciding the colour before painting, rather than layering a second pass on
// top, is the whole fix and the reason this is one function.

/// How loud a button is. Exactly one `Primary` per screen: if two things are
/// filled, the eye has nowhere to land.
#[derive(Clone, Copy, PartialEq)]
pub enum Emphasis {
    /// Filled accent. The single most likely action.
    Primary,
    /// Outlined. Everything else that is a real action.
    Secondary,
    /// Outlined in danger. Destructive, available but not urged.
    Danger,
    /// No border, no fill. Tertiary navigation.
    Ghost,
    /// Text only, small. "Back", "Sign out".
    Link,
}

/// The button. Prefer the named wrappers below; reach for this when you need
/// an icon or a non-default size.
pub fn button(ui: &mut Ui, label: &str, emphasis: Emphasis, enabled: bool) -> Response {
    button_with(ui, None, label, emphasis, enabled)
}

/// A button with a leading icon glyph.
pub fn icon_button(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    emphasis: Emphasis,
    enabled: bool,
) -> Response {
    button_with(ui, Some(icon), label, emphasis, enabled)
}

fn button_with(
    ui: &mut Ui,
    icon: Option<&str>,
    label: &str,
    emphasis: Emphasis,
    enabled: bool,
) -> Response {
    let small = emphasis == Emphasis::Link;
    let font = egui::FontId::proportional(if small { text::SMALL } else { text::BODY });

    let galley = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), colour::TEXT);
    let icon_w = if icon.is_some() { 18.0 } else { 0.0 };
    let (px, _) = if small { (space::XS, 0.0) } else { pad::BUTTON };
    let height = if small { size::CONTROL - space::SM } else { size::CONTROL };
    let width = galley.size().x + icon_w + px * 2.0;

    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, height),
        if enabled { Sense::click() } else { Sense::hover() },
    );

    let hovered = enabled && response.hovered();
    let pressed = enabled && response.is_pointer_button_down_on();

    // Every colour decided here, before a single draw call.
    let (fill, stroke, fg) = match (emphasis, enabled) {
        (_, false) => (Color32::TRANSPARENT, Color32::TRANSPARENT, colour::TEXT_DISABLED),
        (Emphasis::Primary, _) => {
            let f = if pressed {
                colour::ACCENT
            } else if hovered {
                colour::ACCENT_HOVER
            } else {
                colour::ACCENT
            };
            (f, Color32::TRANSPARENT, colour::ON_ACCENT)
        }
        (Emphasis::Secondary, _) => (
            if pressed {
                colour::GLASS_ACTIVE
            } else if hovered {
                colour::GLASS_HOVER
            } else {
                colour::GLASS
            },
            if hovered { colour::EDGE_HI_HOVER } else { colour::EDGE_MID },
            colour::TEXT,
        ),
        (Emphasis::Danger, _) => (
            if hovered { colour::DANGER.gamma_multiply(0.14) } else { Color32::TRANSPARENT },
            colour::DANGER.gamma_multiply(if hovered { 0.75 } else { 0.42 }),
            colour::DANGER,
        ),
        (Emphasis::Ghost, _) => (
            if hovered { colour::GLASS_HOVER } else { Color32::TRANSPARENT },
            Color32::TRANSPARENT,
            if hovered { colour::TEXT } else { colour::TEXT_MUTED },
        ),
        (Emphasis::Link, _) => (
            Color32::TRANSPARENT,
            Color32::TRANSPARENT,
            if hovered { colour::TEXT } else { colour::TEXT_MUTED },
        ),
    };

    let p = ui.painter();
    if fill != Color32::TRANSPARENT {
        p.rect_filled(rect, radius::SM as f32, fill);
    }
    if stroke != Color32::TRANSPARENT {
        p.rect_stroke(
            rect,
            radius::SM as f32,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
    }

    let mut x = rect.left() + px;
    if let Some(glyph) = icon {
        p.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            glyph,
            egui::FontId::proportional(text::HEADING),
            fg,
        );
        x += icon_w;
    }
    // One draw, one colour. No second pass on hover.
    p.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        fg,
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

pub fn primary(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    button(ui, label, Emphasis::Primary, enabled)
}

pub fn secondary(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    button(ui, label, Emphasis::Secondary, enabled)
}

pub fn danger(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    button(ui, label, Emphasis::Danger, enabled)
}

/// No border, no fill until hovered. For toolbar-ish actions.
pub fn ghost(ui: &mut Ui, label: &str) -> Response {
    button(ui, label, Emphasis::Ghost, true)
}

/// Text only. Brightens on hover; it does not recolour and it does not
/// redraw itself on top of itself.
pub fn link(ui: &mut Ui, label: &str) -> Response {
    button(ui, label, Emphasis::Link, true)
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
