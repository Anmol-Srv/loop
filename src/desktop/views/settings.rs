//! Settings: things scoped to you, not to a project. Opened from the account
//! menu at the sidebar's foot, or ⌘,.
//!
//! Profile: who you are to the server, and how the app looks on this Mac.
//! Folders: where your agent works when a task has no project (or its
//! project has no folder for you). Private, like a repo's local path —
//! nobody else's list, and nobody else sees yours.
//! Members: admin only — the team, from `acp-admin` moved into the app.

use chrono::{DateTime, Utc};
use egui::RichText;
use serde_json::{json, Value};

use super::board::str_at;
use super::projects::PEOPLE_KEY;
use crate::desktop::design::{
    avatar, cards as c, colour, motion, radius, shell, size, space, text, theme, viz, widgets as w,
};
use crate::desktop::net::Net;
use crate::desktop::App;

pub(super) const FOLDERS_KEY: &str = "settings:folders";
const ACTION_KEY: &str = "settings:folders:action";

const MEMBERS_KEY: &str = "settings:members";
const MEMBERS_ACTION_KEY: &str = "settings:members:action";

#[derive(Default)]
pub struct State {
    /// Which section is showing: 0 Profile, 1 Folders, 2 Members (admin only).
    section: usize,
    adding: bool,
    name: String,
    path: String,
    error: Option<String>,
    removing: Option<String>,
    busy: bool,
    members: Members,
}

/// What the setup-code result card, once it lands, needs to build the invite
/// message: whose it is and the code itself.
#[derive(Clone)]
struct InviteResult {
    name: String,
    email: String,
    code: String,
}

/// Which mutation `MEMBERS_ACTION_KEY` is carrying, so `settle_members` knows
/// what a success means: a new code to show, or nothing to say at all.
enum MemberAction {
    Add,
    Invite { name: String, email: String },
    Patch,
    Revoke,
}

#[derive(Default)]
struct Members {
    adding: bool,
    name: String,
    email: String,
    department: Option<String>,
    role: Option<String>,
    error: Option<String>,
    busy: bool,
    action: Option<MemberAction>,
    result: Option<InviteResult>,
    revoking: Option<String>,
}

/// Open Settings on Members with the invite form ready — the account menu's
/// "Invite people". Harmless for a non-admin: the tab never shows for them.
pub fn open_invite(s: &mut State) {
    s.section = 2;
    if s.members.result.is_none() && !s.members.adding {
        start_member_add(s);
    }
}

const MEMBER_DEPARTMENTS: [&str; 3] = ["design", "frontend", "backend"];
const MEMBER_ROLES: [&str; 3] = ["member", "manager", "admin"];

