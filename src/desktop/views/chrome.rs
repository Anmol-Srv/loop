//! Routing and the app frame.
//!
//! The shell owns layout; this owns which view is showing and what the sidebar
//! contains. Adding a screen is a `NavItem` and a match arm.

use egui_phosphor::regular as icon;
use serde_json::Value;

use crate::desktop::design::{avatar, colour, motion, radius, shell, size, space, text, theme, viz, widgets as w};
use crate::desktop::{views, App, Tab};

/// The sidebar's badges. Not under `__`: the 30 s refresh keeps them current.
/// Anything that changes a task or a project drops it alongside `home`.
pub const COUNTS: &str = "sidebar:counts";

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    // An agent's page asked for from another view (its name on a task).
    views::agents::follow(app);

    // ⌘K from anywhere, before any view reads the keyboard this frame.
    if ui.ctx().input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
        if app.palette.open {
            app.palette.open = false;
        } else {
            views::palette::open(app);
        }
    }

    views::new_task::shortcut(app, ui.ctx());

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

    // Triage is a row of its own only while something waits in it — or while
    // you are on it, so emptying it does not pull the floor out from under
    // you. An attention badge, under My Tasks.
    let mut items = vec![
        (shell::NavItem::new(icon::HOUSE, "Home", sel(Tab::Home)), Tab::Home),
        (
            shell::NavItem::new(icon::LIST_CHECKS, "My Tasks", sel(Tab::MyTasks)).count(mine_open.to_string()),
            Tab::MyTasks,
        ),
    ];
    if triage > 0 || app.tab == Tab::Triage {
        items.push((shell::NavItem::new(icon::TRAY, "Triage", sel(Tab::Triage)).badge(triage), Tab::Triage));
    }
    items.push((
        shell::NavItem::new(icon::SQUARES_FOUR, "Projects", sel(Tab::Projects)).count(projects_active.to_string()),
        Tab::Projects,
    ));
    items.push((shell::NavItem::new(icon::ROBOT, "Agents", sel(Tab::Agents)), Tab::Agents));
    let destinations: Vec<Tab> = items.iter().map(|(_, t)| *t).collect();
    // Settings is not a destination here: it lives in the account menu at the
    // sidebar's foot, where account-level things belong, and on ⌘,.
    let groups = vec![shell::NavGroup { label: "WORKSPACE", items: items.into_iter().map(|(i, _)| i).collect() }];

    let mut sign_out = false;
    let mut refresh = false;
    let mut open_settings =
        ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma));
    let settings_open = sel(Tab::Settings);

    let (clicked, search) = shell::sidebar(ui, ("Loop", "Airtribe engineering"), &groups, |ui| {
        if email.is_empty() {
            return;
        }
        let narrow = ui.available_width() < size::SIDEBAR_W / 2.0;
        let who = Who { name: &name, email: &email, role: &role, host: &host, server: &server };
        let button = account_button(ui, &who, narrow, settings_open);
        viz::menu_above(&button, ACCOUNT_MENU_W, |ui| {
            account_header(ui, &who);
            viz::menu_rule(ui);
            ui.add_space(space::XS);
            views::settings::appearance_compact(ui);
            ui.add_space(space::SM);
            viz::menu_rule(ui);
            open_settings |= viz::menu_item_with(ui, icon::GEAR_SIX, "Settings", "\u{2318},");
            refresh |= viz::menu_item_with(ui, icon::ARROWS_CLOCKWISE, "Refresh", "");
            viz::menu_rule(ui);
            sign_out |= viz::menu_item_with(ui, icon::SIGN_OUT, "Sign out", "");
        });
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
    if search {
        views::palette::open(app);
    }
    match clicked {
        Some((0, i)) => {
            views::agents::close();
            app.tab = destinations[i.min(destinations.len() - 1)];
            app.task = None;
            if app.tab != Tab::Projects {
                app.project = None;
            }
        }
        _ => {}
    }
    if open_settings {
        views::agents::close();
        app.tab = Tab::Settings;
        app.task = None;
        app.project = None;
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
                Tab::Triage => views::triage::page(app, ui),
                Tab::Projects => views::board::ui(app, ui),
                Tab::Agents => views::agents::ui(app, ui),
                Tab::Settings => views::settings::ui(app, ui),
            }
        }
    });

    views::new_task::ui(app, ui.ctx());
    // After every page, so a task's confirm dialog sits over whichever one
    // asked. A task deleted from its own page leaves it.
    let net = app.net.as_mut().expect("chrome runs signed in");
    let gone = views::menus::settle(ui.ctx(), net, &mut app.board.tasks);
    if gone.is_some() && gone == app.task {
        app.task = None;
    }
    w::toasts(ui.ctx());
}

