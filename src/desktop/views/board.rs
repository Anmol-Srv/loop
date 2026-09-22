//! Board: the project list, and inside a project its flow and its tasks.
//!
//! Two modes, chosen by `app.project`. The project detail screen opens with
//! what the project *is* — name, a meta line of three facts, the description —
//! and only then how far along it is. The list screen answers "where is the
//! work"; someone who has clicked into one project is asking the slower
//! question, and prose at the top is the answer to it.
//!
//! Under that, the flow strip: done/total per discipline. It reports, it does
//! not gate — a discipline may run ahead of the one to its left, so there is
//! no arrow implying an order the data does not have.
//!
//! Below the strip, the work itself, as a table on the same column machinery
//! as the projects list. It was a column of cards grouped by phase; both
//! halves of that were wrong. Cards cannot be compared down a page, which is
//! the whole reason you open a project, and phases are a server-side detail —
//! every project has one default phase and nobody here chooses it, so the
//! headings were dividing the list by a fact with one value.


use chrono::{DateTime, Utc};
use egui::RichText;
use serde_json::Value;

use super::projects::{
    AVATAR_OVERLAP, DEFAULT_PRIORITY, MAX_AVATARS, PEOPLE_KEY, PRIORITIES, PROSE_W,
};
use crate::desktop::design::table::{self, Col};
use crate::desktop::design::tokens::discipline_colour;
use crate::desktop::design::{
    avatar, cards as c, colour, shell, size, space, status_label, text, theme, viz, widgets as w,
};
use crate::desktop::App;

/// The usual order of the flow. Anything the server reports that is not in
/// here keeps its own order, after these — a new discipline should appear
/// rather than vanish because this list has not caught up.
const FLOW_ORDER: [&str; 3] = ["design", "frontend", "backend"];

/// Where an Add task's reply is collected. Under `board:` so the invalidation
/// that follows a success drops the reply along with the stale list.
const ADD_KEY: &str = "board:add-task";
/// The table's id: its scroll salt, and the slot its hover is tracked in.
const TABLE: &str = "board:tasks:table";

// ---- table geometry. Fixed so the columns line up with the header and with
// ---- each other; the description takes whatever is left.
/// The title is what the eye runs down, so it gets the widest fixed column;
/// past this it truncates rather than pushing the row around.
const COL_TASK: f32 = 260.0;
/// The description's floor. Below this a one-liner is cut to nothing useful.
const COL_DESCRIPTION: f32 = 180.0;
/// A face plus a name that is usually two words.
const COL_ASSIGNEE: f32 = 150.0;
/// Just wide enough for the "P0" pill.
const COL_PRIORITY: f32 = 52.0;
const COL_STATUS: f32 = 104.0;
const COL_CREATED: f32 = 78.0;

/// The roster's share of the meta line. Past this the names truncate rather
/// than crowding out the two facts that follow them.
const MEMBERS_W: f32 = 260.0;

/// Alignment lives with the width, so "Created" sits over the age beneath it.
/// Ranked by what a narrow window can do without. Title, assignee and status
/// are the three facts this table exists for and never drop.
const COLS: [Col; 6] = [
    Col::left("Task", COL_TASK),
    Col::fill("Description", COL_DESCRIPTION).rank(3),
    Col::left("Assignee", COL_ASSIGNEE),
    Col::left("Priority", COL_PRIORITY).rank(1),
    Col::left("Status", COL_STATUS),
    Col::right("Created", COL_CREATED).rank(2),
];

/// The open Add task form. Every optional field is an `Option<String>` because
/// that is the shape `viz::select` writes into.
pub struct TaskDraft {
    pub title: String,
    pub body: String,
    pub assignee: Option<String>,
    pub priority: Option<String>,
}

impl Default for TaskDraft {
    fn default() -> Self {
        Self {
            title: String::new(),
            body: String::new(),
            assignee: None,
            priority: Some(DEFAULT_PRIORITY.to_owned()),
        }
    }
}

#[derive(Default)]
pub struct State {
    /// The open create-project form, if there is one. Lives here rather than
    /// in `projects.rs` because both screens share one `State`.
    pub creating: Option<super::projects::Draft>,
    /// The open Add task form, if there is one.
    pub adding: Option<TaskDraft>,
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    match app.project.clone() {
        None => super::projects::ui(app, ui),
        Some(project_id) => project(app, ui, &project_id),
    }
}

// -------------------------------------------------------------- project detail