/// The viewer's own folders, for pickers elsewhere (the task rail, triage's
/// menu). Empty until a page that needs them has asked — call `want_folders`
/// first.
pub(super) fn folders(net: &Net) -> Vec<Value> {
    net.data(FOLDERS_KEY)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

pub(super) fn want_folders(net: &mut Net) {
    net.get_once(FOLDERS_KEY, "/api/user/folders");
}

/// The name of the one folder with `isDefault`, if there is one.
pub(super) fn default_name(folders: &[Value]) -> Option<&str> {
    folders
        .iter()
        .find(|f| f.get("isDefault").and_then(Value::as_bool) == Some(true))
        .and_then(|f| f.get("name"))
        .and_then(Value::as_str)
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let scopes = app.scopes.clone();
    let App {
        net, settings: s, ..
    } = app;
    let net = net.as_mut().expect("signed in");
    want_folders(net);
    settle(net, s);
    let is_admin = net.data("__me").is_some_and(|m| str_at(m, "role") == "admin");
    if is_admin {
        net.get_once(MEMBERS_KEY, "/api/admin/people");
    }
    settle_members(ui.ctx(), net, s);

    shell::page_title(ui, "Settings", "Your account, and how Loop looks on this Mac.", |_| {});
    let sections: &[&str] = if is_admin { &["Profile", "Folders", "Members"] } else { &["Profile", "Folders"] };
    if let Some(i) = c::tabs(ui, sections, s.section.min(sections.len() - 1)) {
        s.section = i;
    }
    match (s.section, is_admin) {
        (0, _) => profile(ui, net, &scopes),
        (2, true) => members_section(ui, net, s),
        _ => folders_section(ui, net, s, can_write),
    }
}

// ------------------------------------------------------------------ profile

fn profile(ui: &mut egui::Ui, net: &Net, scopes: &[String]) {
    let me = net.data("__me");
    let field = |k: &str| me.and_then(|m| m.get(k)).and_then(Value::as_str).unwrap_or("").to_owned();
    let email = field("email");
    let name = Some(field("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| email.clone());
    let role = field("role");
    let department = field("department");

    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add_space(space::XS);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::LG;
            avatar::small(ui, &email, PROFILE_AVATAR);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::XXS;
                ui.add_space(space::XS);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&name)
                            .size(text::TITLE)
                            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                            .color(colour::TEXT()),
                    );
                    if !role.is_empty() {
                        c::chip(ui, &sentence(&role), c::Tone::Neutral, false);
                    }
                });
                ui.label(RichText::new(&email).size(text::BODY).color(colour::TEXT_MUTED()));
            });
        });
        ui.add_space(space::LG);
        shell::property(ui, "Department", |ui| {
            if department.is_empty() {
                w::muted(ui, "None");
            } else {
                c::chip(ui, &sentence(&department), c::discipline_tone(&department), true);
            }
        });
        shell::property(ui, "Server", |ui| {
            ui.label(RichText::new(&net.base_url).monospace().size(text::SMALL).color(colour::TEXT_2()));
        });
        shell::property(ui, "Access", |ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            if scopes.is_empty() {
                w::muted(ui, "None");
            }
            for scope in scopes {
                c::chip(ui, &sentence(scope), c::Tone::Quiet, false);
            }
        });
        ui.add_space(space::SM);
        ui.label(
            RichText::new("Your name, role and department are managed by an admin.")
                .size(text::SMALL)
                .color(colour::TEXT_FAINT()),
        );
    });

    shell::section(ui, "Appearance");
    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        let ctx = ui.ctx().clone();
        let mut a = theme::appearance(&ctx);
        let before = a;
        w::caption(ui, "Theme");
        ui.add_space(space::SM);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::MD, space::MD);
            for mode in theme::Mode::ALL {
                if theme_tile(ui, mode, a.mode == mode, a.accent).clicked() {
                    a.mode = mode;
                }
            }
        });
        ui.add_space(space::LG);
        w::caption(ui, "Accent");
        ui.add_space(space::XS);
        ui.horizontal(|ui| {
            if let Some(i) = accent_swatches(ui, a.accent) {
                a.accent = i;
            }
            ui.add_space(space::XS);
            w::muted(ui, colour::ACCENTS[a.accent].name);
        });
        ui.add_space(space::SM);
        ui.label(
            RichText::new("Saved on this Mac. System follows macOS as it switches between light and dark.")
                .size(text::SMALL)
                .color(colour::TEXT_FAINT()),
        );
        if a != before {
            theme::set_appearance(&ctx, a);
        }
    });
    ui.add_space(space::XXL);
}

/// Room for the Choose… button beside a path field.
pub(super) const CHOOSE_W: f32 = 100.0;

/// Bigger than the sidebar's: this is the page about you.
const PROFILE_AVATAR: f32 = 56.0;
const TILE: egui::Vec2 = egui::vec2(156.0, 98.0);

/// A theme choice drawn as a small window in that palette — sidebar, a card,
/// the accent button — so the choice is seen rather than read. System is the
/// two halves side by side.
fn theme_tile(ui: &mut egui::Ui, mode: theme::Mode, on: bool, accent: usize) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(TILE + egui::vec2(0.0, space::XL), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, format!("{} theme", mode.label()))
    });
    let response = motion::operable(ui, response, radius::LG as f32);
    let hot = response.hovered() || response.has_focus();
    let tile = egui::Rect::from_min_size(rect.min, TILE);
    let a = &colour::ACCENTS[accent];
    match mode {
        theme::Mode::Dark => paint_window(ui.painter(), tile, &colour::DUSK, a.dark[0]),
        theme::Mode::Light => paint_window(ui.painter(), tile, &colour::DAYLIGHT, a.light[0]),
        theme::Mode::System => {
            let (left, right) = tile.split_left_right_at_fraction(0.5);
            paint_window(&ui.painter().with_clip_rect(left), tile, &colour::DUSK, a.dark[0]);
            paint_window(&ui.painter().with_clip_rect(right), tile, &colour::DAYLIGHT, a.light[0]);
        }
    }
    let p = ui.painter();
    let (ring, width) = match (on, hot) {
        (true, _) => (colour::ACCENT(), 2.0),
        (false, true) => (colour::LINE_STRONG(), 1.0),
        _ => (colour::LINE(), 1.0),
    };
    p.rect_stroke(tile, radius::LG as f32, egui::Stroke::new(width, ring), egui::StrokeKind::Outside);
    p.text(
        egui::pos2(tile.left() + space::XXS, tile.bottom() + space::SM),
        egui::Align2::LEFT_TOP,
        mode.label(),
        egui::FontId::new(
            text::BODY,
            egui::FontFamily::Name(if on { theme::SEMIBOLD } else { theme::MEDIUM }.into()),
        ),
        if on { colour::TEXT() } else { colour::TEXT_MUTED() },
    );
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

