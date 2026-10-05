//! The dashboard shell: a fixed sidebar, a content header, and a body.
//!
//! Views do not lay themselves out. They hand the shell a title, optional
//! actions, and a body closure — so every screen agrees on gutters, header
//! height and content width, and a new view is a `NavItem` plus a match arm
//! rather than a new layout.

use egui::{Align, Layout, Response, RichText, Ui};

use super::tokens::{colour, pad, radius, size, space, text};

/// Vertical room for the macOS traffic lights, which overlay the content when
/// the title bar is hidden.
const TRAFFIC_LIGHTS: f32 = size::ROW;

/// Where the wash's bounds are stashed, so a notch can sample the exact colour
/// behind it. The gradient is painted before the items, and the items need to
/// know it — one value, and it never leaves this module.
/// One entry in the sidebar.
///
/// `badge` is an attention count and is drawn in the accent; `count` is
/// ambient and is drawn faint. A pending-review count is a badge; the number
/// of projects is a count. Conflating them makes everything shout.
pub struct NavItem<'a> {
    pub icon: &'a str,
    pub label: &'a str,
    pub selected: bool,
    pub badge: usize,
    pub count: Option<String>,
    /// A coloured dot instead of an icon — used for project and agent rows.
    pub dot: Option<egui::Color32>,
}

impl<'a> NavItem<'a> {
    pub fn new(icon: &'a str, label: &'a str, selected: bool) -> Self {
        Self { icon, label, selected, badge: 0, count: None, dot: None }
    }
    pub fn badge(mut self, n: usize) -> Self {
        self.badge = n;
        self
    }
    pub fn count(mut self, s: impl Into<String>) -> Self {
        self.count = Some(s.into());
        self
    }
    pub fn dot(mut self, c: egui::Color32) -> Self {
        self.dot = Some(c);
        self
    }
}

/// A group of nav items under a small muted heading.
pub struct NavGroup<'a> {
    pub label: &'a str,
    pub items: Vec<NavItem<'a>>,
}

/// Draws the sidebar and returns the index of a clicked item, if any.
/// The sidebar: a brand row, a search affordance, grouped navigation, and the
/// signed-in person pinned at the foot.
///
/// Returns `(group, item)` of whatever was clicked, and whether search was.
/// Grouping is the point: a flat list of four items in 232px reads as an
/// accident, and the group labels are what let projects and agents live here
/// without competing with the primary destinations.
///
/// The footer is drawn at both widths; it reads `ui.available_width()` to
/// decide how much of itself fits. Hiding it when collapsed left a narrow
/// window with no way to sign out.
pub fn sidebar(
    ui: &mut Ui,
    brand: &Brand<'_>,
    groups: &[NavGroup<'_>],
    footer: impl FnOnce(&mut Ui),
) -> (Option<(usize, usize)>, bool, Response) {
    let mut clicked = None;
    let mut search = false;
    let mut switcher = None;
    let narrow = ui.max_rect().width() < size::SIDEBAR_COLLAPSE_AT;
    let width = if narrow { size::SIDEBAR_W_NARROW } else { size::SIDEBAR_W };

    egui::Panel::left("sidebar")
        .exact_size(width)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(colour::CHROME())
                .inner_margin(egui::Margin::symmetric(pad::SIDEBAR.0 as i8, pad::SIDEBAR.1 as i8)),
        )
        .show(ui, |ui| {
            // Room for the traffic lights, which float over this corner
            // because the window has no title bar.
            ui.add_space(TRAFFIC_LIGHTS);

            switcher = Some(workspace_button(ui, brand, narrow));
            if !narrow {
                ui.add_space(space::MD);
                search = search_field(ui).clicked();
                ui.add_space(space::MD);
            } else {
                // The box has no room, so search becomes one more icon row
                // and its hover text carries the shortcut.
                let item = NavItem::new(egui_phosphor::regular::MAGNIFYING_GLASS, "Search (\u{2318}K)", false);
                ui.add_space(space::XS);
                search = nav_item(ui, &item, true).clicked();
                ui.add_space(space::SM);
            }

            for (g, group) in groups.iter().enumerate() {
                if !narrow && !group.label.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        RichText::new(group.label)
                            .size(text::CAPTION)
                            .family(egui::FontFamily::Name(super::theme::BOLD.into()))
                            .color(colour::TEXT_FAINT()),
                    );
                    ui.add_space(space::XXS);
                }
                for (i, item) in group.items.iter().enumerate() {
                    if nav_item(ui, item, narrow).clicked() {
                        clicked = Some((g, i));
                    }
                }
                ui.add_space(space::SM);
            }

            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.add_space(space::SM);
                footer(ui);
                ui.add_space(space::MD);
                let line = ui.available_rect_before_wrap();
                ui.painter().hline(
                    line.x_range(),
                    ui.cursor().top(),
                    egui::Stroke::new(1.0, colour::LINE_SOFT()),
                );
            });
        });

    (clicked, search, switcher.expect("drawn above"))
}

