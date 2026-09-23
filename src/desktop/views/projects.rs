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
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use egui::RichText;
use serde_json::Value;

use super::board::{array, fraction, num_at, str_at};
use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{
    cards as c, colour, shell, size, space, status_label, text, theme, viz, widgets as w,
};
use crate::desktop::net::memo;
use crate::desktop::App;

pub(super) const PROJECTS_KEY: &str = "board:projects";
pub(super) const PEOPLE_KEY: &str = "board:people";
/// Where a create's reply is collected.
const CREATE_KEY: &str = "board:create";
/// The table's id: its scroll salt, and the slot its hover is tracked in.
const TABLE: &str = "projects:table";
/// The filtered, sorted rows, held in egui's temp store between frames.
const SHOWN: &str = "projects:shown";
/// The shared label vocabulary. Not under `board:` — it outlives any one
/// project, so a create's invalidation sweep has no business dropping it.
pub(super) const LABELS_KEY: &str = "projects:labels";
/// Where a new label's reply is collected.
const NEW_LABEL_KEY: &str = "projects:new-label";

/// How far each avatar in the roster sits over the one before it. The project
/// page no longer stacks faces (the initials collided), but the names stay
/// exported for whichever view wants a stack next.
#[allow(dead_code)]
pub(super) const AVATAR_OVERLAP: f32 = 6.0;
/// Past this the roster shows a "+N" instead of more faces.
#[allow(dead_code)]
pub(super) const MAX_AVATARS: usize = 5;
/// The prose measure for a project description: ~70 characters at `text::BODY`,
/// which is where a paragraph stops needing a finger to track the line.
pub(super) const PROSE_W: f32 = 640.0;

// ---- table geometry. Fixed so the columns line up with the header and with
// ---- each other; the description takes whatever is left.
/// A project name is the thing being scanned for, so it gets the widest fixed
/// column — and it now carries the label chips as well, which is what the
/// extra 40 buys. Past this a name truncates rather than pushing the row
/// around.
const COL_NAME: f32 = 260.0;
/// The name never gives up more than this to its chips: under about ten
/// characters the column stops identifying anything.
const NAME_MIN_W: f32 = 90.0;
/// Two label chips on a row, then a "+N". Two is what fits beside a name
/// without the name becoming an abbreviation.
const MAX_LABEL_CHIPS: usize = 2;
/// What one chip is assumed to cost when reserving room for the name. Chips
/// size to their own text, so this is a nominal: labels are short by
/// convention ("Q4", "infra"), and over-reserving costs a few characters of
/// name where under-reserving would clip a chip to nothing.
const LABEL_CHIP_W: f32 = 52.0;
/// The description's floor. Below this a one-liner is cut to nothing useful,
/// so the table would rather squeeze the window than this column.
const COL_DESCRIPTION: f32 = 200.0;
/// A count, right-aligned under its header. Two digits and air.
const COL_TASKS: f32 = 64.0;
/// The bar plus its percentage.
const COL_PROGRESS: f32 = 150.0;
const PROGRESS_BAR_W: f32 = 90.0;
const COL_STATUS: f32 = 104.0;
/// Just wide enough for the "P0" pill, same as every other task table.
const COL_PRIORITY: f32 = 52.0;
/// "in 3 weeks", right-aligned. The longest thing it holds is "11 months ago".
const COL_TARGET: f32 = 88.0;
const COL_CREATED: f32 = 78.0;

/// Alignment lives with the width, so "Progress" and "Created" sit over the
/// figures beneath them rather than at the other end of the column.
/// Ranked by what a narrow window can do without: the description goes
/// first, then the created date — when a project started matters less than
/// when it is due — then the task count, since progress says the same thing in
/// less room, then priority. Target drops last of the droppables, because
/// "which of these is late" is the question the table exists to answer. Name,
/// progress and status never drop.
const COLS: [Col; 8] = [
    Col::left("Project", COL_NAME),
    Col::fill("Description", COL_DESCRIPTION).rank(5),
    Col::right("Tasks", COL_TASKS).rank(3),
    Col::left("Progress", COL_PROGRESS),
    Col::left("Priority", COL_PRIORITY).rank(2),
    Col::left("Status", COL_STATUS),
    Col::right("Target", COL_TARGET).rank(1),
    Col::right("Created", COL_CREATED).rank(4),
];