fn project(app: &mut App, ui: &mut egui::Ui, project_id: &str) {
    let detail_key = format!("board:project:{project_id}");
    let flow_key = format!("board:flow:{project_id}");
    let tasks_key = format!("board:tasks:{project_id}");
    let tasks_path = format!("/api/user/tasks?projectId={project_id}");

    let can_write = app.can_write();
    let net = app.net.as_mut().unwrap();

    // A finished Add task: the list here, the flow strip, the sidebar counts
    // and both personal lists all hold a board without it.
    if net.data(ADD_KEY).is_some() {
        net.invalidate_prefix("board:");
        net.invalidate("home");
        net.invalidate_prefix("task:");
        net.invalidate_prefix("mytasks");
        app.board.adding = None;
    }
    let net = app.net.as_mut().unwrap();

    net.get_once(&detail_key, &format!("/api/user/projects/{project_id}"));
    net.get_once(&flow_key, &format!("/api/user/projects/{project_id}/flow"));
    net.get_once(&tasks_key, &tasks_path);
    // The assignee picker wants names and departments. Same key the list
    // screen fills, so arriving from it costs nothing.
    net.get_once(PEOPLE_KEY, "/api/user/people");

    let detail = net.data(&detail_key).cloned();

    let flow = net.data(&flow_key).cloned();
    let flow_error = net.error(&flow_key).map(str::to_string);

    let mut tasks = array(net.data(&tasks_key));
    let tasks_loading = net.is_loading(&tasks_key);
    let tasks_error = net.error(&tasks_key).map(str::to_string);
    let add_error = net.error(ADD_KEY).map(str::to_string);
    let posting = net.is_loading(ADD_KEY);

    sort_tasks(&mut tasks);

    let mut back = false;
    let mut open_task: Option<String> = None;

    if shell::back(ui, "Projects").clicked() {
        back = true;
    }
    // Name, state and identifier on one line. `page_title`'s trailing slot is
    // right-aligned, and a key belongs *to* the name — it reads as an aside
    // beside it, not as a control at the other end of the header.
    let fallback = Value::Null;
    let head = detail.as_ref().or(flow.as_ref()).unwrap_or(&fallback);
    let name = match str_at(head, "name") {
        "" => "Project",
        n => n,
    };
    let status = str_at(head, "status");
    let key = str_at(head, "key");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        // Bounded to what is left after the chip and the key, so a long name
        // truncates instead of pushing them off the row.
        let width = (ui.available_width() - trailing_w(ui, status, key)).max(size::ROW);
        ui.allocate_ui(egui::vec2(width, size::ROW), |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(name)
                        .size(text::TITLE)
                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                        .color(colour::TEXT),
                )
                .truncate(),
            );
        });
        if !status.is_empty() {
            c::chip(ui, status_label(status), c::status_tone(status), true);
        }
        if !key.is_empty() {
            w::mono_caption(ui, key);
        }
    });

    ui.add_space(space::XXS);
    let (done, total) = flow
        .as_ref()
        .map(|f| (num_at(f, "done"), num_at(f, "total")))
        .unwrap_or((0, 0));
    // The people on a project are whoever holds its tasks — distinct, in
    // first-seen order so the stack does not reshuffle as tasks are added.
    let mut members: Vec<&str> = Vec::new();
    for t in &tasks {
        if let Some(who) = t.get("assigneeName").and_then(Value::as_str) {
            if !members.contains(&who) {
                members.push(who);
            }
        }
    }
    meta_line(ui, head, &members, done, total);

    ui.add_space(space::LG);
    description(ui, str_at(head, "description"));

    if let Some(err) = flow_error {
        ui.add_space(space::XL);
        w::error(ui, &err);
    } else if let Some(f) = flow.as_ref().filter(|f| !flow_columns(f).is_empty()) {
        // Sits with the meta line rather than as a section of its own: it is
        // the same sentence, broken down. A project whose tasks have no
        // discipline yet draws nothing, and the spacing goes with it.
        ui.add_space(space::SM);
        flow_strip(ui, f);
    }


    // The button lives in the heading's trailing slot rather than under the
    // table: the thing you add to is named right there.
    let mut start_add = false;
    let form_open = app.board.adding.is_some();
    shell::section_count_with(ui, "Tasks", tasks.len(), |ui| {
        // While the form is open the button would only re-open what is open.
        if !form_open && w::primary(ui, "Add task", can_write).clicked() {
            start_add = true;
        }
    });
    if start_add {
        app.board.adding = Some(TaskDraft::default());
    }

    // Anyone here can be handed a task; the project has no roster of its own.
    // The menu shows each person's department, since that is what the task's
    // discipline will become.
    let mut members: Vec<(String, String)> =
        array(net.data(PEOPLE_KEY)).iter().map(super::projects::person_option).collect();
    members.sort_by(|a, b| a.1.cmp(&b.1));

    let mut submit: Option<Value> = None;
    let mut close_form = false;
    if let Some(draft) = app.board.adding.as_mut() {
        match add_form(ui, draft, &members, posting, add_error.as_deref()) {
            Some(Form::Submit(body)) => submit = Some(body),
            Some(Form::Cancel) => close_form = true,
            None => {}
        }
        ui.add_space(space::LG);
    }
    if close_form {
        app.board.adding = None;
    }

    if let Some(err) = tasks_error {
        w::error(ui, &err);
    } else if tasks.is_empty() {
        if tasks_loading {
            w::loading(ui, "tasks");
        } else if app.board.adding.is_none() {
            w::empty(ui, "No tasks yet.", "Add one and assign it to someone on this project.");
        }
    } else {
        table(ui, &tasks, &mut open_task);
        ui.add_space(space::XXL);
    }

    if back {
        app.project = None;
    }
    if let Some(id) = open_task {
        app.task = Some(id);
    }
    let net = app.net.as_mut().unwrap();
    if let Some(body) = submit {
        // Drop a previous attempt's error, so the banner belongs to this one.
        net.invalidate(ADD_KEY);
        net.post(ADD_KEY, &format!("/api/user/projects/{project_id}/tasks"), body);
    }
}