/// The product mark: the orange loop, in its own colours on either palette,
/// cut from the same art as the app icon (`scripts/make-icon.py`). 128px,
/// so it stays crisp at 26pt on a Retina display.
const MARK: &[u8] = include_bytes!("../../../assets/logo-small.png");
const MARK_SIZE: f32 = 26.0;

/// Decode once, then hand out the same handle every frame.
///
/// The lookup and the load are deliberately separate statements: `data_mut`
/// holds the context's write lock for the whole closure, and `load_texture`
/// reaches for that same lock, so doing the load inside the closure
/// deadlocks the first frame and the window never appears. It did.
fn mark_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("brand:mark");
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return handle;
    }

    let decoded = image::load_from_memory(MARK).expect("the mark is baked in").to_rgba8();
    let (w, h) = decoded.dimensions();
    let pixels: Vec<egui::Color32> = decoded
        .pixels()
        .map(|p| egui::Color32::from_rgba_unmultiplied(p.0[0], p.0[1], p.0[2], p.0[3]))
        .collect();
    let handle = ctx.load_texture(
        "brand:mark",
        egui::ColorImage {
            size: [w as usize, h as usize],
            pixels,
            source_size: egui::vec2(w as f32, h as f32),
        },
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

/// The workspace on screen, as the sidebar's brand row shows it.
pub struct Brand<'a> {
    pub name: &'a str,
    /// The server's host, or where a private workspace lives.
    pub detail: &'a str,
    /// Only on this Mac: the mark wears a lock.
    pub private: bool,
    /// More than one workspace to switch between: the caret only promises a
    /// menu worth opening.
    pub switchable: bool,
}

