//! Board: the project list, and inside a project its phases with their tasks.
//!
//! Two modes, chosen by `app.project`. Everything is read-only here; clicking a
//! task hands off to the task view by setting `app.task`.

use std::collections::HashMap;

use egui_phosphor::thin as icon;
use serde_json::Value;

use crate::desktop::design::{colour, space, status_colour, status_label, text, widgets as w};
use crate::desktop::App;

const STATUSES: [&str; 6] = ["open", "in_progress", "in_review", "blocked", "done", "dropped"];
const KINDS: [&str; 2] = ["human", "agent"];

#[derive(Default)]
pub struct State {
    /// `None` means "any"; both filters go to the server as query params.
    pub status: Option<&'static str>,
    pub assignee_kind: Option<&'static str>,
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    match app.project.clone() {
        None => projects(app, ui),
        Some(project_id) => project(app, ui, &project_id),
    }
}

// ---------------------------------------------------------------- project list

fn projects(app: &mut App, ui: &mut egui::Ui) {
    let net = app.net.as_mut().unwrap();
    net.get_once("board:projects", "/api/user/projects");

    let list = array(net.data("board:projects"));
    let loading = net.is_loading("board:projects");
    let error = net.error("board:projects").map(str::to_string);

    // Phase progress needs each project's phases. `get_once` caches, so this is
    // one request per project for the life of the session, not per frame.
    let mut progress: HashMap<String, (usize, usize)> = HashMap::new();
    for p in &list {
        let id = str_at(p, "id");
        if id.is_empty() {
            continue;
        }
        let key = format!("board:phases:{id}");
        net.get_once(&key, &format!("/api/user/projects/{id}/phases"));
        let phases = array(net.data(&key));
        if !phases.is_empty() {
            let done = phases.iter().filter(|p| str_at(p, "status") == "done").count();
            progress.insert(id.to_string(), (done, phases.len()));
        }
    }

    w::title(ui, "Projects");
    ui.add_space(space::MD);

    if let Some(err) = error {
        w::error(ui, &err);
        return;
    }
    if list.is_empty() {
        if loading {
            w::loading(ui, "projects");
        } else {
            w::empty(ui, "No projects yet.");
        }
        return;
    }

    let mut open: Option<String> = None;
    for p in &list {
        let id = str_at(p, "id").to_string();
        let name = str_at(p, "name").to_string();
        let key = str_at(p, "key").to_string();
        let status = str_at(p, "status").to_string();
        let counts = progress.get(&id).copied();

        let (hit, _) = w::card_button(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                w::heading(ui, &name);
                ui.add_space(space::SM);
                w::caption(ui, &key);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    w::pill(ui, status_label(&status), status_colour(&status));
                });
            });
            ui.add_space(space::XXS);
            match counts {
                Some((done, total)) => w::muted(ui, &format!("{done} of {total} phases done")),
                None => w::muted(ui, "phases loading"),
            }
        });

        if hit.clicked() {
            open = Some(id);
        }
        ui.add_space(space::SM);
    }

    if let Some(id) = open {
        app.project = Some(id);
    }
}

// -------------------------------------------------------------- project detail

fn project(app: &mut App, ui: &mut egui::Ui, project_id: &str) {
    let status = app.board.status;
    let kind = app.board.assignee_kind;

    let phases_key = format!("board:phases:{project_id}");
    let tasks_key = format!(
        "board:tasks:{project_id}:{}:{}",
        status.unwrap_or("any"),
        kind.unwrap_or("any")
    );
    let mut tasks_path = format!("/api/user/tasks?projectId={project_id}");
    if let Some(s) = status {
        tasks_path.push_str(&format!("&status={s}"));
    }
    if let Some(k) = kind {
        tasks_path.push_str(&format!("&assigneeKind={k}"));
    }

    let net = app.net.as_mut().unwrap();
    net.get_once(&phases_key, &format!("/api/user/projects/{project_id}/phases"));
    net.get_once(&tasks_key, &tasks_path);
    net.get_once("board:projects", "/api/user/projects");

    let mut phases = array(net.data(&phases_key));
    phases.sort_by_key(|p| p.get("position").and_then(Value::as_i64).unwrap_or(0));
    let phases_loading = net.is_loading(&phases_key);
    let phases_error = net.error(&phases_key).map(str::to_string);

    let tasks = array(net.data(&tasks_key));
    let tasks_loading = net.is_loading(&tasks_key);
    let tasks_error = net.error(&tasks_key).map(str::to_string);

    let title = array(net.data("board:projects"))
        .iter()
        .find(|p| str_at(p, "id") == project_id)
        .map(|p| (str_at(p, "name").to_string(), str_at(p, "key").to_string()));

    // Tasks arrive for the whole project in one call; bucket them per phase.
    let mut by_phase: HashMap<String, Vec<Value>> = HashMap::new();
    for t in tasks {
        by_phase.entry(str_at(&t, "phaseId").to_string()).or_default().push(t);
    }

    let mut back = false;
    let mut open_task: Option<String> = None;
    let mut filters_changed = false;

    ui.horizontal(|ui| {
        if w::link(ui, &format!("{} Projects", icon::ARROW_LEFT)).clicked() {
            back = true;
        }
        ui.add_space(space::SM);
        match &title {
            Some((name, key)) => {
                w::title(ui, name);
                ui.add_space(space::SM);
                w::caption(ui, key);
            }
            None => w::title(ui, "Project"),
        }
    });

    ui.add_space(space::MD);
    ui.horizontal(|ui| {
        filters_changed |=
            filter(ui, "board:filter:status", "Any status", &STATUSES, &mut app.board.status);
        ui.add_space(space::SM);
        filters_changed |=
            filter(ui, "board:filter:kind", "Anyone", &KINDS, &mut app.board.assignee_kind);

        if app.board.status.is_some() || app.board.assignee_kind.is_some() {
            ui.add_space(space::SM);
            if w::secondary(ui, "Clear", true).clicked() {
                app.board.status = None;
                app.board.assignee_kind = None;
                filters_changed = true;
            }
        }
    });
    ui.add_space(space::LG);

    if let Some(err) = phases_error {
        w::error(ui, &err);
    } else if phases.is_empty() {
        if phases_loading {
            w::loading(ui, "phases");
        } else {
            w::empty(ui, "This project has no phases yet.");
        }
    } else {
        for p in &phases {
            let phase_id = str_at(p, "id");
            let empty = Vec::new();
            let list = by_phase.get(phase_id).unwrap_or(&empty);
            let done = list.iter().filter(|t| str_at(t, "status") == "done").count();

            phase_header(ui, p, done, list.len());
            ui.add_space(space::SM);

            if let Some(err) = &tasks_error {
                w::error(ui, err);
            } else if list.is_empty() {
                if tasks_loading {
                    w::loading(ui, "tasks");
                } else {
                    w::empty(ui, "No tasks in this phase.");
                }
            } else {
                for t in list {
                    if let Some(id) = task_row(ui, t) {
                        open_task = Some(id);
                    }
                }
            }

            ui.add_space(space::XL);
        }
    }

    if back {
        app.project = None;
    }
    if let Some(id) = open_task {
        app.task = Some(id);
    }
    if filters_changed {
        app.net.as_mut().unwrap().invalidate_prefix("board:tasks");
    }
}