/// Open work first, then what is finished, then what was abandoned; inside a
/// group the urgent thing on top, and ties broken by age so the order does not
/// shuffle between frames.
fn sort_tasks(tasks: &mut [Value]) {
    // `doneAt` is what says a task is over: the terminal state differs by
    // track — design stops at completed, engineering carries on to shipped —
    // and only the stamp knows which one this task's assignee is on.
    fn rank(t: &Value) -> u8 {
        if str_at(t, "status") == "dropped" {
            return 2;
        }
        u8::from(!t.get("doneAt").map_or(true, Value::is_null))
    }
    tasks.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then(num_at(a, "priority").cmp(&num_at(b, "priority")))
            .then(str_at(a, "createdAt").cmp(str_at(b, "createdAt")))
    });
}

/// The three facts about a project, in one quiet row: when it started, who is
/// on it, how much of it is done.
///
/// Not a card. Three facts do not need a container — the space around them
/// already groups them, and a box here would be the first of the nested cards
/// this page exists to avoid. Labels are muted, values full-strength: the
/// label is scaffolding you read once, the value is what you came for.
fn meta_line(ui: &mut egui::Ui, p: &Value, members: &[&str], done: i64, total: i64) {
    ui.horizontal(|ui| {
        // Tight inside a fact, generous between them: proximity does the
        // grouping that separators would otherwise have to.
        ui.spacing_mut().item_spacing.x = space::XS;

        if let Some((relative, absolute)) = created(str_at(p, "createdAt")) {
            label(ui, "Created");
            value(ui, &relative).on_hover_text(absolute);
            ui.add_space(space::XL);
        }

        if members.is_empty() {
            label(ui, "Nobody assigned");
        } else {
            // The same overlapping stack the cards use, so a roster is one
            // shape across the app. The ring is the canvas, not a surface:
            // there is no card behind this row to cut out of.
            ui.spacing_mut().item_spacing.x = -AVATAR_OVERLAP;
            for who in members.iter().take(MAX_AVATARS) {
                let r = avatar::small(ui, who, size::AVATAR_SM).on_hover_text(*who);
                ui.painter().circle_stroke(
                    r.rect.center(),
                    size::AVATAR_SM / 2.0,
                    egui::Stroke::new(1.5, colour::CANVAS),
                );
            }
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.add_space(AVATAR_OVERLAP + space::XS);
            let shown = members.iter().take(MAX_AVATARS).copied().collect::<Vec<_>>().join(", ");
            let rest = members.len().saturating_sub(MAX_AVATARS);
            let names = if rest > 0 { format!("{shown} +{rest}") } else { shown };
            // Capped: three long names would otherwise push the done count off
            // the end of the row.
            ui.allocate_ui(egui::vec2(MEMBERS_W.min(ui.available_width()), size::ROW), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(names).size(text::SMALL).color(colour::TEXT),
                    )
                    .truncate(),
                )
                .on_hover_text(members.join(", "));
            });
        }
        ui.add_space(space::XL);

        if total > 0 {
            value(ui, &format!("{done} of {total}"));
            label(ui, "done");
        } else {
            label(ui, "No tasks yet");
        }
    });
}