/// The workspace switcher: the mark, the workspace's name and where it lives,
/// and a caret. The caller opens the menu under it. Collapsed, it is the mark
/// alone, named for the accessibility tree and the hover.
fn workspace_button(ui: &mut Ui, brand: &Brand<'_>, narrow: bool) -> Response {
    let h = if narrow { size::NAV_ROW + space::XS } else { MARK_SIZE + space::MD };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    let label = format!("Workspace: {}", brand.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let response = super::motion::operable(ui, response, radius::MD as f32);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let hot = response.hovered() || response.has_focus();
    let fill = if open {
        colour::SURFACE_ACTIVE()
    } else {
        super::motion::hover_fill(ui, response.id.with("fill"), hot, colour::TRANSPARENT(), colour::SURFACE_HOVER())
    };
    if fill != colour::TRANSPARENT() {
        ui.painter().rect_filled(rect, radius::MD as f32, fill);
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let mark_c = if narrow {
        rect.center()
    } else {
        egui::pos2(rect.left() + space::XS + MARK_SIZE / 2.0, rect.center().y)
    };
    let texture = mark_texture(ui.ctx());
    let mark = egui::Rect::from_center_size(mark_c, egui::Vec2::splat(MARK_SIZE));
    ui.painter().image(texture.id(), mark, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    if brand.private {
        paint_lock_badge(ui.painter(), mark.right_bottom() - egui::vec2(2.0, 2.0));
    }
    if narrow {
        return if open { response } else { response.on_hover_text(format!("{} \u{00B7} {}", brand.name, brand.detail)) };
    }

    let caret_w = if brand.switchable { size::ICON_COL } else { 0.0 };
    let x = mark.right() + space::SM;
    let room = (rect.right() - space::XS - caret_w - x).max(0.0);
    let name = super::widgets::truncated(
        ui,
        brand.name,
        egui::FontId::new(text::BODY, egui::FontFamily::Name(super::theme::SEMIBOLD.into())),
        colour::TEXT(),
        room,
    );
    let detail = super::widgets::truncated(ui, brand.detail, egui::FontId::proportional(text::CAPTION), colour::TEXT_FAINT(), room);
    let top = rect.center().y - (name.size().y + detail.size().y) / 2.0;
    let name_h = name.size().y;
    let p = ui.painter();
    p.galley(egui::pos2(x, top), name, colour::TEXT());
    p.galley(egui::pos2(x, top + name_h), detail, colour::TEXT_FAINT());
    if brand.switchable {
        p.text(
            egui::pos2(rect.right() - space::XS - caret_w / 2.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            egui_phosphor::regular::CARET_UP_DOWN,
            egui::FontId::proportional(text::BODY),
            if hot || open { colour::TEXT_2() } else { colour::TEXT_FAINT() },
        );
    }
    response
}

/// The mark into `rect`, with the private lock at its corner when asked —
/// the switcher's rows draw it at their own size.
pub fn paint_mark(ui: &Ui, rect: egui::Rect, private: bool) {
    let texture = mark_texture(ui.ctx());
    ui.painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    if private {
        paint_lock_badge(ui.painter(), rect.right_bottom() - egui::vec2(1.0, 1.0));
    }
}

/// A small lock on a disc of the chrome colour, at `c` — the mark's corner
/// in a private workspace, and the private rows of the switcher.
pub fn paint_lock_badge(p: &egui::Painter, c: egui::Pos2) {
    p.circle_filled(c, 6.5, colour::CHROME());
    p.circle_filled(c, 5.5, colour::SURFACE_ACTIVE());
    super::glyph::lock(p, c, 8.0, colour::TEXT_2());
}

/// The palette's front door. Drawn as a box rather than a button because
/// that is where people look for search, and the shortcut chip teaches ⌘K.
fn search_field(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), size::CONTROL + 2.0),
        egui::Sense::click(),
    );
    let response = super::motion::operable(ui, response, radius::SM as f32);
    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, colour::INSET());
    p.rect_stroke(
        rect,
        radius::SM as f32,
        egui::Stroke::new(1.0, if response.hovered() { colour::LINE_STRONG() } else { colour::LINE() }),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(rect.left() + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        egui_phosphor::regular::MAGNIFYING_GLASS,
        egui::FontId::proportional(text::BODY),
        colour::TEXT_FAINT(),
    );
    p.text(
        egui::pos2(rect.left() + space::SM + size::ICON_COL, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Search",
        egui::FontId::proportional(text::SMALL),
        colour::TEXT_FAINT(),
    );
    // The shortcut chip, so the affordance teaches itself.
    for (i, key) in ["K", "\u{2318}"].iter().enumerate() {
        let w = 18.0;
        let chip = egui::Rect::from_center_size(
            egui::pos2(rect.right() - space::SM - w / 2.0 - i as f32 * (w + 2.0), rect.center().y),
            egui::vec2(w, 16.0),
        );
        p.rect_filled(chip, radius::SM as f32 - 2.0, colour::SURFACE_HOVER());
        p.text(
            chip.center(),
            egui::Align2::CENTER_CENTER,
            key,
            egui::FontId::proportional(text::CAPTION),
            colour::TEXT_MUTED(),
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// One sidebar row, for a sidebar that is not the app's own — the Settings
/// window's section list — so both sidebars are the same component.
pub fn nav_row(ui: &mut Ui, item: &NavItem<'_>) -> Response {
    nav_item(ui, item, false)
}

fn nav_item(ui: &mut Ui, item: &NavItem<'_>, narrow: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), size::NAV_ROW),
        egui::Sense::click(),
    );
    // Named, and selected when it is the page showing: a screen reader (and a
    // test) finds a destination by what it says.
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item.label);
        info.selected = Some(item.selected);
        info
    });
    let response = super::motion::operable(ui, response, radius::SM as f32);

    let r = radius::SM as f32;
    // The selected pill eases in, so switching tabs reads as one surface
    // moving rather than two surfaces blinking.
    let sel = super::motion::to(ui, response.id.with("sel"), item.selected, super::motion::BASE);
    let hov = super::motion::to(
        ui,
        response.id.with("hov"),
        response.hovered() || response.has_focus(),
        super::motion::FAST,
    );
    let p = ui.painter();
    if sel > 0.001 {
        p.rect_filled(rect, r, colour::SURFACE_ACTIVE().gamma_multiply(sel));
    } else if hov > 0.001 {
        p.rect_filled(rect, r, colour::SURFACE().gamma_multiply(hov));
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let fg = if item.selected { colour::TEXT() } else { colour::TEXT_MUTED() };
    let p = ui.painter();

    if narrow {
        if let Some(c) = item.dot {
            p.circle_filled(rect.center(), 3.5, c);
        } else {
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                item.icon,
                egui::FontId::proportional(text::HEADING),
                fg,
            );
        }
        // No room for the number: an accent dot says something waits here,
        // and the hover says how much.
        if item.badge > 0 {
            p.circle_filled(rect.center() + egui::vec2(space::SM, -space::SM), 3.5, colour::ACCENT());
            return response.on_hover_text(format!("{} \u{00B7} {}", item.label, item.badge));
        }
        return response.on_hover_text(item.label);
    }

    let x = rect.left() + space::SM;
    if let Some(c) = item.dot {
        p.circle_filled(egui::pos2(x + 6.0, rect.center().y), 3.5, c);
    } else {
        p.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            item.icon,
            egui::FontId::proportional(text::HEADING),
            fg,
        );
    }
    // A badge demands attention and takes the accent; a count is ambient and
    // stays faint. Both right-aligned, never both present. Measured before the
    // label is painted, so a long name truncates into what is left of the rail
    // instead of running under them.
    let trailing = if item.badge > 0 {
        Some((
            p.layout_no_wrap(
                item.badge.to_string(),
                egui::FontId::proportional(text::CAPTION),
                colour::ON_ACCENT(),
            ),
            true,
        ))
    } else {
        item.count.as_ref().map(|count| {
            (
                p.layout_no_wrap(
                    count.clone(),
                    egui::FontId::proportional(text::CAPTION),
                    colour::TEXT_FAINT(),
                ),
                false,
            )
        })
    };
    let trailing_w = trailing
        .as_ref()
        .map(|(g, badge)| g.size().x + if *badge { space::MD } else { 0.0 } + space::SM)
        .unwrap_or(0.0);

    let label_x = x + size::ICON_COL;
    let label = super::widgets::truncated(
        ui,
        item.label,
        egui::FontId::proportional(text::BODY),
        fg,
        (rect.right() - space::SM - trailing_w - label_x).max(0.0),
    );
    p.galley(egui::pos2(label_x, rect.center().y - label.size().y / 2.0), label, fg);

    match trailing {
        Some((galley, true)) => {
            let w = galley.size().x + space::MD;
            let badge = egui::Rect::from_center_size(
                egui::pos2(rect.right() - space::SM - w / 2.0, rect.center().y),
                egui::vec2(w, size::BADGE_H),
            );
            // The accent, as the comment above promises: white on the
            // danger red it used to be was 2.6:1.
            p.rect_filled(badge, radius::PILL as f32, colour::ACCENT());
            p.galley(badge.center() - galley.size() / 2.0, galley, colour::ON_ACCENT());
        }
        Some((galley, false)) => {
            let at = egui::pos2(
                rect.right() - space::SM - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            );
            p.galley(at, galley, colour::TEXT_FAINT());
        }
        None => {}
    }

    response
}

