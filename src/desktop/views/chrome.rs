//! Routing and the app frame.
//!
//! The shell owns layout; this owns which view is showing and what the sidebar
//! contains. Adding a screen is a `NavItem` and a match arm.

use egui_phosphor::thin as icon;
use serde_json::Value;

use crate::desktop::design::{avatar, colour, motion, radius, shell, size, space, text, widgets as w};
use crate::desktop::{views, App, Tab};

/// Destinations, in sidebar order. The index is the routing contract.
const DESTINATIONS: [Tab; 3] = [Tab::Home, Tab::MyTasks, Tab::Projects];

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    // ⌘K from anywhere, before any view reads the keyboard this frame.
    if ui.ctx().input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
        if app.palette.open {
            app.palette.open = false;
        } else {
            views::palette::open(app);
        }
    }

    // Fetched here, not only by Home: the counts ride on every tab. The key is
    // Home's own, so on Home this is the same request, not a second one.
    let net = app.net.as_mut().expect("chrome runs signed in");
    net.get_once("home", "/api/user/home");
    let home = net.data("home");
    let list = |key: &str| {
        home.and_then(|h| h.get(key)).and_then(Value::as_array).cloned().unwrap_or_default()
    };
    // Open means still yours to act on: not finished on either track, and
    // not dropped. `doneAt` is the one finish line both tracks share.
    let mine_open = list("myTasks")
        .iter()
        .filter(|t| {
            t.get("doneAt").is_none_or(Value::is_null)
                && t.get("status").and_then(Value::as_str) != Some("dropped")
        })
        .count();
    let projects_active = list("projects")
        .iter()
        .filter(|p| p.get("status").and_then(Value::as_str) == Some("active"))
        .count();

    let me = net.data("__me");
    let field = |k: &str| {
        me.and_then(|m| m.get(k)).and_then(Value::as_str).unwrap_or("").to_string()
    };
    let email = field("email");
    let role = field("role");
    // The address is the fallback only for a person with no name on file.
    let name = Some(field("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| email.clone());

    let on_task = app.task.is_some();
    let sel = |t: Tab| app.tab == t && !on_task;

    let groups = vec![shell::NavGroup {
        label: "WORKSPACE",
        items: vec![
            shell::NavItem::new(icon::HOUSE, "Home", sel(Tab::Home)),
            shell::NavItem::new(icon::LIST_CHECKS, "My Tasks", sel(Tab::MyTasks))
                .count(mine_open.to_string()),
            shell::NavItem::new(icon::SQUARES_FOUR, "Projects", sel(Tab::Projects))
                .count(projects_active.to_string()),
        ],
    }];

    let mut sign_out = false;
    let mut refresh = false;

    let (clicked, search) = shell::sidebar(
        ui,
        ("Control Plane", "Airtribe engineering"),
        &groups,
        |ui| {
            // Collapsed, there is room for the two actions and nothing else;
            // the name moves into the avatar's hover text.
            if ui.available_width() < size::SIDEBAR_W / 2.0 {
                // Bottom-up, like the sidebar's foot it sits in: the avatar
                // first so it anchors the corner, the actions stacked above.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    if !email.is_empty() {
                        avatar::small(ui, &email, size::AVATAR_MD)
                            .on_hover_text(format!("{name}\n{}", sentence(&role)));
                        ui.add_space(space::XS);
                    }
                    refresh |= footer_icon(ui, icon::ARROWS_CLOCKWISE, "Refresh", true);
                    sign_out |= footer_icon(ui, icon::SIGN_OUT, "Sign out", true);
                });
                return;
            }
            // The buttons are laid out first so they own their corner: a long
            // name then truncates into what is left instead of running under
            // them.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    sign_out |= footer_icon(ui, icon::SIGN_OUT, "Sign out", false);
                    refresh |= footer_icon(ui, icon::ARROWS_CLOCKWISE, "Refresh", false);
                    if email.is_empty() {
                        return;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        avatar::small(ui, &email, size::AVATAR_MD);
                        ui.add_space(space::XS);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&name)
                                        .size(text::SMALL)
                                        .color(colour::TEXT),
                                )
                                .truncate()
                                .selectable(false),
                            )
                            .on_hover_text(email.as_str());
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(sentence(&role))
                                        .size(text::CAPTION)
                                        .color(colour::TEXT_FAINT),
                                )
                                .truncate()
                                .selectable(false),
                            );
                        });
                    });
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
    if search {
        views::palette::open(app);
    }
    match clicked {
        Some((0, i)) => {
            app.tab = DESTINATIONS[i.min(DESTINATIONS.len() - 1)];
            app.task = None;
            if app.tab != Tab::Projects {
                app.project = None;
            }
        }
        _ => {}
    }

    // Before the content, so its Enter and arrow keys are spent on the
    // palette and not on whatever form sits underneath it.
    views::palette::ui(app, ui);

    shell::content(ui, |ui| {
        if app.task.is_some() {
            views::task::ui(app, ui);
        } else {
            match app.tab {
                Tab::Home => views::home::ui(app, ui),
                Tab::MyTasks => views::mytasks::ui(app, ui),
                Tab::Projects => views::board::ui(app, ui),
            }
        }
    });
}

/// `compact` for the collapsed rail, which is narrower than a padded button.
fn footer_icon(ui: &mut egui::Ui, glyph: &str, label: &str, compact: bool) -> bool {
    let response = if compact {
        let r = ui.add(
            egui::Button::new(
                egui::RichText::new(glyph).size(text::HEADING).color(colour::TEXT_MUTED),
            )
            .frame(false)
            .min_size(egui::vec2(size::ICON_COL, size::ICON_COL)),
        );
        motion::operable(ui, r, radius::SM as f32)
    } else {
        w::icon_button(ui, glyph, "", w::Emphasis::Ghost, true)
    };
    response.on_hover_text(label).clicked()
}

/// Roles are stored lower case ("admin"); the footer reads as a caption.
fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|f| f.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
