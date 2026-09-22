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

use chrono::{DateTime, Utc};
use egui::{Align, Layout};
use serde_json::Value;

use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{
    cards as c, colour, shell, space, status_label, text, theme, viz, widgets as w,
};
use crate::desktop::App;

/// The personal list. Prefixed `mytasks` so any view that moves a task can drop
/// it with one `invalidate_prefix`.
const MINE: &str = "mytasks:mine";
/// This view owns its filters and nothing else reads them, so they live in
/// egui's temp store rather than growing `App`.
const FILTERS: &str = "mytasks:filters";
const TABLE: &str = "mytasks:table";

/// What the list is narrowed to. Every field unset is the whole list, so
/// `Default` is "everything".
#[derive(Clone, Default, PartialEq)]
struct State {
    query: String,
    status: Option<String>,
    project: Option<String>,
    priority: Option<String>,
}

/// The status vocabulary, in the order it reads in the menu.
const STATUSES: [&str; 6] =
    ["open", "in_progress", "in_review", "blocked", "done", "dropped"];

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
const COLS: [Col; 6] = [
    Col::left("Task", COL_TASK),
    Col::fill("Description", COL_DESCRIPTION),
    Col::left("Priority", COL_PRIORITY),
    Col::left("Status", COL_STATUS),
    Col::left("Project", COL_PROJECT),
    Col::right("Created", COL_CREATED),
];

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let filters_id = egui::Id::new(FILTERS);
    let mut state: State = ui.ctx().data_mut(|d| d.get_temp(filters_id)).unwrap_or_default();

    let net = app.net.as_mut().unwrap();
    net.get_once(MINE, "/api/user/tasks/mine");

    let loading = net.is_loading(MINE);
    let error = net.error(MINE).map(str::to_owned);
    let tasks = net.data(MINE).cloned().unwrap_or(Value::Null);

    // Newest first, whatever order the server sent. One sort, here, so the
    // filters below never reshuffle the list as they narrow it.
    let mut rows: Vec<&Value> = tasks.as_array().map(|a| a.iter().collect()).unwrap_or_default();
    rows.sort_by(|a, b| {
        str_at(b, "createdAt").unwrap_or_default().cmp(str_at(a, "createdAt").unwrap_or_default())
    });

    let open = rows.iter().filter(|t| !finished(t)).count();
    let mut projects: Vec<&str> = rows.iter().map(|t| project_of(t)).collect();
    projects.sort_unstable();
    projects.dedup();

    shell::page_title(
        ui,
        "My Tasks",
        &format!("{open} open across {}", plural(projects.len(), "project")),
        |_| {},
    );

    if let Some(err) = &error {
        w::error(ui, &format!("Could not load your work. {err}"));
        return;
    }
    if rows.is_empty() {
        if loading {
            w::loading(ui, "your work");
        } else {
            w::empty(
                ui,
                "Nothing assigned to you.",
                "When someone hands you a task it shows up here, newest first.",
            );
        }
        return;
    }

    let shown: Vec<&Value> = rows.iter().copied().filter(|t| keep(t, &state)).collect();
    filter_bar(ui, &mut state, &projects, shown.len(), rows.len());
    ui.ctx().data_mut(|d| d.insert_temp(filters_id, state));

    let mut open_task: Option<String> = None;
    if shown.is_empty() {
        w::empty(ui, "Nothing matches.", "Clear a filter, or search for something else.");
    } else {
        let clicked = table::show(ui, TABLE, &COLS, shown.len(), |row, i| {
            task_row(row, shown[i]);
        });
        if let Some(i) = clicked {
            open_task = str_at(shown[i], "id").map(str::to_owned);
        }
    }
    ui.add_space(space::XXL);

    if let Some(id) = open_task {
        app.task = Some(id);
    }
}

/// Search, then the three menus, then how much of the list is left.
fn filter_bar(
    ui: &mut egui::Ui,
    state: &mut State,
    projects: &[&str],
    shown: usize,
    total: usize,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        viz::search(ui, "Search tasks", &mut state.query);

        let statuses: Vec<(String, String)> = STATUSES
            .iter()
            .map(|s| ((*s).to_owned(), sentence(status_label(s))))
            .collect();
        viz::select(ui, "Any status", &statuses, &mut state.status);

        let names: Vec<(String, String)> =
            projects.iter().map(|p| ((*p).to_owned(), (*p).to_owned())).collect();
        viz::select(ui, "All projects", &names, &mut state.project);

        let priorities: Vec<(String, String)> =
            (0..=4).map(|p| (p.to_string(), format!("P{p}"))).collect();
        viz::select(ui, "Any priority", &priorities, &mut state.priority);

        if *state != State::default() && viz::clear(ui).clicked() {
            *state = State::default();
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("of {total}")).size(text::SMALL).color(colour::TEXT_MUTED),
            );
            ui.add_space(space::XXS);
            ui.label(
                egui::RichText::new(shown.to_string())
                    .size(text::SMALL)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
        });
    });
    ui.add_space(space::MD);
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

fn task_row(row: &mut egui_extras::TableRow<'_, '_>, t: &Value) {
    let status = str_at(t, "status").unwrap_or("open");
    let done = finished(t);

    row.col(|ui| {
        table::cell(ui, &COLS[0], |ui| {
            // Finished work stays legible and stops competing: the rows above
            // it are the ones with something to decide.
            let ink = if done { colour::TEXT_MUTED } else { colour::TEXT };
            table::strong_label(ui, str_at(t, "title").unwrap_or_default(), ink);
            // Blocked rides behind the title rather than replacing the status:
            // the status is still true, the blocker is why it is not moving.
            if blocked(t) {
                ui.add_space(space::XS);
                c::chip(ui, "blocked", c::Tone::Blocked, false);
            }
        });
    });

    // One line. The column clips, and a wrapped cell would make one row taller
    // than the rest of the table.
    row.col(|ui| {
        let body = str_at(t, "body").unwrap_or_default().trim();
        table::muted_cell(ui, &COLS[1], body.lines().next().unwrap_or(""));
    });

    row.col(|ui| {
        table::cell(ui, &COLS[2], |ui| {
            if let Some(p) = t.get("priority").and_then(Value::as_i64) {
                c::chip(ui, &format!("P{p}"), priority_tone(p), false);
            }
        });
    });

    row.col(|ui| {
        table::cell(ui, &COLS[3], |ui| {
            c::chip(ui, &sentence(status_label(status)), c::status_tone(status), true);
        });
    });

    row.col(|ui| table::muted_cell(ui, &COLS[4], project_of(t)));

    row.col(|ui| {
        table::muted_cell(ui, &COLS[5], &age(str_at(t, "createdAt").unwrap_or_default()));
    });
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

fn finished(t: &Value) -> bool {
    matches!(str_at(t, "status"), Some("done") | Some("dropped"))
}

/// The group a task is filed under. A task always has a project; the fallback
/// only exists so a malformed row still lands somewhere visible.
fn project_of<'a>(t: &'a Value) -> &'a str {
    match str_at(t, "projectName") {
        Some(name) if !name.is_empty() => name,
        _ => "No project",
    }
}

/// "2d", "4h". A table column has room for two characters, and the exact
/// minute is never the thing being decided.
fn age(raw: &str) -> String {
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

/// "In review" from "in review". The vocabulary still comes from
/// `status_label`; this only decides where the sentence starts.
fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn num(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}
