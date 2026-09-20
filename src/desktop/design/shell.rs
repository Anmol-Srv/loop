//! The dashboard shell: a fixed sidebar, a content header, and a body.
//!
//! Views do not lay themselves out. They hand the shell a title, optional
//! actions, and a body closure — so every screen agrees on gutters, header
//! height and content width, and a new view is a `NavItem` plus a match arm
//! rather than a new layout.

use egui::{Align, Layout, Response, RichText, Ui};

use super::tokens::{colour, pad, radius, size, space, text};
use super::widgets as w;

/// One entry in the sidebar. `badge` shows a count when non-zero.
pub struct NavItem<'a> {
    pub icon: &'a str,
    pub label: &'a str,
    pub selected: bool,
    pub badge: usize,
}

/// Draws the sidebar and returns the index of a clicked item, if any.
pub fn sidebar(ui: &mut Ui, items: &[NavItem<'_>], footer: impl FnOnce(&mut Ui)) -> Option<usize> {
    let mut clicked = None;

    egui::Panel::left("sidebar")
        .exact_size(size::SIDEBAR_W)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(colour::CHROME)
                .inner_margin(egui::Margin::symmetric(pad::SIDEBAR.0 as i8, pad::SIDEBAR.1 as i8)),
        )
        .show(ui, |ui| {
            // Wordmark
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                ui.add_space(space::XS);
                ui.label(
                    RichText::new("Control Plane")
                        .size(text::HEADING)
                        .family(egui::FontFamily::Name(super::theme::SEMIBOLD.into()))
                        .color(colour::TEXT),
                );
            });
            ui.add_space(space::LG);

            for (i, item) in items.iter().enumerate() {
                if nav_item(ui, item).clicked() {
                    clicked = Some(i);
                }
                ui.add_space(space::XXS);
            }

            // Footer pinned to the bottom, so sign-out never floats mid-panel.
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.add_space(space::SM);
                footer(ui);
            });
        });

    clicked
}

fn nav_item(ui: &mut Ui, item: &NavItem<'_>) -> Response {
    let height = 30.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());

    let (bg, fg) = if item.selected {
        (colour::SURFACE_ACTIVE, colour::TEXT)
    } else if response.hovered() {
        (colour::SURFACE, colour::TEXT)
    } else {
        (egui::Color32::TRANSPARENT, colour::TEXT_MUTED)
    };

    if bg != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, radius::SM as f32, bg);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let p = ui.painter();
    let mut x = rect.left() + space::MD;

    p.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        item.icon,
        egui::FontId::proportional(15.0),
        fg,
    );
    x += 24.0;

    p.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        item.label,
        egui::FontId::proportional(text::BODY),
        fg,
    );

    if item.badge > 0 {
        let label = item.badge.to_string();
        let galley =
            p.layout_no_wrap(label, egui::FontId::proportional(text::CAPTION), colour::ON_ACCENT);
        let w = galley.size().x + 10.0;
        let badge_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - space::MD - w / 2.0, rect.center().y),
            egui::vec2(w, 16.0),
        );
        p.rect_filled(badge_rect, radius::PILL as f32, colour::ACCENT);
        p.galley(
            badge_rect.center() - galley.size() / 2.0,
            galley,
            colour::ON_ACCENT,
        );
    }

    response
}

/// The content area: a header strip, then the body, gutters and max width
/// applied once so no view has to remember them.
pub fn content(
    ui: &mut Ui,
    title: &str,
    breadcrumb: Option<&str>,
    actions: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui),
) {
    egui::Panel::top("content-header")
        .exact_size(size::TOPBAR_H)
        .frame(
            egui::Frame::new()
                .fill(colour::CANVAS)
                .inner_margin(egui::Margin::symmetric(space::XL as i8, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                if let Some(crumb) = breadcrumb {
                    ui.label(
                        RichText::new(crumb).size(text::SMALL).color(colour::TEXT_FAINT),
                    );
                    ui.label(RichText::new("/").size(text::SMALL).color(colour::LINE_STRONG));
                }
                ui.label(
                    RichText::new(title)
                        .size(text::TITLE)
                        .family(egui::FontFamily::Name(super::theme::SEMIBOLD.into()))
                        .color(colour::TEXT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), actions);
            });
        });

    // A hairline under the header instead of a shadow.
    egui::Panel::top("content-rule")
        .exact_size(1.0)
        .frame(egui::Frame::new().fill(colour::LINE_SOFT))
        .show(ui, |_| {});

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(colour::CANVAS)
                .inner_margin(egui::Margin::symmetric(pad::PAGE.0 as i8, pad::PAGE.1 as i8)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_max_width(size::CONTENT_MAX);
                body(ui);
            });
        });
}

/// A section heading inside a page body.
pub fn section(ui: &mut Ui, label: &str) {
    ui.add_space(space::LG);
    ui.label(
        RichText::new(label)
            .size(text::SMALL)
            .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
            .color(colour::TEXT_MUTED),
    );
    ui.add_space(space::SM);
}

/// A statistic tile, as in the reference dashboards: a quiet label, a large
/// number, and an optional state dot.
pub fn stat(ui: &mut Ui, label: &str, value: &str, tint: Option<egui::Color32>) {
    w::card(ui, |ui| {
        ui.set_width(150.0);
        ui.horizontal(|ui| {
            if let Some(c) = tint {
                w::dot(ui, c);
            }
            ui.label(RichText::new(label).size(text::CAPTION).color(colour::TEXT_MUTED));
        });
        ui.add_space(space::XS);
        ui.label(
            RichText::new(value)
                .size(text::DISPLAY)
                .family(egui::FontFamily::Name(super::theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
    });
}