fn paint_window(p: &egui::Painter, r: egui::Rect, pal: &colour::Palette, accent: egui::Color32) {
    let rr = radius::LG as f32;
    p.rect_filled(r, rr, pal.CANVAS);
    let side = egui::Rect::from_min_max(r.min, egui::pos2(r.left() + r.width() * 0.3, r.bottom()));
    p.rect_filled(side, egui::CornerRadius { nw: radius::LG, sw: radius::LG, ne: 0, se: 0 }, pal.CHROME);
    for (i, w) in [0.55, 0.7, 0.45].iter().enumerate() {
        let y = side.top() + 16.0 + i as f32 * 12.0;
        let bar = egui::Rect::from_min_size(egui::pos2(side.left() + 8.0, y), egui::vec2(side.width() * w, 4.0));
        if i == 0 {
            p.rect_filled(bar.expand2(egui::vec2(4.0, 3.0)), 3.0, pal.SURFACE_ACTIVE);
        }
        p.rect_filled(bar, 2.0, if i == 0 { pal.TEXT } else { pal.TEXT_FAINT });
    }
    let card = egui::Rect::from_min_max(
        egui::pos2(side.right() + 10.0, r.top() + 14.0),
        egui::pos2(r.right() - 10.0, r.bottom() - 14.0),
    );
    p.rect_filled(card, 4.0, pal.SURFACE);
    p.rect_stroke(card, 4.0, egui::Stroke::new(1.0, pal.LINE), egui::StrokeKind::Inside);
    let line = |y: f32, w: f32, c: egui::Color32| {
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(card.left() + 8.0, y), egui::vec2(card.width() * w, 4.0)), 2.0, c);
    };
    line(card.top() + 10.0, 0.6, pal.TEXT);
    line(card.top() + 20.0, 0.8, pal.TEXT_MUTED);
    line(card.top() + 28.0, 0.5, pal.TEXT_MUTED);
    let button = egui::Rect::from_min_size(
        egui::pos2(card.left() + 8.0, card.bottom() - 18.0),
        egui::vec2(card.width() * 0.4, 10.0),
    );
    p.rect_filled(button, 3.0, accent);
}

fn accent_swatches(ui: &mut egui::Ui, selected: usize) -> Option<usize> {
    let light = colour::is_light();
    let items: Vec<(egui::Color32, &str)> = colour::ACCENTS
        .iter()
        .map(|a| (if light { a.light[0] } else { a.dark[0] }, a.name))
        .collect();
    viz::swatches(ui, &items, selected)
}

/// The account menu's appearance block: the three modes as one switch, the
/// accents under it. The Profile page has the fuller version.
pub(super) fn appearance_compact(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut a = theme::appearance(&ctx);
    let before = a;
    ui.horizontal(|ui| {
        ui.add_space(space::XS);
        w::caption(ui, "Appearance");
    });
    ui.add_space(space::XXS);
    let items = [
        (egui_phosphor::regular::CIRCLE_HALF, "System"),
        (egui_phosphor::regular::MOON, "Dark"),
        (egui_phosphor::regular::SUN, "Light"),
    ];
    let at = theme::Mode::ALL.iter().position(|m| *m == a.mode).unwrap_or(0);
    if let Some(i) = viz::view_switch(ui, &items, at, "theme") {
        a.mode = theme::Mode::ALL[i];
    }
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        ui.add_space(space::XS);
        if let Some(i) = accent_swatches(ui, a.accent) {
            a.accent = i;
        }
    });
    if a != before {
        theme::set_appearance(&ctx, a);
    }
}

/// The system folder panel, opened at `start` when that is a folder. `None`
/// when it is cancelled. Blocks the frame while the panel is up, which is
/// what a modal panel means; macOS runs its own loop meanwhile.
pub(super) fn choose_folder(title: &str, start: &str) -> Option<String> {
    let mut panel = rfd::FileDialog::new().set_title(title);
    let start = std::path::Path::new(start.trim());
    if start.is_dir() {
        panel = panel.set_directory(start);
    }
    panel.pick_folder().map(|p| p.display().to_string())
}

