//! Board: the project list, and inside a project its flow, phases and tasks.
//!
//! Two modes, chosen by `app.project`. The project detail screen leads with the
//! flow strip — done/total per discipline — because the question a lead asks
//! first is "where is the work", not "which phase are we in". The strip reports
//! only: it does not gate anything, and a discipline may run ahead of the one
//! to its left. The arrow says "usually in this order", nothing stronger.
//!
//! Everything below the strip is a card: a project is a surface with its own
//! progress, a task is a `c::task_card`, and an unclaimed task is a slim card
//! with a Claim button — the same shape the claim zone has on My tasks.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use egui_phosphor::thin as icon;
use serde_json::Value;

use crate::desktop::design::tokens::{discipline_colour, DISCIPLINE_W};
use crate::desktop::design::{
    cards as c, colour, shell, space, status_label, text, theme, widgets as w,
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
        let flow = flows.get(&id);
        let counts = flow.map(|f| (num_at(f, "done"), num_at(f, "total")));

        let hover_id = ui.next_auto_id();
        let hovered = ui.ctx().data(|d| d.get_temp::<bool>(hover_id).unwrap_or(false));
        let out = c::surface(ui, hovered, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&name)
                        .size(text::CARD)
                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                        .color(colour::TEXT),
                );
                ui.add_space(space::SM);
                w::mono_caption(ui, &key);
                ui.add_space(space::SM);
                c::chip(ui, status_label(&status), c::status_tone(&status), true);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match counts {
                        Some((done, total)) => w::muted(ui, &format!("{done} of {total} done")),
                        None => w::muted(ui, "progress loading"),
                    }
                });
            });
            ui.add_space(space::SM);
            let (done, total) = counts.unwrap_or((0, 0));
            w::progress(ui, fraction(done, total), ui.available_width(), colour::ACCENT);
            // Where the work stands per discipline, in one muted line: it is
            // the same question the detail screen's flow strip answers, and a
            // project card is the place you ask it first.
            if let Some(line) = flow.map(per_discipline).filter(|l| !l.is_empty()) {
                ui.add_space(space::SM);
                w::muted(ui, &line);
            }
        });

        let hit = out.response.interact(egui::Sense::click());
        ui.ctx().data_mut(|d| d.insert_temp(hover_id, hit.hovered()));
        if hit.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if hit.clicked() {
            open = Some(id);
        }
        ui.add_space(space::SM);
    }

    if let Some(id) = open {
        app.project = Some(id);
    }
}

