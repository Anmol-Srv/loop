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

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use egui::{Align, Color32, Layout, RichText};
use serde_json::Value;

use super::projects::PRIORITIES;
use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{
    avatar, cards as c, colour, shell, size, space, status_colour, status_label, text, tokens,
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

/// Last frame's tallest figure, so all four cards agree on a height.
const VIZ_H: &str = "home:viz-h";

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
const STATUSES: [&str; 7] =
    ["open", "in_progress", "handoff", "completed", "shipped", "blocked", "dropped"];

/// The statuses at which a task stops holding up the tasks that wait on it.
/// Mirrors the server's `BLOCKER_RESOLVED`: a design handoff unblocks the
/// engineer, it does not have to ship first.
const BLOCKER_RESOLVED: [&str; 4] = ["handoff", "completed", "shipped", "dropped"];

/// The donut's slices, finished-first so the ring fills clockwise from the
/// outcome you want. `dropped` is absent because the whole page excludes it.
const DONUT: [&str; 6] = ["shipped", "completed", "in_progress", "handoff", "blocked", "open"];

/// Table geometry. Fixed so the columns line up with the header and with each
/// other; the task column takes whatever is left. Alignment is declared here
/// too, so "Updated" and the age beneath it cannot disagree.
const COL_DOT: f32 = 22.0;
/// "P0" plus chip padding, the same width the task tables use.
const COL_PRIORITY: f32 = 52.0;
const COL_DEPARTMENT: f32 = 88.0;
const COL_STATUS: f32 = 104.0;
const COL_PROJECT: f32 = 150.0;
/// An avatar and a first name.
const COL_OWNER: f32 = 110.0;
const COL_UPDATED: f32 = 78.0;

const COLS: [Col; 8] = [
    Col::left("", COL_DOT),
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
    Col::fill("What", 140.0),
    Col::fill("Why", 180.0),
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
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let my_person_id = me_str(app, "personId");

    let filters_id = egui::Id::new(FILTERS);
    let mut state: State = ui.ctx().data_mut(|d| d.get_temp(filters_id)).unwrap_or_default();

    let net = app.net.as_mut().unwrap();

    net.get_once(HOME, "/api/user/home");
    net.get_once(TASKS, "/api/user/tasks");
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

    let mut subtitle = format!(
        "{} across {}",
        plural(d.live, "task"),
        plural(d.project_count, "project")
    );
    let dropped = d.all.len() - d.live;
    if dropped > 0 {
        subtitle += &format!(" · {dropped} dropped, not counted");
    }
    shell::page_title(ui, "Overview", &subtitle, |_| {});

    if let Some(err) = &error {
        w::error(ui, &format!("{err} Use Refresh in the sidebar to try again."));
        return;
    }
    if loading && d.all.is_empty() {
        w::loading(ui, "Loading your dashboard");
        return;
    }

    // ---- what a lead opens this page for: the list of things going wrong
    let mut go: Option<Target> = None;
    if !d.alerts.is_empty() {
        attention_list(ui, &d.alerts, &mut go);
        ui.add_space(space::XL);
    }

    // ---- the four figures, all the same height
    let team = list(d.home.as_ref(), "team");
    viz::row(
        ui,
        egui::Id::new(VIZ_H),
        &mut [
            &mut |ui: &mut egui::Ui, h| status_card(ui, h, &d.status),
            &mut |ui: &mut egui::Ui, h| department_card(ui, h, &d.departments),
            &mut |ui: &mut egui::Ui, h| completed_card(ui, h, &d.completed),
            &mut |ui: &mut egui::Ui, h| team_card(ui, h, &team),
        ],
    );

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
    shell::section_count_with(ui, "Tasks", shown.len(), |ui| {
        if filtered {
            w::caption(ui, &format!("filtered from {}", t.live));
        }
    });
    filter_bar(ui, &mut state, &d.filter_departments, &d.filter_projects);
    ui.ctx().data_mut(|d| d.insert_temp(filters_id, state));

    if shown.is_empty() {
        w::empty(ui, "Nothing matches those filters.", "Clear one of them to see more.");
    } else if let Some(i) = table::show(ui, TABLE, &COLS, shown.len(), |row, i| {
        let r = &t.all[shown[i]];
        task_row(row, &rows[r.at], r);
    }) {
        go = str_at(&rows[t.all[shown[i]].at], "id").map(|id| Target::Task(id.to_owned()));
    }
    ui.add_space(space::XXL);

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
    home: Arc<Value>,
    tasks: Arc<Value>,
    /// Every task, newest first, dropped included.
    all: Vec<Row>,
    /// How many of `all` are not dropped.
    live: usize,
    project_count: usize,
    alerts: Vec<Alert>,
    status: StatusFigures,
    departments: Vec<(String, i64, i64)>,
    completed: CompletedFigures,
    filter_departments: Vec<(String, String)>,
    filter_projects: Vec<(String, String)>,
}

/// One task as the table and the filters need it.
struct Row {
    /// Index into the `/tasks` array.
    at: usize,
    bucket: &'static str,
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
            Row {
                at: i,
                bucket: bucket(t),
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
        project_count: projects.len(),
        alerts: attention(&home, &projects, &live, &by_id),
        status: status_figures(&live),
        departments: department_rollup(&projects),
        completed: completed_figures(&live),
        filter_departments: departments.iter().map(|d| ((*d).to_owned(), (*d).to_owned())).collect(),
        filter_projects: projects
            .iter()
            .filter_map(|p| Some((str_at(p, "id")?.to_owned(), str_at(p, "name")?.to_owned())))
            .collect(),
        all,
        home: home.clone(),
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

    for item in list(home, "needsAttention") {
        let (signal, tone, verb) = match str_at(item, "kind") {
            Some("question") => ("Question", c::Tone::Running, "asks"),
            Some("review") => ("Review", c::Tone::Agent, "submitted"),
            _ => continue,
        };
        let Some(id) = str_at(item, "taskId") else { continue };
        let agent = str_at(item, "agentName").unwrap_or("Your agent");
        let body = str_at(item, "body").unwrap_or_default().lines().next().unwrap_or_default();
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
        row.strong(1, &a.subject, colour::TEXT);
        row.muted(2, &a.reason);
    });
    if let Some(i) = clicked {
        *go = Some(shown[i].target.clone());
    }
}

// ------------------------------------------------------------------- figures

/// The ring's numbers: a count per `DONUT` slice, the whole, and the share done.
struct StatusFigures {
    counts: [usize; 6],
    total: usize,
    pct: usize,
}

fn status_figures(rows: &[&Value]) -> StatusFigures {
    // Dropped work is off the board, so it is off the ring and out of the
    // denominator too — otherwise the percentage measures the wrong pile.
    let live: Vec<&&Value> = rows.iter().filter(|t| bucket(t) != "dropped").collect();
    let mut counts = [0; 6];
    for t in &live {
        if let Some(i) = DONUT.iter().position(|s| *s == bucket(t)) {
            counts[i] += 1;
        }
    }
    let total = live.len();
    // `doneAt` is the server's one answer for both tracks: design ends at
    // completed, engineering at shipped, and only the terminal state stamps it.
    let done = live.iter().filter(|t| finished(t)).count();
    let pct = if total == 0 { 0 } else { done * 100 / total };
    StatusFigures { counts, total, pct }
}

/// Status as a ring. The centre carries the only number worth reading from
/// across the room: how much of this is finished.
fn status_card(ui: &mut egui::Ui, min_body: f32, f: &StatusFigures) -> f32 {
    viz::card(ui, "Status", &plural(f.total, "task"), min_body, |ui| {
        let slices: Vec<viz::Slice<'_>> = DONUT
            .iter()
            .zip(LABELS)
            .zip(f.counts)
            .map(|((status, label), count)| viz::Slice {
                label,
                count,
                colour: status_colour(status),
            })
            .collect();
        viz::donut(ui, &slices, &format!("{}%", f.pct), "DONE");
    })
}

/// Sentence-cased status names, positionally matched to `DONUT`. Built once
/// rather than capitalising in the render loop.
const LABELS: [&str; 6] =
    ["Shipped", "Completed", "In progress", "Handoff", "Blocked", "Open"];

/// Progress per department, from the server's per-project rollup — the one
/// number on this page that covers every task, not just the ones on screen.
fn department_rollup(projects: &[&Value]) -> Vec<(String, i64, i64)> {
    // Sum the rollups across projects, keeping first-seen order so the list
    // does not reshuffle between refreshes.
    let mut order: Vec<String> = Vec::new();
    let mut tally: HashMap<String, (i64, i64)> = HashMap::new();
    let (mut all_done, mut all_total) = (0, 0);

    for p in projects {
        all_done += num(p, "done");
        all_total += num(p, "total");
        for d in list(p, "disciplines") {
            let Some(name) = str_at(d, "discipline") else { continue };
            let slot = tally.entry(name.to_owned()).or_insert_with(|| {
                order.push(name.to_owned());
                (0, 0)
            });
            slot.0 += num(d, "done");
            slot.1 += num(d, "total");
        }
    }

    // Whatever the departments do not account for is unassigned work — a task
    // with no assignee has no department yet. It is real, so it gets a row
    // rather than quietly vanishing from the total.
    let labelled: i64 = order.iter().filter_map(|d| tally.get(d)).map(|(_, t)| t).sum();
    let labelled_done: i64 = order.iter().filter_map(|d| tally.get(d)).map(|(d, _)| d).sum();
    if all_total > labelled {
        order.push("Unassigned".to_owned());
        tally.insert("Unassigned".to_owned(), (all_done - labelled_done, all_total - labelled));
    }

    order
        .into_iter()
        .map(|name| {
            let (done, total) = tally.get(&name).copied().unwrap_or((0, 0));
            (name, done, total)
        })
        .collect()
}

fn department_card(ui: &mut egui::Ui, min_body: f32, rollup: &[(String, i64, i64)]) -> f32 {
    viz::card(ui, "By department", "done / total", min_body, |ui| {
        for (name, done, total) in rollup.iter().take(viz::MAX_BARS) {
            let (done, total) = (*done, *total);
            let fill = if total == 0 { 0.0 } else { done as f32 / total as f32 };
            viz::bar_row(
                ui,
                name,
                fill,
                discipline_tint(name),
                &format!("{done}/{total}"),
                tokens::DISCIPLINE_W,
            );
        }
        overflow(ui, rollup.len());
    })
}

/// Completions per day for the last week, bucketed on `doneAt` — the moment
/// the task reached the end of its track, not the last time anyone touched it.
/// Design ends at completed and engineering at shipped; the stamp knows. The delta
/// compares the same measure over the previous week.
struct CompletedFigures {
    buckets: Vec<f32>,
    total: usize,
    delta: String,
    /// Day initials, oldest bucket first, so the last column is today.
    names: Vec<String>,
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
        _ if total == 0 && previous == 0 => String::new(),
        0 => "same as prev".to_owned(),
        d if d > 0 => format!("+{d} vs prev"),
        d => format!("{d} vs prev"),
    };

    let names: Vec<String> = (0..WEEK)
        .map(|i| {
            let day = today - chrono::Duration::days((WEEK - 1 - i) as i64);
            initial(day.weekday()).to_owned()
        })
        .collect();
    CompletedFigures { buckets, total, delta, names }
}

