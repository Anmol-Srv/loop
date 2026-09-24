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

/// A task title in a row: takes the flexible slot and truncates rather than
/// running through whatever follows it.
///
/// egui's default wrap mode in a horizontal layout is `Extend`, so a plain
/// label lets a long title overrun its trailing column and leaves the
/// right-aligned group with no width to lay out in. Every list row in the app
/// had this; the mockup's `.t{overflow:hidden;text-overflow:ellipsis}` was the
/// contract and nothing implemented it.
pub fn row_title(ui: &mut Ui, s: &str, reserve_trailing: f32) {
    let available = (ui.available_width() - reserve_trailing).max(size::ROW);
    ui.add_sized(
        [available, size::ROW],
        egui::Label::new(RichText::new(s).size(text::BODY).color(colour::TEXT))
            .truncate()
            .halign(egui::Align::LEFT),
    );
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

/// One line, ellipsised rather than wrapped. A label that grows a second line
/// breaks the height every control on its row agreed to.
pub fn truncated(
    ui: &Ui,
    label: &str,
    font: egui::FontId,
    ink: Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), font, ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
    ui.painter().layout_job(job)
}

/// A filled dot. The densest possible way to show state — used in list rows
/// where a pill would be too loud repeated forty times.
pub fn dot(ui: &mut Ui, c: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size::DOT), Sense::hover());
    ui.painter().circle_filled(rect.center(), size::DOT / 2.0 - 0.5, c);
}

/// The agent mark: a small robot head, painted — the UI font has no glyph for
/// it, and one shape everywhere is what lets "an agent holds this" read at a
/// glance in a table, a rail and a thread alike.
pub fn agent_mark(ui: &mut Ui, side: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    paint_agent_mark(ui.painter(), rect, colour::AGENT);
    response
}

pub fn paint_agent_mark(p: &egui::Painter, rect: egui::Rect, ink: Color32) {
    let s = rect.width();
    let stroke = egui::Stroke::new((s / 11.0).max(1.0), ink);
    // The head sits low in the square, leaving the top for the antenna.
    let head = egui::Rect::from_min_max(
        egui::pos2(rect.left() + s * 0.12, rect.top() + s * 0.34),
        egui::pos2(rect.right() - s * 0.12, rect.bottom() - s * 0.06),
    );
    p.rect_stroke(head, s * 0.16, stroke, egui::StrokeKind::Inside);
    let eye_y = head.center().y;
    for dx in [-0.17, 0.17] {
        p.circle_filled(egui::pos2(head.center().x + s * dx, eye_y), s * 0.075, ink);
    }
    let top = egui::pos2(head.center().x, rect.top() + s * 0.12);
    p.line_segment([top, egui::pos2(top.x, head.top())], stroke);
    p.circle_filled(top, s * 0.08, ink);
}

/// A tinted pill. For one-off state, not for every row.
pub fn pill(ui: &mut Ui, label: &str, c: Color32) {
    let pad = Vec2::new(space::SM, space::XXS);
    let galley = truncated(
        ui,
        label,
        egui::FontId::proportional(text::CAPTION),
        c,
        (ui.available_width() - pad.x * 2.0).max(size::DOT * 4.0),
    );
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
    let ink = super::tokens::discipline_colour(value);
    let galley = truncated(ui, value, egui::FontId::monospace(text::CAPTION), ink, rect.width());
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
}

/// "waiting on X" — what a blocked row says instead of a status.
///
/// A blocked task is not in a *state*, it is in a *relationship*, and naming
/// the thing it waits on is what makes the row actionable. A "blocked" pill
/// tells you to go and find out; this tells you.
pub fn blocked_by(ui: &mut Ui, what: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(format!("\u{2933} waiting on \u{201c}{what}\u{201d}"))
                .size(text::SMALL)
                .color(colour::TEXT_FAINT),
        )
        .truncate(),
    );
}

/// A hairline progress track. Used for phase and discipline completion.
///
/// Two points tall with fully rounded caps: at this weight the bar is a rule
/// that happens to be coloured, so a page can carry four of them without any
/// one reading as a component. The track is `LINE` rather than `INSET` — a
/// well needs depth, a rule needs only to be visible.
pub fn progress(ui: &mut Ui, fraction: f32, width: f32, tint: Color32) {
    let h = space::XXS;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, h / 2.0, colour::LINE);
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.0 {
        // Floored at the cap diameter: below it a rounded rect paints as a
        // lens, and one task done out of two hundred should still be a mark.
        let filled = Vec2::new((rect.width() * f).max(h), h);
        p.rect_filled(egui::Rect::from_min_size(rect.min, filled), h / 2.0, tint);
    }
}

// ---------------------------------------------------------------- surfaces

/// The glass edge: one even hairline all round, brighter on hover. No lit top
/// rim; it read as a stray border on every card.
pub fn glass_edge(ui: &Ui, rect: egui::Rect, r: f32, hovered: bool) {
    let tone = if hovered { colour::EDGE_MID_HOVER } else { colour::EDGE_MID };
    ui.painter().rect_stroke(rect, r, egui::Stroke::new(1.0, tone), egui::StrokeKind::Inside);
}

