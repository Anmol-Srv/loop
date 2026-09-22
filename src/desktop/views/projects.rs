//! Projects: the list, and the form that makes one.
//!
//! The detail screen is `board.rs`; this file is everything before you click
//! into a project. Split out so the two can change independently.
//!
//! The list is a table, not a grid of cards. Cards were the right unit when a
//! project was a paragraph; they are the wrong one for "which of these twenty
//! is behind", because you cannot compare a column of cards down a page. Same
//! reasoning that turned Home into a table, same column/hover/keyboard
//! machinery — a project row should feel like a task row.
//!
//! The create form makes tasks too. A project with no tasks is a heading, and
//! the old flow made you create one, click into it, and start again; handing
//! out the first few in the same step is the difference between a plan and an
//! empty board.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use egui::RichText;
use serde_json::Value;

use super::board::{array, fraction, num_at, str_at};
use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{
    cards as c, colour, shell, space, status_label, text, viz, widgets as w,
};
use crate::desktop::App;

const PROJECTS_KEY: &str = "board:projects";
pub(super) const PEOPLE_KEY: &str = "board:people";
/// Where a create's reply is collected.
const CREATE_KEY: &str = "board:create";
/// The table's id: its scroll salt, and the slot its hover is tracked in.
const TABLE: &str = "projects:table";

/// How far each avatar in the roster sits over the one before it.
pub(super) const AVATAR_OVERLAP: f32 = 6.0;
/// Past this the roster shows a "+N" instead of more faces.
pub(super) const MAX_AVATARS: usize = 5;
/// The prose measure for a project description: ~70 characters at `text::BODY`,
/// which is where a paragraph stops needing a finger to track the line.
pub(super) const PROSE_W: f32 = 640.0;

// ---- table geometry. Fixed so the columns line up with the header and with
// ---- each other; the description takes whatever is left.
/// A project name is the thing being scanned for, so it gets the widest fixed
/// column — past this a name truncates rather than pushing the row around.
const COL_NAME: f32 = 220.0;
/// The description's floor. Below this a one-liner is cut to nothing useful,
/// so the table would rather squeeze the window than this column.
const COL_DESCRIPTION: f32 = 200.0;
/// A count, right-aligned under its header. Two digits and air.
const COL_TASKS: f32 = 64.0;
/// The bar plus its percentage.
const COL_PROGRESS: f32 = 150.0;
const PROGRESS_BAR_W: f32 = 90.0;
const COL_STATUS: f32 = 104.0;
const COL_CREATED: f32 = 78.0;

/// Alignment lives with the width, so "Progress" and "Created" sit over the
/// figures beneath them rather than at the other end of the column.
/// Ranked by what a narrow window can do without: the description goes
/// first, then the created date, then the task count — progress says the same
/// thing in less room. Name, progress and status never drop.
const COLS: [Col; 6] = [
    Col::left("Project", COL_NAME),
    Col::fill("Description", COL_DESCRIPTION).rank(3),
    Col::right("Tasks", COL_TASKS).rank(1),
    Col::left("Progress", COL_PROGRESS),
    Col::left("Status", COL_STATUS),
    Col::right("Created", COL_CREATED).rank(2),
];

/// What the trailing controls on a draft-task row need: three menus, a Remove,
/// and the gaps between them. The title input takes the rest.
const TASK_CONTROLS_W: f32 = 420.0;
/// Below this the title input stops giving ground; the row wraps instead.
const TASK_TITLE_MIN_W: f32 = 160.0;

/// Priority, as the server stores it and as a person reads it. The value is a
/// string because that is what `viz::select` slots hold; it becomes an int on
/// submit.
pub(super) const PRIORITIES: [(&str, &str); 5] = [
    ("0", "P0 Urgent"),
    ("1", "P1 High"),
    ("2", "P2 Normal"),
    ("3", "P3 Low"),
    ("4", "P4 Someday"),
];
/// The middle of the scale, where a new row starts.
pub(super) const DEFAULT_PRIORITY: &str = "2";

/// What the create form holds. `None` on `State::creating` means the form is
/// closed, which is also how the Create project button knows not to redraw
/// itself over an open form.
#[derive(Default)]
pub struct Draft {
    pub title: String,
    pub description: String,
    /// The project's first tasks. Rows with no title are dropped on submit,
    /// so an accidental Add task costs nothing.
    pub tasks: Vec<DraftTask>,
}

/// One row of the form's Tasks section. Every optional field is an
/// `Option<String>` because that is the shape `viz::select` writes into.
pub struct DraftTask {
    pub title: String,
    pub assignee: Option<String>,
    pub priority: Option<String>,
}

impl Default for DraftTask {
    fn default() -> Self {
        Self {
            title: String::new(),
            assignee: None,
            priority: Some(DEFAULT_PRIORITY.to_owned()),
        }
    }
}

