//! My Tasks: everything assigned to the signed-in person, newest first.
//!
//! One table, not one per project: grouping answered "what is in this
//! project", which is the project page's question, and it fought the ordering
//! — newest-first across your whole workload is the thing you actually scan.
//! The project is a column and a filter instead.
//!
//! Nothing is hidden. A "show finished" toggle made the done pile a mode you
//! had to remember to leave; a status filter says the same thing out loud and
//! composes with the others.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::menus::{task_items, Pick, Viewer};
use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{
    cards as c, colour, shell, space, status_label, viz, widgets as w,
};
use crate::desktop::net::memo;
use crate::desktop::App;

/// The personal list. Prefixed `mytasks` so any view that moves a task can drop
/// it with one `invalidate_prefix`.
pub(super) const MINE: &str = "mytasks:mine";
/// The archived ones, under the Archived toggle.
const ARCHIVED: &str = "mytasks:archived";
/// This view owns its filters and nothing else reads them, so they live in
/// egui's temp store rather than growing `App`.
const FILTERS: &str = "mytasks:filters";
const TABLE: &str = "mytasks:table";
/// The sorted list and the filtered one, redone only when the reply or the
/// filters change — not every frame.
const SORTED: &str = "mytasks:sorted";
const SHOWN: &str = "mytasks:shown";

/// What the list is narrowed to. Every field unset is the whole list, so
/// `Default` is "everything".
#[derive(Clone, Default, PartialEq)]
struct State {
    query: String,
    status: Option<String>,
    project: Option<String>,
    priority: Option<String>,
    /// The archived tasks instead of the live ones.
    archived: bool,
}

/// The status vocabulary, in the order it reads in the menu: both tracks' happy
/// paths run left to right, then the two states either of them can land in.
/// `handoff` is design-only and `shipped` engineering-only, but the menu offers
/// every value — a personal list holds work from both tracks.
const STATUSES: [&str; 7] =
    ["open", "in_progress", "handoff", "completed", "shipped", "blocked", "dropped"];

// ---- table geometry. Fixed so every group's columns line up with every
// ---- other's; the description takes whatever is left.
/// The title is what is being scanned for, so it gets the widest fixed column;
/// past this it truncates rather than pushing the row around.
const COL_TASK: f32 = 300.0;
/// The description's floor. Below this a one-liner is cut to nothing useful.
const COL_DESCRIPTION: f32 = 180.0;
/// "P0" in a chip.
const COL_PRIORITY: f32 = 52.0;
const COL_STATUS: f32 = 104.0;
/// The project a task belongs to, now that the list is not grouped by it.
const COL_PROJECT: f32 = 150.0;
const COL_CREATED: f32 = 78.0;

