//! My Tasks: everything assigned to the signed-in person.
//!
//! Grouped by status by default — what is moving first, then what waits, then
//! what is done — with project and priority as the other ways to cut it.
//! Newest first inside every group. Two views of the same groups: a list,
//! which is dense and scans, and a board, where a card dragged to another
//! column is a status change the server still gets the final word on.
//!
//! Nothing is hidden. A "show finished" toggle made the done pile a mode you
//! had to remember to leave; a status filter says the same thing out loud and
//! composes with the others.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use egui_phosphor::regular as icon;
use serde_json::{json, Value};

use super::menus::{task_items, Pick, Viewer};
use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{
    cards as c, colour, motion, radius, shell, size, space, status_colour, status_label, text, theme, viz,
    widgets as w,
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

/// How the page is laid out, apart from what it is filtered to: Clear leaves
/// these alone.
#[derive(Clone, Default, PartialEq)]
struct View {
    board: bool,
    /// `None` is by status.
    group: Option<String>,
    /// Group keys folded shut in the list.
    folded: Vec<String>,
}

const VIEW: &str = "mytasks:view";
/// A dropped card's PATCH. Under `mytasks` so the refresh after it sweeps it.
const MOVE: &str = "mytasks:move";
/// The move in flight: (task id, from, to). Drawn in its new column until the
/// server answers, so a drop lands where it was dropped.
const MOVING: &str = "mytasks:moving";

/// Status groups in the list: what is moving, what is stuck, what waits, then
/// what is finished. The board reads left to right in life order instead.
const LIST_ORDER: [&str; 7] =
    ["in_progress", "blocked", "open", "handoff", "completed", "shipped", "dropped"];
const BOARD_ORDER: [&str; 7] =
    ["open", "in_progress", "blocked", "handoff", "completed", "shipped", "dropped"];
/// Columns the board shows even when empty, so there is somewhere to drop.
/// Blocked, Handoff (design's alone) and Dropped appear once something is in
/// them; the card menu's Move to reaches them before that.
const BOARD_ALWAYS: [&str; 4] = ["open", "in_progress", "completed", "shipped"];
/// Folded until opened: an ending worth keeping, not worth the room.
const FOLDED_BY_DEFAULT: &str = "dropped";

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
    let mut view: View = ui.ctx().data_mut(|d| d.get_temp(egui::Id::new(VIEW))).unwrap_or_else(|| View {
        folded: vec![FOLDED_BY_DEFAULT.to_owned()],
        ..View::default()
    });

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
            ui.add_space(space::SM);
        }
        let views = [(icon::ROWS, "List"), (icon::KANBAN, "Board")];
        if let Some(i) = viz::view_switch(ui, &views, usize::from(view.board), "view") {
            view.board = i == 1;
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
                "When someone hands you a task it shows up here. Or start one yourself.",
            );
            if viewer.can_write {
                let mut start = false;
                ui.vertical_centered(|ui| start = super::new_task::button(ui));
                if start {
                    super::new_task::open(app);
                }
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(VIEW), view));
        finish(app, open_task, picked);
        return;
    }

    let shown_ix = memo(ui.ctx(), egui::Id::new(SHOWN), (key, generation, state.clone()), || {
        (0..rows.len()).filter(|&i| keep(rows[i], &state)).collect::<Vec<usize>>()
    });
    let shown: Vec<&Value> = shown_ix.iter().map(|&i| rows[i]).collect();
    filter_bar(ui, &mut state, &projects, &mut view);
    ui.ctx().data_mut(|d| d.insert_temp(filters_id, state.clone()));

    // The count heads the groups: how many of how many survive the filters.
    let count = if shown.len() < rows.len() {
        format!("{} of {} tasks", shown.len(), rows.len())
    } else {
        plural(rows.len(), "task")
    };
    w::caption(ui, &count);
    ui.add_space(space::XS);

    if rows.is_empty() {
        w::empty(ui, "Nothing archived.", "Tasks of yours that are archived show here.");
    } else if shown.is_empty() {
        w::empty(ui, "Nothing matches.", "Clear a filter, or search for something else.");
    } else {
        let by = Group::of(view.group.as_deref());
        let moving: Option<(String, String, String)> = ui.ctx().data(|d| d.get_temp(egui::Id::new(MOVING)));
        let groups = group(&shown, by, view.board, moving.as_ref());
        if view.board {
            let can_move = by == Group::Status && !state.archived && viewer.can_write && moving.is_none();
            let out = board(ui, &groups, &viewer, can_move);
            open_task = out.open.or(open_task);
            picked = out.picked.or(picked);
            if let Some((id, from, to)) = out.moved {
                let net = app.net.as_mut().unwrap();
                net.invalidate(MOVE);
                net.patch(MOVE, &format!("/api/user/tasks/{id}"), json!({ "status": to, "expectedStatus": from }));
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(MOVING), (id, from, to)));
            }
        } else {
            let out = list(ui, &groups, &viewer, &mut view.folded);
            open_task = out.open.or(open_task);
            picked = out.picked.or(picked);
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(VIEW), view));
    settle_move(ui.ctx(), app.net.as_mut().unwrap());
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
fn filter_bar(ui: &mut egui::Ui, state: &mut State, projects: &[&str], view: &mut View) {
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

        let groupings = [
            ("project".to_owned(), "Group: Project".to_owned()),
            ("priority".to_owned(), "Group: Priority".to_owned()),
        ];
        // Not a filter, so it reads as a setting rather than a narrowing:
        // the resting label is the current grouping, status included.
        viz::value_select(ui, "Group: Status", &groupings, &mut view.group);

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
        let ink = if done || super::board::archived(t) { colour::TEXT_MUTED() } else { colour::TEXT() };
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

// ------------------------------------------------------------------ groups

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Status,
    Project,
    Priority,
}