/// What the trailing controls on a draft-task row need: three menus, a Remove,
/// and the gaps between them. The title input takes the rest.
const TASK_CONTROLS_W: f32 = 420.0;
/// Below this the title input stops giving ground; the row wraps instead.
const TASK_TITLE_MIN_W: f32 = 160.0;
/// A label name is one or two words; wider only invites a sentence.
const NEW_LABEL_W: f32 = 180.0;

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

/// The colours the label API accepts, exactly, paired with how the menu says
/// them. Sending anything else is a 400, so the list is closed on purpose.
const LABEL_COLOURS: [(&str, &str); 7] = [
    ("slate", "Slate"),
    ("blue", "Blue"),
    ("green", "Green"),
    ("amber", "Amber"),
    ("red", "Red"),
    ("purple", "Purple"),
    ("pink", "Pink"),
];
/// The quietest of the seven, which is the right default for a label whose
/// colour the person did not think about.
const DEFAULT_LABEL_COLOUR: &str = "slate";

/// What the create form holds. `None` on `State::creating` means the form is
/// closed, which is also how the Create project button knows not to redraw
/// itself over an open form.
pub struct Draft {
    pub title: String,
    pub description: String,
    /// Priority as `viz::select` holds it — a string, parsed to an int on
    /// submit, the same way a draft task's is.
    pub priority: Option<String>,
    /// Picked, so never malformed. `None` means "not set" and the key is
    /// omitted from the request.
    pub start: Option<NaiveDate>,
    pub target: Option<NaiveDate>,
    /// Label ids, in the order they were picked.
    pub labels: Vec<String>,
    /// Whether the make-a-label row is showing, and what is in it. Draft state
    /// rather than a widget's own memory, so Cancel clears it with the rest.
    pub new_label: bool,
    pub new_label_name: String,
    pub new_label_colour: Option<String>,
    /// The project's first tasks. Rows with no title are dropped on submit,
    /// so an accidental Add task costs nothing.
    pub tasks: Vec<DraftTask>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            priority: Some(DEFAULT_PRIORITY.to_owned()),
            start: None,
            target: None,
            labels: Vec::new(),
            new_label: false,
            new_label_name: String::new(),
            new_label_colour: Some(DEFAULT_LABEL_COLOUR.to_owned()),
            tasks: Vec::new(),
        }
    }
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
        net.invalidate(super::chrome::COUNTS);
        net.invalidate_prefix("task:");
        // The form hands out tasks too, so an assignee's personal list is stale
        // the moment this succeeds.
        net.invalidate_prefix("mytasks");
        app.board.creating = None;
    }

    // A new label is a side errand on the way to a project: fold the returned
    // id into the draft's selection and drop the cached vocabulary, so the
    // picker shows it already ticked. The endpoint is idempotent, so this is
    // also what happens when the name was already taken.
    let created = app.net.as_ref().unwrap().data(NEW_LABEL_KEY).cloned();
    if let Some(label) = created {
        let net = app.net.as_mut().unwrap();
        net.invalidate(NEW_LABEL_KEY);
        net.invalidate(LABELS_KEY);
        if let Some(draft) = app.board.creating.as_mut() {
            let id = str_at(&label, "id").to_owned();
            if !id.is_empty() && !draft.labels.contains(&id) {
                draft.labels.push(id);
            }
            draft.new_label_name.clear();
        }
    }
    let net = app.net.as_mut().unwrap();

    net.get_once(PROJECTS_KEY, "/api/user/projects");
    // The assignee picker needs names, and the rows need them to resolve
    // `memberIds`. One fetch serves both.
    net.get_once(PEOPLE_KEY, "/api/user/people");
    // Shared across every project, so one fetch serves the form's picker and
    // nothing in the table needs its own.
    net.get_once(LABELS_KEY, "/api/user/labels");

    let generation = net.generation(PROJECTS_KEY);
    let list = net.shared(PROJECTS_KEY);
    let list: &[Value] = list.as_deref().and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let people = array(net.data(PEOPLE_KEY));
    let labels = array(net.data(LABELS_KEY));
    let loading = net.is_loading(PROJECTS_KEY);
    let error = net.error(PROJECTS_KEY).map(str::to_string);
    let create_error = net.error(CREATE_KEY).map(str::to_string);
    // The label request is its own errand with its own row far down the form,
    // so its failure is reported there rather than in the banner up here.
    let label_error = net.error(NEW_LABEL_KEY).map(str::to_string);
    let creating = net.is_loading(CREATE_KEY);

    // Each row carries its own `done`/`total`, the numbers `/flow` gives, so
    // the list no longer asks for a `/flow` per project.
    let flows: HashMap<String, Value> = HashMap::new();

    let mut start_create = false;
    shell::page_title(ui, "Projects", "", |ui| {
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

    let mut post: Option<Post> = None;
    if let Some(draft) = app.board.creating.as_mut() {
        post = create_form(ui, draft, &people, &labels, creating, label_error.as_deref());
        ui.add_space(space::LG);
    }

    let mut open: Option<String> = None;
    let mut shown: Arc<Vec<Value>> = Arc::default();
    if !list.is_empty() {
        toolbar(ui, &mut app.board.filters, &labels);
        // Filtered and sorted once per reply or filter change, not per frame.
        let f = &app.board.filters;
        let stamp = (generation, f.search.clone(), f.status.clone(), f.label.clone(), f.priority.clone());
        shown = memo(ui.ctx(), egui::Id::new(SHOWN), stamp, || {
            let mut rows: Vec<Value> = list.iter().filter(|p| f.keeps(p)).cloned().collect();
            sort_projects(&mut rows, &flows);
            rows
        });
        ui.label(
            RichText::new(count_line(shown.len(), list.len()))
                .size(text::SMALL)
                .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                .color(colour::TEXT_MUTED),
        );
        ui.add_space(space::SM);
    }
    if let Some(err) = error {
        w::error(ui, &err);
    } else if list.is_empty() {
        if loading {
            w::loading(ui, "Loading projects");
        } else if app.board.creating.is_none() {
            w::empty(
                ui,
                "No projects yet.",
                "Create one and hand out its first tasks in the same step.",
            );
        }
    } else if shown.is_empty() {
        w::empty(ui, "No projects match these filters.", "Clear them to see every project.");
    } else {
        table(ui, &shown, &flows, &mut open);
        ui.add_space(space::XXL);
    }

    let net = app.net.as_mut().unwrap();
    match post {
        Some(Post::Project(body)) => net.post(CREATE_KEY, "/api/user/projects", body),
        Some(Post::Label(body)) => net.post(NEW_LABEL_KEY, "/api/user/labels", body),
        None => {}
    }
    if let Some(id) = open {
        app.project = Some(id);
    }
}

/// "5 projects", or "2 of 5 projects" once a filter is hiding some. Over the
/// table rather than in the toolbar, where it ran into the last filter at 820.
fn count_line(shown: usize, total: usize) -> String {
    let noun = if total == 1 { "project" } else { "projects" };
    if shown == total {
        format!("{total} {noun}")
    } else {
        format!("{shown} of {total} {noun}")
    }
}

// ------------------------------------------------------------------- filters

/// The list's filter bar. Lives on `board::State` so a filtered list is still
/// filtered after a trip into a project and back.
#[derive(Default)]
pub struct ListFilters {
    pub search: String,
    pub status: Option<String>,
    pub label: Option<String>,
    pub priority: Option<String>,
}

impl ListFilters {
    fn any(&self) -> bool {
        !self.search.trim().is_empty()
            || self.status.is_some()
            || self.label.is_some()
            || self.priority.is_some()
    }

    fn keeps(&self, p: &Value) -> bool {
        let labels = array(p.get("labels"));
        let names: Vec<&str> = labels.iter().map(|l| str_at(l, "name")).collect();
        let mut hay = vec![str_at(p, "name"), str_at(p, "description")];
        hay.extend(names);
        viz::matches(&self.search, &hay)
            && self.status.as_deref().is_none_or(|s| s == str_at(p, "status"))
            && self.label.as_deref().is_none_or(|id| labels.iter().any(|l| str_at(l, "id") == id))
            && self.priority.as_deref().is_none_or(|v| v == num_at(p, "priority").to_string())
    }
}

/// Status in the order a person works through them: what is moving, what is
/// parked, what is over.
pub(super) const PROJECT_STATUSES: [(&str, &str); 4] =
    [("active", "Active"), ("paused", "Paused"), ("done", "Done"), ("archived", "Archived")];

fn toolbar(ui: &mut egui::Ui, f: &mut ListFilters, labels: &[Value]) {
    let statuses = owned(&PROJECT_STATUSES);
    let priorities = owned(&PRIORITIES);
    let label_options: Vec<(String, String)> = labels
        .iter()
        .map(|l| (str_at(l, "id").to_owned(), str_at(l, "name").to_owned()))
        .collect();
    viz::toolbar(ui, |ui| {
        viz::search(ui, "Search name, description, label\u{2026}", &mut f.search);
        viz::select(ui, "Status", &statuses, &mut f.status);
        viz::select(ui, "Label", &label_options, &mut f.label);
        viz::select(ui, "Priority", &priorities, &mut f.priority);
        if f.any() && viz::clear(ui).clicked() {
            *f = ListFilters::default();
        }
    });
}

/// A `(value, label)` table as the owned pairs `viz::select` takes.
pub(super) fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect()
}

