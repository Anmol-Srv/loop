//! Home: the dashboard.
//!
//! The previous version was a column of cards — the right unit for three
//! items and the wrong one for twenty, because you cannot count a column of
//! cards. This is the shape the owner asked for: four visualisations that read
//! at a glance, a filter bar, and a table.
//!
//! Two fetches. `GET /api/user/home` carries the projects rollup and the team
//! band; other views drop it under the key `"home"` when they change a task,
//! so the name is load-bearing. `GET /api/user/tasks` carries the table: the
//! whole workspace, one row per task.
//!
//! At a few thousand rows, sorting, indexing and tallying that list every
//! frame was most of the frame. So all of it is derived once per payload —
//! `Derived`, redone when either reply's generation moves — and a frame only
//! filters and draws.
//!
//! The endpoint takes server-side filters, and the table ignores them. At
//! twenty rows a round trip per filter click buys nothing and costs a cache
//! key per combination, a spinner on every menu selection, and a filter menu
//! that can only offer the values the last response happened to contain.
//! One fetch, filtered in memory, keeps the menus honest and the clicks free.
//! Move to query params the day the workspace outgrows a single response.
//!
//! Every number here counts the same population, the non-dropped tasks, so
//! the subtitle, the donut and the table heading agree. All four cards sit on
//! the whole workspace: Status and Completed count
//! the task list, By department sums the server's per-project rollup, Team
//! load is `team[]` from one grouped query.
//!
//! Approvals are gone from this page: the inbox owns them, and the table's
//! "Mine" filter covers what used to be the claim list. Nobody takes work
//! here: a task is handed out when it is written, and its assignee is the
//! only one who can move it.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Local, NaiveDate, Utc};
use egui::{Align, Layout, RichText};
use serde_json::Value;

use super::projects::PRIORITIES;
use super::menus::{task_items, Pick, Viewer};
use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{
    avatar, cards as c, colour, radius, shell, size, space, status_colour, status_label, text, theme,
    viz, widgets as w,
};
use crate::desktop::net::memo;
use crate::desktop::{App, Tab};

const HOME: &str = "home";
/// The table's rows: the whole workspace, unfiltered.
const TASKS: &str = "home:tasks";
/// The archived ones, which the table shows instead under its Archived toggle.
/// The figures above it never count them.
const ARCHIVED: &str = "home:archived";

/// `App` owns no home state, and this view is the only thing that reads these
/// filters, so they live in egui's temp store rather than growing the struct.
const FILTERS: &str = "home:filters";
/// The table's id: its scroll salt, and the slot its hover is tracked in.
const TABLE: &str = "home:table";
/// The attention list's table id. Its own, so its hover never lights a task row.
const ATTENTION: &str = "home:attention";

/// Days in the completed chart. Seven, because the label says week.
const WEEK: usize = 7;

/// A project due within this many days with under half its work done is at
/// risk. A week is one planning cycle: still enough time to cut scope.
const AT_RISK_DAYS: i64 = 7;
/// In progress with no update for longer than this reads as stalled. A working
/// week — shorter flags anyone who took a long weekend.
const STALLED_DAYS: i64 = 5;
/// The attention list is the top of the pile, not the pile: past six rows it
/// pushes the figures below the fold, and the table has the rest.
const ATTENTION_MAX: usize = 6;

/// The status vocabulary, in the order it reads in the filter menu: the two
/// tracks' happy paths first, then the two states either track can land in.
/// `status_label` turns each one into words.
const STATUSES: [&str; 8] =
    ["open", "in_progress", "research", "completed", "handoff", "shipped", "blocked", "dropped"];

/// The statuses at which a task stops holding up the tasks that wait on it.
/// Mirrors the server's `BLOCKER_RESOLVED`: a design handoff unblocks the
/// engineer, it does not have to ship first.
const BLOCKER_RESOLVED: [&str; 4] = ["handoff", "completed", "shipped", "dropped"];

// Table geometry. Fixed so the columns line up with the header and with each
// other; the task column takes whatever is left. Alignment is declared here
// too, so "Updated" and the age beneath it cannot disagree.

/// "P0" plus chip padding, the same width the task tables use.
const COL_PRIORITY: f32 = 52.0;
const COL_DEPARTMENT: f32 = 88.0;
const COL_STATUS: f32 = 120.0;
const COL_PROJECT: f32 = 150.0;
/// An avatar and a first name.
const COL_OWNER: f32 = 110.0;
const COL_UPDATED: f32 = 78.0;

const COLS: [Col; 7] = [
    Col::fill("Task", COL_PROJECT),
    Col::left("Priority", COL_PRIORITY).rank(1),
    Col::left("Department", COL_DEPARTMENT).rank(2),
    Col::left("Status", COL_STATUS),
    Col::left("Project", COL_PROJECT),
    Col::left("Owner", COL_OWNER),
    Col::right("Updated", COL_UPDATED).rank(3),
];

/// The attention list: what kind of trouble, what is in it, and why. Two fill
/// columns so a long project name and a long reason share the slack.
const COL_SIGNAL: f32 = 84.0;
const ATTENTION_COLS: [Col; 3] = [
    Col::left("", COL_SIGNAL),
    Col::fill("", 140.0),
    Col::fill("", 180.0),
];

/// What the table is filtered to. Every field is "no filter" when unset, so
/// `Default` is the unfiltered view.
#[derive(Clone, Default, PartialEq)]
pub struct State {
    /// Free text, matched against title, project and owner.
    pub query: String,
    /// Only tasks assigned to me.
    pub mine: bool,
    pub department: Option<String>,
    /// "0"–"4", as `PRIORITIES` spells them.
    pub priority: Option<String>,
    pub status: Option<String>,
    /// A project id, not a name.
    pub project: Option<String>,
    /// The table lists archived tasks instead.
    pub archived: bool,
    /// A figure from the overview, clicked: the table shows just those.
    pub quick: Option<Quick>,
    /// A person from the Team list, clicked: (person id, name).
    pub owner: Option<(String, String)>,
}

/// The overview's figures, each one also a filter on the table below.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Quick {
    InProgress,
    Blocked,
    NotStarted,
    ToShip,
    DoneWeek,
    WithAgents,
}