impl Group {
    fn of(v: Option<&str>) -> Self {
        match v {
            Some("project") => Group::Project,
            Some("priority") => Group::Priority,
            _ => Group::Status,
        }
    }
}

/// One group: a key (the status, project or priority it gathers), what its
/// header says, the dot beside it, and its rows, newest first.
struct Bucket<'a> {
    key: String,
    label: String,
    dot: Option<egui::Color32>,
    rows: Vec<&'a Value>,
}

/// The status a row is drawn under: the one it is moving to while a drop is
/// in flight, so it does not snap back until the server says no.
fn status_of<'a>(t: &'a Value, moving: Option<&'a (String, String, String)>) -> &'a str {
    match moving {
        Some((id, _, to)) if str_at(t, "id") == Some(id.as_str()) => to,
        _ => str_at(t, "status").unwrap_or("open"),
    }
}

fn group<'a>(
    rows: &[&'a Value],
    by: Group,
    board: bool,
    moving: Option<&(String, String, String)>,
) -> Vec<Bucket<'a>> {
    let key_of = |t: &Value| -> String {
        match by {
            Group::Status => status_of(t, moving).to_owned(),
            Group::Project => project_of(t).to_owned(),
            Group::Priority => t.get("priority").and_then(Value::as_i64).map_or("none".into(), |p| p.to_string()),
        }
    };
    // The order the groups appear in: fixed for status and priority, by name
    // for projects with the project-less ones last.
    let mut keys: Vec<String> = match by {
        Group::Status => {
            let order: &[&str] = if board { &BOARD_ORDER } else { &LIST_ORDER };
            order.iter().map(|s| (*s).to_owned()).collect()
        }
        Group::Priority => (0..=4).map(|p| p.to_string()).chain(["none".to_owned()]).collect(),
        Group::Project => Vec::new(),
    };
    for t in rows {
        let k = key_of(t);
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    if by == Group::Project {
        keys.sort_by(|a, b| (a == NO_PROJECT).cmp(&(b == NO_PROJECT)).then_with(|| a.cmp(b)));
    }

    keys.into_iter()
        .map(|key| {
            let members: Vec<&Value> = rows.iter().copied().filter(|t| key_of(t) == key).collect();
            let (label, dot) = match by {
                Group::Status => (status_label(&key).to_owned(), Some(status_colour(&key))),
                Group::Priority if key == "none" => ("No priority".to_owned(), None),
                Group::Priority => {
                    let p: i64 = key.parse().unwrap_or(4);
                    (format!("P{p}"), Some(priority_dot(p)))
                }
                Group::Project => (key.clone(), None),
            };
            Bucket { key, label, dot, rows: members }
        })
        .filter(|b| !b.rows.is_empty() || (board && by == Group::Status && BOARD_ALWAYS.contains(&b.key.as_str())))
        .collect()
}