/// The button beside a path field that opens the panel.
pub(super) fn choose_button(ui: &mut egui::Ui, enabled: bool) -> bool {
    w::icon_button(ui, egui_phosphor::regular::FOLDER_OPEN, "Choose\u{2026}", w::Emphasis::Secondary, enabled)
        .on_hover_text("Pick the folder in Finder")
        .clicked()
}

fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|f| f.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

// ------------------------------------------------------------------ folders

fn folders_section(ui: &mut egui::Ui, net: &mut Net, s: &mut State, can_write: bool) {
    let rows = folders(net);
    let error = net.error(FOLDERS_KEY).map(str::to_owned);
    let loading = net.is_loading(FOLDERS_KEY) && rows.is_empty() && error.is_none();

    shell::section_count_with(ui, "Folders", rows.len(), |ui| {
        if can_write && !s.adding && w::ghost(ui, "+ Add folder").clicked() {
            s.adding = true;
            s.name.clear();
            s.path.clear();
            s.error = None;
        }
    });
    ui.label(
        RichText::new("Folders your agent can work in when a task has no project.")
            .size(text::SMALL)
            .color(colour::TEXT_MUTED()),
    );
    ui.add_space(space::SM);

    if let Some(err) = &error {
        w::error(ui, err);
        return;
    }
    if let Some(err) = &s.error {
        w::error(ui, err);
        ui.add_space(space::SM);
    }

    if s.adding {
        add_form(ui, net, s);
        ui.add_space(space::MD);
    }

    if loading {
        w::loading(ui, "Loading folders");
        return;
    }
    if rows.is_empty() {
        if !s.adding {
            w::empty(
                ui,
                "No folders yet.",
                "Add one so your agent has somewhere to work when a task has no project.",
            );
        }
        return;
    }

    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                ui.add_space(space::MD);
            }
            folder_row(ui, net, row, s, can_write);
        }
    });

    if let Some(id) = s.removing.clone() {
        let name = rows
            .iter()
            .find(|f| f.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("this folder")
            .to_owned();
        match confirm_remove(ui.ctx(), egui::Id::new("settings:folder:remove"), &name) {
            Some(true) => {
                s.removing = None;
                s.busy = true;
                net.invalidate(ACTION_KEY);
                net.send(
                    ACTION_KEY,
                    reqwest::Method::DELETE,
                    &format!("/api/user/folders/{id}"),
                    Value::Null,
                );
            }
            Some(false) => s.removing = None,
            None => {}
        }
    }
}

fn add_form(ui: &mut egui::Ui, net: &mut Net, s: &mut State) {
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        w::field(ui, "Name", &mut s.name, false, "e.g. mycohort-api\u{2026}");
        ui.add_space(space::MD);
        ui.horizontal(|ui| {
            let room = ui.available_width() - CHOOSE_W - space::SM;
            ui.allocate_ui_with_layout(
                egui::vec2(room, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| w::field(ui, "Path", &mut s.path, false, "/Users/you/code/mycohort-api"),
            );
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                if choose_button(ui, true) {
                    if let Some(path) = choose_folder("Choose a folder for your agent", &s.path) {
                        // The folder's own name is almost always what it
                        // should be called here.
                        if s.name.trim().is_empty() {
                            s.name = std::path::Path::new(&path)
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                        }
                        s.path = path;
                    }
                }
            });
        });
        let path_typed = !s.path.trim().is_empty();
        let path_ok = s.path.trim().starts_with('/');
        if path_typed && !path_ok {
            ui.add_space(space::XS);
            w::error(
                ui,
                "Needs a full path starting with /, like /Users/you/code/mycohort-api.",
            );
        }
        ui.add_space(space::MD);
        let ready = !s.name.trim().is_empty() && path_ok && !s.busy;
        ui.horizontal(|ui| {
            if w::primary(ui, "Add", ready).clicked() {
                s.busy = true;
                s.error = None;
                net.invalidate(ACTION_KEY);
                net.post(
                    ACTION_KEY,
                    "/api/user/folders",
                    json!({ "name": s.name.trim(), "path": s.path.trim() }),
                );
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                s.adding = false;
            }
        });
    });
}