/// Active, paused, done, archived; inside each, what is late first, then the
/// urgent, then whatever is due soonest — a project with no date goes last,
/// since it has promised nothing yet. Created date breaks the last tie so rows
/// do not trade places between frames.
fn sort_projects(rows: &mut [Value], flows: &HashMap<String, Value>) {
    let status_rank = |p: &Value| {
        PROJECT_STATUSES.iter().position(|(s, _)| *s == str_at(p, "status")).unwrap_or(4)
    };
    let key = |p: &Value| {
        let flow = flows.get(str_at(p, "id")).unwrap_or(p);
        let (done, total) = (num_at(flow, "done"), num_at(flow, "total"));
        let overdue = matches!(health(p, done, total), Some((_, Health::Overdue)));
        let target = parse_date(str_at(p, "targetDate"));
        (status_rank(p), !overdue, num_at(p, "priority"), target.is_none(), target)
    };
    rows.sort_by(|a, b| {
        key(a).cmp(&key(b)).then(str_at(a, "createdAt").cmp(str_at(b, "createdAt")))
    });
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

/// `flow` overrides the row's own `done`/`total` when given; the list passes
/// none, since `/projects` rows carry both.
fn project_row(row: &mut table::Cells<'_, '_, '_>, p: &Value, flow: Option<&Value>) {
    let flow = flow.unwrap_or(p);
    let (done, total) = (num_at(flow, "done"), num_at(flow, "total"));
    let status = str_at(p, "status");

    // The name, then its labels. The name is what gives ground when there are
    // chips: a truncated name is still recognisable, a clipped chip is not.
    row.at(0, |ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        let labels = array(p.get("labels"));
        let shown = labels.len().min(MAX_LABEL_CHIPS);
        let extra = labels.len() - shown;
        let chips = shown + usize::from(extra > 0);
        let width =
            (ui.available_width() - chips as f32 * (LABEL_CHIP_W + space::XS)).max(NAME_MIN_W);
        ui.allocate_ui(egui::vec2(width, table::ROW_H), |ui| {
            table::strong_label(ui, str_at(p, "name"), colour::TEXT);
        });
        for label in labels.iter().take(shown) {
            c::chip(ui, str_at(label, "name"), label_tone(str_at(label, "colour")), false);
        }
        if extra > 0 {
            c::chip(ui, &format!("+{extra}"), c::Tone::Quiet, false);
        }
    });

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
        let priority = num_at(p, "priority").clamp(0, 4);
        c::chip(ui, &format!("P{priority}"), priority_tone(priority), false);
    });

    row.at(5, |ui| {
        c::chip(ui, status_label(status), c::status_tone(status), true);
    });

    // A date the table can act on: "in 3 weeks" answers "is this project in
    // trouble", where "2026-10-15" makes you do the subtraction yourself.
    match health(p, done, total) {
        Some((days, h)) => row.at(6, |ui| {
            let r = ui.add(
                egui::Label::new(RichText::new(relative_day(days)).size(text::SMALL).color(h.ink()))
                    .truncate()
                    .selectable(false),
            );
            r.on_hover_text(h.hover(p, done, total));
        }),
        None => row.muted(6, ""),
    }

    row.muted(7, &age(p));
}