/// A priority group's dot: the chip scale, as a dot.
fn priority_dot(p: i64) -> egui::Color32 {
    match p {
        0 => colour::DANGER(),
        1 => colour::WARN(),
        2 => colour::TEXT_MUTED(),
        _ => colour::IDLE(),
    }
}

/// What a click, a right-click or a drop asked for this frame.
#[derive(Default)]
struct Out {
    open: Option<String>,
    picked: Option<(Value, Pick)>,
    /// (task id, from, to).
    moved: Option<(String, String, String)>,
}

// --------------------------------------------------------------------- list

fn list(ui: &mut egui::Ui, groups: &[Bucket<'_>], viewer: &Viewer, folded: &mut Vec<String>) -> Out {
    let mut out = Out::default();
    // The column labels ride on the first open group only; every group below
    // lines up under them.
    let mut header = true;
    for (gi, g) in groups.iter().enumerate() {
        let open = !folded.contains(&g.key);
        if gi > 0 {
            ui.add_space(space::MD);
        }
        if group_header(ui, g, open).clicked() {
            if open {
                folded.push(g.key.clone());
            } else {
                folded.retain(|k| k != &g.key);
            }
        }
        if !open {
            continue;
        }
        ui.add_space(space::XS);
        let rows = &g.rows;
        let clicked = table::show_group(
            ui,
            &format!("{TABLE}:{}", g.key),
            &COLS,
            rows.len(),
            header,
            |row, i| task_row(row, rows[i]),
            |ui, i| {
                if let Some(pick) = task_items(ui, rows[i], viewer, true) {
                    out.picked = Some((rows[i].clone(), pick));
                }
            },
        );
        header = false;
        if let Some(i) = clicked {
            out.open = str_at(rows[i], "id").map(str::to_owned);
        }
    }
    out
}

/// A group's heading in the list: a caret that folds it, its dot, its name
/// and how many. The whole row is the target.
fn group_header(ui: &mut egui::Ui, g: &Bucket<'_>, open: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), size::CONTROL), egui::Sense::click());
    let n = g.rows.len();
    response.widget_info(|| {
        let state = if open { "expanded" } else { "collapsed" };
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{}, {n}, {state}", g.label))
    });
    let response = motion::operable(ui, response, radius::SM as f32);
    let hot = response.hovered() || response.has_focus();
    let fill = motion::hover_fill(ui, response.id.with("fill"), hot, colour::TRANSPARENT(), colour::SURFACE_HOVER());
    let p = ui.painter();
    if fill != colour::TRANSPARENT() {
        p.rect_filled(rect, radius::SM as f32, fill);
    }
    let mut x = rect.left() + space::XS;
    p.text(
        egui::pos2(x + space::SM, rect.center().y),
        egui::Align2::CENTER_CENTER,
        if open { icon::CARET_DOWN } else { icon::CARET_RIGHT },
        egui::FontId::proportional(text::SMALL),
        if hot { colour::TEXT_2() } else { colour::TEXT_FAINT() },
    );
    x += space::LG + space::XS;
    if let Some(dot) = g.dot {
        p.circle_filled(egui::pos2(x + 3.5, rect.center().y), 3.5, dot);
        x += space::MD + space::XXS;
    }
    let label = p.layout_no_wrap(
        g.label.clone(),
        egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::SEMIBOLD.into())),
        colour::TEXT_2(),
    );
    let lw = label.size().x;
    p.galley(egui::pos2(x, rect.center().y - label.size().y / 2.0), label, colour::TEXT_2());
    p.text(
        egui::pos2(x + lw + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        n.to_string(),
        egui::FontId::proportional(text::SMALL),
        colour::TEXT_FAINT(),
    );
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

// -------------------------------------------------------------------- board

/// A board column's narrowest: a title of two lines and a row of chips. The
/// columns share the width when they all fit, and scroll sideways at this
/// width when they do not.
const COLUMN_W: f32 = 232.0;
/// A column's floor, so an empty one is still a place to drop onto.
const COLUMN_MIN_H: f32 = 160.0;

/// What a dragged card carries: its id and the status it is leaving.
#[derive(Clone)]
struct Drag {
    id: String,
    from: String,
}

fn board(ui: &mut egui::Ui, groups: &[Bucket<'_>], viewer: &Viewer, can_move: bool) -> Out {
    let mut out = Out::default();
    let n = groups.len().max(1) as f32;
    // Two points short of the edge, or the scroll area clips the last
    // column's right hairline.
    let width = ((ui.available_width() - space::MD * (n - 1.0) - 2.0) / n).max(COLUMN_W);
    // The board ends at the window's foot and each column scrolls on its own,
    // so a long column never drags the short ones (and the page) with it.
    let height = (ui.clip_rect().bottom() - ui.cursor().top() - space::XL).max(COLUMN_MIN_H);
    egui::ScrollArea::horizontal().id_salt("mytasks:board").show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = space::MD;
            for g in groups {
                column(ui, g, viewer, can_move, width, height, &mut out);
            }
        });
    });
    out
}