impl Quick {
    const ALL: [Quick; 6] =
        [Quick::InProgress, Quick::Blocked, Quick::NotStarted, Quick::ToShip, Quick::DoneWeek, Quick::WithAgents];

    fn label(self) -> &'static str {
        match self {
            Quick::InProgress => "In progress",
            Quick::Blocked => "Blocked",
            Quick::NotStarted => "Not started",
            Quick::ToShip => "Ready to ship",
            Quick::DoneWeek => "Done this week",
            Quick::WithAgents => "With agents",
        }
    }

    /// What the figure counts, for its tooltip.
    fn hint(self) -> &'static str {
        match self {
            Quick::InProgress => "Being worked on now, research included.",
            Quick::Blocked => "Waiting on work that isn\u{2019}t finished yet.",
            Quick::NotStarted => "Open: nobody has started it.",
            Quick::ToShip => "Completed or handed off, but not out yet.",
            Quick::DoneWeek => "Reached the end of its track in the last seven days.",
            Quick::WithAgents => "Held by an agent right now.",
        }
    }

    fn matches(self, r: &Row) -> bool {
        match self {
            Quick::InProgress => matches!(r.bucket, "in_progress" | "research"),
            Quick::Blocked => r.bucket == "blocked",
            Quick::NotStarted => r.bucket == "open",
            Quick::ToShip => matches!(r.bucket, "completed" | "handoff") && !r.finished,
            Quick::DoneWeek => r.done_days.is_some_and(|d| d < WEEK as i64),
            Quick::WithAgents => r.agent,
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let my_person_id = me_str(app, "personId");

    let filters_id = egui::Id::new(FILTERS);
    let mut state: State = ui.ctx().data_mut(|d| d.get_temp(filters_id)).unwrap_or_default();

    let viewer = Viewer::of(app);
    let net = app.net.as_mut().unwrap();

    net.get_once(HOME, "/api/user/home");
    net.get_once(TASKS, "/api/user/tasks");
    // With the rest, not once the figures are drawn: that made it a second wave.
    net.get_once(super::agent_session::ACTIVE_KEY, "/api/user/agents/active");
    if state.archived {
        net.get_once(ARCHIVED, "/api/user/tasks?archived=true");
    }

    let loading = net.is_loading(HOME) || net.is_loading(TASKS);
    let error = net.error(HOME).or_else(|| net.error(TASKS)).map(str::to_owned);
    // The alerts and the week chart read the clock, so a new hour counts as
    // a change too.
    // ponytail: hourly, so "stalled" can trail the real moment by up to an hour.
    let stamp = (net.generation(HOME), net.generation(TASKS), Utc::now().timestamp() / 3600);
    let (home, tasks) = (net.shared(HOME), net.shared(TASKS));
    let d = memo(ui.ctx(), egui::Id::new(DERIVED), stamp, || derive(home.clone(), tasks));
    // The table's own source: the same shape, from the archived list.
    let archived_stamp = (net.generation(HOME), net.generation(ARCHIVED));
    let archived = net.shared(ARCHIVED);
    let t = if state.archived {
        memo(ui.ctx(), egui::Id::new(DERIVED).with("archived"), archived_stamp, || derive(home, archived))
    } else {
        d.clone()
    };
    let rows = t.rows();

    shell::page_title(ui, "Home", "", |_| {});

    if let Some(err) = &error {
        if w::error_retry(ui, err) {
            let net = app.net.as_mut().unwrap();
            net.invalidate(HOME);
            net.invalidate(TASKS);
        }
        return;
    }
    if loading && d.all.is_empty() {
        w::loading(ui, "Loading your dashboard");
        return;
    }

    // ---- the overview: six figures, each a filter on the table below
    let mut scroll_to_table = false;
    if let Some(q) = overview(ui, &d, state.quick) {
        state.quick = if state.quick == Some(q) { None } else { Some(q) };
        scroll_to_table = state.quick.is_some();
    }

    // ---- what a lead opens this page for: the list of things going wrong
    let mut go: Option<Target> = None;
    if !d.alerts.is_empty() {
        attention_list(ui, &d.alerts, &mut go);
    }

    // ---- this week, and who holds what
    ui.add_space(space::XL);
    let (opened, show_all) = this_week(ui, &d, d.rows());
    if let Some(id) = opened {
        go = Some(Target::Task(id));
    }
    if show_all {
        state.quick = Some(Quick::DoneWeek);
        scroll_to_table = true;
    }
    ui.add_space(space::LG);
    if let Some((id, name)) = team(ui, &d, state.owner.as_ref().map(|(id, _)| id.as_str())) {
        let same = state.owner.as_ref().is_some_and(|(o, _)| *o == id);
        state.owner = if same { None } else { Some((id, name)) };
        scroll_to_table = state.owner.is_some();
    }

    // ---- whose agents are holding what, right now
    if let Some(id) = super::agent_session::at_work(ui, app.net.as_mut().unwrap(), &my_person_id) {
        go = Some(Target::Task(id));
    }

    // The count is drawn from last frame's filter state; a click repaints, so
    // the lag is never seen. Refiltered only when the filters or the rows move.
    let shown = memo(
        ui.ctx(),
        egui::Id::new(SHOWN),
        (stamp, archived_stamp, state.clone(), my_person_id.clone()),
        || {
            let needle = state.query.trim().to_lowercase();
            (0..t.all.len())
                .filter(|&i| keep(&t.all[i], &rows[t.all[i].at], &state, &my_person_id, &needle))
                .collect::<Vec<usize>>()
        },
    );
    let filtered = state != State::default();
    let mut new_task = false;
    if scroll_to_table {
        ui.scroll_to_cursor(Some(Align::TOP));
    }
    shell::section_count_with(ui, "Tasks", shown.len(), |ui| {
        if viewer.can_write {
            new_task = super::new_task::button(ui);
            ui.add_space(space::SM);
        }
        if filtered {
            w::caption(ui, &format!("filtered from {}", t.live));
        }
    });
    filter_bar(ui, &mut state, &d.filter_departments, &d.filter_projects);
    ui.ctx().data_mut(|d| d.insert_temp(filters_id, state));

    let mut picked: Option<(Value, Pick)> = None;
    if shown.is_empty() {
        w::empty(ui, "Nothing matches those filters.", "Clear one of them to see more.");
    } else if let Some(i) = table::show_with_menu(
        ui,
        TABLE,
        &COLS,
        shown.len(),
        |row, i| {
            let r = &t.all[shown[i]];
            task_row(row, &rows[r.at], r, &my_person_id);
        },
        |ui, i| {
            let row = &rows[t.all[shown[i]].at];
            if let Some(pick) = task_items(ui, row, &viewer, true) {
                picked = Some((row.clone(), pick));
            }
        },
    ) {
        go = str_at(&rows[t.all[shown[i]].at], "id").map(|id| Target::Task(id.to_owned()));
    }
    ui.add_space(space::XXL);

    match picked {
        Some((t, Pick::Open)) => go = str_at(&t, "id").map(|id| Target::Task(id.to_owned())),
        Some((t, pick)) => app.board.tasks.pick(app.net.as_mut().unwrap(), &t, pick),
        None => {}
    }

    if new_task {
        super::new_task::open(app);
    }
    match go {
        Some(Target::Task(id)) => app.task = Some(id),
        Some(Target::Project(id)) => {
            app.tab = Tab::Projects;
            app.project = Some(id);
        }
        None => {}
    }
}

// ------------------------------------------------------------ derived, once

/// Where the derived page lives in egui's temp store, and the filtered rows.
const DERIVED: &str = "home:derived";
const SHOWN: &str = "home:shown";

/// Everything this page works out from its two payloads. Rows point back into
/// `tasks` by index rather than copying it.
struct Derived {
    tasks: Arc<Value>,
    /// Every task, newest first, dropped included.
    all: Vec<Row>,
    /// How many of `all` are not dropped.
    live: usize,
    alerts: Vec<Alert>,
    completed: CompletedFigures,
    /// Everyone holding live work, busiest first.
    team: Vec<Member>,
    /// Indexes into `all` of what finished in the last seven days, newest first.
    finished: Vec<usize>,
    filter_departments: Vec<(String, String)>,
    filter_projects: Vec<(String, String)>,
}

/// One task as the table and the filters need it.
struct Row {
    /// Index into the `/tasks` array.
    at: usize,
    bucket: &'static str,
    /// At the end of its track (`doneAt` set).
    finished: bool,
    /// Days since it finished, when it has.
    done_days: Option<i64>,
    /// An agent holds it right now.
    agent: bool,
    /// Its assignee's person id.
    owner: Option<String>,
    /// Title, project and owner, lowercased once, for the search box.
    search: [String; 3],
    /// "waiting on …", when something unresolved holds it up.
    waiting: Option<String>,
}

impl Derived {
    fn rows(&self) -> &[Value] {
        self.tasks.as_array().map(Vec::as_slice).unwrap_or_default()
    }
}

fn derive(home: Option<Arc<Value>>, tasks: Option<Arc<Value>>) -> Derived {
    let home = home.unwrap_or_else(|| Arc::new(Value::Null));
    let tasks = tasks.unwrap_or_else(|| Arc::new(Value::Null));
    let rows: &[Value] = tasks.as_array().map(Vec::as_slice).unwrap_or_default();
    let projects = list(&home, "projects");

    // Every task in the workspace, newest first. The list should not reshuffle
    // when a filter narrows it.
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&a, &b| {
        str_at(&rows[b], "createdAt")
            .unwrap_or_default()
            .cmp(str_at(&rows[a], "createdAt").unwrap_or_default())
    });

    // Blockers resolve against the whole list, dropped included — a dropped
    // blocker is a resolved one, and it still has a title.
    let by_id: HashMap<&str, &Value> =
        order.iter().filter_map(|&i| Some((str_at(&rows[i], "id")?, &rows[i]))).collect();

    let all: Vec<Row> = order
        .iter()
        .map(|&i| {
            let t = &rows[i];
            let lower = |k: &str| str_at(t, k).unwrap_or_default().to_lowercase();
            let done_days = str_at(t, "doneAt")
                .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
                .map(|when| (Local::now().date_naive() - when.with_timezone(&Local).date_naive()).num_days());
            let agent = t.get("delegate").is_some_and(|d| {
                d.is_object() && !matches!(str_at(d, "state"), Some("done" | "stopped"))
            });
            Row {
                at: i,
                bucket: bucket(t),
                finished: done_days.is_some(),
                done_days,
                agent,
                owner: str_at(t, "assigneePersonId").map(str::to_owned),
                search: [lower("title"), lower("projectName"), lower("assigneeName")],
                waiting: (outstanding(t) > 0).then(|| format!("waiting on {}", blocker(t, &by_id))),
            }
        })
        .collect();

    // Dropped work is not work: every number on this page counts the rest,
    // so the subtitle, the donut and the table heading cannot disagree. The
    // table still reaches dropped tasks through its status filter.
    let live: Vec<&Value> =
        all.iter().filter(|r| r.bucket != "dropped").map(|r| &rows[r.at]).collect();

    // Only departments that actually appear: a menu of empty filters is a
    // menu of ways to get an empty table.
    let mut departments: Vec<&str> = live
        .iter()
        .filter_map(|t| str_at(t, "discipline"))
        .filter(|d| !d.is_empty())
        .collect();
    departments.sort_unstable();
    departments.dedup();

    Derived {
        live: live.len(),
        alerts: attention(&home, &projects, &live, &by_id),
        completed: completed_figures(&live),
        team: members(&all, rows),
        finished: {
            let mut f: Vec<usize> = (0..all.len())
                .filter(|&i| all[i].bucket != "dropped" && all[i].done_days.is_some_and(|d| d < WEEK as i64))
                .collect();
            f.sort_by(|&a, &b| {
                str_at(&rows[all[b].at], "doneAt").unwrap_or_default().cmp(str_at(&rows[all[a].at], "doneAt").unwrap_or_default())
            });
            f
        },
        filter_departments: departments.iter().map(|d| ((*d).to_owned(), (*d).to_owned())).collect(),
        filter_projects: projects
            .iter()
            .filter_map(|p| Some((str_at(p, "id")?.to_owned(), str_at(p, "name")?.to_owned())))
            .collect(),
        all,
        tasks: tasks.clone(),
    }
}