/// What the status chip and the key will take on the title row, so the name
/// can be bounded to what is left. Mirrors `cards::chip`'s own sizing — the
/// label, its dot, and padding either side.
fn trailing_w(ui: &egui::Ui, status: &str, key: &str) -> f32 {
    let mut width = 0.0;
    if !status.is_empty() {
        let galley = ui.painter().layout_no_wrap(
            status_label(status).to_owned(),
            egui::FontId::proportional(text::CAPTION),
            colour::TEXT,
        );
        width += galley.size().x + space::SM * 3.0 + 10.0;
    }
    if !key.is_empty() {
        let galley = ui.painter().layout_no_wrap(
            key.to_owned(),
            egui::FontId::monospace(text::CAPTION),
            colour::TEXT,
        );
        width += galley.size().x + space::SM;
    }
    width
}

/// A meta-line label: the word, not the fact.
fn label(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT_MUTED));
}

/// A meta-line value. Full ink — the owner wants the numbers readable at a
/// glance, and a muted figure is one the eye skips.
fn value(ui: &mut egui::Ui, s: &str) -> egui::Response {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT))
}

/// "3 days ago", with the absolute date for the hover.
///
/// Wordier than `age`, on purpose: a chip repeated down forty rows wants
/// `3d`, a fact you read once wants the sentence.
fn created(ts: &str) -> Option<(String, String)> {
    let then = DateTime::parse_from_rfc3339(ts).ok()?.with_timezone(&Utc);
    let secs = (Utc::now() - then).num_seconds().max(0);
    let relative = match secs {
        s if s < 3600 => "just now".to_owned(),
        s if s < 86_400 => units(s / 3600, "hour"),
        s if s < 2_592_000 => units(s / 86_400, "day"),
        s => units(s / 2_592_000, "month"),
    };
    Some((relative, then.format("%-d %B %Y, %H:%M UTC").to_string()))
}

fn units(n: i64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

/// The description, as prose.
///
/// Held to `PROSE_W` rather than the content width: the page is 1080 wide and
/// a paragraph that wide loses the line it is on. Paragraphs are split on the
/// blank line the API sends them with; nothing wraps this in a card, because
/// text on the canvas is already the most readable thing we can do with it.
fn description(ui: &mut egui::Ui, body: &str) {
    let body = body.trim();
    if body.is_empty() {
        w::caption(ui, "No description");
        return;
    }
    ui.scope(|ui| {
        ui.set_max_width(PROSE_W.min(ui.available_width()));
        for (i, para) in body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).enumerate() {
            if i > 0 {
                ui.add_space(space::MD);
            }
            ui.label(RichText::new(para).size(text::BODY).color(colour::TEXT_2));
        }
    });
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

/// The per-discipline split, as one line of facts.
///
/// This was a labelled progress bar per discipline. The bar was a figure's
/// worth of ink for a fraction that is 0/1 or 1/1 on a project this size, and
/// the meta line above already carries the total — what is left worth saying
/// is the split, in the same label/value vocabulary as the line it sits under.
///
/// It reports, it does not gate: no arrow between the disciplines, because
/// frontend and backend can and do run before design has finished.
fn flow_strip(ui: &mut egui::Ui, flow: &Value) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        for (i, d) in flow_columns(flow).iter().enumerate() {
            if i > 0 {
                ui.add_space(space::XL);
            }
            let name = str_at(d, "discipline");
            // The discipline's own colour carries the name, so the split is
            // scannable without a legend or a swatch beside it.
            ui.label(
                RichText::new(name)
                    .size(text::SMALL)
                    .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                    .color(discipline_colour(name)),
            );
            value(ui, &format!("{}/{}", num_at(d, "done"), num_at(d, "total")));
        }
    });
}

// --------------------------------------------------------------------- table

/// The project's tasks, on the shared table — so a task row is the same row
/// the projects list draws, down to the alignment of its dates.
fn table(ui: &mut egui::Ui, rows: &[Value], open: &mut Option<String>) {
    let clicked = table::show(ui, TABLE, &COLS, rows.len(), |row, i| {
        task_row(row, &rows[i]);
    });
    if let Some(i) = clicked {
        *open = Some(str_at(&rows[i], "id").to_owned());
    }
}

