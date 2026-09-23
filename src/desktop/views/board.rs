//! Board: the project list, and inside a project its properties, resources
//! and tasks.
//!
//! Two modes, chosen by `app.project`. The project page is Linear's shape: a
//! reading column (name, description, the per-department split, resources,
//! tasks) and a properties rail beside it. Status, priority, dates, labels and
//! who is on it are facts you glance at and change in place, so they live in
//! the rail; the old meta line under the title carried three of them and let
//! you change none, and never said the project was late.
//!
//! The flow strip reports done/total per department. It does not gate — a
//! department may run ahead of the one to its left, so there is no arrow
//! implying an order the data does not have.
//!
//! The work itself is a table on the same column machinery as the projects
//! list, sliced into open work and finished: cards cannot be compared down a
//! page, which is the whole reason you open a project.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use egui::RichText;
use serde_json::{json, Value};

use super::projects::{
    date_field, health, label_tone, owned, parse_date, priority_tone, relative_day, Health,
    DEFAULT_PRIORITY, LABELS_KEY, PEOPLE_KEY, PRIORITIES, PROJECTS_KEY, PROJECT_STATUSES, PROSE_W,
};
use crate::desktop::design::table::{self, Col};
use crate::desktop::design::tokens::discipline_colour;
use crate::desktop::design::{
    avatar, cards as c, colour, motion, radius, shell, size, space, status_label, text, theme,
    viz, widgets as w,
};
use crate::desktop::net::Net;
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
/// past this it truncates rather than pushing the row around. Sized so title,
/// assignee and status fit the content column beside the rail at 1100 wide —
/// any wider and the table shoved the rail off the window.
const COL_TASK: f32 = 210.0;
/// The description's floor. Below this a one-liner is cut to nothing useful.
const COL_DESCRIPTION: f32 = 180.0;
/// A face plus a name that is usually two words.
const COL_ASSIGNEE: f32 = 140.0;
/// Just wide enough for the "P0" pill.
const COL_PRIORITY: f32 = 52.0;
const COL_STATUS: f32 = 104.0;
const COL_CREATED: f32 = 78.0;

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
    /// The project list's filter bar, kept across a trip into a project.
    pub filters: super::projects::ListFilters,
    /// Each project page's drafts, keyed by project id, so a half-typed edit
    /// survives going to a task and coming back.
    pages: HashMap<String, Page>,
}

/// What one project page remembers between frames.
#[derive(Default)]
struct Page {
    /// Name and description, while Edit is open.
    editing: Option<(String, String)>,
    /// The date being typed in the rail, and what is typed so far.
    date: Option<(DateSlot, String)>,
    /// The label set as last picked, held until the server has echoed it back.
    /// Without it a second tick made before the first PATCH lands would be
    /// computed from the stale set and undo the first.
    labels: Option<Vec<String>>,
    /// Which slice of the task table: open work, finished, or all.
    tab: usize,
    attach: Option<Attach>,
    /// The resource whose Remove is waiting on a yes.
    removing: Option<String>,
    /// A PATCH the server parked for approval rather than applied.
    notice: Option<String>,
    /// A remove that failed for a reason other than the link being gone.
    resource_error: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum DateSlot {
    Start,
    Target,
}

impl DateSlot {
    fn field(self) -> &'static str {
        match self {
            DateSlot::Start => "startDate",
            DateSlot::Target => "targetDate",
        }
    }

    fn label(self) -> &'static str {
        match self {
            DateSlot::Start => "Start date",
            DateSlot::Target => "Target date",
        }
    }
}

/// The open "+ Add" form under Resources.
#[derive(Default)]
struct Attach {
    /// `viz::select`'s slot. `None` is the resting kind, the first of `KINDS`.
    kind: Option<String>,
    url: String,
    title: String,
}