// ---------------------------------------------------------------- project list

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let net = app.net.as_mut().unwrap();

    // A finished create has invalidated the list, the sidebar counts and every
    // rollup that counts projects. Tasks came with it, so the task caches go
    // too.
    if net.data(CREATE_KEY).is_some() {
        net.invalidate(CREATE_KEY);
        net.invalidate(PROJECTS_KEY);
        net.invalidate("home");
        net.invalidate_prefix("task:");
        app.board.creating = None;
    }
    let net = app.net.as_mut().unwrap();

    net.get_once(PROJECTS_KEY, "/api/user/projects");
    // The assignee picker needs names, and the rows need them to resolve
    // `memberIds`. One fetch serves both.
    net.get_once(PEOPLE_KEY, "/api/user/people");

    let list = array(net.data(PROJECTS_KEY));
    let people = array(net.data(PEOPLE_KEY));
    let loading = net.is_loading(PROJECTS_KEY);
    let error = net.error(PROJECTS_KEY).map(str::to_string);
    let create_error = net.error(CREATE_KEY).map(str::to_string);
    let creating = net.is_loading(CREATE_KEY);

    // One `/flow` per project — cached, so once per project per session. A
    // project with no tasks still answers, with zeroes.
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

    let mut start_create = false;
    shell::page_title(ui, "Projects", &subtitle(list.len(), loading), |ui| {
        // The form is the button's own state: while it is open the button
        // would only re-open what is already open.
        if app.board.creating.is_none() && w::primary(ui, "Create project", can_write).clicked() {
            start_create = true;
        }
    });
    if start_create {
        app.board.creating = Some(Draft::default());
    }

    if let Some(err) = &create_error {
        w::error(ui, err);
        ui.add_space(space::MD);
    }

    let mut submit: Option<Value> = None;
    if let Some(draft) = app.board.creating.as_mut() {
        submit = create_form(ui, draft, &people, creating);
        ui.add_space(space::LG);
    }

    let mut open: Option<String> = None;
    if let Some(err) = error {
        w::error(ui, &err);
    } else if list.is_empty() {
        if loading {
            w::loading(ui, "projects");
        } else if app.board.creating.is_none() {
            w::empty(
                ui,
                "No projects yet.",
                "Create one and hand out its first tasks in the same step.",
            );
        }
    } else {
        table(ui, &list, &flows, &mut open);
        ui.add_space(space::XXL);
    }

    let net = app.net.as_mut().unwrap();
    if let Some(body) = submit {
        net.post(CREATE_KEY, "/api/user/projects", body);
    }
    if let Some(id) = open {
        app.project = Some(id);
    }
}

fn subtitle(n: usize, loading: bool) -> String {
    match (n, loading) {
        (0, true) => String::new(),
        (1, _) => "1 project".to_owned(),
        (n, _) => format!("{n} projects"),
    }
}

// --------------------------------------------------------------------- table

/// Public so `tests/ui_render.rs` can draw it offscreen with fake rows: the
/// running app cannot be screenshotted from here, and alignment is not
/// something to verify by reading code.
pub fn table(
    ui: &mut egui::Ui,
    rows: &[Value],
    flows: &HashMap<String, Value>,
    open: &mut Option<String>,
) {
    let clicked = table::show(ui, TABLE, &COLS, rows.len(), |row, i| {
        project_row(row, &rows[i], flows.get(str_at(&rows[i], "id")));
    });
    if let Some(i) = clicked {
        *open = Some(str_at(&rows[i], "id").to_owned());
    }
}

fn project_row(row: &mut table::Cells<'_, '_, '_>, p: &Value, flow: Option<&Value>) {
    let (done, total) = flow.map(|f| (num_at(f, "done"), num_at(f, "total"))).unwrap_or((0, 0));
    let status = str_at(p, "status");

    row.strong(0, str_at(p, "name"), colour::TEXT);

    // One line. The column clips, and a wrapped cell would make one row taller
    // than the rest of the table.
    row.muted(1, str_at(p, "description"));

    // Who is on a project is whoever holds its tasks, so the number of tasks
    // is the honest figure here; the faces live on the task rows themselves.
    if total > 0 {
        row.text(2, &total.to_string(), colour::TEXT);
    } else {
        row.muted(2, "");
    }

    // The bar first, at a fixed width, then a percentage. A "3/8" here varied
    // in width and dragged the bar's left edge around with it, and said the
    // same thing as the Tasks column beside it.
    row.at(3, |ui| {
        if total > 0 {
            w::progress(ui, fraction(done, total), PROGRESS_BAR_W, colour::ACCENT);
            ui.add_space(space::SM);
            ui.label(
                RichText::new(format!("{}%", done * 100 / total))
                    .size(text::SMALL)
                    .color(colour::TEXT),
            );
        }
        // No tasks: nothing, not a dash. The Tasks column beside it has
        // already said "\u{2014}", and two dashes in a row read as one wide one.
    });

    row.at(4, |ui| {
        c::chip(ui, status_label(status), c::status_tone(status), true);
    });

    row.muted(5, &age(p));
}