fn task_row(row: &mut table::Cells<'_, '_, '_>, t: &Value) {
    let status = str_at(t, "status");
    let blocked = num_at(t, "blockersDone") < num_at(t, "blockersTotal");

    row.at(0, |ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        table::strong_label(ui, str_at(t, "title"), colour::TEXT);
        // Blockers beat the status column: a task marked in progress that
        // waits on someone else is not in progress, and the chip beside its
        // title is what says so.
        if blocked {
            c::chip(ui, "blocked", c::Tone::Blocked, false);
        }
    });

    // One line. The column clips, and a wrapped cell would make one row taller
    // than the rest of the table.
    row.muted(1, str_at(t, "body").lines().next().unwrap_or("").trim());

    row.at(2, |ui| {
        let who = str_at(t, "assigneeName");
        if who.is_empty() {
            ui.label(RichText::new("Unassigned").size(text::SMALL).color(colour::TEXT_FAINT));
            return;
        }
        ui.spacing_mut().item_spacing.x = space::XS;
        avatar::small(ui, who, size::AVATAR_SM);
        ui.label(RichText::new(who).size(text::SMALL).color(colour::TEXT_2));
    });

    row.at(3, |ui| {
        let p = num_at(t, "priority").clamp(0, 4);
        c::chip(ui, &format!("P{p}"), priority_tone(p), false);
    });

    row.at(4, |ui| {
        c::chip(ui, status_label(status), c::status_tone(status), true);
    });

    row.muted(5, &since(str_at(t, "createdAt")));
}

/// How loud a priority is allowed to be. P0 and P1 are the only ones worth
/// colour; below normal the pill recedes, because a column of five tinted
/// pills is a column you stop reading.
fn priority_tone(priority: i64) -> c::Tone {
    match priority {
        0 => c::Tone::Blocked,
        1 => c::Tone::Running,
        2 => c::Tone::Neutral,
        _ => c::Tone::Quiet,
    }
}

// ------------------------------------------------------------------ add task

/// A server error that names a field belongs under that field; anything else
/// is a banner. Matched on the word because the API reports validation in
/// prose, not in a field/message pair.
fn names<'a>(error: Option<&'a str>, field: &str) -> Option<&'a str> {
    error.filter(|e| e.to_lowercase().contains(field))
}

/// What the form said this frame.
enum Form {
    Submit(Value),
    Cancel,
}

/// The Add task form, inline under the heading rather than in a modal.
///
/// Same trade the create-project form makes: covering the list to ask what to
/// add to the list is worse than pushing it down a few rows.
fn add_form(
    ui: &mut egui::Ui,
    draft: &mut TaskDraft,
    members: &[(String, String)],
    busy: bool,
    error: Option<&str>,
) -> Option<Form> {
    let mut out = None;

    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        w::field(ui, "Title", &mut draft.title, false, "e.g. Design the payment step…");
        if let Some(err) = names(error, "title") {
            ui.add_space(space::XXS);
            w::error(ui, err);
        }
        ui.add_space(space::MD);
        w::field_multiline(
            ui,
            "Description",
            &mut draft.body,
            2,
            "e.g. Cards and UPI only, no wallets…",
        );
        ui.add_space(space::MD);

        let priorities: Vec<(String, String)> =
            PRIORITIES.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            viz::select(ui, "Unassigned", members, &mut draft.assignee);
            viz::select(ui, "P2 Normal", &priorities, &mut draft.priority);
        });
        ui.add_space(space::LG);

        // Anything the server did not pin on a field stays a banner, but it
        // sits with the button that caused it rather than above the form.
        if let Some(err) = error.filter(|_| names(error, "title").is_none()) {
            w::error(ui, err);
            ui.add_space(space::MD);
        }

        // An untitled task has nothing to be listed as, so Add stays off.
        let ready = !draft.title.trim().is_empty() && !busy;
        ui.horizontal(|ui| {
            if w::primary(ui, if busy { "Adding…" } else { "Add" }, ready).clicked() {
                out = Some(Form::Submit(serde_json::json!({
                    "title": draft.title.trim(),
                    "body": draft.body.trim(),
                    "assigneeId": draft.assignee,
                    "priority": draft
                        .priority
                        .as_deref()
                        .and_then(|p| p.parse::<i32>().ok())
                        .unwrap_or(2),
                })));
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                out = Some(Form::Cancel);
            }
        });
    });

    out
}

// ---------------------------------------------------------------------- pieces

/// How long ago, in the two characters a table column has room for.
fn since(ts: &str) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(ts) else {
        return String::new();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// How long since the task last moved. Nothing in this file reads it any more
/// — the table dates a task from when it was created — but it is part of this
/// module's exported vocabulary and the task views are mid-rewrite, so it
/// stays rather than being deleted out from under them.
#[allow(dead_code)]
pub(super) fn age(t: &Value) -> String {
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

pub(super) fn array(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

pub(super) fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

pub(super) fn num_at(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

pub(super) fn fraction(done: i64, total: i64) -> f32 {
    if total <= 0 {
        return 0.0;
    }
    done as f32 / total as f32
}
