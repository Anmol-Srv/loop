//! The dashboard shell: a fixed sidebar, a content header, and a body.
//!
//! Views do not lay themselves out. They hand the shell a title, optional
//! actions, and a body closure — so every screen agrees on gutters, header
//! height and content width, and a new view is a `NavItem` plus a match arm
//! rather than a new layout.

use egui::{Align, Layout, Response, RichText, Ui};

use super::tokens::{colour, pad, radius, size, space, text};
use super::widgets as w;

/// Vertical room for the macOS traffic lights, which overlay the content when
/// the title bar is hidden.
const TRAFFIC_LIGHTS: f32 = 30.0;

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
            // A pink wash from the top, fading out. Painted behind everything
            // in the panel, and the glass cards on the canvas pick it up.
            w::gradient_v(
                ui,
                ui.max_rect().expand(space::XL),
                colour::WASH_TOP,
                colour::WASH_BOTTOM,
            );
            // The window has no title bar, so the traffic lights float over
            // this corner. Leave them room rather than drawing under them.
            ui.add_space(TRAFFIC_LIGHTS);

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
        egui::FontId::proportional(text::HEADING),
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

/// The content area. No header strip: gutters and scrolling applied once, then
/// the view owns everything inside. Each view carries its own heading and back
/// control, so a shared header would only repeat them.
pub fn content(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(colour::CANVAS)
                .inner_margin(egui::Margin::symmetric(pad::PAGE.0 as i8, pad::PAGE.1 as i8)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, body);
        });
}

/// A section heading with something trailing on the right — a count, a live
/// pill, an action. The task view rebuilt this inline because `section` takes
/// a label and nothing else.
pub fn section_with(ui: &mut Ui, label: &str, trailing: impl FnOnce(&mut Ui)) {
    ui.add_space(space::LG);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(text::SMALL)
                .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
                .color(colour::TEXT_MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
    ui.add_space(space::SM);
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