fn folder_row(ui: &mut egui::Ui, net: &mut Net, row: &Value, s: &mut State, can_write: bool) {
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
    let path = row.get("path").and_then(Value::as_str).unwrap_or_default();
    let is_default = row
        .get("isDefault")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    ui.horizontal(|ui| {
        ui.set_min_height(size::CONTROL);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if can_write {
                viz::more(ui, |ui| {
                    if !is_default && viz::menu_item(ui, "Set as default", false, None) {
                        s.busy = true;
                        net.invalidate(ACTION_KEY);
                        net.post(
                            ACTION_KEY,
                            &format!("/api/user/folders/{id}/default"),
                            Value::Null,
                        );
                    }
                    if viz::menu_item(ui, "Remove\u{2026}", true, None) {
                        s.removing = Some(id.clone());
                    }
                });
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                ui.label(
                    RichText::new(name)
                        .size(text::BODY)
                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                        .color(colour::TEXT()),
                );
                if is_default {
                    c::chip(ui, "Default", c::Tone::Ok, false);
                }
            });
        });
    });
    ui.add_space(space::XXS);
    ui.label(
        RichText::new(path)
            .size(text::SMALL)
            .color(colour::TEXT_MUTED()),
    );
}

/// Fold in whatever the add, remove or set-default request returned.
fn settle(net: &mut Net, s: &mut State) {
    if s.busy && !net.is_loading(ACTION_KEY) {
        s.busy = false;
        match net.peek(ACTION_KEY).cloned() {
            Some(Ok(_)) => {
                net.invalidate(ACTION_KEY);
                net.invalidate(FOLDERS_KEY);
                s.adding = false;
            }
            Some(Err(e)) => {
                net.invalidate(ACTION_KEY);
                s.error = Some(e);
            }
            None => {}
        }
    }
}

/// "Remove the "mycohort-api" folder?" — Some(true) to remove, Some(false) to
/// keep, None while it is still asking. The same shape as `viz::confirm_remove`,
/// which is a label's wording and private to `tag_picker`; this is a folder's.
fn confirm_remove(ctx: &egui::Context, id: egui::Id, name: &str) -> Option<bool> {
    let mut answer = None;
    let modal = egui::Modal::new(id.with("modal"))
        .backdrop_color(colour::CANVAS().gamma_multiply(0.7))
        .frame(
            egui::Frame::new()
                .fill(colour::SURFACE())
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG()))
                .corner_radius(radius::LG)
                .inner_margin(egui::Margin::same(space::XL as i8)),
        )
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                RichText::new(format!("Remove the \u{201c}{name}\u{201d} folder?"))
                    .size(text::CARD)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT()),
            );
            ui.add_space(space::SM);
            ui.label(
                RichText::new(
                    "Your agent won\u{2019}t offer it for a task with no project. A task already pinned \
                     to it falls back to your default.",
                )
                .size(text::BODY)
                .color(colour::TEXT_2()),
            );
            ui.add_space(space::LG);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::danger(ui, "Remove", true).clicked() {
                    answer = Some(true);
                }
                if w::ghost(ui, "Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
    if modal.should_close() && answer.is_none() {
        answer = Some(false);
    }
    answer
}

// ------------------------------------------------------------------ members

fn members(net: &Net) -> Vec<Value> {
    net.data(MEMBERS_KEY).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn members_section(ui: &mut egui::Ui, net: &mut Net, s: &mut State) {
    let rows = members(net);
    let error = net.error(MEMBERS_KEY).map(str::to_owned);
    let loading = net.is_loading(MEMBERS_KEY) && rows.is_empty() && error.is_none();
    let me_id = net.data("__me").map(|m| str_at(m, "personId").to_owned());

    shell::section_count_with(ui, "Members", rows.len(), |ui| {
        if !s.members.adding && w::primary(ui, "Invite someone", true).clicked() {
            start_member_add(s);
        }
    });
    ui.add_space(space::SM);

    if let Some(result) = s.members.result.clone() {
        if member_result_card(ui, net, &result) {
            s.members.result = None;
        }
        ui.add_space(space::MD);
    }

    if let Some(err) = &error {
        w::error(ui, err);
        return;
    }
    if let Some(err) = &s.members.error {
        w::error(ui, err);
        ui.add_space(space::SM);
    }

    if s.members.adding {
        member_add_form(ui, net, s);
        ui.add_space(space::MD);
    }

    if loading {
        w::loading(ui, "Loading members");
        return;
    }
    if rows.is_empty() {
        if !s.members.adding {
            w::empty(ui, "No members yet.", "Invite someone to get the team onto Loop.");
        }
        return;
    }

    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                ui.add_space(space::MD);
            }
            member_row(ui, net, row, s, me_id.as_deref());
        }
    });

    if let Some(id) = s.members.revoking.clone() {
        let name = rows
            .iter()
            .find(|p| p.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("this person")
            .to_owned();
        match confirm_revoke(ui.ctx(), egui::Id::new("settings:member:revoke"), &name) {
            Some(true) => {
                if let Some(row) = rows.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(id.as_str())) {
                    let email = row.get("email").and_then(Value::as_str).unwrap_or_default().to_owned();
                    s.members.revoking = None;
                    s.members.busy = true;
                    s.members.action = Some(MemberAction::Revoke);
                    net.invalidate(MEMBERS_ACTION_KEY);
                    net.post(MEMBERS_ACTION_KEY, "/api/admin/revoke", json!({ "email": email }));
                }
            }
            Some(false) => s.members.revoking = None,
            None => {}
        }
    }
}

