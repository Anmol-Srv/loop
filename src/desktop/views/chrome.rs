//! Routing and the app frame.
//!
//! The shell owns layout; this owns which view is showing and what the sidebar
//! contains. Adding a screen is a `NavItem` and a match arm.

use egui_phosphor::thin as icon;
use serde_json::Value;

use crate::desktop::design::{avatar, colour, motion, radius, shell, size, space, text, widgets as w};
use crate::desktop::{views, App, Tab};

/// The sidebar's badges. Not under `__`: the 30 s refresh keeps them current.
/// Anything that changes a task or a project drops it alongside `home`.
pub const COUNTS: &str = "sidebar:counts";

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    // ⌘K from anywhere, before any view reads the keyboard this frame.
    if ui.ctx().input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
        if app.palette.open {
            app.palette.open = false;
        } else {
            views::palette::open(app);
        }
    }

    // The counts ride on every tab, so they are their own small fetch rather
    // than a read of Home's payload, which every other tab would then wait on.
    // Open means still yours to act on: not finished on either track, and not
    // dropped — the server counts it that way.
    let net = app.net.as_mut().expect("chrome runs signed in");
    net.get_once(COUNTS, "/api/user/counts");
    let count = |k: &str| {
        net.data(COUNTS).and_then(|c| c.get(k)).and_then(Value::as_i64).unwrap_or(0)
    };
    let mine_open = count("myOpen");
    let projects_active = count("activeProjects");
    let triage = count("triage").max(0) as usize;

    let me = net.data("__me");
    let field = |k: &str| {
        me.and_then(|m| m.get(k)).and_then(Value::as_str).unwrap_or("").to_string()
    };
    let email = field("email");
    let role = field("role");
    // The address is the fallback only for a person with no name on file.
    let name = Some(field("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| email.clone());

    // Which server this is, beside who you are: with a hosted and a local one
    // both in play, a write to the wrong one is the mistake worth preventing.
    let server = net.base_url.clone();
    let host = server.split("://").last().unwrap_or(&server).split('/').next().unwrap_or("").to_string();

    let on_task = app.task.is_some();
    let sel = |t: Tab| app.tab == t && !on_task;

    // Triage is a row of its own only while something waits in it: an
    // attention badge, under My Tasks, which is where its group sits.
    let mut items = vec![
        (shell::NavItem::new(icon::HOUSE, "Home", sel(Tab::Home)), Tab::Home),
        (
            shell::NavItem::new(icon::LIST_CHECKS, "My Tasks", sel(Tab::MyTasks)).count(mine_open.to_string()),
            Tab::MyTasks,
        ),
    ];
    if triage > 0 {
        items.push((shell::NavItem::new(icon::TRAY, "Triage", false).badge(triage), Tab::MyTasks));
    }
    items.push((
        shell::NavItem::new(icon::SQUARES_FOUR, "Projects", sel(Tab::Projects)).count(projects_active.to_string()),
        Tab::Projects,
    ));
    items.push((shell::NavItem::new(icon::ROBOT, "Agents", sel(Tab::Agents)), Tab::Agents));
    let destinations: Vec<Tab> = items.iter().map(|(_, t)| *t).collect();
    let groups = vec![shell::NavGroup { label: "WORKSPACE", items: items.into_iter().map(|(i, _)| i).collect() }];

    let mut sign_out = false;
    let mut refresh = false;

    let (clicked, search) = shell::sidebar(
        ui,
        ("Loop", "Airtribe engineering"),
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
                            .on_hover_text(format!("{name}\n{}\n{server}", sentence(&role)));
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
                            .on_hover_text(format!("{email}\n{}", sentence(&role)));
                            // The server, not the role, under the name: the role
                            // rarely changes, and a write to the wrong server is
                            // the mistake worth preventing. A third line pushed
                            // the footer off the window.
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&host)
                                        .size(text::CAPTION)
                                        .color(colour::TEXT_FAINT),
                                )
                                .truncate()
                                .selectable(false),
                            )
                            .on_hover_text(server.as_str());
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
            app.tab = destinations[i.min(destinations.len() - 1)];
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
                Tab::Agents => views::agents::ui(app, ui),
            }
        }
    });

    // After every page, so a task's confirm dialog sits over whichever one
    // asked. A task deleted from its own page leaves it.
    let net = app.net.as_mut().expect("chrome runs signed in");
    let gone = views::menus::settle(ui.ctx(), net, &mut app.board.tasks);
    if gone.is_some() && gone == app.task {
        app.task = None;
    }
    w::toasts(ui.ctx());
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