/// What a project's resources can be: `(api value, label, example)`. A mirror
/// of the task page's evidence kinds, less the commit — a hash belongs to the
/// task that made it, not to the project it rolls up into, so every kind here
/// is a URL and one check covers them all.
const KINDS: [(&str, &str, &str); 4] = [
    ("pr", "PR", "https://github.com/org/repo/pull/123"),
    ("doc", "Doc", "https://\u{2026}"),
    ("figma", "Figma link", "https://figma.com/file/\u{2026}"),
    ("link", "Link", "https://\u{2026}"),
];

fn kind_label(kind: &str) -> &'static str {
    KINDS.iter().find(|(k, _, _)| *k == kind).map_or("Link", |(_, l, _)| l)
}

/// Same hue per kind as the task page, so a PR reads as a PR on both.
fn kind_tone(kind: &str) -> c::Tone {
    match kind {
        "pr" => c::Tone::Info,
        "commit" => c::Tone::Ok,
        "figma" => c::Tone::Agent,
        _ => c::Tone::Quiet,
    }
}

/// The task table's three slices. Open work is the default: it is what someone
/// opening a project is there to chase.
const TABS: [&str; 3] = ["Open work", "Finished", "All"];

/// The rail's progress bar. Short: it sits beside "2 of 6 done" in a value
/// column about 140 wide.
const RAIL_BAR_W: f32 = 56.0;

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    match app.project.clone() {
        None => super::projects::ui(app, ui),
        Some(project_id) => project(app, ui, &project_id),
    }
}

// -------------------------------------------------------------- project detail

/// Every request key and path this page uses, in one place.
struct Keys {
    detail: String,
    detail_path: String,
    flow: String,
    tasks: String,
    patch: String,
    artifacts: String,
    artifacts_path: String,
    attach: String,
    remove: String,
}

impl Keys {
    fn new(id: &str) -> Self {
        Self {
            detail: format!("board:project:{id}"),
            detail_path: format!("/api/user/projects/{id}"),
            flow: format!("board:flow:{id}"),
            tasks: format!("board:tasks:{id}"),
            // Not under `board:`: an Add task sweeps that prefix, and a PATCH
            // reply swept before it is read would leave its edit open forever.
            patch: format!("project:patch:{id}"),
            artifacts: format!("project:artifacts:{id}"),
            artifacts_path: format!("/api/user/artifacts?parentType=project&parentId={id}"),
            attach: format!("project:attach:{id}"),
            remove: format!("project:remove:{id}"),
        }
    }
}

/// What the page asked the server for this frame. Collected and sent at the
/// end, once nothing is borrowing `app`.
enum Request {
    Patch(Value),
    Attach(Value),
    Remove(String),
}