/// The account menu's width: the sidebar's own, so it reads as part of it
/// rather than a panel laid over the page. Room for the theme switch.
const ACCOUNT_MENU_W: f32 = size::SIDEBAR_W - space::LG;
/// The account button's height — two lines of text and an avatar, a larger
/// target than a nav row because it is the corner people aim for.
const ACCOUNT_H: f32 = 44.0;

struct Who<'a> {
    name: &'a str,
    email: &'a str,
    role: &'a str,
    host: &'a str,
    server: &'a str,
}

/// Who is signed in, and where: the sidebar's foot. One target that opens the
/// account menu, rather than a row of bare icons beside a truncated name.
/// Collapsed, it is the avatar alone.
fn account_button(ui: &mut egui::Ui, who: &Who<'_>, narrow: bool, active: bool) -> egui::Response {
    let w = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, ACCOUNT_H), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Account: {}", who.name))
    });
    let response = motion::operable(ui, response, radius::MD as f32);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let hot = response.hovered() || response.has_focus();
    let fill = if open || active {
        colour::SURFACE_ACTIVE()
    } else {
        motion::hover_fill(ui, response.id.with("fill"), hot, colour::TRANSPARENT(), colour::SURFACE_HOVER())
    };
    let p = ui.painter();
    if fill != colour::TRANSPARENT() {
        p.rect_filled(rect, radius::MD as f32, fill);
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    if narrow {
        let disc = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(size::AVATAR_MD + 4.0));
        avatar::paint(p, disc, who.email);
        return if open {
            response
        } else {
            response.on_hover_text(format!("{}\n{} \u{00B7} {}", who.name, sentence(who.role), who.server))
        };
    }

    let side = size::AVATAR_MD + 4.0;
    let disc = egui::Rect::from_min_size(
        egui::pos2(rect.left() + space::SM, rect.center().y - side / 2.0),
        egui::Vec2::splat(side),
    );
    avatar::paint(p, disc, who.email);

    let caret_w = size::ICON_COL;
    let x = disc.right() + space::SM;
    let room = (rect.right() - space::SM - caret_w - x).max(0.0);
    let name = w::truncated(
        ui,
        who.name,
        egui::FontId::new(text::BODY, egui::FontFamily::Name(theme::SEMIBOLD.into())),
        colour::TEXT(),
        room,
    );
    // The server under the name, beside the role: with a hosted and a local
    // one both in play, a write to the wrong one is the mistake to prevent.
    let sub = [sentence(who.role), who.host.to_owned()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{00B7} ");
    let sub = w::truncated(ui, &sub, egui::FontId::proportional(text::CAPTION), colour::TEXT_FAINT(), room);
    let top = rect.center().y - (name.size().y + sub.size().y) / 2.0;
    let sub_y = top + name.size().y;
    let p = ui.painter();
    p.galley(egui::pos2(x, top), name, colour::TEXT());
    p.galley(egui::pos2(x, sub_y), sub, colour::TEXT_FAINT());
    p.text(
        egui::pos2(rect.right() - space::SM - caret_w / 2.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        icon::CARET_UP_DOWN,
        egui::FontId::proportional(text::BODY),
        if hot || open { colour::TEXT_2() } else { colour::TEXT_FAINT() },
    );
    if open {
        response
    } else {
        response.on_hover_text(who.server)
    }
}

/// The top of the account menu: who, and the full server address the button
/// had to cut short.
fn account_header(ui: &mut egui::Ui, who: &Who<'_>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        ui.add_space(space::XS);
        avatar::small(ui, who.email, size::AVATAR_MD + 4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(
                egui::Label::new(
                    egui::RichText::new(who.name)
                        .size(text::BODY)
                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                        .color(colour::TEXT()),
                )
                .truncate()
                .selectable(false),
            );
            ui.add(
                egui::Label::new(egui::RichText::new(who.email).size(text::CAPTION).color(colour::TEXT_MUTED()))
                    .truncate()
                    .selectable(false),
            );
        });
    });
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        ui.add_space(space::XS);
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("Signed in to {}", who.host))
                    .size(text::CAPTION)
                    .color(colour::TEXT_FAINT()),
            )
            .truncate()
            .selectable(false),
        )
        .on_hover_text(who.server);
    });
    ui.add_space(space::XS);
}

/// Roles are stored lower case ("admin"); the footer reads as a caption.
fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|f| f.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
