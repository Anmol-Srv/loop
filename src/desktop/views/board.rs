//! Board: the project list, and inside a project its flow, phases and tasks.
//!
//! Two modes, chosen by `app.project`. The project detail screen leads with the
//! flow strip — done/total per discipline — because the question a lead asks
//! first is "where is the work", not "which phase are we in". The strip reports
//! only: it does not gate anything, and a discipline may run ahead of the one
//! to its left. The arrow says "usually in this order", nothing stronger.

use std::collections::HashMap;

use egui_phosphor::thin as icon;
use serde_json::Value;

use crate::desktop::design::tokens::{discipline_colour, DISCIPLINE_W};
use crate::desktop::design::avatar;
use crate::desktop::design::{
    colour, shell, size, space, status_colour, status_label, text, widgets as w,
};
use crate::desktop::App;

const STATUSES: [&str; 6] = ["open", "in_progress", "in_review", "blocked", "done", "dropped"];
const KINDS: [&str; 2] = ["human", "agent"];

/// The usual order of the flow. Anything the server reports that is not in
/// here keeps its own order, after these — a new discipline should appear
/// rather than vanish because this list has not caught up.
const FLOW_ORDER: [&str; 3] = ["design", "frontend", "backend"];

/// Where a claim's reply is collected.
const CLAIM_KEY: &str = "board:claim";

/// Room kept on the right of a task row so the title truncates rather than
/// running under the avatar, pill or Claim button that follows it. Four
/// discipline columns, which is the widest of the three endings.
/// ponytail: a reservation, not a measurement. Lay the trailing group out
/// first and read its width back if a blocker title ever outgrows this.
const TRAILING_TASK: f32 = DISCIPLINE_W * 4.0;

#[derive(Default)]
pub struct State {
    /// `None` means "any"; both filters go to the server as query params.
    pub status: Option<&'static str>,
    pub assignee_kind: Option<&'static str>,
    /// A claim is out; its reply invalidates the board when it lands.
    pub claiming: bool,
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

    // One `/flow` per project — the same request count the old per-project
    // `/phases` fetch cost, and it answers the question the card actually asks
    // (how much of the work is done, not how many phases are closed).
    // `get_once` caches, so this is once per project for the session.
    let mut flows: HashMap<String, Value> = HashMap::new();
    for p in &list {
        let id = str_at(p, "id");
        if id.is_empty() {
            continue;
        }
        let key = format!("board:flow:{id}");
        net.get_once(&key, &format!("/api/user/projects/{id}/flow"));
        if let Some(flow) = net.data(&key) {
            flows.insert(id.to_string(), flow.clone());
        }
    }

    shell::page_title(ui, "Projects", "", |_| {});

    if let Some(err) = error {
        w::error(ui, &err);
        return;
    }
    if list.is_empty() {
        if loading {
            w::loading(ui, "projects");
        } else {
            w::empty(ui, "No projects yet.", "Create one with: acp project new <key> <name>");
        }
        return;
    }