fn project(app: &mut App, ui: &mut egui::Ui, project_id: &str) {
    let keys = Keys::new(project_id);
    let can_write = app.can_write();
    let page = app.board.pages.entry(project_id.to_owned()).or_default();
    let net = app.net.as_mut().unwrap();

    settle(net, &keys, page);

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

    net.get_once(&keys.detail, &keys.detail_path);
    net.get_once(&keys.flow, &format!("/api/user/projects/{project_id}/flow"));
    net.get_once(&keys.tasks, &format!("/api/user/tasks?projectId={project_id}"));
    net.get_once(&keys.artifacts, &keys.artifacts_path);
    // The assignee picker wants names and departments. Same key the list
    // screen fills, so arriving from it costs nothing.
    net.get_once(PEOPLE_KEY, "/api/user/people");
    net.get_once(LABELS_KEY, "/api/user/labels");

    let detail = net.data(&keys.detail).cloned();
    let flow = net.data(&keys.flow).cloned();
    let flow_error = net.error(&keys.flow).map(str::to_string);
    let mut tasks = array(net.data(&keys.tasks));
    let tasks_loading = net.is_loading(&keys.tasks);
    let tasks_error = net.error(&keys.tasks).map(str::to_string);
    let add_error = net.error(ADD_KEY).map(str::to_string);
    let posting = net.is_loading(ADD_KEY);
    let patching = net.is_loading(&keys.patch);
    let patch_error = net.error(&keys.patch).map(str::to_string);
    let refreshing = net.is_loading(&keys.detail);
    let artifacts = net.data(&keys.artifacts).and_then(Value::as_array).cloned();
    let artifacts_error = net.error(&keys.artifacts).map(str::to_string);
    let attaching = net.is_loading(&keys.attach);
    let attach_error = net.error(&keys.attach).map(str::to_string);
    let all_labels = array(net.data(LABELS_KEY));
    // Anyone here can be handed a task; the project has no roster of its own.
    // The menu shows each person's department, since that is what the task's
    // department will become.
    let mut members: Vec<(String, String)> =
        array(net.data(PEOPLE_KEY)).iter().map(super::projects::person_option).collect();
    members.sort_by(|a, b| a.1.cmp(&b.1));

    sort_tasks(&mut tasks);

    // The picked label set is only worth holding while the server has not yet
    // caught up with it.
    if !patching && !refreshing {
        page.labels = None;
    }

    let mut back = false;
    let mut open_task: Option<String> = None;
    let requests: Vec<Request> = Vec::new();

    if shell::back(ui, "Projects").clicked() {
        back = true;
    }

    let fallback = Value::Null;
    let head = detail.as_ref().or(flow.as_ref()).unwrap_or(&fallback);
    let (done, total) = flow
        .as_ref()
        .map(|f| (num_at(f, "done"), num_at(f, "total")))
        .unwrap_or((0, 0));

    let form_open = app.board.adding.is_some();
    let mut start_add = false;
    let mut add_outcome: Option<Form> = None;

    // Both halves of the page edit the same drafts and queue requests; the
    // rail and the content never run at once, so a RefCell is all it takes.
    let page_cell = std::cell::RefCell::new(page);
    let requests_cell = std::cell::RefCell::new(requests);
    shell::with_rail(
        ui,
        |ui, part| {
            let mut page = page_cell.borrow_mut();
            let page: &mut Page = &mut page;
            let mut requests = requests_cell.borrow_mut();
            let requests: &mut Vec<Request> = &mut requests;
            if part == shell::Part::Header {
                headline(ui, head, page, can_write, patching, requests);
                return;
            }

            if let Some(err) = &flow_error {
                ui.add_space(space::XL);
                w::error(ui, err);
            } else if let Some(f) = flow.as_ref().filter(|f| !flow_columns(f).is_empty()) {
                // The same sentence as the rail's progress, broken down by
                // department. A project whose tasks have no department yet
                // draws nothing, and the spacing goes with it.
                ui.add_space(space::MD);
                flow_strip(ui, f);
            }

            shell::divider(ui);
            resources(
                ui,
                project_id,
                artifacts.as_deref(),
                artifacts_error.as_deref(),
                page,
                can_write,
                attaching,
                attach_error.as_deref(),
                requests,
            );

            shell::divider(ui);
            let finished = tasks.iter().filter(|t| is_finished(t)).count();
            let counts = [tasks.len() - finished, finished, tasks.len()];
            // The button lives in the heading's trailing slot rather than under
            // the table: the thing you add to is named right there.
            shell::section_count_with(ui, "Tasks", tasks.len(), |ui| {
                // While the form is open the button would only re-open it.
                if !form_open {
                    let r = w::primary(ui, "Add task", can_write);
                    if !can_write {
                        r.on_disabled_hover_text("Needs write access.");
                    } else if r.clicked() {
                        start_add = true;
                    }
                }
            });
            if !tasks.is_empty() {
                let labels: Vec<String> =
                    TABS.iter().zip(counts).map(|(l, n)| format!("{l} {n}")).collect();
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                if let Some(i) = c::tabs(ui, &refs, page.tab) {
                    page.tab = i;
                }
            }

            if let Some(draft) = app.board.adding.as_mut() {
                add_outcome = add_form(ui, draft, &members, posting, add_error.as_deref());
                ui.add_space(space::LG);
            }

            let shown: Vec<Value> = tasks
                .iter()
                .filter(|t| match page.tab {
                    0 => !is_finished(t),
                    1 => is_finished(t),
                    _ => true,
                })
                .cloned()
                .collect();
            if let Some(err) = &tasks_error {
                w::error(ui, err);
            } else if tasks.is_empty() {
                if tasks_loading {
                    w::loading(ui, "Loading tasks");
                } else if !form_open {
                    w::empty(
                        ui,
                        "No tasks yet.",
                        "Add one and assign it to someone on this project.",
                    );
                }
            } else if shown.is_empty() {
                let (what, next) = if page.tab == 0 {
                    ("No open work.", "Everything here is finished \u{2014} see Finished.")
                } else {
                    ("Nothing finished yet.", "Tasks land here once they are done or dropped.")
                };
                w::empty(ui, what, next);
            } else {
                table(ui, &shown, &mut open_task);
                ui.add_space(space::XXL);
            }
        },
        |ui| {
            let mut page = page_cell.borrow_mut();
            let page: &mut Page = &mut page;
            let mut requests = requests_cell.borrow_mut();
            let requests: &mut Vec<Request> = &mut requests;
            rail(
                ui,
                head,
                page,
                &tasks,
                &all_labels,
                (done, total),
                can_write,
                requests,
            );
            if let Some(err) = &patch_error {
                ui.add_space(space::SM);
                w::error(ui, err);
            } else if let Some(n) = &page.notice {
                ui.add_space(space::SM);
                w::caption(ui, n);
            }
        },
    );

    if start_add {
        app.board.adding = Some(TaskDraft::default());
    }
    let mut submit: Option<Value> = None;
    match add_outcome {
        Some(Form::Submit(body)) => submit = Some(body),
        Some(Form::Cancel) => app.board.adding = None,
        None => {}
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
    for request in requests_cell.into_inner() {
        match request {
            Request::Patch(body) => {
                net.invalidate(&keys.patch);
                net.patch(&keys.patch, &keys.detail_path, body);
            }
            Request::Attach(body) => {
                net.invalidate(&keys.attach);
                net.post(&keys.attach, "/api/user/artifacts", body);
            }
            Request::Remove(id) => {
                net.invalidate(&keys.remove);
                net.send(
                    &keys.remove,
                    reqwest::Method::DELETE,
                    &format!("/api/user/artifacts/{id}"),
                    Value::Null,
                );
            }
        }
    }
}

/// Fold in whatever replies landed since last frame.
fn settle(net: &mut Net, keys: &Keys, page: &mut Page) {
    if let Some(reply) = net.data(&keys.patch).cloned() {
        net.invalidate(&keys.patch);
        // Refetched in place rather than invalidated, so the page keeps the old
        // values on screen until the new ones arrive instead of blinking.
        net.get(&keys.detail, &keys.detail_path);
        // The list's rows and Home's projects carry these same fields.
        net.invalidate(PROJECTS_KEY);
        net.invalidate("home");
        page.editing = None;
        page.date = None;
        page.notice = (str_at(&reply, "status") == "proposed")
            .then(|| "Sent for approval \u{2014} it changes once someone signs off.".to_owned());
    }
    if net.data(&keys.attach).is_some() {
        net.invalidate(&keys.attach);
        net.invalidate(&keys.artifacts);
        page.attach = None;
    }
    if let Some(result) = net.peek(&keys.remove).cloned() {
        net.invalidate(&keys.remove);
        net.invalidate(&keys.artifacts);
        page.removing = None;
        // A second Remove of the same link is a 404 that says so; the list
        // refresh already tells the truth, so that one is not an error.
        page.resource_error = result.err().filter(|e| !e.contains("already gone"));
    }
}

/// Finished is `doneAt` set or dropped: each track has its own last state, and
/// only the stamp knows which one this task's assignee is on.
fn is_finished(t: &Value) -> bool {
    str_at(t, "status") == "dropped" || !t.get("doneAt").map_or(true, Value::is_null)
}

/// The title, and the description as prose — or both as fields, once Edit is
/// pressed.
fn headline(
    ui: &mut egui::Ui,
    head: &Value,
    page: &mut Page,
    can_write: bool,
    patching: bool,
    requests: &mut Vec<Request>,
) {
    let name = match str_at(head, "name") {
        "" => "Project",
        n => n,
    };

    if let Some((draft_name, draft_body)) = page.editing.as_mut() {
        ui.add_space(space::XS);
        w::field(ui, "Name", draft_name, false, "e.g. Checkout redesign\u{2026}");
        ui.add_space(space::MD);
        w::field_multiline(ui, "Description", draft_body, 4, "What this project is for\u{2026}");
        ui.add_space(space::MD);
        let ready = !draft_name.trim().is_empty() && !patching;
        let mut cancel = false;
        ui.horizontal(|ui| {
            if w::primary(ui, if patching { "Saving\u{2026}" } else { "Save" }, ready).clicked() {
                requests.push(Request::Patch(json!({
                    "name": draft_name.trim(),
                    "description": draft_body.trim(),
                })));
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                cancel = true;
            }
        });
        if cancel {
            page.editing = None;
        }
        return;
    }

    let mut edit = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if can_write && w::ghost(ui, "Edit").on_hover_text("Edit name and description").clicked()
            {
                edit = true;
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
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
        });
    });
    if edit {
        page.editing = Some((name.to_owned(), str_at(head, "description").to_owned()));
    }
    ui.add_space(space::MD);
    description(ui, str_at(head, "description"));
}

