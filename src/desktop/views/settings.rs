//! Settings: things scoped to you, not to a project.
//!
//! Folders today: where your agent works when a task has no project (or its
//! project has no folder for you). Private, like a repo's local path —
//! nobody else's list, and nobody else sees yours.

use egui::RichText;
use serde_json::{json, Value};

use crate::desktop::design::{
    cards as c, colour, radius, shell, size, space, text, theme, viz, widgets as w,
};
use crate::desktop::net::Net;
use crate::desktop::App;

pub(super) const FOLDERS_KEY: &str = "settings:folders";
const ACTION_KEY: &str = "settings:folders:action";

#[derive(Default)]
pub struct State {
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

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let App {
        net, settings: s, ..
    } = app;
    let net = net.as_mut().expect("signed in");
    want_folders(net);
    settle(net, s);

    shell::page_title(ui, "Settings", "", |_| {});

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
            .color(colour::TEXT_MUTED),
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
                        .color(colour::TEXT),
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
            .color(colour::TEXT_MUTED),
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
        .backdrop_color(colour::CANVAS.gamma_multiply(0.7))
        .frame(
            egui::Frame::new()
                .fill(colour::SURFACE)
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG))
                .corner_radius(radius::LG)
                .inner_margin(egui::Margin::same(space::XL as i8)),
        )
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                RichText::new(format!("Remove the \u{201c}{name}\u{201d} folder?"))
                    .size(text::CARD)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
            ui.add_space(space::SM);
            ui.label(
                RichText::new(
                    "Your agent won\u{2019}t offer it for a task with no project. A task already pinned \
                     to it falls back to your default.",
                )
                .size(text::BODY)
                .color(colour::TEXT_2),
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
