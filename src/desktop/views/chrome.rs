//! Routing and the dashboard frame.
//!
//! The shell owns layout; this file owns which view is showing. Adding a
//! screen is a `NavItem` and a match arm — no new layout code.

use egui_phosphor::thin as icon;

use crate::desktop::design::{avatar, colour, shell, space, text, widgets as w};
use crate::desktop::{views, App, Tab};

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let pending = app
        .net
        .as_ref()
        .and_then(|n| n.data("inbox"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let on_task = app.task.is_some();
    let items = [
        shell::NavItem {
            icon: icon::SQUARES_FOUR,
            label: "Board",
            selected: app.tab == Tab::Board && !on_task,
            badge: 0,
        },
        shell::NavItem {
            icon: icon::TRAY,
            label: "Inbox",
            selected: app.tab == Tab::Inbox && !on_task,
            badge: pending,
        },
    ];

    let signed_in_as = app
        .net
        .as_ref()
        .and_then(|n| n.data("__me"))
        .and_then(|m| m.get("email"))
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .to_string();
    let read_only = !app.can_write();

    let mut sign_out = false;
    let mut refresh = false;
    let me = signed_in_as.clone();
    let clicked = shell::sidebar(ui, &items, |ui| {
        ui.add_space(space::XS);
        if w::link(ui, "Sign out").clicked() {
            sign_out = true;
        }
        if w::link(ui, "Refresh").clicked() {
            refresh = true;
        }
        if read_only {
            w::pill(ui, "read only", colour::TEXT_MUTED);
        }
        if !me.is_empty() {
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                avatar::small(ui, &me, 24.0);
                ui.add_space(space::XS);
                ui.label(
                    egui::RichText::new(me.split('@').next().unwrap_or(&me))
                        .size(text::SMALL)
                        .color(colour::TEXT_MUTED),
                );
            });
        }
    });

    if sign_out {
        app.sign_out();
        return;
    }
    if refresh {
        if let Some(n) = app.net.as_mut() {
            n.results.clear();
        }
    }
    match clicked {
        Some(0) => {
            app.tab = Tab::Board;
            app.task = None;
        }
        Some(1) => {
            app.tab = Tab::Inbox;
            app.task = None;
        }
        _ => {}
    }

    shell::content(ui, |ui| {
        if app.task.is_some() {
            views::task::ui(app, ui);
        } else {
            match app.tab {
                Tab::Board => views::board::ui(app, ui),
                Tab::Inbox => views::inbox::ui(app, ui),
            }
        }
    });
}