// ----------------------------------------------------------- needs attention

#[derive(Clone)]
enum Target {
    Project(String),
    Task(String),
}

/// One row of the attention list. `rank` is the kind of trouble, most severe
/// first; `weight` orders rows of the same kind, larger first.
struct Alert {
    rank: u8,
    weight: i64,
    tone: c::Tone,
    signal: &'static str,
    subject: String,
    reason: String,
    target: Target,
}

/// Everything worth a lead's attention, most severe first, capped.
///
/// First what an agent is waiting on me for — a question to answer, work to
/// review — since nothing moves on those tasks until I do. Then five kinds, in
/// order: a project past its date, a task waiting on another,
/// a project that will miss its date at this pace, work nobody has touched,
/// and urgent work nobody holds. Past-due outranks blocked because a date is
/// a promise to someone outside the team; a blocked task is still internal.
fn attention(
    home: &Value,
    projects: &[&Value],
    live: &[&Value],
    by_id: &HashMap<&str, &Value>,
) -> Vec<Alert> {
    let today = Local::now().date_naive();
    let mut out = Vec::new();

    // Triage rides in `needsAttention` as `kind: "triage"`, or in a `triage`
    // array of the same shape: either is read, so the server may pick.
    let triage = list(home, "triage").into_iter().map(|i| (i, true));
    for (item, filed) in list(home, "needsAttention").into_iter().map(|i| (i, false)).chain(triage) {
        let kind = if filed { Some("triage") } else { str_at(item, "kind") };
        let (signal, tone, verb) = match kind {
            Some("question") => ("Question", c::Tone::Ask, "asks"),
            Some("review") => ("Review", c::Tone::Ask, "submitted"),
            Some("triage") => ("Triage", c::Tone::Info, "filed"),
            _ => continue,
        };
        let Some(id) = str_at(item, "taskId") else { continue };
        let agent = str_at(item, "agentName").unwrap_or("Your agent");
        let body = str_at(item, "body").unwrap_or_default().lines().next().unwrap_or_default();
        if signal == "Triage" {
            // The body is the category: "Slack Agent filed a bug".
            let what = match body {
                "" => "this",
                "bug" => "a bug",
                "feature" => "a feature request",
                "feedback" => "feedback",
                "question" => "a question",
                "chore" => "a chore",
                _ => "a task",
            };
            out.push(Alert {
                rank: 0,
                weight: -1,
                tone,
                signal,
                subject: str_at(item, "title").unwrap_or_default().to_owned(),
                reason: format!("{agent} filed {what} \u{00B7} accept or dismiss"),
                target: Target::Task(id.to_owned()),
            });
            continue;
        }
        out.push(Alert {
            rank: 0,
            weight: 0,
            tone,
            signal,
            subject: str_at(item, "title").unwrap_or_default().to_owned(),
            reason: if body.is_empty() { format!("{agent} is waiting on you") } else { format!("{agent} {verb}: {body}") },
            target: Target::Task(id.to_owned()),
        });
    }

    for p in projects {
        let status = str_at(p, "status").unwrap_or_default();
        let Some(target) = str_at(p, "targetDate").and_then(|d| d.parse::<NaiveDate>().ok())
        else {
            continue;
        };
        let (done, total) = (num(p, "done"), num(p, "total"));
        let progress = format!("{done} of {total} done");
        let days = (target - today).num_days();
        let subject = str_at(p, "name").unwrap_or_default().to_owned();
        let id = str_at(p, "id").unwrap_or_default().to_owned();

        if days < 0 && (status == "active" || status == "paused") {
            out.push(Alert {
                rank: 1,
                weight: -days,
                tone: c::Tone::Blocked,
                signal: "Overdue",
                subject,
                reason: format!("{} past target · {progress}", plural(-days as usize, "day")),
                target: Target::Project(id),
            });
        } else if status == "active" && days <= AT_RISK_DAYS && done * 2 < total {
            let due = match days {
                0 => "due today".to_owned(),
                d => format!("due in {}", plural(d as usize, "day")),
            };
            out.push(Alert {
                rank: 3,
                weight: -days,
                tone: c::Tone::Running,
                signal: "At risk",
                subject,
                reason: format!("{due} · {progress}"),
                target: Target::Project(id),
            });
        }
    }

    for t in live {
        if finished(t) {
            continue;
        }
        let id = str_at(t, "id").unwrap_or_default().to_owned();
        let subject = str_at(t, "title").unwrap_or_default().to_owned();
        let priority = num(t, "priority");

        if outstanding(t) > 0 {
            out.push(Alert {
                rank: 2,
                weight: -priority,
                tone: c::Tone::Blocked,
                signal: "Blocked",
                subject,
                reason: format!("waiting on {}", blocker(t, by_id)),
                target: Target::Task(id),
            });
            continue;
        }

        let idle = days_since(str_at(t, "updatedAt").unwrap_or_default());
        if str_at(t, "status") == Some("in_progress") && idle > STALLED_DAYS {
            let who = str_at(t, "assigneeName").map(first_name).unwrap_or("nobody");
            out.push(Alert {
                rank: 4,
                weight: idle,
                tone: c::Tone::Running,
                signal: "Stalled",
                subject,
                reason: format!("no update in {} · {who}", plural(idle as usize, "day")),
                target: Target::Task(id),
            });
        } else if priority <= 1 && str_at(t, "assigneeName").is_none() {
            out.push(Alert {
                rank: 5,
                weight: -priority,
                tone: c::Tone::Quiet,
                signal: "Unassigned",
                subject,
                reason: format!("P{priority} · nobody holds it"),
                target: Target::Task(id),
            });
        }
    }

    out.sort_by_key(|a| (a.rank, -a.weight));
    out
}