/// The content area. No header strip: gutters and scrolling applied once, then
/// the view owns everything inside. Each view carries its own heading and back
/// control, so a shared header would only repeat them.
pub fn content(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(colour::CANVAS())
                .inner_margin(egui::Margin::symmetric(0, pad::PAGE.1 as i8)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical().content_margin(egui::Margin::symmetric(space::XXL as i8, 0)).show(ui, |ui| {
                // Cap the measure and centre it: past ~1080px the cards just
                // stretch, and a task title 1400px wide is unreadable.
                let avail = ui.available_width();
                if avail > size::CONTENT_MAX {
                    let side = (avail - size::CONTENT_MAX) / 2.0;
                    ui.horizontal(|ui| {
                        ui.add_space(side);
                        ui.vertical(|ui| {
                            ui.set_max_width(size::CONTENT_MAX);
                            body(ui);
                        });
                    });
                } else {
                    body(ui);
                }
                edge_scroll(ui);
            });
        });
}

const EDGE_SCROLL_GAIN: f32 = 10.0;
const EDGE_SCROLL_MIN: f32 = 120.0;
const EDGE_SCROLL_MAX: f32 = 3000.0;

pub fn edge_scroll(ui: &Ui) {
    let ctx = ui.ctx();
    if !drag_selecting(ctx) {
        return;
    }
    let Some(pointer) = ctx.input(|i| i.pointer.latest_pos()) else { return };
    let view = ui.clip_rect();
    let past = if pointer.y < view.top() {
        pointer.y - view.top()
    } else if pointer.y > view.bottom() {
        pointer.y - view.bottom()
    } else {
        0.0
    };
    let wheel = if view.contains(pointer) {
        ctx.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y))
    } else {
        0.0
    };
    let edge = if past == 0.0 {
        0.0
    } else {
        let speed = (past.abs() * EDGE_SCROLL_GAIN).clamp(EDGE_SCROLL_MIN, EDGE_SCROLL_MAX);
        -past.signum() * speed * ctx.input(|i| i.stable_dt)
    };
    if edge + wheel != 0.0 {
        ui.scroll_with_delta_animation(egui::vec2(0.0, edge + wheel), egui::style::ScrollAnimation::none());
    }
}