fn completed_card(ui: &mut egui::Ui, min_body: f32, f: &CompletedFigures) -> f32 {
    let names: Vec<&str> = f.names.iter().map(String::as_str).collect();
    viz::card(ui, "Completed", "last 7 days", min_body, |ui| {
        viz::headline(ui, &f.total.to_string(), &f.delta);
        ui.add_space(space::MD);
        viz::columns(ui, &f.buckets, &names, colour::ACCENT);
    })
}

fn initial(day: chrono::Weekday) -> &'static str {
    ["M", "T", "W", "T", "F", "S", "S"][day.num_days_from_monday() as usize]
}

/// Who is carrying what. The bar is `open` — work the person can act on
/// now. What is theirs but out of their hands (`review`) and what they are
/// holding up for others (`blocking`) ride underneath in words, because a
/// third bar colour would need a legend and nobody reads a legend.
fn team_card(ui: &mut egui::Ui, min_body: f32, team: &[&Value]) -> f32 {
    let peak = team.iter().map(|p| num(p, "open")).max().unwrap_or(0);

    viz::card(ui, "Team load", "open tasks", min_body, |ui| {
        // `ui.columns` hands each card a justified layout, and a justified
        // label that wraps spreads its letters across the line. Nothing here
        // wants justifying.
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            for p in team.iter().take(viz::MAX_BARS) {
                let open = num(p, "open");
                let fill = if peak == 0 { 0.0 } else { open as f32 / peak as f32 };
                viz::bar_row(
                    ui,
                    first_name(str_at(p, "name").unwrap_or_default()),
                    fill,
                    colour::ACCENT,
                    &open.to_string(),
                    tokens::DISCIPLINE_W,
                );
                load_note(ui, num(p, "blocking"), num(p, "review"));
            }
            overflow(ui, team.len());
        });
    })
}