// ---------------------------------------------------------------------- rail

/// Everything about the project that is a fact rather than prose, each
/// editable where it sits when the viewer may write.
#[allow(clippy::too_many_arguments)]
fn rail(
    ui: &mut egui::Ui,
    head: &Value,
    page: &mut Page,
    tasks: &[Value],
    all_labels: &[Value],
    (done, total): (i64, i64),
    can_write: bool,
    requests: &mut Vec<Request>,
) {
    let status = str_at(head, "status");
    shell::property(ui, "Status", |ui| {
        if !can_write {
            if !status.is_empty() {
                c::chip(ui, status_label(status), c::status_tone(status), true);
            }
            return;
        }
        let options: Vec<(String, String)> =
            owned(&PROJECT_STATUSES).into_iter().filter(|(v, _)| v != status).collect();
        let mut slot = None;
        viz::select(ui, status_label(status), &options, &mut slot);
        if let Some(next) = slot {
            requests.push(Request::Patch(json!({ "status": next })));
        }
    });

    let priority = num_at(head, "priority").clamp(0, 4);
    shell::property(ui, "Priority", |ui| {
        if !can_write {
            c::chip(ui, &format!("P{priority}"), priority_tone(priority), false);
            return;
        }
        let current = PRIORITIES[priority as usize];
        let options: Vec<(String, String)> =
            owned(&PRIORITIES).into_iter().filter(|(v, _)| v != current.0).collect();
        let mut slot = None;
        viz::select(ui, current.1, &options, &mut slot);
        if let Some(p) = slot.and_then(|p| p.parse::<i32>().ok()) {
            requests.push(Request::Patch(json!({ "priority": p })));
        }
    });

    for slot in [DateSlot::Start, DateSlot::Target] {
        date_row(ui, head, slot, page, (done, total), can_write, requests);
    }

    let chosen: Vec<String> = page.labels.clone().unwrap_or_else(|| {
        array(head.get("labels")).iter().map(|l| str_at(l, "id").to_owned()).collect()
    });
    shell::property(ui, "Labels", |ui| {
        if !can_write {
            let labels = array(head.get("labels"));
            if labels.is_empty() {
                faint(ui, "None");
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::XS, space::XS);
                for l in &labels {
                    c::chip(ui, str_at(l, "name"), label_tone(str_at(l, "colour")), false);
                }
            });
            return;
        }
        let options: Vec<(String, String)> = all_labels
            .iter()
            .map(|l| (str_at(l, "id").to_owned(), str_at(l, "name").to_owned()))
            .collect();
        let mut picked = chosen.clone();
        viz::multi_select(ui, "Add labels", &options, &mut picked);
        if picked != chosen {
            requests.push(Request::Patch(json!({ "labelIds": picked })));
            page.labels = Some(picked);
        }
    });

    shell::property(ui, "Progress", |ui| {
        if total == 0 {
            faint(ui, "No tasks yet");
            return;
        }
        ui.spacing_mut().item_spacing.x = space::SM;
        w::progress(ui, fraction(done, total), RAIL_BAR_W, colour::ACCENT);
        value(ui, &format!("{done} of {total} done"));
    });

    // One row per person holding work here, busiest first. A stack of
    // overlapping faces made the initials collide into one smear; a name and
    // what they still have open is what the row is for.
    let mut people: Vec<(&str, usize)> = Vec::new();
    for t in tasks {
        let who = str_at(t, "assigneeName");
        if who.is_empty() {
            continue;
        }
        let open = usize::from(!is_finished(t));
        match people.iter_mut().find(|(n, _)| *n == who) {
            Some((_, n)) => *n += open,
            None => people.push((who, open)),
        }
    }
    people.sort_by(|a, b| b.1.cmp(&a.1));
    shell::property(ui, "People", |ui| {
        if people.is_empty() {
            faint(ui, "Nobody yet");
            return;
        }
        ui.vertical(|ui| {
            for (who, open) in &people {
                ui.horizontal(|ui| {
                    ui.set_min_height(size::CONTROL);
                    ui.spacing_mut().item_spacing.x = space::XS;
                    avatar::small(ui, who, size::AVATAR_SM);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let note = if *open == 0 { "all done".to_owned() } else { format!("{open} open") };
                        faint(ui, &note);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(*who).size(text::SMALL).color(colour::TEXT),
                                )
                                .truncate(),
                            );
                        });
                    });
                });
            }
        });
    });

    let key = str_at(head, "key");
    if !key.is_empty() {
        shell::property(ui, "Key", |ui| w::mono_caption(ui, key));
    }
    if let Some((relative, absolute)) = created(str_at(head, "createdAt")) {
        shell::property(ui, "Created", |ui| {
            value(ui, &relative).on_hover_text(absolute);
        });
    }
}