/// The attention list, as a table: one surface, row rules, hover and keyboard
/// reach for free. A card per alert was the other option, and six cards is
/// the column-of-cards this page stopped being.
fn attention_list(ui: &mut egui::Ui, alerts: &[Alert], go: &mut Option<Target>) {
    let shown = &alerts[..alerts.len().min(ATTENTION_MAX)];
    shell::section_count_with(ui, "Needs attention", alerts.len(), |ui| {
        if alerts.len() > shown.len() {
            w::caption(ui, &format!("the {} most severe", shown.len()));
        }
    });
    let clicked = table::show(ui, ATTENTION, &ATTENTION_COLS, shown.len(), |row, i| {
        let a = &shown[i];
        row.at(0, |ui| {
            c::chip(ui, a.signal, a.tone, false);
        });
        row.strong(1, &a.subject, colour::TEXT());
        row.muted(2, &a.reason);
    });
    if let Some(i) = clicked {
        *go = Some(shown[i].target.clone());
    }
}

// ------------------------------------------------------------------- figures

/// One person's live work, split the way the Team list draws it.
struct Member {
    id: String,
    name: String,
    in_progress: usize,
    to_ship: usize,
    blocked: usize,
    open: usize,
}

impl Member {
    fn total(&self) -> usize {
        self.in_progress + self.to_ship + self.blocked + self.open
    }
}

