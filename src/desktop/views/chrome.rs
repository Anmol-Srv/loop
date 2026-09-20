//! Top bar and central routing. Owns which view is showing; the views
//! themselves only render.

use crate::desktop::{theme, views, App, Tab};

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::top("chrome")
        .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(14, 9)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Control Plane").strong());
                ui.add_space(14.0);

                if ui.selectable_label(app.tab == Tab::Board && app.task.is_none(), "Board").clicked() {
                    app.tab = Tab::Board;
                    app.task = None;
                }

                let pending = app
                    .net
                    .as_ref()
                    .and_then(|n| n.data("inbox"))
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let inbox_label = if pending > 0 { format!("Inbox  {pending}") } else { "Inbox".to_string() };
                if ui.selectable_label(app.tab == Tab::Inbox && app.task.is_none(), inbox_label).clicked() {
                    app.tab = Tab::Inbox;
                    app.task = None;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Sign out").clicked() {
                        app.sign_out();
                    }
                    if ui.small_button("Refresh").clicked() {
                        if let Some(n) = app.net.as_mut() {
                            n.results.clear();
                        }
                    }
                    if !app.can_write() {
                        theme::pill(ui, "read only", theme::MUTED);
                    }
                });
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(16)))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if app.task.is_some() {
                    views::task::ui(app, ui);
                } else {
                    match app.tab {
                        Tab::Board => views::board::ui(app, ui),
                        Tab::Inbox => views::inbox::ui(app, ui),
                    }
                }
            });
        });
}