    let mut open: Option<String> = None;
    for p in &list {
        let id = str_at(p, "id").to_string();
        let name = str_at(p, "name").to_string();
        let key = str_at(p, "key").to_string();
        let status = str_at(p, "status").to_string();
        let counts = flows.get(&id).map(|f| (num_at(f, "done"), num_at(f, "total")));

        let (hit, _) = w::card_button(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                w::heading(ui, &name);
                ui.add_space(space::SM);
                w::mono_caption(ui, &key);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    w::pill(ui, status_label(&status), status_colour(&status));
                });
            });
            ui.add_space(space::SM);
            match counts {
                Some((done, total)) => {
                    w::progress(ui, fraction(done, total), ui.available_width(), colour::ACCENT);
                    ui.add_space(space::XS);
                    w::muted(ui, &format!("{done} of {total} done"));
                }
                None => w::muted(ui, "progress loading"),
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

    let flow_key = format!("board:flow:{project_id}");
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

    // Fold in a claim that has come back, before anything reads the cache. On
    // success the board and the home screen both hold the old assignee; on
    // failure the reply stays put so the row below can show why.
    if app.board.claiming && !net.is_loading(CLAIM_KEY) {
        match net.peek(CLAIM_KEY) {
            Some(Ok(_)) => {
                app.board.claiming = false;
                // `board:claim` is itself under this prefix, so the reply is
                // dropped along with the stale lists. That is what we want.
                net.invalidate_prefix("board:");
                net.invalidate("home");
            }
            Some(Err(_)) => app.board.claiming = false,
            None => {}
        }
    }

    net.get_once(&flow_key, &format!("/api/user/projects/{project_id}/flow"));
    net.get_once(&phases_key, &format!("/api/user/projects/{project_id}/phases"));
    net.get_once(&tasks_key, &tasks_path);

    let flow = net.data(&flow_key).cloned();
    let flow_loading = net.is_loading(&flow_key);
    let flow_error = net.error(&flow_key).map(str::to_string);

    let mut phases = array(net.data(&phases_key));
    phases.sort_by_key(|p| p.get("position").and_then(Value::as_i64).unwrap_or(0));
    let phases_loading = net.is_loading(&phases_key);
    let phases_error = net.error(&phases_key).map(str::to_string);

    let tasks = array(net.data(&tasks_key));
    let tasks_loading = net.is_loading(&tasks_key);
    let tasks_error = net.error(&tasks_key).map(str::to_string);
    let claim_error = net.error(CLAIM_KEY).map(str::to_string);

    // Tasks arrive for the whole project in one call; bucket them per phase,
    // and index them by id so a blocker can be named rather than numbered.
    let mut by_phase: HashMap<String, Vec<Value>> = HashMap::new();
    let mut titles: HashMap<String, String> = HashMap::new();
    for t in tasks {
        titles.insert(str_at(&t, "id").to_string(), str_at(&t, "title").to_string());
        by_phase.entry(str_at(&t, "phaseId").to_string()).or_default().push(t);
    }

    let mut back = false;
    let mut open_task: Option<String> = None;
    let mut claim: Option<String> = None;
    let mut filters_changed = false;

    if shell::back(ui, "Projects").clicked() {
        back = true;
    }
    // The project key goes in the trailing slot, not the subtitle: the subtitle
    // is a sentence about progress, and a mono identifier reads as a label on
    // the title rather than a second line of prose about it.
    match &flow {
        Some(f) => {
            let key = str_at(f, "key").to_string();
            shell::page_title(
                ui,
                str_at(f, "name"),
                &format!("{} of {} done", num_at(f, "done"), num_at(f, "total")),
                |ui| w::mono_caption(ui, &key),
            );
        }
        None => shell::page_title(ui, "Project", "", |_| {}),
    }

    if let Some(err) = flow_error {
        w::error(ui, &err);
    } else {
        match &flow {
            Some(f) => flow_strip(ui, f),
            None if flow_loading => w::loading(ui, "flow"),
            None => w::empty(ui, "No flow for this project yet.", ""),
        }
    }

    ui.add_space(space::XL);
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

    if let Some(err) = claim_error {
        ui.add_space(space::SM);
        w::error(ui, &err);
    }
    ui.add_space(space::LG);

    if let Some(err) = phases_error {
        w::error(ui, &err);
    } else if phases.is_empty() {
        if phases_loading {
            w::loading(ui, "phases");
        } else {
            w::empty(
                ui,
                "This project has no phases yet.",
                "A phase groups the tasks for one stage of the work.",
            );
        }
    } else {
        for p in &phases {
            let phase_id = str_at(p, "id");
            let none = Vec::new();
            let list = by_phase.get(phase_id).unwrap_or(&none);
            let done = list.iter().filter(|t| str_at(t, "status") == "done").count();

            phase_header(ui, p, done, list.len());
            ui.add_space(space::SM);

            if let Some(err) = &tasks_error {
                w::error(ui, err);
            } else if list.is_empty() {
                if tasks_loading {
                    w::loading(ui, "tasks");
                } else {
                    w::empty(ui, "No tasks in this phase.", "");
                }
            } else {
                w::card_list(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for t in list {
                        match task_row(ui, t, &titles) {
                            Some(Hit::Open(id)) => open_task = Some(id),
                            Some(Hit::Claim(id)) => claim = Some(id),
                            None => {}
                        }
                    }
                });
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
    let net = app.net.as_mut().unwrap();
    if filters_changed {
        net.invalidate_prefix("board:tasks");
    }
    if let Some(id) = claim {
        // Drop a previous claim's error, so the banner belongs to this attempt.
        net.invalidate(CLAIM_KEY);
        net.post(CLAIM_KEY, &format!("/api/user/tasks/{id}/claim"), Value::Null);
        app.board.claiming = true;
    }
}

/// The flow strip: one column per discipline that has tasks.
///
/// It reports, it does not gate. A column is where that discipline stands, and
/// the single arrow after design says only what the usual order is — frontend
/// and backend can and do run before design has finished.
fn flow_strip(ui: &mut egui::Ui, flow: &Value) {
    let mut columns: Vec<&Value> = flow
        .get("disciplines")
        .and_then(Value::as_array)
        .map(|d| d.iter().filter(|d| num_at(d, "total") > 0).collect())
        .unwrap_or_default();
    // Usual order first, then whatever else the server reports.
    columns.sort_by_key(|d| {
        FLOW_ORDER
            .iter()
            .position(|o| *o == str_at(d, "discipline"))
            .unwrap_or(FLOW_ORDER.len())
    });

    if columns.is_empty() {
        w::empty(ui, "No tasks have a discipline yet.", "");
        return;
    }

    w::card(ui, |ui| {
        let full = ui.available_width();
        ui.set_width(full);
        let arrow = if columns.len() > 1 { 1.0 } else { 0.0 };
        let gaps = space::LG * (columns.len() as f32 - 1.0 + arrow);
        let width =
            ((full - gaps - space::MD * arrow) / columns.len() as f32).max(DISCIPLINE_W);

        // The floor above stops a column collapsing to nothing, which means at
        // enough disciplines the strip is wider than the card. `horizontal_top`
        // neither wraps nor scrolls, so it would simply run off the edge; this
        // scrolls instead. Below the floor it shrinks to fit and no bar shows,
        // so today's three disciplines are unchanged.
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = space::LG;
                for (i, d) in columns.iter().enumerate() {
                    let name = str_at(d, "discipline");
                    let (done, total) = (num_at(d, "done"), num_at(d, "total"));
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        w::muted(ui, name);
                        ui.add_space(space::XS);
                        w::progress(ui, fraction(done, total), width, discipline_colour(name));
                        ui.add_space(space::XS);
                        w::caption(ui, &format!("{done} / {total} done"));
                    });
                    // Hand-painted: a faint glyph between two columns. No widget
                    // is a bare separator, and `muted` is a step too bright.
                    if i == 0 && columns.len() > 1 {
                        ui.label(
                            egui::RichText::new(icon::ARROW_RIGHT)
                                .size(text::BODY)
                                .color(colour::TEXT_FAINT),
                        );
                    }
                }
            });
        });
    });
}

