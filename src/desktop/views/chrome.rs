//! Routing and the app frame.
//!
//! The shell owns layout; this owns which view is showing and what the sidebar
//! contains. Adding a screen is a `NavItem` and a match arm.

use egui_phosphor::thin as icon;
use serde_json::Value;

use crate::desktop::design::{avatar, colour, shell, size, space, text, widgets as w};
use crate::desktop::{views, App, Tab};

/// Destinations, in sidebar order. The index is the routing contract.
const DESTINATIONS: [Tab; 4] = [Tab::Home, Tab::MyTasks, Tab::Projects, Tab::Inbox];

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let home = app.net.as_ref().and_then(|n| n.data("home")).cloned();
    let pending = home
        .as_ref()
        .and_then(|h| h.get("waitingOnMe"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let mine_open = home
        .as_ref()
        .and_then(|h| h.get("myTasks"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|t| t.get("status").and_then(Value::as_str) != Some("done"))
                .count()
        })
        .unwrap_or(0);
    let projects: Vec<(String, String)> = home
        .as_ref()
        .and_then(|h| h.get("projects"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some((
                        p.get("id")?.as_str()?.to_string(),
                        p.get("name")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let me = app
        .net
        .as_ref()
        .and_then(|n| n.data("__me"))
        .and_then(|m| m.get("email"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let role = app
        .net
        .as_ref()
        .and_then(|n| n.data("__me"))
        .and_then(|m| m.get("role"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let on_task = app.task.is_some();
    let sel = |t: Tab| app.tab == t && !on_task;

    // Project dots reuse the discipline palette so a project keeps one colour.
    let dot_for = |i: usize| match i % 3 {
        0 => colour::ACCENT,
        1 => colour::AGENT,
        _ => colour::OK,
    };

    let mut groups = vec![shell::NavGroup {
        label: "WORKSPACE",
        items: vec![
            shell::NavItem::new(icon::HOUSE, "Home", sel(Tab::Home)),
            shell::NavItem::new(icon::LIST_CHECKS, "My Tasks", sel(Tab::MyTasks))
                .count(mine_open.to_string()),
            shell::NavItem::new(icon::SQUARES_FOUR, "Projects", sel(Tab::Projects))
                .count(projects.len().to_string()),
            shell::NavItem::new(icon::TRAY, "Inbox", sel(Tab::Inbox)).badge(pending),
        ],
    }];

    if !projects.is_empty() {
        groups.push(shell::NavGroup {
            label: "PROJECTS",
            items: projects
                .iter()
                .enumerate()
                .map(|(i, (id, name))| {
                    let open = app.project.as_deref() == Some(id.as_str());
                    shell::NavItem::new("", name, open && app.tab == Tab::Projects && !on_task)
                        .dot(dot_for(i))
                })
                .collect(),
        });
    }

    let mut sign_out = false;
    let mut refresh = false;

    let clicked = shell::sidebar(
        ui,
        ("Control Plane", "Airtribe engineering"),
        &groups,
        |ui| {
            ui.horizontal(|ui| {
                if !me.is_empty() {
                    avatar::small(ui, &me, size::AVATAR_MD);
                    ui.add_space(space::XS);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(
                            egui::RichText::new(me.split('@').next().unwrap_or(&me))
                                .size(text::SMALL)
                                .color(colour::TEXT),
                        );
                        ui.label(
                            egui::RichText::new(&role)
                                .size(text::CAPTION)
                                .color(colour::TEXT_FAINT),
                        );
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if w::icon_button(ui, icon::SIGN_OUT, "", w::Emphasis::Ghost, true)
                        .on_hover_text("Sign out")
                        .clicked()
                    {
                        sign_out = true;
                    }
                    if w::icon_button(ui, icon::ARROWS_CLOCKWISE, "", w::Emphasis::Ghost, true)
                        .on_hover_text("Refresh")
                        .clicked()
                    {
                        refresh = true;
                    }
                });
            });
        },
    );

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
        Some((0, i)) => {
            app.tab = DESTINATIONS[i.min(DESTINATIONS.len() - 1)];
            app.task = None;
            if app.tab != Tab::Projects {
                app.project = None;
            }
        }
        Some((1, i)) => {
            if let Some((id, _)) = projects.get(i) {
                app.tab = Tab::Projects;
                app.project = Some(id.clone());
                app.task = None;
            }
        }
        _ => {}
    }

    shell::content(ui, |ui| {
        if app.task.is_some() {
            views::task::ui(app, ui);
        } else {
            match app.tab {
                Tab::Home => views::home::ui(app, ui),
                Tab::MyTasks => views::mytasks::ui(app, ui),
                Tab::Projects => views::board::ui(app, ui),
                Tab::Inbox => views::inbox::ui(app, ui),
            }
        }
    });

}