/// Alignment lives with the width, so "Created" sits over the age beneath it.
/// Ranked by what a narrow window can do without. Title, status and project
/// are why you open this screen, so they stay.
const COLS: [Col; 6] = [
    Col::left("Task", COL_TASK),
    Col::fill("Description", COL_DESCRIPTION).rank(3),
    Col::left("Priority", COL_PRIORITY).rank(1),
    Col::left("Status", COL_STATUS),
    Col::left("Project", COL_PROJECT),
    Col::right("Created", COL_CREATED).rank(2),
];

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let filters_id = egui::Id::new(FILTERS);
    let mut state: State = ui.ctx().data_mut(|d| d.get_temp(filters_id)).unwrap_or_default();

    let viewer = Viewer::of(app);
    let net = app.net.as_mut().unwrap();
    let key = if state.archived { ARCHIVED } else { MINE };
    net.get_once(MINE, "/api/user/tasks/mine");
    if state.archived {
        net.get_once(ARCHIVED, "/api/user/tasks/mine?archived=true");
    }

    let loading = net.is_loading(key);
    let error = net.error(key).map(str::to_owned);
    let generation = net.generation(key);
    let tasks = net.shared(key).unwrap_or_else(|| Arc::new(Value::Null));

    // Newest first, whatever order the server sent. One sort, here, so the
    // filters below never reshuffle the list as they narrow it.
    let sorted = memo(ui.ctx(), egui::Id::new(SORTED), (key, generation), || {
        let all: &[Value] = tasks.as_array().map(Vec::as_slice).unwrap_or_default();
        let mut order: Vec<usize> = (0..all.len()).collect();
        order.sort_by(|&a, &b| {
            str_at(&all[b], "createdAt")
                .unwrap_or_default()
                .cmp(str_at(&all[a], "createdAt").unwrap_or_default())
        });
        let mut projects: Vec<String> = all.iter().map(|t| project_of(t).to_owned()).collect();
        projects.sort_unstable();
        projects.dedup();
        (tasks.clone(), order, projects)
    });
    let (tasks, order, projects) = &*sorted;
    let all: &[Value] = tasks.as_array().map(Vec::as_slice).unwrap_or_default();
    // Triage is its own group above the list: filed for you, not yet taken.
    let (triage, rows): (Vec<&Value>, Vec<&Value>) =
        order.iter().map(|&i| &all[i]).partition(|t| str_at(t, "status") == Some("triage") && !state.archived);
    let projects: Vec<&str> = projects.iter().map(String::as_str).collect();

    let open = rows.iter().filter(|t| !finished(t)).count();
    let in_projects = projects.iter().filter(|p| **p != NO_PROJECT).count();
    let subtitle = format!("{open} open across {}", plural(in_projects, "project"));

    let mut new_task = false;
    shell::page_title(ui, "My Tasks", &subtitle, |ui| {
        if viewer.can_write {
            new_task = super::new_task::button(ui);
        }
    });
    if new_task {
        super::new_task::open(app);
    }

    if let Some(err) = &error {
        w::error(
            ui,
            &format!("Could not load your work. {err} Use Refresh in the sidebar to try again."),
        );
        return;
    }
    let mut open_task: Option<String> = None;
    let mut picked: Option<(Value, Pick)> = None;
    // Triage has a tab of its own; here it is one quiet line that goes there,
    // so the decisions do not crowd the work already yours.
    if !state.archived && !triage.is_empty() {
        if super::triage::link_row(ui, triage.len()) {
            app.tab = crate::desktop::Tab::Triage;
        }
        ui.add_space(space::LG);
    }

    // The archived list may be empty; its toggle is still the way back.
    // With only triage, the line above is the page.
    if rows.is_empty() && !state.archived {
        if loading && triage.is_empty() {
            w::loading(ui, "Loading your work");
        } else if triage.is_empty() {
            w::empty(
                ui,
                "Nothing assigned to you.",
                "When someone hands you a task it shows up here, newest first.",
            );
        }
        finish(app, open_task, picked);
        return;
    }

    let shown_ix = memo(ui.ctx(), egui::Id::new(SHOWN), (key, generation, state.clone()), || {
        (0..rows.len()).filter(|&i| keep(rows[i], &state)).collect::<Vec<usize>>()
    });
    let shown: Vec<&Value> = shown_ix.iter().map(|&i| rows[i]).collect();
    filter_bar(ui, &mut state, &projects);
    ui.ctx().data_mut(|d| d.insert_temp(filters_id, state));

    // The count heads the table rather than ending the toolbar, where it ran
    // into the last filter once the bar wrapped. A plain caption, not a
    // section heading: the page title already names what this is.
    let count = if shown.len() < rows.len() {
        format!("{} of {} tasks", shown.len(), rows.len())
    } else {
        plural(rows.len(), "task")
    };
    w::caption(ui, &count);
    ui.add_space(space::SM);

    if rows.is_empty() {
        w::empty(ui, "Nothing archived.", "Tasks of yours that are archived show here.");
    } else if shown.is_empty() {
        w::empty(ui, "Nothing matches.", "Clear a filter, or search for something else.");
    } else {
        let clicked = table::show_with_menu(
            ui,
            TABLE,
            &COLS,
            shown.len(),
            |row, i| task_row(row, shown[i]),
            |ui, i| {
                if let Some(pick) = task_items(ui, shown[i], &viewer, true) {
                    picked = Some((shown[i].clone(), pick));
                }
            },
        );
        if let Some(i) = clicked {
            open_task = str_at(shown[i], "id").map(str::to_owned);
        }
    }
    ui.add_space(space::XXL);
    finish(app, open_task, picked);
}

fn finish(app: &mut App, mut open_task: Option<String>, picked: Option<(Value, Pick)>) {
    match picked {
        Some((t, Pick::Open)) => open_task = str_at(&t, "id").map(str::to_owned),
        Some((t, pick)) => app.board.tasks.pick(app.net.as_mut().unwrap(), &t, pick),
        None => {}
    }
    if let Some(id) = open_task {
        app.task = Some(id);
    }
}