/// A date in the rail: words at rest, the shared typed date field once
/// clicked. Blank saves as "no date", which is how a date is cleared.
fn date_row(
    ui: &mut egui::Ui,
    head: &Value,
    slot: DateSlot,
    page: &mut Page,
    (done, total): (i64, i64),
    can_write: bool,
    requests: &mut Vec<Request>,
) {
    let raw = str_at(head, slot.field());
    let label = match slot {
        DateSlot::Start => "Start",
        DateSlot::Target => "Target",
    };
    let editing = page.date.as_ref().is_some_and(|(s, _)| *s == slot);

    let mut open = false;
    shell::property(ui, label, |ui| {
        if editing {
            faint(ui, "Editing\u{2026}");
            return;
        }
        let (words, ink, hover) = date_words(head, slot, done, total);
        let sense = if can_write { egui::Sense::click() } else { egui::Sense::hover() };
        let r = ui.add(
            egui::Label::new(RichText::new(words).size(text::SMALL).color(ink))
                .sense(sense)
                .selectable(false)
                .truncate(),
        );
        if !can_write {
            r.on_hover_text(hover);
            return;
        }
        let r = motion::operable(ui, r, radius::SM as f32);
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if r.on_hover_text(format!("{hover} \u{00B7} click to change")).clicked() {
            open = true;
        }
    });
    if open {
        page.date = Some((slot, raw.to_owned()));
    }

    let mut close = false;
    if let Some((s, typed)) = page.date.as_mut().filter(|(s, _)| *s == slot) {
        date_field(ui, s.label(), typed);
        ui.add_space(space::XS);
        w::caption(ui, "Leave blank to clear it.");
        ui.add_space(space::SM);
        let trimmed = typed.trim();
        let parsed = parse_date(trimmed);
        let ok = trimmed.is_empty() || parsed.is_some();
        ui.horizontal(|ui| {
            if w::primary(ui, "Save", ok).clicked() {
                let v = parsed.map_or(Value::Null, |d| Value::String(d.to_string()));
                requests.push(Request::Patch(json!({ s.field(): v })));
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                close = true;
            }
        });
        ui.add_space(space::SM);
    }
    if close {
        page.date = None;
    }
}