/// "design 2/5 · frontend 1/3" — the flow, folded onto one line for a card.
fn per_discipline(flow: &Value) -> String {
    flow_columns(flow)
        .iter()
        .map(|d| format!("{} {}/{}", str_at(d, "discipline"), num_at(d, "done"), num_at(d, "total")))
        .collect::<Vec<_>>()
        .join(" · ")
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

    let can_write = app.can_write();
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
    let busy = net.is_loading(CLAIM_KEY) || app.board.claiming;

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

            if let Some(err) = &tasks_error {
                w::error(ui, err);
            } else if list.is_empty() {
                if tasks_loading {
                    w::loading(ui, "tasks");
                } else {
                    w::empty(ui, "No tasks in this phase.", "");
                }
            } else {
                for t in list {
                    match task_card(ui, t, &titles, can_write && !busy) {
                        Some(Hit::Open(id)) => open_task = Some(id),
                        Some(Hit::Claim(id)) => claim = Some(id),
                        None => {}
                    }
                }
            }
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

/// The disciplines of a flow that have any work, in the usual order.
fn flow_columns(flow: &Value) -> Vec<&Value> {
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
    columns
}

/// The flow strip: one column per discipline that has tasks.
///
/// It reports, it does not gate. A column is where that discipline stands, and
/// the single arrow after design says only what the usual order is — frontend
/// and backend can and do run before design has finished.
fn flow_strip(ui: &mut egui::Ui, flow: &Value) {
    let columns = flow_columns(flow);

    if columns.is_empty() {
        w::empty(ui, "No tasks have a discipline yet.", "");
        return;
    }

    c::surface(ui, false, |ui| {
        let full = ui.available_width();
        ui.set_width(full);
        let arrow = if columns.len() > 1 { 1.0 } else { 0.0 };
        let gaps = space::LG * (columns.len() as f32 - 1.0 + arrow);
        let width = ((full - gaps - space::MD * arrow) / columns.len() as f32)
            .max(DISCIPLINE_W);

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
                        ui.label(
                            egui::RichText::new(name)
                                .size(text::SMALL)
                                .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                                .color(colour::TEXT_2),
                        );
                        ui.add_space(space::SM);
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

/// The header above a phase's tasks: its number and name, how many tasks it
/// holds, then its state and progress on the right.
fn phase_header(ui: &mut egui::Ui, p: &Value, done: usize, total: usize) {
    let position = p.get("position").and_then(Value::as_i64).unwrap_or(0);
    let status = str_at(p, "status");
    let gate = p.get("gate").and_then(Value::as_bool).unwrap_or(false);
    let label = format!("{position:02} · {}", str_at(p, "name"));

    shell::section_count_with(ui, &label, total, |ui| {
        w::muted(ui, &format!("{done} / {total}"));
        ui.add_space(space::SM);
        if gate {
            c::chip(ui, "gate", c::Tone::Running, false);
        }
        c::chip(ui, status_label(status), c::status_tone(status), true);
    });
}

/// What a click on a card meant.
enum Hit {
    Open(String),
    Claim(String),
}

/// One task. `titles` names blockers that are in this project's task list.
///
/// Unclaimed work drops to a slim card: it has no state worth four chips and
/// no people, and the one thing to do with it is take it — the same shape the
/// claim zone has on My tasks.
fn task_card(
    ui: &mut egui::Ui,
    t: &Value,
    titles: &HashMap<String, String>,
    can_claim: bool,
) -> Option<Hit> {
    let status = str_at(t, "status").to_string();
    let is_agent = str_at(t, "assigneeKind") == "agent";
    let claimed = str_at(t, "claimedBy").to_string();
    let person = str_at(t, "assigneePersonId").to_string();
    let id = str_at(t, "id").to_string();
    let title = str_at(t, "title").to_string();
    let discipline = str_at(t, "discipline").to_string();
    let assigned = is_agent || !claimed.is_empty() || !person.is_empty();
    let blocked = num_at(t, "blockersDone") < num_at(t, "blockersTotal");

    if !assigned && !blocked {
        return claimable_card(ui, &id, &title, &discipline, can_claim);
    }

    // The blocker's title where we hold it, its short id where the blocker
    // lives in a phase the current filter excluded.
    let waiting = blocked.then(|| {
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

    // Blockers beat the status column: a task marked in progress that waits on
    // someone else is not in progress, and the chip should not claim it is.
    let state = if blocked { "blocked" } else { status.as_str() };

    let mut chips: Vec<(String, c::Tone, bool)> = vec![(age(t), c::Tone::Quiet, false)];
    if !discipline.is_empty() {
        chips.push((discipline.clone(), c::discipline_tone(&discipline), false));
    }
    chips.push((capitalise(status_label(state)), c::status_tone(state), true));

    let context = match &waiting {
        Some(what) => waiting_on(what),
        None => str_at(t, "phaseName").to_string(),
    };

    let trailing: Vec<(String, c::Tone)> = if is_agent {
        let label = if claimed.is_empty() { "agent".to_owned() } else { claimed.clone() };
        vec![(label, c::Tone::Agent)]
    } else {
        Vec::new()
    };
    let seed = if claimed.is_empty() { person } else { claimed };
    // The agent chip already names who holds it; a face beside it is noise.
    let people: Vec<String> =
        if is_agent || seed.is_empty() { Vec::new() } else { vec![seed] };

    let hit = c::task_card(
        ui,
        &c::TaskCard {
            title: &title,
            chips: &chips,
            context: &context,
            trailing_chips: &trailing,
            people: &people,
        },
    );
    hit.clicked().then_some(Hit::Open(id))
}

/// An unclaimed task: a discipline, a title, and the one thing to do with it.
fn claimable_card(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    discipline: &str,
    can_claim: bool,
) -> Option<Hit> {
    let mut claimed = false;
    let hit = c::slim_card(ui, |ui| {
        if !discipline.is_empty() {
            c::chip(ui, discipline, c::discipline_tone(discipline), false);
            ui.add_space(space::XS);
        }
        ui.label(
            egui::RichText::new(title)
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            claimed = w::button(ui, "Claim", w::Emphasis::Ghost, can_claim).clicked();
            ui.add_space(space::SM);
            w::muted(ui, "unassigned");
        });
    })
    .interact(egui::Sense::click());

    if claimed {
        return Some(Hit::Claim(id.to_string()));
    }
    hit.clicked().then(|| Hit::Open(id.to_string()))
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

/// `w::blocked_by`'s wording, as a string — a card's context line is text, not
/// a widget.
/// ponytail: the same sentence lives in mytasks.rs. Fold them together the day
/// a third screen needs it.
fn waiting_on(what: &str) -> String {
    format!("\u{2933} waiting on \u{201c}{what}\u{201d}")
}

/// How long since the task last moved, for the quiet leading chip.
fn age(t: &Value) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(str_at(t, "updatedAt")) else {
        return "no activity".to_owned();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
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