/// The header above a phase's tasks: its number, name, state and progress.
fn phase_header(ui: &mut egui::Ui, p: &Value, done: usize, total: usize) {
    let position = p.get("position").and_then(Value::as_i64).unwrap_or(0);
    let status = str_at(p, "status");

    ui.horizontal(|ui| {
        // Hand-painted: a monospaced ordinal, so phase numbers line up in a
        // column. No widget covers "small mono figure" yet.
        ui.label(
            egui::RichText::new(format!("{position:02}"))
                .monospace()
                .size(text::CAPTION)
                .color(colour::TEXT_FAINT),
        );
        ui.add_space(space::SM);
        w::heading(ui, str_at(p, "name"));
        ui.add_space(space::SM);
        w::pill(ui, status_label(status), status_colour(status));
        if p.get("gate").and_then(Value::as_bool).unwrap_or(false) {
            w::pill(ui, "gate", colour::WARN);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            w::muted(ui, &format!("{done}/{total} done"));
        });
    });
}

/// One task. Returns its id when clicked.
fn task_row(ui: &mut egui::Ui, t: &Value) -> Option<String> {
    let status = str_at(t, "status").to_string();
    let kind = str_at(t, "assigneeKind").to_string();
    let is_agent = kind == "agent";
    let claimed = str_at(t, "claimedBy").to_string();
    let id = str_at(t, "id").to_string();
    let title = str_at(t, "title").to_string();
    let priority = t.get("priority").and_then(Value::as_i64).unwrap_or(0);

    let hit = w::row(ui, |ui| {
        w::dot(ui, status_colour(&status));
        ui.add_space(space::XS);
        w::body(ui, &title);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            w::id(ui, &id);
            ui.add_space(space::SM);
            w::caption(ui, &format!("P{priority}"));
            ui.add_space(space::SM);
            w::muted(ui, status_label(&status));
            ui.add_space(space::SM);
            // The one pill on a task row: delegated work is what you scan for.
            if is_agent {
                let label = if claimed.is_empty() {
                    "agent".to_string()
                } else {
                    format!("agent · {claimed}")
                };
                w::pill(ui, &label, colour::AGENT);
            } else if kind == "human" {
                // The task JSON carries `assigneePersonId`, not an email, so
                // there is no seed for an avatar. Name the kind instead.
                w::muted(ui, "human");
            }
        });
    });

    hit.clicked().then_some(id)
}

// ---------------------------------------------------------------------- pieces

/// A one-of-many dropdown over `options`, `None` meaning any. Returns true when
/// the selection changed.
fn filter(
    ui: &mut egui::Ui,
    id: &str,
    any_label: &str,
    options: &[&'static str],
    slot: &mut Option<&'static str>,
) -> bool {
    let mut changed = false;
    let selected = status_label(slot.unwrap_or(any_label)).to_string();
    egui::ComboBox::from_id_salt(id)
        .selected_text(egui::RichText::new(selected).size(text::BODY))
        .show_ui(ui, |ui| {
            let any = egui::RichText::new(any_label).size(text::BODY);
            if ui.selectable_label(slot.is_none(), any).clicked() && slot.is_some() {
                *slot = None;
                changed = true;
            }
            for opt in options {
                let label = egui::RichText::new(status_label(opt)).size(text::BODY);
                if ui.selectable_label(*slot == Some(*opt), label).clicked()
                    && *slot != Some(*opt)
                {
                    *slot = Some(*opt);
                    changed = true;
                }
            }
        });
    changed
}

fn array(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