/// A rail date in words, the colour it should be, and its hover. The target
/// says how it stands — "5 days overdue" in red, amber when at risk — because
/// that is the question a target date is there to answer.
fn date_words(head: &Value, slot: DateSlot, done: i64, total: i64) -> (String, egui::Color32, String) {
    let raw = str_at(head, slot.field());
    let Some(date) = parse_date(raw) else {
        return ("Not set".to_owned(), colour::TEXT_FAINT, "No date yet".to_owned());
    };
    let days = (date - Utc::now().date_naive()).num_days();
    if slot == DateSlot::Start {
        return (relative_day(days), colour::TEXT, raw.to_owned());
    }
    match health(head, done, total) {
        Some((_, Health::Overdue)) => (
            relative_day(days).replace(" ago", " overdue"),
            colour::DANGER,
            Health::Overdue.hover(head, done, total),
        ),
        Some((_, h)) => {
            let ink = if h == Health::Fine { colour::TEXT } else { h.ink() };
            (relative_day(days), ink, h.hover(head, done, total))
        }
        None => (relative_day(days), colour::TEXT, raw.to_owned()),
    }
}

fn faint(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT_FAINT));
}

// ----------------------------------------------------------------- resources

/// The project's own links: the design file, the spec, the umbrella PR. One
/// list, the same shape as a task's.
#[allow(clippy::too_many_arguments)]
fn resources(
    ui: &mut egui::Ui,
    project_id: &str,
    rows: Option<&[Value]>,
    error: Option<&str>,
    page: &mut Page,
    can_write: bool,
    attaching: bool,
    attach_error: Option<&str>,
    requests: &mut Vec<Request>,
) {
    let mut add = false;
    shell::section_count_with(ui, "Resources", rows.map_or(0, <[Value]>::len), |ui| {
        if can_write && page.attach.is_none() && w::ghost(ui, "+ Add").clicked() {
            add = true;
        }
    });
    if add {
        page.attach = Some(Attach::default());
    }

    if let Some(form) = page.attach.as_mut() {
        let mut close = false;
        attach_form(ui, project_id, form, attaching, attach_error, &mut close, requests);
        if close {
            page.attach = None;
        }
        ui.add_space(space::MD);
    }

    if let Some(err) = error {
        w::error(ui, &format!("Could not load resources: {err}"));
        return;
    }
    if let Some(err) = &page.resource_error {
        w::error(ui, &format!("Could not remove that link: {err}. Try again."));
        ui.add_space(space::SM);
    }
    let Some(rows) = rows else {
        w::loading(ui, "Loading resources");
        return;
    };
    if rows.is_empty() {
        if page.attach.is_none() {
            w::empty(ui, "No resources yet.", "PRs, docs and Figma files live here.");
        }
        return;
    }

    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                ui.add_space(space::SM);
            }
            resource_row(ui, row, page, can_write, requests);
        }
    });
}

