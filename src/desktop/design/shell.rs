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
/// Returns `(group, item)` of whatever was clicked. Grouping is the point: a
/// flat list of four items in 232px reads as an accident, and the group labels
/// are what let projects and agents live here without competing with the
/// primary destinations.
pub fn sidebar(
    ui: &mut Ui,
    brand: (&str, &str),
    groups: &[NavGroup<'_>],
    footer: impl FnOnce(&mut Ui),
) -> Option<(usize, usize)> {
    let mut clicked = None;
    let narrow = ui.max_rect().width() < size::SIDEBAR_COLLAPSE_AT;
    let width = if narrow { size::SIDEBAR_W_NARROW } else { size::SIDEBAR_W };

    egui::Panel::left("sidebar")
        .exact_size(width)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(colour::CHROME)
                .inner_margin(egui::Margin::symmetric(pad::SIDEBAR.0 as i8, pad::SIDEBAR.1 as i8)),
        )
        .show(ui, |ui| {
            // Room for the traffic lights, which float over this corner
            // because the window has no title bar.
            ui.add_space(TRAFFIC_LIGHTS);

            if !narrow {
                brand_row(ui, brand.0, brand.1);
                ui.add_space(space::MD);
                search_field(ui);
                ui.add_space(space::MD);
            }

            for (g, group) in groups.iter().enumerate() {
                if !narrow && !group.label.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        RichText::new(group.label)
                            .size(text::CAPTION)
                            .family(egui::FontFamily::Name(super::theme::BOLD.into()))
                            .color(colour::TEXT_FAINT),
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

            if !narrow {
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.add_space(space::SM);
                    footer(ui);
                    ui.add_space(space::MD);
                    let line = ui.available_rect_before_wrap();
                    ui.painter().hline(
                        line.x_range(),
                        ui.cursor().top(),
                        egui::Stroke::new(1.0, colour::LINE_SOFT),
                    );
                });
            }
        });

    clicked
}

/// The product mark and name.
fn brand_row(ui: &mut Ui, name: &str, tagline: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(26.0), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, radius::SM as f32, colour::ACCENT);
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "A",
            egui::FontId::proportional(text::SMALL),
            colour::ON_ACCENT,
        );
        ui.add_space(space::XS);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(
                RichText::new(name)
                    .size(text::BODY)
                    .family(egui::FontFamily::Name(super::theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
            ui.label(
                RichText::new(tagline)
                    .size(text::CAPTION)
                    .color(colour::TEXT_FAINT),
            );
        });
    });
}

/// A search affordance. Not wired yet — it is drawn because its absence is the
/// loudest thing missing from a sidebar of this shape, and because the
/// keyboard shortcut needs somewhere to advertise itself.
fn search_field(ui: &mut Ui) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), size::CONTROL + 2.0),
        egui::Sense::click(),
    );
    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, colour::INSET);
    p.rect_stroke(
        rect,
        radius::SM as f32,
        egui::Stroke::new(1.0, if response.hovered() { colour::LINE_STRONG } else { colour::LINE }),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(rect.left() + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        egui_phosphor::thin::MAGNIFYING_GLASS,
        egui::FontId::proportional(text::BODY),
        colour::TEXT_FAINT,
    );
    p.text(
        egui::pos2(rect.left() + space::SM + size::ICON_COL, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Search",
        egui::FontId::proportional(text::SMALL),
        colour::TEXT_FAINT,
    );
    // The shortcut chip, so the affordance teaches itself.
    for (i, key) in ["K", "\u{2318}"].iter().enumerate() {
        let w = 18.0;
        let chip = egui::Rect::from_center_size(
            egui::pos2(rect.right() - space::SM - w / 2.0 - i as f32 * (w + 2.0), rect.center().y),
            egui::vec2(w, 16.0),
        );
        p.rect_filled(chip, radius::SM as f32 - 2.0, colour::SURFACE_HOVER);
        p.text(
            chip.center(),
            egui::Align2::CENTER_CENTER,
            key,
            egui::FontId::proportional(text::CAPTION),
            colour::TEXT_MUTED,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

fn nav_item(ui: &mut Ui, item: &NavItem<'_>, narrow: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), size::NAV_ROW),
        egui::Sense::click(),
    );
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
        p.rect_filled(rect, r, colour::SURFACE_ACTIVE.gamma_multiply(sel));
    } else if hov > 0.001 {
        p.rect_filled(rect, r, colour::SURFACE.gamma_multiply(hov));
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let fg = if item.selected { colour::TEXT } else { colour::TEXT_MUTED };
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
        return response;
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
    p.text(
        egui::pos2(x + size::ICON_COL, rect.center().y),
        egui::Align2::LEFT_CENTER,
        item.label,
        egui::FontId::proportional(text::BODY),
        fg,
    );

    // A badge demands attention and takes the accent; a count is ambient and
    // stays faint. Both right-aligned, never both present.
    if item.badge > 0 {
        let label = item.badge.to_string();
        let galley =
            p.layout_no_wrap(label, egui::FontId::proportional(text::CAPTION), colour::ON_ACCENT);
        let w = galley.size().x + space::MD;
        let badge = egui::Rect::from_center_size(
            egui::pos2(rect.right() - space::SM - w / 2.0, rect.center().y),
            egui::vec2(w, size::BADGE_H),
        );
        p.rect_filled(badge, radius::PILL as f32, colour::DANGER);
        p.galley(badge.center() - galley.size() / 2.0, galley, colour::TEXT);
    } else if let Some(count) = &item.count {
        p.text(
            egui::pos2(rect.right() - space::SM, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            count,
            egui::FontId::proportional(text::CAPTION),
            colour::TEXT_FAINT,
        );
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
                .fill(colour::CANVAS)
                .inner_margin(egui::Margin::symmetric(space::XXL as i8, pad::PAGE.1 as i8)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
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
            });
        });
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
        ui.label(
            RichText::new(title)
                .size(text::TITLE)
                .family(egui::FontFamily::Name(super::theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
    if !subtitle.is_empty() {
        ui.add_space(space::XXS);
        ui.label(
            RichText::new(subtitle)
                .size(text::SMALL)
                .color(colour::TEXT_MUTED),
        );
    }
    ui.add_space(space::LG);
}

/// A back control above a page title. One spelling, so the two panes that have
/// one stop disagreeing about the gap beneath it.
pub fn back(ui: &mut Ui, label: &str) -> egui::Response {
    let response = super::widgets::icon_button(
        ui,
        egui_phosphor::thin::ARROW_LEFT,
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
                .color(colour::TEXT_MUTED),
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
        ui.label(
            RichText::new(label)
                .size(text::SMALL)
                .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
                .color(colour::TEXT_MUTED),
        );
        ui.label(
            RichText::new(count.to_string())
                .size(text::SMALL)
                .color(colour::TEXT_FAINT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
    ui.add_space(space::MD);
}

/// A section heading inside a page body.
pub fn section(ui: &mut Ui, label: &str) {
    ui.add_space(space::XL);
    ui.label(
        RichText::new(label)
            .size(text::SMALL)
            .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
            .color(colour::TEXT_MUTED),
    );
    ui.add_space(space::MD);
}