fn drag_selecting(ctx: &egui::Context) -> bool {
    let Some(dragged) = ctx.dragged_id() else { return false };
    let labels = ctx
        .plugin_opt::<egui::text_selection::LabelSelectionState>()
        .is_some_and(|p| p.lock().has_selection());
    labels || (ctx.text_edit_focused() && ctx.memory(|m| m.focused()) == Some(dragged))
}

pub fn selectable(ui: &mut Ui, label: egui::Label) -> (egui::Pos2, std::sync::Arc<egui::Galley>, Response) {
    let (pos, galley, response) = label.selectable(true).layout_in_ui(ui);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), galley.text()));
    let view = ui.clip_rect();
    let unreached = ui.ctx().pointer_interact_pos().is_some_and(|p| {
        (p.y > view.bottom() && response.rect.top() > p.y) || (p.y < view.top() && response.rect.bottom() < p.y)
    });
    if !unreached {
        egui::text_selection::LabelSelectionState::label_text_selection(
            ui,
            &response,
            pos,
            galley.clone(),
            ui.visuals().text_color(),
            egui::Stroke::NONE,
        );
    }
    (pos, galley, response)
}

/// The top of a page: title, optional subtitle, optional trailing control.
///
/// Six panes each drew their own, landing on four different gaps under the
/// same rank of element and two different title weights. `shell::content`
/// deliberately owns no header strip — but that left the one element present
/// on every screen as the one element with no shared implementation.
pub fn page_title(
    ui: &mut Ui,
    title: &str,
    subtitle: &str,
    trailing: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            trailing(ui);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(text::TITLE)
                            .family(egui::FontFamily::Name(super::theme::SEMIBOLD.into()))
                            .color(colour::TEXT()),
                    )
                    .truncate(),
                );
            });
        });
    });
    if !subtitle.is_empty() {
        ui.add_space(space::XXS);
        ui.label(
            RichText::new(subtitle)
                .size(text::SMALL)
                .color(colour::TEXT_MUTED()),
        );
    }
    ui.add_space(space::LG);
}

/// A back control above a page title. One spelling, so the two panes that have
/// one stop disagreeing about the gap beneath it.
pub fn back(ui: &mut Ui, label: &str) -> egui::Response {
    let response = super::widgets::icon_button(
        ui,
        egui_phosphor::regular::ARROW_LEFT,
        label,
        super::widgets::Emphasis::Link,
        true,
    );
    ui.add_space(space::XS);
    response
}