fn resource_row(
    ui: &mut egui::Ui,
    row: &Value,
    page: &mut Page,
    can_write: bool,
    requests: &mut Vec<Request>,
) {
    let id = str_at(row, "id");
    let kind = str_at(row, "kind");
    let url = str_at(row, "url");
    let title = str_at(row, "title").trim();
    let where_ = host_path(url);
    let confirming = page.removing.as_deref() == Some(id);

    ui.horizontal(|ui| {
        ui.set_min_height(size::CONTROL);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            if can_write {
                // Destructive, so it asks first — inline, where the eye already is.
                if confirming {
                    if w::ghost(ui, "Keep").clicked() {
                        page.removing = None;
                    }
                    if w::danger(ui, "Remove", true).clicked() {
                        requests.push(Request::Remove(id.to_owned()));
                    }
                    faint(ui, "Remove this link?");
                } else if w::ghost(ui, "Remove").clicked() {
                    page.removing = Some(id.to_owned());
                }
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                c::chip(ui, kind_label(kind), kind_tone(kind), false);
                let name = if title.is_empty() { where_ } else { title };
                // Half the row at most, so the address beside it still shows
                // where the link goes.
                let room = if title.is_empty() { ui.available_width() } else { ui.available_width() * 0.55 };
                let r = ui.add(
                    egui::Label::new(
                        RichText::new(elide(ui, name, room))
                            .size(text::SMALL)
                            .color(colour::TEXT),
                    )
                    .sense(egui::Sense::click())
                    .selectable(false),
                );
                let r = motion::operable(ui, r, radius::SM as f32);
                if r.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if r.on_hover_text(url).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
                if !title.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(where_).size(text::SMALL).color(colour::TEXT_MUTED),
                        )
                        .truncate(),
                    );
                }
            });
        });
    });
}