fn start_member_add(s: &mut State) {
    s.members.adding = true;
    s.members.name.clear();
    s.members.email.clear();
    s.members.department = None;
    s.members.role = Some("member".to_owned());
    s.members.error = None;
}

fn member_add_form(ui: &mut egui::Ui, net: &mut Net, s: &mut State) {
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        w::field(ui, "Name", &mut s.members.name, false, "e.g. Priya Sharma\u{2026}");
        ui.add_space(space::MD);
        w::field(ui, "Email", &mut s.members.email, false, "name@airtribe.live");
        let email_typed = !s.members.email.trim().is_empty();
        let email_ok = s.members.email.trim().to_lowercase().ends_with("@airtribe.live");
        if email_typed && !email_ok {
            ui.add_space(space::XS);
            w::error(ui, "Needs an @airtribe.live address.");
        }
        ui.add_space(space::MD);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                w::caption(ui, "Department");
                ui.add_space(space::XXS);
                let options: Vec<(String, String)> =
                    MEMBER_DEPARTMENTS.iter().map(|d| (d.to_string(), sentence(d))).collect();
                viz::select(ui, "Department", &options, &mut s.members.department);
            });
            ui.add_space(space::MD);
            ui.vertical(|ui| {
                w::caption(ui, "Role");
                ui.add_space(space::XXS);
                let options: Vec<(String, String)> =
                    MEMBER_ROLES.iter().map(|r| (r.to_string(), sentence(r))).collect();
                viz::select(ui, "Role", &options, &mut s.members.role);
            });
        });
        ui.add_space(space::MD);
        let ready = !s.members.name.trim().is_empty()
            && email_ok
            && s.members.department.is_some()
            && s.members.role.is_some()
            && !s.members.busy;
        ui.horizontal(|ui| {
            if w::primary(ui, "Add and create setup code", ready).clicked() {
                s.members.busy = true;
                s.members.error = None;
                s.members.action = Some(MemberAction::Add);
                net.invalidate(MEMBERS_ACTION_KEY);
                net.post(
                    MEMBERS_ACTION_KEY,
                    "/api/admin/people",
                    json!({
                        "email": s.members.email.trim(),
                        "name": s.members.name.trim(),
                        "department": s.members.department.clone().unwrap_or_default(),
                        "role": s.members.role.clone().unwrap_or_else(|| "member".to_owned()),
                    }),
                );
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                s.members.adding = false;
            }
        });
    });
}