/// How loud a priority is allowed to be. Same vocabulary as the task tables —
/// P0 and P1 get colour, below normal the pill recedes — because a project's
/// priority and a task's priority are the same scale and must not read as two.
pub(super) fn priority_tone(priority: i64) -> c::Tone {
    match priority {
        0 => c::Tone::Blocked,
        1 => c::Tone::Running,
        2 => c::Tone::Neutral,
        _ => c::Tone::Quiet,
    }
}

/// The seven server colour names mapped onto the chip vocabulary that already
/// exists. Not a new palette: a label sits in the same row as a status and a
/// priority, and an eighth hue would only make that row louder.
///
/// slate/blue/green/amber/red/purple land on Quiet/Info/Ok/Running/Blocked/
/// Agent, each the same hue. Pink has no token of its own and takes Agent's
/// violet, the nearest hue that is not already a state colour — mapping it to
/// the rose `DANGER` would make a "Marketing" chip read as a failure.
pub(super) fn label_tone(name: &str) -> c::Tone {
    match name {
        "blue" => c::Tone::Info,
        "green" => c::Tone::Ok,
        "amber" => c::Tone::Running,
        "red" => c::Tone::Blocked,
        "purple" | "pink" => c::Tone::Agent,
        _ => c::Tone::Quiet,
    }
}