/// A person as the assignee menu shows them: name and department, because
/// picking the person is what sets the task's discipline.
pub(super) fn person_option(p: &Value) -> (String, String) {
    let department = str_at(p, "department");
    let label = if department.is_empty() {
        str_at(p, "name").to_string()
    } else {
        format!("{} \u{00B7} {department}", str_at(p, "name"))
    };
    (str_at(p, "id").to_string(), label)
}

/// How old the project is, in the two characters a table column has room for.
/// `board::age` reads `updatedAt`; a project's interesting date is when it was
/// started, so the field differs and the wording is shorter.
fn age(p: &Value) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(str_at(p, "createdAt")) else {
        return String::new();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

// ---------------------------------------------------------------- create form

/// The create form, inline under the header rather than in a modal.
///
/// Returns the request body once, on the frame Create is clicked. A modal was
/// the obvious first thought and the wrong one: covering the list to ask what
/// to add to the list is a worse trade than pushing it down — and the form is
/// now long enough that a modal would need its own scrollbar.
fn create_form(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    people: &[Value],
    busy: bool,
) -> Option<Value> {
    let mut submit = None;
    let mut cancel = false;

    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        w::heading(ui, "New project");
        ui.add_space(space::MD);

        w::field(ui, "Title", &mut draft.title, false, "Checkout redesign");
        ui.add_space(space::MD);
        w::field_multiline(
            ui,
            "Description",
            &mut draft.description,
            3,
            "What this project is for, and what done looks like.",
        );
        ui.add_space(space::MD);

        // A task goes to anyone here. The project has no roster of its own:
        // the people on it are the people holding its tasks.
        let assignable: Vec<(String, String)> = people.iter().map(person_option).collect();
        tasks_section(ui, draft, &assignable);
        ui.add_space(space::LG);

        // A project with no title has nothing to be called and nothing to
        // derive a key from, so Create stays off until there is one.
        let ready = !draft.title.trim().is_empty() && !busy;
        ui.horizontal(|ui| {
            if w::primary(ui, if busy { "Creating…" } else { "Create" }, ready).clicked() {
                submit = Some(serde_json::json!({
                    "name": draft.title.trim(),
                    "description": draft.description.trim(),
                    "tasks": task_bodies(draft),
                }));
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if cancel {
        *draft = Draft::default();
        return None;
    }
    submit
}

/// The first tasks, one row each. Empty by default: a project that is only a
/// name is still a legitimate thing to create, and pre-seeding a blank row
/// makes the form look like it is demanding one.
fn tasks_section(ui: &mut egui::Ui, draft: &mut Draft, assignable: &[(String, String)]) {
    w::heading(ui, "Tasks");
    ui.add_space(space::XS);
    w::caption(
        ui,
        "Handed out with the project. A task takes its discipline from whoever \
         holds it, so picking the person is picking the department.",
    );
    ui.add_space(space::MD);

    let priorities: Vec<(String, String)> =
        PRIORITIES.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect();

    let mut remove: Option<usize> = None;
    for (i, task) in draft.tasks.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            let width = (ui.available_width() - TASK_CONTROLS_W).max(TASK_TITLE_MIN_W);
            task_title(ui, width, &mut task.title);
            viz::select(ui, "Unassigned", assignable, &mut task.assignee);
            viz::select(ui, "P2 Normal", &priorities, &mut task.priority);
            if w::ghost(ui, "Remove").clicked() {
                remove = Some(i);
            }
        });
        ui.add_space(space::XS);
    }
    if let Some(i) = remove {
        draft.tasks.remove(i);
    }

    ui.add_space(space::SM);
    if w::secondary(ui, "Add task", true).clicked() {
        draft.tasks.push(DraftTask::default());
    }
}

/// A bare single-line input, sized to the row. `w::field` is the labelled
/// version and stacks its own caption above the box; a task row's columns are
/// labelled once by the placeholders, not once per row.
///
/// Private to this file because it is the only place a form row needs an
/// unlabelled input — promote it into `widgets` when a second one turns up.
fn task_title(ui: &mut egui::Ui, width: f32, value: &mut String) -> egui::Response {
    ui.add_sized(
        [width, viz::HEIGHT],
        egui::TextEdit::singleline(value)
            .hint_text(
                RichText::new("What needs doing")
                    .size(text::BODY)
                    .color(colour::TEXT_DISABLED),
            )
            .margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8)),
    )
}

/// The `tasks` array of the create request. Untitled rows never leave the
/// form: an accidental Add task should not create an unnamed task.
fn task_bodies(draft: &Draft) -> Vec<Value> {
    draft
        .tasks
        .iter()
        .filter(|t| !t.title.trim().is_empty())
        .map(|t| {
            serde_json::json!({
                "title": t.title.trim(),
                "body": "",
                "assigneeId": t.assignee,
                "priority": t
                    .priority
                    .as_deref()
                    .and_then(|p| p.parse::<i32>().ok())
                    .unwrap_or(2),
            })
        })
        .collect()
}