/// Search, then the three menus. The count is not here: it rides on the
/// table's heading, where a wrapped toolbar cannot run into it.
fn filter_bar(ui: &mut egui::Ui, state: &mut State, projects: &[&str]) {
    viz::toolbar(ui, |ui| {
        viz::search(ui, "Search tasks, descriptions, projects…", &mut state.query);

        let statuses: Vec<(String, String)> =
            STATUSES.iter().map(|s| ((*s).to_owned(), status_label(s).to_owned())).collect();
        viz::select(ui, "Any status", &statuses, &mut state.status);

        let names: Vec<(String, String)> =
            projects.iter().map(|p| ((*p).to_owned(), (*p).to_owned())).collect();
        viz::select(ui, "All projects", &names, &mut state.project);

        let priorities: Vec<(String, String)> =
            (0..=4).map(|p| (p.to_string(), format!("P{p}"))).collect();
        viz::select(ui, "Any priority", &priorities, &mut state.priority);

        if viz::filter(ui, "Archived", state.archived, false).clicked() {
            state.archived = !state.archived;
        }

        if *state != State::default() && viz::clear(ui).clicked() {
            *state = State::default();
        }
    });
}

/// Every filter is an AND, and an unset one passes everything.
fn keep(t: &Value, state: &State) -> bool {
    if let Some(s) = &state.status {
        if str_at(t, "status") != Some(s.as_str()) {
            return false;
        }
    }
    if let Some(p) = &state.project {
        if project_of(t) != p {
            return false;
        }
    }
    if let Some(p) = &state.priority {
        if num(t, "priority").to_string() != *p {
            return false;
        }
    }
    // Title, description and project: the three things you would think to
    // type. Not the status — that is what the menu beside the box is for.
    viz::matches(
        &state.query,
        &[
            str_at(t, "title").unwrap_or_default(),
            str_at(t, "body").unwrap_or_default(),
            project_of(t),
        ],
    )
}

// --------------------------------------------------------------------- table

fn task_row(row: &mut table::Cells<'_, '_, '_>, t: &Value) {
    let status = str_at(t, "status").unwrap_or("open");
    let done = finished(t);

    row.at(0, |ui| {
        // Finished work stays legible and stops competing: the rows above it
        // are the ones with something to decide.
        let ink = if done || super::board::archived(t) { colour::TEXT_MUTED } else { colour::TEXT };
        let labels = t.get("labels").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        super::projects::name_with_labels(ui, str_at(t, "title").unwrap_or_default(), ink, labels);
        // You are the owner of every row here, so the mark rides on the title.
        super::home::agent_marker(ui, t);
        // Blocked rides behind the title rather than replacing the status:
        // the status is still true, the blocker is why it is not moving.
        if blocked(t) {
            ui.add_space(space::XS);
            c::chip(ui, "Blocked", c::Tone::Blocked, false);
        }
    });

    // One line. The column clips, and a wrapped cell would make one row taller
    // than the rest of the table.
    row.muted(1, str_at(t, "body").unwrap_or_default().trim().lines().next().unwrap_or(""));

    row.at(2, |ui| {
        if let Some(p) = t.get("priority").and_then(Value::as_i64) {
            c::chip(ui, &format!("P{p}"), priority_tone(p), false);
        }
    });

    row.at(3, |ui| {
        if super::board::archived(t) {
            super::board::archived_chip(ui);
        } else {
            c::chip(ui, status_label(status), c::status_tone(status), true);
        }
    });

    // A standalone task reads "—", faint, like every empty cell.
    row.muted(4, str_at(t, "projectName").unwrap_or_default());
    row.muted(5, &age(str_at(t, "createdAt").unwrap_or_default()));
}

// -------------------------------------------------------------------- pieces

/// P0 shouts and P4 whispers, in the same chip vocabulary as status.
fn priority_tone(p: i64) -> c::Tone {
    match p {
        0 => c::Tone::Blocked,
        1 => c::Tone::Running,
        2 => c::Tone::Neutral,
        _ => c::Tone::Quiet,
    }
}

fn blocked(t: &Value) -> bool {
    num(t, "blockersDone") < num(t, "blockersTotal")
}

/// Whether the task is off your plate. `doneAt` is the server's single answer
/// for both tracks — it is stamped only at the terminal state, which is
/// `completed` for design and `shipped` for engineering, so no status test can
/// stand in for it. Dropped work is off the plate too without ever earning the
/// stamp, which is why it rides alongside rather than being inferred.
fn finished(t: &Value) -> bool {
    t.get("doneAt").is_some_and(|v| !v.is_null()) || str_at(t, "status") == Some("dropped")
}

/// What a standalone task is filed under in the project filter.
const NO_PROJECT: &str = "No project";

/// The project filter's value for a task: its project, or "No project" for a
/// standalone one.
fn project_of<'a>(t: &'a Value) -> &'a str {
    match str_at(t, "projectName") {
        Some(name) if !name.is_empty() => name,
        _ => NO_PROJECT,
    }
}

/// "2d", "4h". A table column has room for two characters, and the exact
/// minute is never the thing being decided.
pub(super) fn age(raw: &str) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(raw) else {
        return String::new();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn num(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}