/// How a project stands against its target date.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Health {
    Fine,
    /// Due within `AT_RISK_DAYS` and under half done.
    AtRisk,
    Overdue,
}

/// A week is the horizon a team can still do something about; closer than
/// that with most of the work open, the date is a warning, not a plan.
const AT_RISK_DAYS: i64 = 7;

impl Health {
    pub(super) fn ink(self) -> egui::Color32 {
        match self {
            Health::Fine => colour::TEXT_MUTED,
            Health::AtRisk => colour::WARN,
            Health::Overdue => colour::DANGER,
        }
    }

    /// The date itself, and why it is coloured when it is.
    pub(super) fn hover(self, p: &Value, done: i64, total: i64) -> String {
        let date = str_at(p, "targetDate");
        match self {
            Health::Fine => format!("Target {date}"),
            Health::AtRisk => format!(
                "At risk \u{2014} due {date}, {}% done",
                (fraction(done, total) * 100.0) as i64
            ),
            Health::Overdue => format!("Overdue \u{2014} was due {date}"),
        }
    }
}

/// Days to the target (negative once past), and what that means for this
/// project. A date on a finished or archived project is history rather than a
/// warning, so it never colours.
pub(super) fn health(p: &Value, done: i64, total: i64) -> Option<(i64, Health)> {
    let target = parse_date(str_at(p, "targetDate"))?;
    let days = (target - Utc::now().date_naive()).num_days();
    let h = if matches!(str_at(p, "status"), "done" | "archived") {
        Health::Fine
    } else if days < 0 {
        Health::Overdue
    } else if days <= AT_RISK_DAYS && fraction(done, total) < 0.5 {
        Health::AtRisk
    } else {
        Health::Fine
    };
    Some((days, h))
}