/// Everyone holding live work, busiest first. Finished and dropped work is
/// not load.
fn members(all: &[Row], rows: &[Value]) -> Vec<Member> {
    let mut by: Vec<Member> = Vec::new();
    for r in all.iter().filter(|r| !r.finished && r.bucket != "dropped") {
        let Some(id) = &r.owner else { continue };
        let at = match by.iter().position(|m| &m.id == id) {
            Some(at) => at,
            None => {
                by.push(Member {
                    id: id.clone(),
                    name: str_at(&rows[r.at], "assigneeName").unwrap_or("Someone").to_owned(),
                    in_progress: 0,
                    to_ship: 0,
                    blocked: 0,
                    open: 0,
                });
                by.len() - 1
            }
        };
        let m = &mut by[at];
        match r.bucket {
            "in_progress" | "research" => m.in_progress += 1,
            "completed" | "handoff" => m.to_ship += 1,
            "blocked" => m.blocked += 1,
            _ => m.open += 1,
        }
    }
    by.sort_by(|a, b| b.total().cmp(&a.total()).then(a.name.cmp(&b.name)));
    by
}

/// A quiet panel: the surface every section on this page sits on.
fn panel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(colour::SURFACE())
        .stroke(egui::Stroke::new(1.0, colour::LINE()))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::symmetric(space::LG as i8, space::LG as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn panel_title(ui: &mut egui::Ui, title: &str, note: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        ui.label(
            RichText::new(title)
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT()),
        );
        if !note.is_empty() {
            ui.label(RichText::new(note).size(text::SMALL).color(colour::TEXT_MUTED()));
        }
    });
}

/// The overview: six figures in one band, each a filter. Clicking one shows
/// just those tasks in the table below (and scrolls to it); clicking it again
/// clears it. Returns the figure clicked.
fn overview(ui: &mut egui::Ui, d: &Derived, active: Option<Quick>) -> Option<Quick> {
    let count = |q: Quick| d.all.iter().filter(|r| r.bucket != "dropped" && q.matches(r)).count();
    let mut clicked = None;
    egui::Frame::new()
        .fill(colour::SURFACE())
        .stroke(egui::Stroke::new(1.0, colour::LINE()))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::same(space::XS as i8))
        .show(ui, |ui| {
            let width = ui.available_width();
            let per_row = if width >= 860.0 { Quick::ALL.len() } else { 3 };
            let cell_w = width / per_row as f32;
            let cell_h = 84.0;
            for (row_i, chunk) in Quick::ALL.chunks(per_row).enumerate() {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, cell_h), egui::Sense::hover());
                for (i, q) in chunk.iter().enumerate() {
                    let n = count(*q);
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + i as f32 * cell_w, rect.top()),
                        egui::vec2(cell_w, cell_h),
                    );
                    let id = ui.id().with(("home:quick", row_i, i));
                    let r = ui.interact(cell.shrink(2.0), id, egui::Sense::click());
                    let r = crate::desktop::design::motion::operable(ui, r, radius::MD as f32);
                    let on = active == Some(*q);
                    let hot = r.hovered() || r.has_focus();
                    let p = ui.painter();
                    if on {
                        p.rect_filled(cell.shrink(2.0), radius::MD as f32, colour::ACCENT_SOFT());
                    } else if hot {
                        p.rect_filled(cell.shrink(2.0), radius::MD as f32, colour::SURFACE_HOVER());
                    }
                    if i > 0 && !on && active != Some(chunk[i - 1]) {
                        p.vline(cell.left(), cell.y_range().shrink(space::LG), egui::Stroke::new(1.0, colour::LINE()));
                    }
                    // Red for what is stuck, amber for what waits on a ship;
                    // every other number in white.
                    let ink = match q {
                        Quick::Blocked if n > 0 => colour::DANGER(),
                        _ => colour::TEXT(),
                    };
                    let x = cell.left() + space::LG;
                    let label_ink = if on { colour::ACCENT() } else { colour::TEXT_MUTED() };
                    p.text(egui::pos2(x, cell.top() + space::MD), egui::Align2::LEFT_TOP, q.label(),
                        egui::FontId::proportional(text::SMALL), label_ink);
                    p.text(egui::pos2(x, cell.top() + space::MD + text::SMALL + space::XS), egui::Align2::LEFT_TOP,
                        n.to_string(), egui::FontId::new(text::TITLE * 1.2, egui::FontFamily::Name(theme::SEMIBOLD.into())), ink);
                    let foot = if on { "Showing below \u{00B7} click to clear" } else { "Show tasks" };
                    if hot || on {
                        p.text(egui::pos2(x, cell.bottom() - space::MD), egui::Align2::LEFT_BOTTOM, foot,
                            egui::FontId::proportional(text::CAPTION), if on { colour::ACCENT() } else { colour::TEXT_FAINT() });
                    }
                    if hot {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let spoken = format!("{}: {n}", q.label());
                    let r = r.on_hover_text(q.hint());
                    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &spoken));
                    if r.clicked() {
                        clicked = Some(*q);
                    }
                }
            }
        });
    clicked
}

/// This week: what finished each day, large enough to read, beside the list of
/// what finished. Returns a task to open, or `true` for "show them all".
fn this_week(ui: &mut egui::Ui, d: &Derived, rows: &[Value]) -> (Option<String>, bool) {
    let mut open = None;
    let mut all = false;
    panel(ui, |ui| {
        let total = d.completed.total;
        let note = if d.completed.delta.is_empty() { String::new() } else { d.completed.delta.clone() };
        panel_title(ui, "Finished this week", &format!("{total} \u{00B7} {note}"));
        ui.add_space(space::MD);
        let width = ui.available_width();
        let chart_w = if width >= 760.0 { width * 0.48 } else { width };
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(chart_w, 170.0), Layout::top_down(Align::Min), |ui| {
                week_chart(ui, &d.completed.buckets);
            });
            if width < 760.0 {
                return;
            }
            ui.add_space(space::XL);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                if d.finished.is_empty() {
                    w::muted(ui, "Nothing finished in the last seven days yet.");
                    return;
                }
                for &i in d.finished.iter().take(5) {
                    let t = &rows[d.all[i].at];
                    if finished_row(ui, t) {
                        open = str_at(t, "id").map(str::to_owned);
                    }
                }
                if d.finished.len() > 5 && w::link(ui, &format!("Show all {}", d.finished.len())).clicked() {
                    all = true;
                }
            });
        });
    });
    (open, all)
}