fn column(
    ui: &mut egui::Ui,
    g: &Bucket<'_>,
    viewer: &Viewer,
    can_move: bool,
    width: f32,
    height: f32,
    out: &mut Out,
) {
    let bg = ui.painter().add(egui::Shape::Noop);
    let inner = ui
        .allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                ui.set_height(height);
                egui::Frame::new().inner_margin(egui::Margin::same(space::SM as i8)).show(ui, |ui| {
                    ui.set_width(width - space::SM * 2.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(size::CONTROL - space::XS);
                        ui.spacing_mut().item_spacing.x = space::SM;
                        ui.add_space(space::XS);
                        if let Some(dot) = g.dot {
                            w::dot(ui, dot);
                        }
                        ui.label(
                            egui::RichText::new(&g.label)
                                .size(text::SMALL)
                                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                                .color(colour::TEXT_2()),
                        );
                        ui.label(egui::RichText::new(g.rows.len().to_string()).size(text::SMALL).color(colour::TEXT_FAINT()));
                    });
                    ui.add_space(space::XS);
                    egui::ScrollArea::vertical()
                        .id_salt(("mytasks:column", &g.key))
                        .max_height(ui.available_height().max(0.0))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for t in &g.rows {
                                card(ui, t, viewer, can_move, out);
                            }
                            if g.rows.is_empty() {
                                ui.add_space(space::LG);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new("No tasks").size(text::SMALL).color(colour::TEXT_FAINT()),
                                    );
                                });
                            }
                        });
                });
            },
        )
        .response;
    let rect = inner.rect;
    let zone = ui.interact(rect, egui::Id::new(("mytasks:column", &g.key)), egui::Sense::hover());
    // A drop target only for a card that would actually change status here.
    let target = can_move
        && zone.dnd_hover_payload::<Drag>().is_some_and(|d| d.from != g.key);
    let (fill, stroke) = if target {
        (colour::ACCENT_SOFT(), egui::Stroke::new(1.5, colour::ACCENT()))
    } else {
        (colour::CHROME(), egui::Stroke::new(1.0, colour::LINE_SOFT()))
    };
    ui.painter().set(bg, egui::Shape::rect_filled(rect, radius::LG, fill));
    ui.painter().rect_stroke(rect, radius::LG as f32, stroke, egui::StrokeKind::Inside);
    if can_move {
        if let Some(d) = zone.dnd_release_payload::<Drag>() {
            if d.from != g.key {
                out.moved = Some((d.id.clone(), d.from.clone(), g.key.clone()));
            }
        }
    }
}