/// "blocks 2 tasks · 1 in review" under a person's bar, lined up with the bar
/// rather than the name. Silent when both are zero. Blocking is in the danger
/// ink: it is the one number here that says who to go and talk to.
fn load_note(ui: &mut egui::Ui, blocking: i64, review: i64) {
    if blocking == 0 && review == 0 {
        return;
    }
    // Tucked up under its own bar, so it reads as that person's and not the
    // next one's. `interact_size` would otherwise make this a control-height
    // row and push the card taller than every figure beside it.
    let indent = tokens::DISCIPLINE_W + ui.spacing().item_spacing.x;
    ui.add_space(-space::SM);
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = 0.0;
        ui.spacing_mut().item_spacing.x = space::XS;
        ui.add_space(indent - space::XS);
        if blocking > 0 {
            ui.label(
                RichText::new(format!("blocks {}", plural(blocking as usize, "task")))
                    .size(text::CAPTION)
                    .color(colour::DANGER),
            );
        }
        if blocking > 0 && review > 0 {
            w::caption(ui, "·");
        }
        if review > 0 {
            w::caption(ui, &format!("{review} in review"));
        }
    });
}

/// "+N more" under a capped bar list. Silent when nothing was cut, so the
/// line only ever appears when it is telling the truth.
fn overflow(ui: &mut egui::Ui, total: usize) {
    if total > viz::MAX_BARS {
        w::caption(ui, &format!("+{} more", total - viz::MAX_BARS));
    }
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
    // Title, project and owner: the three things you would think to type.
    // Not the status — that is what the menu beside the box is for.
    needle.is_empty() || r.search.iter().any(|h| h.contains(needle))
}