/// Seven columns, today last: the count over each bar and the day under it.
fn week_chart(ui: &mut egui::Ui, buckets: &[f32]) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 150.0), egui::Sense::hover());
    let n = buckets.len().max(1) as f32;
    let gap = space::MD;
    let bar_w = ((rect.width() - gap * (n - 1.0)) / n).min(48.0);
    let span = bar_w * n + gap * (n - 1.0);
    let left = rect.left() + (rect.width() - span) / 2.0;
    let label_h = text::SMALL + space::XS;
    let value_h = text::SMALL + space::XS;
    let plot_top = rect.top() + value_h;
    let plot_bottom = rect.bottom() - label_h;
    let peak = buckets.iter().cloned().fold(1.0_f32, f32::max);
    let today = Local::now().date_naive();
    let p = ui.painter();
    p.hline(rect.x_range(), plot_bottom, egui::Stroke::new(1.0, colour::LINE()));
    for (k, v) in buckets.iter().enumerate() {
        let x = left + k as f32 * (bar_w + gap);
        let is_today = k + 1 == buckets.len();
        let h = if *v > 0.0 { (v / peak * (plot_bottom - plot_top)).max(4.0) } else { 0.0 };
        if h > 0.0 {
            let bar = egui::Rect::from_min_max(egui::pos2(x, plot_bottom - h), egui::pos2(x + bar_w, plot_bottom));
            p.rect_filled(bar, 4.0, colour::ACCENT());
            p.text(egui::pos2(x + bar_w / 2.0, bar.top() - 2.0), egui::Align2::CENTER_BOTTOM, format!("{}", *v as i64),
                egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::SEMIBOLD.into())), colour::TEXT());
        }
        let day = today - chrono::Duration::days((buckets.len() - 1 - k) as i64);
        let name = if is_today { "Today".to_owned() } else { day.format("%a").to_string() };
        p.text(egui::pos2(x + bar_w / 2.0, plot_bottom + space::XS), egui::Align2::CENTER_TOP, name,
            egui::FontId::proportional(text::CAPTION),
            if is_today { colour::TEXT() } else { colour::TEXT_MUTED() });
    }
}

/// A finished task: its end state, the title, who, when. The row opens it.
fn finished_row(ui: &mut egui::Ui, t: &Value) -> bool {
    let status = str_at(t, "status").unwrap_or("completed");
    let title = str_at(t, "title").unwrap_or("Untitled");
    let who = str_at(t, "assigneeName").map(first_name).unwrap_or("");
    let when = str_at(t, "doneAt").map(age).unwrap_or_default();
    let r = w::row(ui, |ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        w::dot(ui, status_colour(status));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(when).size(text::CAPTION).color(colour::TEXT_FAINT()));
            if !who.is_empty() {
                ui.label(RichText::new(who).size(text::SMALL).color(colour::TEXT_MUTED()));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(title).size(text::SMALL).color(colour::TEXT())).truncate());
            });
        });
    });
    r.on_hover_text(format!("{} \u{00B7} open the task", status_label(status))).clicked()
}

/// Team: everyone's live work as one stacked bar — in progress, ready to
/// ship, blocked, not started — on a shared scale, so who is overloaded and
/// who is stuck reads at a glance. A row filters the table to that person.
fn team(ui: &mut egui::Ui, d: &Derived, active: Option<&str>) -> Option<(String, String)> {
    let mut picked = None;
    panel(ui, |ui| {
        ui.horizontal(|ui| {
            panel_title(ui, "Team", "live work per person");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::MD;
                for (label, c) in [("Not started", colour::TEXT_FAINT()), ("Blocked", colour::DANGER()), ("Ready to ship", colour::INFO()), ("In progress", colour::WARN())] {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::XS;
                        w::dot(ui, c);
                        ui.label(RichText::new(label).size(text::CAPTION).color(colour::TEXT_MUTED()));
                    });
                }
            });
        });
        ui.add_space(space::SM);
        if d.team.is_empty() {
            w::muted(ui, "Nobody holds live work right now.");
            return;
        }
        let peak = d.team.iter().map(Member::total).max().unwrap_or(1).max(1) as f32;
        for m in &d.team {
            let on = active == Some(m.id.as_str());
            let r = w::row(ui, |ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                avatar::small(ui, &m.name, size::AVATAR_SM);
                let name_w = 110.0;
                let (nr, _) = ui.allocate_exact_size(egui::vec2(name_w, size::ROW), egui::Sense::hover());
                let g = w::truncated(ui, first_name(&m.name), egui::FontId::proportional(text::SMALL),
                    if on { colour::ACCENT() } else { colour::TEXT() }, name_w);
                ui.painter().galley(egui::pos2(nr.left(), nr.center().y - g.size().y / 2.0), g, colour::TEXT());
                let summary = {
                    let mut parts = Vec::new();
                    if m.in_progress > 0 { parts.push(format!("{} in progress", m.in_progress)); }
                    if m.to_ship > 0 { parts.push(format!("{} to ship", m.to_ship)); }
                    if m.blocked > 0 { parts.push(format!("{} blocked", m.blocked)); }
                    if m.open > 0 { parts.push(format!("{} not started", m.open)); }
                    parts.join(" \u{00B7} ")
                };
                let text_w = 230.0_f32.min(ui.available_width() * 0.45);
                let bar_room = (ui.available_width() - text_w - space::MD).max(40.0);
                let (br, _) = ui.allocate_exact_size(egui::vec2(bar_room, size::ROW), egui::Sense::hover());
                let total_w = bar_room * (m.total() as f32 / peak);
                let mut x = br.left();
                let p = ui.painter();
                let track = egui::Rect::from_center_size(egui::pos2(br.center().x, br.center().y), egui::vec2(bar_room, 8.0));
                p.rect_filled(track, 4.0, colour::LINE());
                for (n, c) in [(m.in_progress, colour::WARN()), (m.to_ship, colour::INFO()), (m.blocked, colour::DANGER()), (m.open, colour::TEXT_FAINT())] {
                    if n == 0 { continue; }
                    let w_ = total_w * n as f32 / m.total() as f32;
                    p.rect_filled(egui::Rect::from_min_size(egui::pos2(x, track.top()), egui::vec2(w_, 8.0)), 4.0, c);
                    x += w_;
                }
                ui.add_space(space::MD);
                ui.add(egui::Label::new(RichText::new(summary).size(text::CAPTION).color(colour::TEXT_MUTED())).truncate());
            });
            if on {
                ui.painter().rect_stroke(r.rect, radius::SM as f32, egui::Stroke::new(1.0, colour::ACCENT()), egui::StrokeKind::Inside);
            }
            if r.on_hover_text(if on { "Showing their tasks below \u{00B7} click to clear" } else { "Show their tasks below" }).clicked() {
                picked = Some((m.id.clone(), m.name.clone()));
            }
        }
    });
    picked
}