/// A glass card. Translucent, so the sidebar wash shows through, with a
/// hairline edge doing the work a shadow would do elsewhere.
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
    let response =
        super::motion::operable(ui, out.response.interact(egui::Sense::click()), radius::MD as f32);

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

/// A row with a coloured spine down its left edge — used to mark a row as
/// delegated to an agent. Restored as a widget after the board port dropped it
/// for being hand-painted geometry: the signal was worth keeping, the literals
/// were not. Delegated work should be visible in a scan, not read for.
pub fn row_marked<R>(ui: &mut Ui, spine: Color32, add: impl FnOnce(&mut Ui) -> R) -> Response {
    let top = ui.cursor().top();
    let response = row(ui, add);
    let rect = egui::Rect::from_min_size(
        egui::pos2(response.rect.left(), top),
        egui::vec2(space::XXS, response.rect.height()),
    );
    ui.painter().rect_filled(rect, radius::SM as f32, spine);
    response
}

/// A clickable list row of fixed height, so columns align down the page.
/// Hover tints the background rather than moving anything.
pub fn row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> Response {
    let w = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, size::ROW), Sense::click());
    let response = super::motion::operable(ui, response, radius::SM as f32);

    // Eased rather than snapped: a row that lights up instantly on every
    // pointer move reads as flicker when you drag down a long list.
    let tint = super::motion::hover_fill(
        ui,
        response.id.with("hover"),
        response.hovered() || response.has_focus(),
        Color32::TRANSPARENT,
        colour::SURFACE_HOVER,
    );
    if tint != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, radius::SM as f32, tint);
    }
    if response.hovered() {
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
    let icon_w = if icon.is_some() { size::ICON_COL } else { 0.0 };
    // A link has no box, so it has no padding to sit inside: any inset here
    // pushes its text off the column every neighbouring value starts on.
    let (px, _) = if small { (0.0, 0.0) } else { pad::BUTTON };
    let height = if small { size::CONTROL - space::SM } else { size::CONTROL };
    let width = galley.size().x + icon_w + px * 2.0;

    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, height),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    // Painted by hand, so it names itself for the accessibility tree: a
    // screen reader, and a test that clicks a button by its label.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    // Keyboard operability, applied once here rather than at every call site.
    let response = if enabled {
        super::motion::operable(ui, response, radius::SM as f32)
    } else {
        response
    };

    let hovered = enabled && (response.hovered() || response.has_focus());
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

/// A labelled input for something with more than one line in it. Fixed at
/// `rows` rather than growing, so a form does not reflow while it is typed in.
pub fn field_multiline(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    rows: usize,
    hint: &str,
) -> Response {
    ui.vertical(|ui| {
        // An unlabelled box (a comment box, an answer box) takes no caption
        // row: an empty one left a blank line above it.
        if !label.is_empty() {
            caption(ui, label);
            ui.add_space(space::XXS);
        }
        ui.add_sized(
            [ui.available_width(), size::CONTROL * rows as f32],
            egui::TextEdit::multiline(value)
                .desired_rows(rows)
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
    // No leading space: every caller already sits under a section heading that
    // spaces itself, and the pair made an empty phase taller than a full one.
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
        // The ellipsis belongs to the widget, not to eight call sites that
        // each have to remember it.
        muted(ui, &format!("{what}\u{2026}"));
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

// ---------------------------------------------------------------- toast

const TOAST: &str = "widgets:toast";

/// Say how an action from a menu went, for a moment, at the foot of the
/// window. A row's menu has no page of its own to put a notice on, and
/// "Copied" belongs nowhere else.
pub fn toast(ctx: &egui::Context, message: impl Into<String>, failed: bool) {
    let at = ctx.input(|i| i.time);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(TOAST), (message.into(), failed, at)));
}

/// Draw the current toast, if it has not run out. Once a frame, from the shell.
pub fn toasts(ctx: &egui::Context) {
    let id = egui::Id::new(TOAST);
    let Some((message, failed, at)) = ctx.data(|d| d.get_temp::<(String, bool, f64)>(id)) else { return };
    // A failure is a sentence someone has to read; "Copied." is a glance.
    let lasts = if failed { 6.0 } else { 2.0 };
    let left = lasts - (ctx.input(|i| i.time) - at);
    if left <= 0.0 {
        ctx.data_mut(|d| d.remove::<(String, bool, f64)>(id));
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
    egui::Area::new(id)
        .order(egui::Order::Tooltip)
        .interactable(false)
        .anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -space::XL))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(colour::SURFACE_ACTIVE)
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG))
                .corner_radius(radius::MD)
                .inner_margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(message)
                            .size(text::SMALL)
                            .color(if failed { colour::DANGER } else { colour::TEXT }),
                    );
                });
        });
}