fn member_row(ui: &mut egui::Ui, net: &mut Net, row: &Value, s: &mut State, me_id: Option<&str>) {
    let id = row.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
    let email = row.get("email").and_then(Value::as_str).unwrap_or_default();
    let role = row.get("role").and_then(Value::as_str).unwrap_or_default();
    let department = row.get("department").and_then(Value::as_str).unwrap_or_default();
    let invite_expires_at = row.get("inviteExpiresAt").and_then(Value::as_str);
    let last_seen_at = row.get("lastSeenAt").and_then(Value::as_str);
    // A live session counts as joined too: someone signed in by an admin's
    // `acp-admin session` has no password yet, but they are plainly in.
    let joined = row.get("joined").and_then(Value::as_bool).unwrap_or(false) || last_seen_at.is_some();
    let is_me = me_id == Some(id.as_str());

    ui.horizontal(|ui| {
        ui.set_min_height(size::CONTROL);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            viz::more(ui, |ui| {
                if viz::menu_item(ui, "New setup code", false, None) {
                    s.members.busy = true;
                    s.members.action =
                        Some(MemberAction::Invite { name: name.to_owned(), email: email.to_owned() });
                    net.invalidate(MEMBERS_ACTION_KEY);
                    net.post(MEMBERS_ACTION_KEY, "/api/admin/invite", json!({ "email": email }));
                }
                let why = is_me.then_some("You can\u{2019}t remove your own access");
                if viz::menu_item(ui, "Remove access\u{2026}", true, why) {
                    s.members.revoking = Some(id.clone());
                }
            });

            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                avatar::small(ui, email, size::AVATAR_MD);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::XXS;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(name)
                                .size(text::BODY)
                                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                                .color(colour::TEXT()),
                        );
                        member_status_chip(ui, joined, invite_expires_at);
                    });
                    ui.label(RichText::new(email).size(text::SMALL).color(colour::TEXT_MUTED()));
                });
            });
        });
    });

    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;

        let mut dept_slot: Option<String> = None;
        let dept_options: Vec<(String, String)> = MEMBER_DEPARTMENTS
            .iter()
            .filter(|d| **d != department)
            .map(|d| (d.to_string(), sentence(d)))
            .collect();
        viz::value_select(ui, &sentence(department), &dept_options, &mut dept_slot);
        if let Some(next) = dept_slot {
            s.members.busy = true;
            s.members.action = Some(MemberAction::Patch);
            net.invalidate(MEMBERS_ACTION_KEY);
            net.patch(MEMBERS_ACTION_KEY, &format!("/api/admin/people/{id}"), json!({ "department": next }));
        }

        let mut role_slot: Option<String> = None;
        let role_options: Vec<(String, String)> = MEMBER_ROLES
            .iter()
            .filter(|r| **r != role)
            .map(|r| (r.to_string(), sentence(r)))
            .collect();
        viz::value_select(ui, &sentence(role), &role_options, &mut role_slot);
        if let Some(next) = role_slot {
            s.members.busy = true;
            s.members.action = Some(MemberAction::Patch);
            net.invalidate(MEMBERS_ACTION_KEY);
            net.patch(MEMBERS_ACTION_KEY, &format!("/api/admin/people/{id}"), json!({ "role": next }));
        }

        if let Some(seen) = last_seen_at {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                w::muted(ui, &format!("Last seen {}", relative(seen)));
            });
        }
    });
}

fn member_status_chip(ui: &mut egui::Ui, joined: bool, invite_expires_at: Option<&str>) {
    if joined {
        c::chip(ui, "Joined", c::Tone::Ok, false);
        return;
    }
    match invite_expires_at {
        Some(t) => {
            let h = hours_left(t);
            if h > 0 {
                c::chip(ui, &format!("Invite pending \u{00b7} expires in {h}h"), c::Tone::Running, false);
            } else {
                c::chip(ui, "Invite expired", c::Tone::Quiet, false);
            }
        }
        None => {
            c::chip(ui, "Not invited", c::Tone::Quiet, false);
        }
    }
}

/// The setup-code result card. True once "Done" is clicked, for the caller to
/// dismiss it — this draws the card and nothing else, so it cannot also hold
/// the state it would need to dismiss itself.
fn member_result_card(ui: &mut egui::Ui, net: &Net, r: &InviteResult) -> bool {
    let mut done = false;
    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        w::caption(ui, "Setup code");
        ui.add_space(space::XS);
        ui.label(
            RichText::new(&r.code)
                .monospace()
                .size(text::TITLE)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT()),
        );
        ui.add_space(space::SM);
        ui.label(
            RichText::new(format!("Valid 48 hours, single use. For {}.", r.email))
                .size(text::SMALL)
                .color(colour::TEXT_MUTED()),
        );
        ui.add_space(space::MD);
        ui.horizontal(|ui| {
            if w::secondary(ui, "Copy code", true).clicked() {
                ui.ctx().copy_text(r.code.clone());
                w::toast(ui.ctx(), "Copied.", false);
            }
            ui.add_space(space::XS);
            if w::secondary(ui, "Copy invite message", true).clicked() {
                ui.ctx().copy_text(invite_message(r, &net.base_url));
                w::toast(ui.ctx(), "Copied.", false);
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Done").clicked() {
                done = true;
            }
        });
    });
    done
}

fn invite_message(r: &InviteResult, base_url: &str) -> String {
    format!(
        "Hey {name}, you\u{2019}re set up on Loop \u{2014} here\u{2019}s how to get in:\n\n\
         1. Install Loop \u{2014} paste this into Terminal:\n   \
            {install}\n   \
            (Run the same command any time to update.)\n\
         2. Loop opens; choose \u{201c}First time here?\u{201d}\n\
         3. Enter:\n   \
            Email: {email}\n   \
            Code: {code}\n   \
            Password: pick one, 8+ characters\n   \
            Server: {base_url}\n\n\
         The code is valid for 48 hours and works once, so set it up soon.",
        name = r.name,
        email = r.email,
        code = r.code,
        install = INSTALL_COMMAND,
    )
}