/// Completions per day for the last week, bucketed on `doneAt` — the moment
/// the task reached the end of its track, not the last time anyone touched it.
/// Design ends at completed and engineering at shipped; the stamp knows. The delta
/// compares the same measure over the previous week.
struct CompletedFigures {
    buckets: Vec<f32>,
    total: usize,
    delta: String,
}

fn completed_figures(rows: &[&Value]) -> CompletedFigures {
    let today = Local::now().date_naive();
    let mut buckets = vec![0.0_f32; WEEK];
    let mut previous = 0usize;

    // `doneAt` alone decides: it is stamped at whichever state ends the task's
    // track, so there is no status to cross-check it against.
    for t in rows {
        let Some(raw) = str_at(t, "doneAt") else { continue };
        let Ok(when) = DateTime::parse_from_rfc3339(raw) else { continue };
        let days = (today - when.with_timezone(&Local).date_naive()).num_days();
        if (0..WEEK as i64).contains(&days) {
            buckets[WEEK - 1 - days as usize] += 1.0;
        } else if (WEEK as i64..2 * WEEK as i64).contains(&days) {
            previous += 1;
        }
    }

    let total: f32 = buckets.iter().sum();
    let total = total as usize;
    let delta = match total as i64 - previous as i64 {
        _ if total == 0 && previous == 0 => "none yet".to_owned(),
        0 => "same as prev".to_owned(),
        d if d > 0 => format!("+{d} vs prev"),
        d => format!("{d} vs prev"),
    };

    CompletedFigures { buckets, total, delta }
}

// ---------------------------------------------------------------- filter bar

fn filter_bar(
    ui: &mut egui::Ui,
    state: &mut State,
    departments: &[(String, String)],
    projects: &[(String, String)],
) {
    viz::toolbar(ui, |ui| {
        viz::search(ui, "Search tasks, projects, people…", &mut state.query);

        // What was picked from the overview or the Team list, shown as an
        // active filter that clears with a click.
        if let Some(q) = state.quick {
            if viz::filter(ui, q.label(), true, false).on_hover_text("Click to clear").clicked() {
                state.quick = None;
            }
        }
        if let Some((_, name)) = state.owner.clone() {
            if viz::filter(ui, first_name(&name), true, false).on_hover_text("Click to clear").clicked() {
                state.owner = None;
            }
        }

        if viz::filter(ui, "Mine", state.mine, false).clicked() {
            state.mine = !state.mine;
        }

        viz::select(ui, "All departments", departments, &mut state.department);

        let options: Vec<(String, String)> =
            PRIORITIES.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect();
        viz::select(ui, "Any priority", &options, &mut state.priority);

        let options: Vec<(String, String)> =
            STATUSES.iter().map(|s| ((*s).to_owned(), status_label(s).to_owned())).collect();
        viz::select(ui, "Any status", &options, &mut state.status);

        viz::select(ui, "All projects", projects, &mut state.project);

        if viz::filter(ui, "Archived", state.archived, false).clicked() {
            state.archived = !state.archived;
        }

        if *state != State::default() && viz::clear(ui).clicked() {
            *state = State::default();
        }
    });
}

/// `needle` is the search box, trimmed and lowercased once for every row.
fn keep(r: &Row, t: &Value, state: &State, my_person_id: &str, needle: &str) -> bool {
    // Dropped tasks are out of every count, so they are out of the table too —
    // unless dropped is exactly what was asked for.
    if r.bucket == "dropped" && state.status.as_deref() != Some("dropped") {
        return false;
    }
    if state.mine
        && (my_person_id.is_empty() || str_at(t, "assigneePersonId") != Some(my_person_id))
    {
        return false;
    }
    if let Some(d) = &state.department {
        if str_at(t, "discipline") != Some(d.as_str()) {
            return false;
        }
    }
    if let Some(p) = &state.priority {
        if num(t, "priority").to_string() != *p {
            return false;
        }
    }
    if let Some(s) = &state.status {
        if r.bucket != s.as_str() {
            return false;
        }
    }
    if let Some(p) = &state.project {
        if str_at(t, "projectId") != Some(p.as_str()) {
            return false;
        }
    }
    if let Some(q) = state.quick {
        if !q.matches(r) {
            return false;
        }
    }
    if let Some((id, _)) = &state.owner {
        if r.owner.as_deref() != Some(id.as_str()) {
            return false;
        }
    }
    // Title, project and owner: the three things you would think to type.
    // Not the status — that is what the menu beside the box is for.
    needle.is_empty() || r.search.iter().any(|h| h.contains(needle))
}

// --------------------------------------------------------------------- table