/// A day count as a person would say it. Days up to a fortnight, then weeks,
/// then months — the unit people plan in changes with the distance, and
/// "in 84 days" is a number nobody converts.
pub(super) fn relative_day(days: i64) -> String {
    let (n, unit) = match days.abs() {
        0 => return "today".to_owned(),
        n if n < 14 => (n, "day"),
        n if n < 60 => (n / 7, "week"),
        n => (n / 30, "month"),
    };
    let plural = if n == 1 { "" } else { "s" };
    if days > 0 {
        format!("in {n} {unit}{plural}")
    } else {
        format!("{n} {unit}{plural} ago")
    }
}

/// The one date format the API takes. Blank parses to `None` like anything
/// else malformed; callers that care about the difference check for empty.
pub(super) fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
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
    labels: &[Value],
    busy: bool,
    label_error: Option<&str>,
) -> Option<Post> {
    let mut post = None;
    let mut cancel = false;

    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        w::heading(ui, "New project");
        ui.add_space(space::MD);

        w::field(ui, "Title", &mut draft.title, false, "e.g. Checkout redesign…");
        ui.add_space(space::MD);
        w::field_multiline(
            ui,
            "Description",
            &mut draft.description,
            3,
            "e.g. Rebuild checkout so paying takes one screen…",
        );
        ui.add_space(space::MD);

        properties_row(ui, draft, labels);
        ui.add_space(space::SM);
        new_label_row(ui, draft, &mut post, label_error);
        ui.add_space(space::LG);

        // A task goes to anyone here. The project has no roster of its own:
        // the people on it are the people holding its tasks.
        let assignable: Vec<(String, String)> = people.iter().map(person_option).collect();
        tasks_section(ui, draft, &assignable);
        ui.add_space(space::LG);

        // A project with no title has nothing to be called and nothing to
        // derive a key from, so Create stays off until there is one. A target
        // before the start would only come back as a 400, so that waits too,
        // and says why.
        let dates_ok = match (draft.start, draft.target) {
            (Some(start), Some(target)) => target >= start,
            _ => true,
        };
        if !dates_ok {
            w::caption(ui, "The target date is before the start date.");
            ui.add_space(space::SM);
        }
        let ready = !draft.title.trim().is_empty() && dates_ok && !busy;
        ui.horizontal(|ui| {
            if w::primary(ui, if busy { "Creating…" } else { "Create" }, ready).clicked() {
                post = Some(Post::Project(project_body(draft)));
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
    post
}

/// What the form asked for this frame. Two different requests come out of one
/// form — making a label is a side errand on the way to making a project — so
/// the caller has to know which endpoint it is posting to.
enum Post {
    Project(Value),
    Label(Value),
}

/// The create request. Unset optionals are absent keys rather than nulls:
/// the API treats a missing `targetDate` as "no target" and a null as a value
/// it has to reject.
fn project_body(draft: &Draft) -> Value {
    let mut body = serde_json::json!({
        "name": draft.title.trim(),
        "description": draft.description.trim(),
        "tasks": task_bodies(draft),
        "priority": draft
            .priority
            .as_deref()
            .and_then(|p| p.parse::<i32>().ok())
            .unwrap_or(2),
    });
    let fields = body.as_object_mut().expect("json! built an object");
    for (key, date) in [("startDate", draft.start), ("targetDate", draft.target)] {
        if let Some(date) = date {
            fields.insert(key.to_owned(), Value::String(date.to_string()));
        }
    }
    if !draft.labels.is_empty() {
        fields.insert("labelIds".to_owned(), serde_json::json!(draft.labels));
    }
    body
}

/// The project's properties, in one row under the description: priority, the
/// two dates, labels. A row rather than a column because none of them is
/// required — four stacked fields read as a form to fill in, where four
/// controls side by side read as four things you may set.
fn properties_row(ui: &mut egui::Ui, draft: &mut Draft, labels: &[Value]) {
    let priorities: Vec<(String, String)> =
        PRIORITIES.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect();
    let options: Vec<(String, String)> = labels
        .iter()
        .map(|l| (str_at(l, "id").to_owned(), str_at(l, "name").to_owned()))
        .collect();

    // Wrapped, so four slots that do not fit at 820 fall to a second line
    // instead of running off the form's edge.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::MD, space::SM);
        ui.spacing_mut().interact_size.x = size::PICKER_W;
        labelled(ui, "Priority", |ui| {
            viz::select(ui, "P2 Normal", &priorities, &mut draft.priority);
        });
        labelled(ui, "Start date", |ui| {
            viz::date_picker(ui, "Not set", &mut draft.start);
        });
        labelled(ui, "Target date", |ui| {
            viz::date_picker(ui, "Not set", &mut draft.target);
        });
        labelled(ui, "Labels", |ui| {
            viz::multi_select(ui, "None", &options, &mut draft.labels);
        });
    });
}

/// A caption over a control, so a picker in the properties row carries its
/// name the same way the fields above it do.
fn labelled(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        w::caption(ui, label);
        ui.add_space(space::XXS);
        add(ui);
    });
}