// --------------------------------------------------------------------- table

fn task_row(row: &mut table::Cells<'_, '_, '_>, t: &Value, r: &Row) {
    let status = r.bucket;
    let department = str_at(t, "discipline").unwrap_or_default();

    row.at(0, |ui| w::dot(ui, status_colour(status)));

    row.at(1, |ui| {
        table::strong_label(ui, str_at(t, "title").unwrap_or_default(), super::board::title_ink(t));
        // The blocker rides behind the title rather than in its own column:
        // it is a footnote on the task, not a property every row has. Keyed
        // on the counts, not the status — a blocker can resolve while the
        // status still says blocked.
        if let Some(waiting) = &r.waiting {
            ui.add_space(space::XS);
            w::caption(ui, waiting);
        }
    });

    row.at(2, |ui| {
        let p = num(t, "priority").clamp(0, 4);
        c::chip(ui, &format!("P{p}"), priority_tone(p), false);
    });
    row.at(3, |ui| {
        if !department.is_empty() {
            c::chip(ui, department, c::discipline_tone(department), false);
        }
    });
    row.at(4, |ui| {
        if super::board::archived(t) {
            super::board::archived_chip(ui);
        } else {
            c::chip(ui, status_label(status), c::status_tone(status), status != "blocked");
        }
    });
    row.muted(5, str_at(t, "projectName").unwrap_or_default());

    row.at(6, |ui| match str_at(t, "assigneeName") {
        Some(name) => {
            avatar::small(ui, str_at(t, "assigneeEmail").unwrap_or(name), size::AVATAR_SM);
            ui.add_space(space::XS);
            ui.label(RichText::new(first_name(name)).size(text::SMALL).color(colour::TEXT_2));
            agent_marker(ui, t);
        }
        None => {
            ui.label(RichText::new("Unassigned").size(text::SMALL).color(colour::TEXT_FAINT));
        }
    });

    row.muted(7, &age(str_at(t, "updatedAt").unwrap_or_default()));
}

/// The small agent mark beside an owner whose task is with one of their
/// agents, the agent's name on hover.
pub(super) fn agent_marker(ui: &mut egui::Ui, t: &Value) {
    let Some(name) = t.get("delegate").and_then(|d| d.get("name")).and_then(Value::as_str) else {
        return;
    };
    w::agent_mark(ui, size::AVATAR_SM - space::XS).on_hover_text(format!("With {name}"));
}

/// How loud a priority is allowed to be — the task tables' scale, so P1 is
/// the same amber on every page. P0 and P1 are the only ones worth colour.
fn priority_tone(priority: i64) -> c::Tone {
    match priority {
        0 => c::Tone::Blocked,
        1 => c::Tone::Running,
        2 => c::Tone::Neutral,
        _ => c::Tone::Quiet,
    }
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


/// A department's bar colour, matching the chip it wears everywhere else.
fn discipline_tint(discipline: &str) -> Color32 {
    match c::discipline_tone(discipline) {
        c::Tone::Agent => colour::AGENT,
        c::Tone::Info => colour::INFO,
        c::Tone::Ok => colour::OK,
        _ => colour::LINE_STRONG,
    }
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