/// "github.com/org/repo/pull/12" from the full URL: the scheme is noise, the
/// host and path say where the link goes.
fn host_path(url: &str) -> &str {
    url.split_once("://").map_or(url, |(_, rest)| rest).trim_end_matches('/')
}

/// Cut `s` to fit `width` at the row's type size, with an ellipsis.
fn elide(ui: &egui::Ui, s: &str, width: f32) -> String {
    let font = egui::FontId::proportional(text::SMALL);
    let full = ui.painter().layout_no_wrap(s.to_owned(), font, colour::TEXT).size().x;
    if full <= width || full <= 0.0 {
        return s.to_owned();
    }
    let keep = (s.chars().count() as f32 * (width / full)) as usize;
    s.chars().take(keep.saturating_sub(1)).collect::<String>() + "\u{2026}"
}

/// The typed attach form: the kind decides the example, and a URL that is not
/// one says so before it is sent.
fn attach_form(
    ui: &mut egui::Ui,
    project_id: &str,
    form: &mut Attach,
    busy: bool,
    error: Option<&str>,
    close: &mut bool,
    requests: &mut Vec<Request>,
) {
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        let (default, default_label, _) = KINDS[0];
        let options: Vec<(String, String)> =
            KINDS[1..].iter().map(|(k, l, _)| ((*k).to_owned(), (*l).to_owned())).collect();
        w::caption(ui, "Kind");
        ui.add_space(space::XXS);
        viz::select(ui, default_label, &options, &mut form.kind);
        let kind = form.kind.clone().unwrap_or_else(|| default.to_owned());
        let hint = KINDS.iter().find(|(k, _, _)| *k == kind).map_or("https://\u{2026}", |(_, _, h)| h);

        ui.add_space(space::MD);
        let typed = form.url.trim().to_owned();
        let bad = !typed.is_empty() && !(typed.starts_with("http://") || typed.starts_with("https://"));
        let entry = w::field(ui, "URL", &mut form.url, false, hint);
        if bad {
            // The border carries it at a glance; the caption is for the person
            // who cannot tell this red from the line around every other field.
            ui.painter().rect_stroke(
                entry.rect,
                radius::SM as f32,
                egui::Stroke::new(1.0, colour::DANGER),
                egui::StrokeKind::Inside,
            );
            ui.add_space(space::XXS);
            w::caption(ui, "Needs a URL starting http:// or https://.");
        }
        ui.add_space(space::MD);
        w::field(ui, "Title", &mut form.title, false, "Optional \u{2014} \u{201c}Checkout spec\u{201d}\u{2026}");
        ui.add_space(space::LG);

        if let Some(err) = error {
            w::error(ui, err);
            ui.add_space(space::MD);
        }
        let ready = !typed.is_empty() && !bad && !busy;
        ui.horizontal(|ui| {
            if w::primary(ui, if busy { "Adding\u{2026}" } else { "Add" }, ready).clicked() {
                requests.push(Request::Attach(json!({
                    "parentType": "project",
                    "parentId": project_id,
                    "kind": kind,
                    "url": typed,
                    "title": form.title.trim(),
                })));
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                *close = true;
            }
        });
    });
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
    // Only when the status column does not already say so: "blocked" twice on
    // one row is one fact wearing two chips.
    let blocked =
        num_at(t, "blockersDone") < num_at(t, "blockersTotal") && status != "blocked";

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