/// Installs Loop, and updates it: always the latest GitHub release
/// (`scripts/install.sh`, published by `scripts/release-mac.sh`).
const INSTALL_COMMAND: &str =
    "curl -fsSL https://raw.githubusercontent.com/Anmol-Srv/loop/master/scripts/install.sh | bash";

/// "Remove Jane's access?" — the same shape as `confirm_remove`, worded for a
/// person rather than a folder: what leaves is their sessions and their place
/// on the team, not a row they can re-add in a click.
fn confirm_revoke(ctx: &egui::Context, id: egui::Id, name: &str) -> Option<bool> {
    let mut answer = None;
    let modal = egui::Modal::new(id.with("modal"))
        .backdrop_color(colour::CANVAS().gamma_multiply(0.7))
        .frame(
            egui::Frame::new()
                .fill(colour::SURFACE())
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG()))
                .corner_radius(radius::LG)
                .inner_margin(egui::Margin::same(space::XL as i8)),
        )
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                RichText::new(format!("Remove {name}\u{2019}s access?"))
                    .size(text::CARD)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT()),
            );
            ui.add_space(space::SM);
            ui.label(
                RichText::new(
                    "Every session they hold ends immediately. Their history on the board stays; \
                     they can be invited back later under the same address.",
                )
                .size(text::BODY)
                .color(colour::TEXT_2()),
            );
            ui.add_space(space::LG);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::danger(ui, "Remove access", true).clicked() {
                    answer = Some(true);
                }
                if w::ghost(ui, "Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
    if modal.should_close() && answer.is_none() {
        answer = Some(false);
    }
    answer
}

/// Fold in whatever the add, invite, patch or revoke request returned.
fn settle_members(ctx: &egui::Context, net: &mut Net, s: &mut State) {
    if !s.members.busy || net.is_loading(MEMBERS_ACTION_KEY) {
        return;
    }
    s.members.busy = false;
    let action = s.members.action.take();
    match net.peek(MEMBERS_ACTION_KEY).cloned() {
        Some(Ok(data)) => {
            net.invalidate(MEMBERS_ACTION_KEY);
            net.invalidate(MEMBERS_KEY);
            net.invalidate(PEOPLE_KEY);
            match action {
                Some(MemberAction::Add) => {
                    let person = data.get("person");
                    let name = person.and_then(|p| p.get("name")).and_then(Value::as_str).unwrap_or_default();
                    let email = person.and_then(|p| p.get("email")).and_then(Value::as_str).unwrap_or_default();
                    let code = data.get("code").and_then(Value::as_str).unwrap_or_default();
                    s.members.result = Some(InviteResult {
                        name: name.to_owned(),
                        email: email.to_owned(),
                        code: code.to_owned(),
                    });
                    s.members.adding = false;
                }
                Some(MemberAction::Invite { name, email }) => {
                    let code = data.get("code").and_then(Value::as_str).unwrap_or_default().to_owned();
                    s.members.result = Some(InviteResult { name, email, code });
                }
                Some(MemberAction::Revoke) => {
                    w::toast(ctx, "Access removed.", false);
                }
                Some(MemberAction::Patch) | None => {}
            }
        }
        Some(Err(e)) => {
            net.invalidate(MEMBERS_ACTION_KEY);
            match action {
                Some(MemberAction::Add) => s.members.error = Some(e),
                _ => w::toast(ctx, e, true),
            }
        }
        None => {}
    }
}

/// Parse a server timestamp. Every date this view reads comes back as
/// RFC3339, the same as `board`'s.
fn parse_ts(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts).ok().map(|t| t.with_timezone(&Utc))
}

/// Hours left on a setup code, rounded up so "expires in 1h" still means
/// there is time, not none. 0 once it has actually expired.
fn hours_left(expires_at: &str) -> i64 {
    let Some(then) = parse_ts(expires_at) else { return 0 };
    let mins = (then - Utc::now()).num_minutes();
    if mins <= 0 { 0 } else { (mins + 59) / 60 }
}

/// How long ago, for a last-seen caption.
fn relative(ts: &str) -> String {
    let Some(then) = parse_ts(ts) else { return String::new() };
    match (Utc::now() - then).num_seconds().max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}