/// The header above a phase's tasks: its number, name, state and progress.
fn phase_header(ui: &mut egui::Ui, p: &Value, done: usize, total: usize) {
    let position = p.get("position").and_then(Value::as_i64).unwrap_or(0);
    let status = str_at(p, "status");

    ui.horizontal(|ui| {
        w::mono_caption(ui, &format!("{position:02}"));
        ui.add_space(space::SM);
        w::heading(ui, str_at(p, "name"));
        ui.add_space(space::SM);
        w::pill(ui, status_label(status), status_colour(status));
        if p.get("gate").and_then(Value::as_bool).unwrap_or(false) {
            w::pill(ui, "gate", colour::WARN);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            w::muted(ui, &format!("{done} / {total}"));
        });
    });
}

/// What a click on a row meant.
enum Hit {
    Open(String),
    Claim(String),
}

/// One task. `titles` names blockers that are in this project's task list.
fn task_row(ui: &mut egui::Ui, t: &Value, titles: &HashMap<String, String>) -> Option<Hit> {
    let status = str_at(t, "status").to_string();
    let kind = str_at(t, "assigneeKind").to_string();
    let is_agent = kind == "agent";
    let claimed = str_at(t, "claimedBy").to_string();
    let person = str_at(t, "assigneePersonId").to_string();
    let id = str_at(t, "id").to_string();
    let title = str_at(t, "title").to_string();
    let discipline = str_at(t, "discipline").to_string();
    let assigned = !kind.is_empty() || !claimed.is_empty() || !person.is_empty();
    let blocked = num_at(t, "blockersDone") < num_at(t, "blockersTotal");

    // The blocker's title where we hold it, its short id where the blocker
    // lives in a phase the current filter excluded.
    let waiting_on = blocked.then(|| {
        let first = t
            .get("blockedBy")
            .and_then(Value::as_array)
            .and_then(|b| b.first())
            .and_then(Value::as_str)
            .unwrap_or("");
        titles
            .get(first)
            .cloned()
            .unwrap_or_else(|| first.get(..8).unwrap_or(first).to_string())
    });

    let mut claim = false;

    // Delegated rows carry a spine as well as the pill: at a glance down a
    // long phase, the edge is what you actually see.
    let body = |ui: &mut egui::Ui| {
        w::dot(ui, status_colour(&status));
        ui.add_space(space::XS);
        w::discipline(ui, &discipline);
        w::row_title(ui, &title, TRAILING_TASK);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Exactly one of three endings: what it waits on, who holds it, or
            // an invitation to take it.
            if let Some(what) = &waiting_on {
                w::blocked_by(ui, what);
            } else if assigned {
                let seed = if claimed.is_empty() { &person } else { &claimed };
                if !seed.is_empty() {
                    avatar::small(ui, seed, size::AVATAR_SM);
                    ui.add_space(space::SM);
                }
                if is_agent {
                    let label = if claimed.is_empty() { "agent" } else { &claimed };
                    w::pill(ui, label, colour::AGENT);
                } else {
                    w::pill(ui, status_label(&status), status_colour(&status));
                }
            } else {
                claim = w::ghost(ui, "Claim").clicked();
                ui.add_space(space::SM);
                w::muted(ui, "unassigned");
            }
        });
    };

    let hit = if is_agent {
        w::row_marked(ui, colour::AGENT, body)
    } else {
        w::row(ui, body)
    };

    if claim {
        return Some(Hit::Claim(id));
    }
    hit.clicked().then(|| Hit::Open(id))
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

fn num_at(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn fraction(done: i64, total: i64) -> f32 {
    if total <= 0 {
        return 0.0;
    }
    done as f32 / total as f32
}