/// A section heading with something trailing on the right — a count, a live
/// pill, an action. The task view rebuilt this inline because `section` takes
/// a label and nothing else.
pub fn section_with(ui: &mut Ui, label: &str, trailing: impl FnOnce(&mut Ui)) {
    ui.add_space(space::XL);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(text::SMALL)
                .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
                .color(colour::TEXT_MUTED()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
    ui.add_space(space::MD);
}

/// A section heading with a count beside it, the count a shade fainter.
///
/// Two views folded the count into the label string because nothing offered
/// this; the result was a count in the same tone as its heading, which reads
/// as part of the title.
pub fn section_count(ui: &mut Ui, label: &str, count: usize) {
    section_count_with(ui, label, count, |_| {});
}

/// A section heading, its count, and something on the right.
///
/// The trailing slot is why this went unused: two views needed a "View all"
/// or a matched-disciplines note beside the heading, `section_count` offered
/// nowhere to put it, so both hand-rolled the whole thing and lost the fainter
/// count in the process.
pub fn section_count_with(
    ui: &mut Ui,
    label: &str,
    count: usize,
    trailing: impl FnOnce(&mut Ui),
) {
    ui.add_space(space::XL);
    ui.horizontal(|ui| {
        // The count belongs to its label; at the page's default spacing the
        // two drifted apart and read as a heading and a stray number.
        ui.spacing_mut().item_spacing.x = space::XS;
        ui.label(
            RichText::new(label)
                .size(text::SMALL)
                .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
                .color(colour::TEXT_MUTED()),
        );
        ui.label(
            RichText::new(count.to_string())
                .size(text::SMALL)
                .color(colour::TEXT_FAINT()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
    ui.add_space(space::MD);
}

/// A section heading inside a page body.
/// A hairline between two stacked sections.
///
/// A labelled heading alone was not enough separation on the task page: four
/// sections down one column at the same weight read as one long column with
/// words in it. The rule is what says "this is a different thing".
pub fn divider(ui: &mut Ui) {
    ui.add_space(space::XL);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 1.0),
        egui::Sense::hover(),
    );
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, colour::LINE()));
}

/// Which half of a railed page's content is being asked for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The title and its actions.
    Header,
    /// Everything under them.
    Body,
}

/// A page split into a content column and a properties rail.
///
/// The rail is Linear's shape and it earns its place: status, priority and
/// owner are facts you glance at, not prose you read, so they do not belong
/// in the reading column.
///
/// Below `RAIL_AT` there is not room for both, and the rail goes between the
/// page's header and its body — title first, so you know what you are looking
/// at; then its properties, which are what you opened it to check; then the
/// long part. Putting the rail above everything worked for a task's six rows
/// and buried a project's name a full screen down under its people list.
///
/// `content` is called twice, `Header` then `Body`, which is what lets the
/// rail sit between them without either half borrowing the page's state
/// twice.
pub fn with_rail(
    ui: &mut Ui,
    mut content: impl FnMut(&mut Ui, Part),
    rail: impl FnOnce(&mut Ui),
) {
    if ui.available_width() < RAIL_AT {
        content(ui, Part::Header);
        ui.add_space(space::LG);
        rail_surface(ui, rail);
        content(ui, Part::Body);
        return;
    }

    let full = ui.available_width();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::XXL;
        ui.allocate_ui_with_layout(
            egui::vec2(full - size::RAIL_W - space::XXL, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(ui.available_width());
                content(ui, Part::Header);
                content(ui, Part::Body);
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(size::RAIL_W, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(size::RAIL_W);
                rail_surface(ui, rail);
            },
        );
    });
}

/// Content width at which the properties rail sits beside the content rather
/// than under it. Below this the reading column would be narrower than the
/// rail, which is the wrong way round.
const RAIL_AT: f32 = 760.0;

fn rail_surface(ui: &mut Ui, rail: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(colour::SURFACE())
        .stroke(egui::Stroke::new(1.0, colour::LINE()))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::same(space::LG as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Every picker in the rail is one width, so Status, Priority, the
            // dates and Labels line up as a column of equal controls.
            ui.spacing_mut().interact_size.x = size::PICKER_W;
            rail(ui);
        });
}

/// One row of the properties rail: a muted label, then the value.
///
/// The label column is fixed so every value in the rail starts at the same x
/// — a ragged left edge down a list of facts is the thing that makes a rail
/// look like a pile.
pub fn property(ui: &mut Ui, label: &str, value: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(size::CONTROL);
        ui.spacing_mut().item_spacing.x = space::SM;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(PROPERTY_LABEL_W, size::CONTROL),
            egui::Sense::hover(),
        );
        ui.painter().text(
            egui::pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(text::SMALL),
            colour::TEXT_MUTED(),
        );
        value(ui);
    });
    ui.add_space(space::XS);
}

/// Wide enough for "Department", which is the longest label the rail carries.
const PROPERTY_LABEL_W: f32 = 82.0;

pub fn section(ui: &mut Ui, label: &str) {
    ui.add_space(space::XL);
    ui.label(
        RichText::new(label)
            .size(text::SMALL)
            .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
            .color(colour::TEXT_MUTED()),
    );
    ui.add_space(space::MD);
}
