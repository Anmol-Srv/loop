//! Settings: things scoped to you, not to a project. Opened from the account
//! menu at the sidebar's foot, or ⌘,.
//!
//! Profile: who you are to the server, and how the app looks on this Mac.
//! Folders: where your agent works when a task has no project (or its
//! project has no folder for you). Private, like a repo's local path —
//! nobody else's list, and nobody else sees yours.

use egui::RichText;
use serde_json::{json, Value};

use crate::desktop::design::{
    avatar, cards as c, colour, motion, radius, shell, size, space, text, theme, viz, widgets as w,
};
use crate::desktop::net::Net;
use crate::desktop::App;

pub(super) const FOLDERS_KEY: &str = "settings:folders";
const ACTION_KEY: &str = "settings:folders:action";

#[derive(Default)]
pub struct State {
    /// Which section is showing: 0 Profile, 1 Folders.
    section: usize,
    adding: bool,
    name: String,
    path: String,
    error: Option<String>,
    removing: Option<String>,
    busy: bool,
}

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

const SECTIONS: [&str; 2] = ["Profile", "Folders"];

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let scopes = app.scopes.clone();
    let App {
        net, settings: s, ..
    } = app;
    let net = net.as_mut().expect("signed in");
    want_folders(net);
    settle(net, s);

    shell::page_title(ui, "Settings", "Your account, and how Loop looks on this Mac.", |_| {});
    if let Some(i) = c::tabs(ui, &SECTIONS, s.section) {
        s.section = i;
    }
    match s.section {
        0 => profile(ui, net, &scopes),
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
    ui.add_space(space::XS);
    let items = [
        (egui_phosphor::thin::CIRCLE_HALF, "System"),
        (egui_phosphor::thin::MOON, "Dark"),
        (egui_phosphor::thin::SUN, "Light"),
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
        w::field(
            ui,
            "Path",
            &mut s.path,
            false,
            "/Users/you/code/mycohort-api",
        );
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