/// Making a label without leaving the form.
///
/// Collapsed behind a link, because the common path is picking one that
/// already exists: a name box and a colour menu sitting open would make "new"
/// look like the expected move and turn one control into three.
fn new_label_row(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    post: &mut Option<Post>,
    error: Option<&str>,
) {
    if !draft.new_label {
        if w::link(ui, "+ New label").clicked() {
            draft.new_label = true;
        }
        return;
    }

    let colours: Vec<(String, String)> =
        LABEL_COLOURS.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        row_input(ui, NEW_LABEL_W, "e.g. Platform…", &mut draft.new_label_name);
        viz::select(ui, "Slate", &colours, &mut draft.new_label_colour);
        // The endpoint is idempotent, so a name that already exists comes back
        // as that label rather than as a duplicate or an error — which makes
        // this safe to press on a name you are not sure about.
        let ready = !draft.new_label_name.trim().is_empty();
        if w::secondary(ui, "Add label", ready).clicked() {
            *post = Some(Post::Label(serde_json::json!({
                "name": draft.new_label_name.trim(),
                "colour": draft.new_label_colour.as_deref().unwrap_or(DEFAULT_LABEL_COLOUR),
            })));
        }
        if w::ghost(ui, "Close").clicked() {
            draft.new_label = false;
            draft.new_label_name.clear();
        }
        if let Some(err) = error {
            w::error(ui, err);
        }
    });
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
            row_input(ui, width, "e.g. Design the payment step…", &mut task.title);
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
/// version and stacks its own caption above the box; a row of controls is
/// labelled once by the placeholders, not once per row.
///
/// Private to this file because the two rows that need one are both here —
/// promote it into `widgets` when a third turns up.
fn row_input(ui: &mut egui::Ui, width: f32, hint: &str, value: &mut String) -> egui::Response {
    ui.add_sized(
        [width, viz::HEIGHT],
        egui::TextEdit::singleline(value)
            .hint_text(RichText::new(hint).size(text::BODY).color(colour::TEXT_DISABLED))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The two bits of this file that are arithmetic rather than layout: the
    /// date the form accepts, and the words the Target column puts on it.
    #[test]
    fn dates_and_distances() {
        assert_eq!(parse_date(" 2026-10-15 ").map(|d| d.to_string()), Some("2026-10-15".into()));
        assert!(parse_date("15/10/2026").is_none());
        assert!(parse_date("").is_none());

        assert_eq!(relative_day(0), "today");
        assert_eq!(relative_day(1), "in 1 day");
        assert_eq!(relative_day(21), "in 3 weeks");
        assert_eq!(relative_day(-2), "2 days ago");
        assert_eq!(relative_day(-90), "3 months ago");
    }
}
