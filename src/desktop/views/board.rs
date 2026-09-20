//! Board: the project list, and inside a project its phases with their tasks.
//!
//! Two modes, chosen by `app.project`. Everything is read-only here; clicking a
//! task hands off to the task view by setting `app.task`.

use std::collections::HashMap;

use serde_json::Value;

use crate::desktop::{theme, App};

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

    heading(ui, "Projects");

    if let Some(err) = error {
        ui.colored_label(theme::DANGER, err);
        return;
    }
    if list.is_empty() {
        if loading {
            ui.spinner();
        } else {
            muted(ui, "No projects yet.");
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

        let hit = card(ui, true, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&name).size(14.0).strong());
                ui.add_space(8.0);
                ui.label(egui::RichText::new(&key).monospace().size(11.0).color(theme::MUTED));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::pill(ui, &status, theme::status(&status));
                });
            });
            ui.add_space(3.0);
            match counts {
                Some((done, total)) => muted(ui, &format!("{done} of {total} phases done")),
                None => muted(ui, "phases loading"),
            }
        });

        if hit.clicked() {
            open = Some(id);
        }
        ui.add_space(8.0);
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
        if ui.small_button("Projects").clicked() {
            back = true;
        }
        ui.add_space(10.0);
        match &title {
            Some((name, key)) => {
                ui.label(egui::RichText::new(name).size(17.0).strong());
                ui.add_space(6.0);
                ui.label(egui::RichText::new(key).monospace().size(11.0).color(theme::MUTED));
            }
            None => {
                ui.label(egui::RichText::new("Project").size(17.0).strong());
            }
        }
    });

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        filters_changed |= filter(ui, "board:filter:status", "Any status", &STATUSES, &mut app.board.status);
        ui.add_space(6.0);
        filters_changed |= filter(ui, "board:filter:kind", "Anyone", &KINDS, &mut app.board.assignee_kind);

        if app.board.status.is_some() || app.board.assignee_kind.is_some() {
            ui.add_space(6.0);
            if ui.small_button("Clear").clicked() {
                app.board.status = None;
                app.board.assignee_kind = None;
                filters_changed = true;
            }
        }
    });
    ui.add_space(14.0);

    if let Some(err) = phases_error {
        ui.colored_label(theme::DANGER, err);
    } else if phases.is_empty() {
        if phases_loading {
            ui.spinner();
        } else {
            muted(ui, "This project has no phases yet.");
        }
    } else {
        for p in &phases {
            let phase_id = str_at(p, "id");
            let empty = Vec::new();
            let list = by_phase.get(phase_id).unwrap_or(&empty);
            let done = list.iter().filter(|t| str_at(t, "status") == "done").count();

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{:02}", p.get("position").and_then(Value::as_i64).unwrap_or(0)))
                        .monospace()
                        .size(11.0)
                        .color(theme::MUTED),
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new(str_at(p, "name")).size(14.0).strong());
                ui.add_space(6.0);
                let st = str_at(p, "status");
                theme::pill(ui, st, theme::status(st));
                if p.get("gate").and_then(Value::as_bool).unwrap_or(false) {
                    theme::pill(ui, "gate", theme::WARN);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    muted(ui, &format!("{done}/{} done", list.len()));
                });
            });

            ui.add_space(6.0);

            if let Some(err) = &tasks_error {
                ui.colored_label(theme::DANGER, err.as_str());
            } else if list.is_empty() {
                if tasks_loading {
                    ui.spinner();
                } else {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        muted(ui, "No tasks in this phase.");
                    });
                }
            } else {
                for t in list {
                    if let Some(id) = task_row(ui, t) {
                        open_task = Some(id);
                    }
                    ui.add_space(5.0);
                }
            }

            ui.add_space(20.0);
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

/// One task. Returns its id when clicked.
fn task_row(ui: &mut egui::Ui, t: &Value) -> Option<String> {
    let status = str_at(t, "status").to_string();
    let kind = str_at(t, "assigneeKind").to_string();
    let is_agent = kind == "agent";
    let claimed = str_at(t, "claimedBy").to_string();
    let id = str_at(t, "id").to_string();
    let title = str_at(t, "title").to_string();
    let priority = t.get("priority").and_then(Value::as_i64).unwrap_or(0);

    let hit = card(ui, true, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            dot(ui, theme::status(&status));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(&title).size(13.0));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::id_label(ui, &id);
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("P{priority}")).size(11.0).color(theme::MUTED));
                ui.add_space(4.0);
                theme::pill(ui, &status, theme::status(&status));
                if is_agent {
                    let label = if claimed.is_empty() {
                        "agent".to_string()
                    } else {
                        format!("agent · {claimed}")
                    };
                    theme::pill(ui, &label, theme::AGENT);
                } else if kind == "human" {
                    theme::pill(ui, "human", theme::MUTED);
                }
            });
        });
    });

    // Delegated work has to read at a glance, so it gets a spine of its own.
    if is_agent {
        let spine = egui::Rect::from_min_size(hit.rect.min, egui::vec2(3.0, hit.rect.height()));
        ui.painter().rect_filled(spine, 2.0, theme::AGENT);
    }

    hit.clicked().then_some(id)
}

// ---------------------------------------------------------------------- pieces

/// A bordered full-width row. Clickable ones outline on hover rather than
/// shifting colour, which keeps the page still as the pointer crosses it.
fn card(ui: &mut egui::Ui, clickable: bool, add: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    let inner = egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });

    if !clickable {
        return inner.response;
    }

    let response = inner
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            6.0,
            egui::Stroke::new(1.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// A fixed-width status marker, so every title in a list starts on the same x.
fn dot(ui: &mut egui::Ui, colour: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, colour);
}

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
    let selected = slot.unwrap_or(any_label).to_string();
    egui::ComboBox::from_id_salt(id)
        .selected_text(egui::RichText::new(selected).size(12.0))
        .show_ui(ui, |ui| {
            if ui.selectable_label(slot.is_none(), any_label).clicked() && slot.is_some() {
                *slot = None;
                changed = true;
            }
            for opt in options {
                if ui.selectable_label(*slot == Some(*opt), *opt).clicked() && *slot != Some(*opt) {
                    *slot = Some(*opt);
                    changed = true;
                }
            }
        });
    changed
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(17.0).strong());
    ui.add_space(12.0);
}

fn muted(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).color(theme::MUTED));
}

fn array(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