/// A board card: the title as the loudest thing, then priority, project and
/// age. Click opens it, right-click is the row menu, and when the board is
/// grouped by status it can be dragged to another column.
fn card(ui: &mut egui::Ui, t: &Value, viewer: &Viewer, can_move: bool, out: &mut Out) {
    let id_str = str_at(t, "id").unwrap_or_default();
    let id = egui::Id::new(("mytasks:card", id_str));
    let dragging = ui.ctx().is_being_dragged(id);
    let hovered = ui.ctx().data(|d| d.get_temp::<bool>(id.with("hot")).unwrap_or(false));
    let done = finished(t) || super::board::archived(t);
    let title = str_at(t, "title").unwrap_or_default();

    let frame = ui.scope(|ui| {
        if dragging {
            ui.multiply_opacity(0.35);
        }
        egui::Frame::new()
            .fill(if hovered { colour::SURFACE_HOVER() } else { colour::SURFACE() })
            .stroke(egui::Stroke::new(1.0, if hovered { colour::LINE_STRONG() } else { colour::LINE() }))
            .corner_radius(radius::MD)
            .inner_margin(egui::Margin::symmetric(space::MD as i8, (space::SM + space::XXS) as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mut job = egui::text::LayoutJob::simple(
                    title.to_owned(),
                    egui::FontId::new(text::BODY, egui::FontFamily::Name(theme::MEDIUM.into())),
                    if done { colour::TEXT_MUTED() } else { colour::TEXT() },
                    ui.available_width(),
                );
                job.wrap.max_rows = 2;
                job.wrap.overflow_character = Some('\u{2026}');
                ui.add(egui::Label::new(job).selectable(false));
                ui.add_space(space::SM);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    if let Some(p) = t.get("priority").and_then(Value::as_i64) {
                        c::chip(ui, &format!("P{p}"), priority_tone(p), false);
                    }
                    if blocked(t) {
                        c::chip(ui, "Blocked", c::Tone::Blocked, false);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(age(str_at(t, "createdAt").unwrap_or_default()))
                                .size(text::CAPTION)
                                .color(colour::TEXT_FAINT()),
                        );
                        super::home::agent_marker(ui, t);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(project_of(t)).size(text::SMALL).color(colour::TEXT_MUTED()),
                            )
                            .truncate()
                            .selectable(false),
                        );
                    });
                });
            })
            .response
    });
    let sense = if can_move { egui::Sense::click_and_drag() } else { egui::Sense::click() };
    let response = ui.interact(frame.response.rect, id, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    let response = motion::operable(ui, response, radius::MD as f32);
    // Read before `data_mut`: both take the context's lock.
    let hot = response.hovered() || response.has_focus();
    ui.ctx().data_mut(|d| d.insert_temp(id.with("hot"), hot));
    if can_move {
        response.dnd_set_drag_payload(Drag {
            id: id_str.to_owned(),
            from: str_at(t, "status").unwrap_or("open").to_owned(),
        });
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if dragging {
        ghost(ui.ctx(), id, title);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if response.clicked() {
        out.open = Some(id_str.to_owned());
    }
    viz::context_menu(&response, |ui| {
        // The drag's keyboard-and-click twin: the same moves, from the menu.
        if can_move {
            let from = str_at(t, "status").unwrap_or("open");
            viz::submenu(ui, "Move to", None, |ui| {
                for to in BOARD_ORDER {
                    if viz::menu_choice(ui, status_label(to), to == from) && to != from {
                        out.moved = Some((id_str.to_owned(), from.to_owned(), to.to_owned()));
                    }
                }
            });
            viz::menu_rule(ui);
        }
        if let Some(pick) = task_items(ui, t, viewer, true) {
            out.picked = Some((t.clone(), pick));
        }
    });
    ui.add_space(space::SM);
}

/// The card's title, riding under the pointer while it is dragged.
fn ghost(ctx: &egui::Context, id: egui::Id, title: &str) {
    let Some(at) = ctx.pointer_interact_pos() else { return };
    egui::Area::new(id.with("ghost"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(at + egui::vec2(space::MD, space::XS))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(colour::SURFACE_ACTIVE())
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG()))
                .corner_radius(radius::MD)
                .inner_margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8))
                .show(ui, |ui| {
                    ui.set_max_width(COLUMN_W - space::XL);
                    ui.add(
                        egui::Label::new(egui::RichText::new(title).size(text::BODY).color(colour::TEXT()))
                            .truncate()
                            .selectable(false),
                    );
                });
        });
}

/// Fold in a dropped card's reply: say where it went, or why it did not.
fn settle_move(ctx: &egui::Context, net: &mut crate::desktop::net::Net) {
    let moving_id = egui::Id::new(MOVING);
    let Some((_, _, to)) = ctx.data(|d| d.get_temp::<(String, String, String)>(moving_id)) else { return };
    if net.is_loading(MOVE) {
        return;
    }
    let Some(result) = net.peek(MOVE).cloned() else { return };
    match result {
        Ok(v) if str_at(&v, "status") == Some("proposed") => {
            w::toast(ctx, "Recorded as a proposed change \u{2014} it needs approval.", false)
        }
        Ok(_) => w::toast(ctx, format!("Moved to {}.", status_label(&to)), false),
        // The server's reason — a missing PR, a move the track does not have.
        Err(e) => w::toast(ctx, format!("{e} Open the task to finish the move there."), true),
    }
    net.invalidate(MOVE);
    super::task::invalidate_after_move(net);
    ctx.data_mut(|d| d.remove::<(String, String, String)>(moving_id));
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