fn task_row(row: &mut table::Cells<'_, '_, '_>, t: &Value, r: &Row, my_person_id: &str) {
    let status = r.bucket;
    let department = str_at(t, "discipline").unwrap_or_default();
    let mine = !my_person_id.is_empty() && str_at(t, "assigneePersonId") == Some(my_person_id);

    row.at(0, |ui| {
        let labels = t.get("labels").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        super::projects::name_with_labels(ui, str_at(t, "title").unwrap_or_default(), super::board::title_ink(t), labels);
        // The blocker rides behind the title rather than in its own column:
        // it is a footnote on the task, not a property every row has. Keyed
        // on the counts, not the status — a blocker can resolve while the
        // status still says blocked.
        if let Some(waiting) = &r.waiting {
            ui.add_space(space::XS);
            w::caption(ui, waiting);
        }
    });

    row.at(1, |ui| {
        let p = num(t, "priority").clamp(0, 4);
        c::priority(ui, p);
    });
    if department.is_empty() {
        row.muted(2, "");
    } else {
        row.at(2, |ui| {
            c::discipline(ui, department);
        });
    }
    row.at(3, |ui| {
        if super::board::archived(t) {
            super::board::archived_chip(ui);
        } else {
            // Triage files under open for the figures; the chip says what it is.
            let status = if str_at(t, "status") == Some("triage") { "triage" } else { status };
            // A live delegate says more than "In progress" ever could: whether
            // it needs you right now or is just getting on with it. The task's
            // own status is still a hover away, not hidden.
            let agent = t
                .get("delegate")
                .filter(|d| d.is_object() && !finished(t))
                .and_then(|d| agent_status(d, mine));
            match agent {
                Some((label, tone)) => {
                    c::state(ui, &label, tone).on_hover_text(status_label(status));
                }
                None => {
                    c::state(ui, status_label(status), c::status_tone(status));
                }
            }
        }
    });
    row.muted(4, str_at(t, "projectName").unwrap_or_default());

    row.at(5, |ui| match str_at(t, "assigneeName") {
        Some(name) => {
            avatar::small(ui, str_at(t, "assigneeEmail").unwrap_or(name), size::AVATAR_SM);
            ui.add_space(space::XS);
            ui.label(RichText::new(first_name(name)).size(text::SMALL).color(colour::TEXT_2()));
            agent_marker(ui, t);
        }
        None => {
            ui.label(RichText::new("Unassigned").size(text::SMALL).color(colour::TEXT_FAINT()));
        }
    });

    row.muted(6, &age(str_at(t, "updatedAt").unwrap_or_default()));
}

/// The small agent mark beside an owner whose task is with one of their
/// agents, the agent's name on hover.
pub(super) fn agent_marker(ui: &mut egui::Ui, t: &Value) {
    let Some(name) = t.get("delegate").and_then(|d| d.get("name")).and_then(Value::as_str) else {
        return;
    };
    w::agent_mark(ui, size::AVATAR_SM - space::XS).on_hover_text(format!("With {name}"));
}

/// What a live delegate is doing, in the status column's own words and tone
/// — the same three buckets the task page's header pill reads into, so a
/// list row and the task it opens never disagree. The state itself is never
/// hidden from a teammate; `mine` only decides whether "you" or the owner's
/// name is who it is waiting on, the way `agent_session::now_line` already
/// splits it for the session's own line.
pub(super) fn agent_status(d: &Value, mine: bool) -> Option<(String, c::Tone)> {
    let state = d.get("state").and_then(Value::as_str)?;
    Some(match state {
        "working" | "acknowledged" => ("Agent working".to_owned(), c::Tone::Info),
        "plan_review" | "needs_input" | "in_review" => {
            let who = if mine {
                "you".to_owned()
            } else {
                str_at(d, "ownerName")
                    .and_then(|n| n.split_whitespace().next())
                    .unwrap_or("the owner")
                    .to_owned()
            };
            (format!("Waiting on {who}"), c::Tone::Ask)
        }
        "done" => ("Agent done".to_owned(), c::Tone::Quiet),
        other => (super::agents::state_words(other).to_owned(), c::Tone::Neutral),
    })
}

/// The first unresolved blocker, as "Cart totals API (Anmol)". Falls back to
/// an honest count when the blocker is not in this payload.
fn blocker(t: &Value, by_id: &HashMap<&str, &Value>) -> String {
    let named = t
        .get("blockedBy")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|id| by_id.get(id).copied())
        .find(|b| !BLOCKER_RESOLVED.contains(&str_at(b, "status").unwrap_or_default()));
    let Some(b) = named else {
        return plural(outstanding(t) as usize, "other task");
    };
    let title = str_at(b, "title").unwrap_or_default();
    match str_at(b, "assigneeName") {
        Some(name) => format!("{title} ({})", first_name(name)),
        None => title.to_owned(),
    }
}

// -------------------------------------------------------------------- pieces

/// The state a row is filed under. Unfinished blockers beat the status field:
/// a task marked `in_progress` that is waiting on someone else is not in
/// progress, and filing it as though it were hides the thing to go and unblock.
fn bucket(t: &Value) -> &'static str {
    // Dropped beats blocked: nobody is waiting to unblock abandoned work.
    if str_at(t, "status") == Some("dropped") {
        return "dropped";
    }
    if outstanding(t) > 0 {
        return "blocked";
    }
    let status = str_at(t, "status").unwrap_or("open");
    STATUSES.iter().copied().find(|s| *s == status).unwrap_or("open")
}

fn outstanding(t: &Value) -> i64 {
    (num(t, "blockersTotal") - num(t, "blockersDone")).max(0)
}

/// Whether the task is over. The server stamps `doneAt` at the terminal state
/// of whichever track the assignee's department puts it on, so this is the one
/// test that is right for both — `completed` is the end for design and the
/// middle for engineering, and the status alone cannot tell you which.
fn finished(t: &Value) -> bool {
    t.get("doneAt").is_some_and(|v| !v.is_null())
}


fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// "2d", "4h". A table column has room for two characters, and the exact
/// minute is never the thing being decided.
fn age(raw: &str) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(raw) else {
        return String::new();
    };
    let seconds = (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0);
    match seconds {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// Whole days since an RFC 3339 stamp; zero when it does not parse, so a
/// missing stamp never reads as stale.
fn days_since(raw: &str) -> i64 {
    DateTime::parse_from_rfc3339(raw)
        .map(|then| (Utc::now() - then.with_timezone(&Utc)).num_days().max(0))
        .unwrap_or(0)
}

fn me_str(app: &App, key: &str) -> String {
    app.net
        .as_ref()
        .and_then(|n| n.data("__me"))
        .and_then(|m| m.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn list<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn num(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}
