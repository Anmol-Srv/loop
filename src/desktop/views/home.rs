//! Home: the dashboard.
//!
//! The previous version was a column of cards — the right unit for three
//! items and the wrong one for twenty, because you cannot count a column of
//! cards. This is the shape the owner asked for: four visualisations that read
//! at a glance, a filter bar, and a table.
//!
//! Two fetches. `GET /api/user/home` carries the projects rollup, the team
//! band and the sidebar badges — chrome.rs reads that payload under the key
//! `"home"`, so the name is load-bearing. `GET /api/user/tasks` carries the
//! table: the whole workspace, one row per task.
//!
//! The endpoint takes server-side filters, and the table ignores them. At
//! twenty rows a round trip per filter click buys nothing and costs a cache
//! key per combination, a spinner on every menu selection, and a filter menu
//! that can only offer the values the last response happened to contain.
//! One fetch, filtered in memory, keeps the menus honest and the clicks free.
//! Move to query params the day the workspace outgrows a single response.
//!
//! All four cards now sit on the whole workspace: Status and Completed count
//! the task list, By discipline sums the server's per-project rollup, Team
//! load is `team[]` from one grouped query.
//!
//! Approvals are gone from this page: the inbox owns them, and the table's
//! "Mine" filter covers what used to be the claim list. Nobody takes work
//! here: a task is handed out when it is written, and its assignee is the
//! only one who can move it.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Local, Utc};
use egui::{Align, Color32, Layout, RichText};
use serde_json::Value;

use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{
    avatar, cards as c, colour, shell, size, space, status_colour, status_label, text, theme,
    tokens, viz, widgets as w,
};
use crate::desktop::App;

const HOME: &str = "home";
/// The table's rows: the whole workspace, unfiltered.
const TASKS: &str = "home:tasks";

/// `App` owns no home state, and this view is the only thing that reads these
/// filters, so they live in egui's temp store rather than growing the struct.
const FILTERS: &str = "home:filters";
/// The table's id: its scroll salt, and the slot its hover is tracked in.
const TABLE: &str = "home:table";

/// Last frame's tallest figure, so all four cards agree on a height.
const VIZ_H: &str = "home:viz-h";

/// Days in the completed chart. Seven, because the label says week.
const WEEK: usize = 7;

/// The status vocabulary, in the order it reads in the filter menu: the two
/// tracks' happy paths first, then the two states either track can land in.
/// `status_label` turns each one into words.
const STATUSES: [&str; 7] =
    ["open", "in_progress", "handoff", "completed", "shipped", "blocked", "dropped"];

/// The donut's slices, finished-first so the ring fills clockwise from the
/// outcome you want. `dropped` is absent on purpose: abandoned work is not
/// work, and counting it made the centre percentage a share of a total nobody
/// intends to finish.
const DONUT: [&str; 6] = ["shipped", "completed", "in_progress", "handoff", "blocked", "open"];

/// Table geometry. Fixed so the columns line up with the header and with each
/// other; the task column takes whatever is left. Alignment is declared here
/// too, so "Updated" and the age beneath it cannot disagree.
const COL_DOT: f32 = 22.0;
const COL_DISCIPLINE: f32 = 88.0;
const COL_STATUS: f32 = 104.0;
const COL_PROJECT: f32 = 150.0;
const COL_PHASE: f32 = 120.0;
const COL_OWNER: f32 = 70.0;
const COL_UPDATED: f32 = 78.0;

const COLS: [Col; 8] = [
    Col::left("", COL_DOT),
    Col::fill("Task", COL_PROJECT),
    Col::left("Discipline", COL_DISCIPLINE).rank(2),
    Col::left("Status", COL_STATUS),
    Col::left("Project", COL_PROJECT),
    // Phase goes first: it is one server-managed value on every row today.
    Col::left("Phase", COL_PHASE).rank(4),
    Col::left("Owner", COL_OWNER),
    Col::right("Updated", COL_UPDATED).rank(3),
];

/// What the table is filtered to. Every field is "no filter" when unset, so
/// `Default` is the unfiltered view.
#[derive(Clone, Default, PartialEq)]
pub struct State {
    /// Free text, matched against title, project and phase.
    pub query: String,
    /// Only tasks assigned to me.
    pub mine: bool,
    pub discipline: Option<String>,
    pub status: Option<String>,
    /// A project id, not a name.
    pub project: Option<String>,
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let my_person_id = me_str(app, "personId");

    let filters_id = egui::Id::new(FILTERS);
    let mut state: State = ui.ctx().data_mut(|d| d.get_temp(filters_id)).unwrap_or_default();

    let net = app.net.as_mut().unwrap();

    net.get_once(HOME, "/api/user/home");
    net.get_once(TASKS, "/api/user/tasks");

    let loading = net.is_loading(HOME) || net.is_loading(TASKS);
    let error = net.error(HOME).or_else(|| net.error(TASKS)).map(str::to_owned);
    let home = net.data(HOME).cloned().unwrap_or(Value::Null);
    let tasks = net.data(TASKS).cloned().unwrap_or(Value::Null);

    let projects = list(&home, "projects");
    let team = list(&home, "team");

    // Every task in the workspace, in the order the server sorted them.
    // Newest first. The server's order is its own business; the list should
    // not reshuffle when a filter narrows it.
    let mut rows: Vec<&Value> = tasks.as_array().map(|a| a.iter().collect()).unwrap_or_default();
    rows.sort_by(|a, b| {
        str_at(b, "createdAt").unwrap_or_default().cmp(str_at(a, "createdAt").unwrap_or_default())
    });

    // With the whole workspace here, a blocker's title resolves whenever the
    // blocking task still exists.
    let known: HashMap<&str, &str> = rows
        .iter()
        .filter_map(|t| Some((str_at(t, "id")?, str_at(t, "title")?)))
        .collect();
    let owners: HashMap<&str, &str> = team
        .iter()
        .filter_map(|p| Some((str_at(p, "personId")?, str_at(p, "email")?)))
        .collect();

    // The subtitle counts the same list the donut and the table count, so the
    // three numbers cannot drift apart. The projects rollup carries its own
    // `total`, but it is a second measurement of the same thing and only the
    // list can be clicked into.
    shell::page_title(
        ui,
        "Overview",
        &format!(
            "{} across {}",
            plural(rows.len(), "task"),
            plural(projects.len(), "project")
        ),
        |_| {},
    );


    if let Some(err) = &error {
        w::error(ui, err);
        return;
    }
    if loading && rows.is_empty() {
        w::loading(ui, "Loading");
        return;
    }

    // ---- the four figures, all the same height
    viz::row(
        ui,
        egui::Id::new(VIZ_H),
        &mut [
            &mut |ui: &mut egui::Ui, h| status_card(ui, h, &rows),
            &mut |ui: &mut egui::Ui, h| discipline_card(ui, h, &projects),
            &mut |ui: &mut egui::Ui, h| completed_card(ui, h, &rows),
            &mut |ui: &mut egui::Ui, h| team_card(ui, h, &team),
        ],
    );
    ui.add_space(space::XL);

    // The count is drawn from last frame's filter state; a click repaints, so
    // the lag is never seen.
    let shown: Vec<&Value> = rows
        .iter()
        .copied()
        .filter(|t| keep(t, &state, &my_person_id))
        .collect();
    filter_bar(ui, &mut state, &rows, &projects, shown.len());
    ui.ctx().data_mut(|d| d.insert_temp(filters_id, state));

    let mut open_task: Option<String> = None;

    if shown.is_empty() {
        w::empty(
            ui,
            "Nothing matches those filters.",
            "Clear one of them to see more.",
        );
    } else {
        table(ui, &shown, &known, &owners, &mut open_task);
    }
    ui.add_space(space::XXL);

    if let Some(id) = open_task {
        app.task = Some(id);
    }
}

// ------------------------------------------------------------------- figures

/// Status as a ring. The centre carries the only number worth reading from
/// across the room: how much of this is finished.
fn status_card(ui: &mut egui::Ui, min_body: f32, rows: &[&Value]) -> f32 {
    // Dropped work is off the board, so it is off the ring and out of the
    // denominator too — otherwise the percentage measures the wrong pile.
    let live: Vec<&&Value> = rows.iter().filter(|t| bucket(t) != "dropped").collect();
    let count = |s: &str| live.iter().filter(|t| bucket(t) == s).count();
    let total = live.len();
    // `doneAt` is the server's one answer for both tracks: design ends at
    // completed, engineering at shipped, and only the terminal state stamps it.
    let done = live.iter().filter(|t| finished(t)).count();
    let pct = if total == 0 { 0 } else { done * 100 / total };

    viz::card(ui, "Status", &plural(total, "task"), min_body, |ui| {
        let slices: Vec<viz::Slice<'_>> = DONUT
            .iter()
            .zip(LABELS)
            .map(|(status, label)| viz::Slice {
                label,
                count: count(status),
                colour: donut_tint(status),
            })
            .collect();
        viz::donut(ui, &slices, &format!("{pct}%"), "DONE");
    })
}

/// Sentence-cased status names, positionally matched to `DONUT`. Built once
/// rather than capitalising in the render loop.
const LABELS: [&str; 6] =
    ["Shipped", "Completed", "In progress", "Handoff", "Blocked", "Open"];

/// Progress per discipline, from the server's per-project rollup — the one
/// number on this page that covers every task, not just the ones on screen.
fn discipline_card(ui: &mut egui::Ui, min_body: f32, projects: &[&Value]) -> f32 {
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

    // Whatever the disciplines do not account for is unlabelled work. It is
    // real, so it gets a row rather than quietly vanishing from the total.
    let labelled: i64 = order.iter().filter_map(|d| tally.get(d)).map(|(_, t)| t).sum();
    let labelled_done: i64 = order.iter().filter_map(|d| tally.get(d)).map(|(d, _)| d).sum();
    if all_total > labelled {
        order.push("none".to_owned());
        tally.insert("none".to_owned(), (all_done - labelled_done, all_total - labelled));
    }

    viz::card(ui, "By discipline", "done / total", min_body, |ui| {
        for name in order.iter().take(viz::MAX_BARS) {
            let (done, total) = tally.get(name).copied().unwrap_or((0, 0));
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
        overflow(ui, order.len());
    })
}

/// Completions per day for the last week, bucketed on `doneAt` — the moment
/// the task reached the end of its track, not the last time anyone touched it.
/// Design ends at completed and engineering at shipped; the stamp knows. The delta
/// compares the same measure over the previous week.
fn completed_card(ui: &mut egui::Ui, min_body: f32, rows: &[&Value]) -> f32 {
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

    // Day initials, oldest bucket first, so the last column is today.
    let names: Vec<String> = (0..WEEK)
        .map(|i| {
            let day = today - chrono::Duration::days((WEEK - 1 - i) as i64);
            initial(day.weekday()).to_owned()
        })
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();

    viz::card(ui, "Completed", "last 7 days", min_body, |ui| {
        viz::headline(ui, &total.to_string(), &delta);
        ui.add_space(space::MD);
        viz::columns(ui, &buckets, &names, colour::ACCENT);
    })
}

fn initial(day: chrono::Weekday) -> &'static str {
    ["M", "T", "W", "T", "F", "S", "S"][day.num_days_from_monday() as usize]
}

/// Who is carrying what. These are real counts from the server's grouped
/// query, including `blocking` — the number that says who to go and unblock.
fn team_card(ui: &mut egui::Ui, min_body: f32, team: &[&Value]) -> f32 {
    let peak = team.iter().map(|p| num(p, "open")).max().unwrap_or(0);

    viz::card(ui, "Team load", "open", min_body, |ui| {
        for p in team.iter().take(viz::MAX_BARS) {
            let open = num(p, "open");
            let blocking = num(p, "blocking");
            let fill = if peak == 0 { 0.0 } else { open as f32 / peak as f32 };
            viz::bar_row(
                ui,
                first_name(str_at(p, "name").unwrap_or_default()),
                fill,
                if blocking > 0 { colour::DANGER } else { colour::ACCENT },
                &open.to_string(),
                tokens::DISCIPLINE_W,
            );
        }

        overflow(ui, team.len());

        let note: Vec<String> = team
            .iter()
            .filter(|p| num(p, "blocking") > 0)
            .map(|p| {
                format!(
                    "{} is blocking {}",
                    first_name(str_at(p, "name").unwrap_or_default()),
                    plural(num(p, "blocking") as usize, "task")
                )
            })
            .collect();
        if !note.is_empty() {
            w::caption(ui, &note.join(", "));
        }
    })
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
    rows: &[&Value],
    projects: &[&Value],
    shown: usize,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        viz::search(ui, "Search tasks", &mut state.query);

        if viz::filter(ui, "Mine", state.mine, false).clicked() {
            state.mine = !state.mine;
        }

        // Only disciplines that actually appear: a menu of empty filters is a
        // menu of ways to get an empty table.
        let mut disciplines: Vec<&str> = rows
            .iter()
            .filter_map(|t| str_at(t, "discipline"))
            .filter(|d| !d.is_empty())
            .collect();
        disciplines.sort_unstable();
        disciplines.dedup();
        let options: Vec<(String, String)> =
            disciplines.iter().map(|d| ((*d).to_owned(), (*d).to_owned())).collect();
        viz::select(ui, "All disciplines", &options, &mut state.discipline);

        let options: Vec<(String, String)> = STATUSES
            .iter()
            .map(|s| ((*s).to_owned(), sentence(status_label(s))))
            .collect();
        viz::select(ui, "Any status", &options, &mut state.status);

        let names: Vec<(String, String)> = projects
            .iter()
            .filter_map(|p| Some((str_at(p, "id")?.to_owned(), str_at(p, "name")?.to_owned())))
            .collect();
        viz::select(ui, "All projects", &names, &mut state.project);

        if *state != State::default() && viz::clear(ui).clicked() {
            *state = State::default();
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // The count is the number the bar exists to move, so it is the one
            // thing on this strip in full-strength ink.
            ui.label(
                RichText::new(format!("of {}", rows.len()))
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            );
            ui.add_space(space::XXS);
            ui.label(
                RichText::new(shown.to_string())
                    .size(text::SMALL)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
        });
    });
    ui.add_space(space::MD);
}

fn keep(t: &Value, state: &State, my_person_id: &str) -> bool {
    if state.mine
        && !(str_at(t, "assigneeKind") == Some("person")
            && str_at(t, "assigneePersonId") == Some(my_person_id)
            && !my_person_id.is_empty())
    {
        return false;
    }
    if let Some(d) = &state.discipline {
        if str_at(t, "discipline") != Some(d.as_str()) {
            return false;
        }
    }
    if let Some(s) = &state.status {
        if bucket(t) != s.as_str() {
            return false;
        }
    }
    if let Some(p) = &state.project {
        if str_at(t, "projectId") != Some(p.as_str()) {
            return false;
        }
    }
    // Title, project and phase: the three things you would think to type.
    // Not the status — that is what the menu beside the box is for.
    viz::matches(
        &state.query,
        &[
            str_at(t, "title").unwrap_or_default(),
            str_at(t, "projectName").unwrap_or_default(),
            str_at(t, "phaseName").unwrap_or_default(),
        ],
    )
}

// --------------------------------------------------------------------- table

#[allow(clippy::too_many_arguments)]
fn table(
    ui: &mut egui::Ui,
    rows: &[&Value],
    known: &HashMap<&str, &str>,
    owners: &HashMap<&str, &str>,
    open_task: &mut Option<String>,
) {
    let clicked = table::show(ui, TABLE, &COLS, rows.len(), |row, i| {
        task_row(row, rows[i], known, owners);
    });
    if let Some(i) = clicked {
        *open_task = str_at(rows[i], "id").map(str::to_owned);
    }
}

fn task_row(
    row: &mut table::Cells<'_, '_, '_>,
    t: &Value,
    known: &HashMap<&str, &str>,
    owners: &HashMap<&str, &str>,
) {
    let status = bucket(t);
    let discipline = str_at(t, "discipline").unwrap_or_default();

    row.at(0, |ui| w::dot(ui, status_colour(status)));

    row.at(1, |ui| {
        table::strong_label(ui, str_at(t, "title").unwrap_or_default(), colour::TEXT);
        // The blocker rides behind the title rather than in its own column:
        // it is a footnote on the task, not a property every row has.
        if status == "blocked" {
            ui.add_space(space::XS);
            w::caption(ui, &format!("\u{21b3} waiting on {}", blocker(t, known)));
        }
    });

    row.at(2, |ui| {
        if !discipline.is_empty() {
            c::chip(ui, discipline, c::discipline_tone(discipline), false);
        }
    });
    row.at(3, |ui| {
        c::chip(ui, &sentence(status_label(status)), c::status_tone(status), status != "blocked");
    });
    row.muted(4, str_at(t, "projectName").unwrap_or_default());
    row.muted(5, str_at(t, "phaseName").unwrap_or_default());

    row.at(6, |ui| match owner(t, owners) {
        Some(seed) => {
            avatar::small(ui, &seed, size::AVATAR_SM);
        }
        None => w::caption(ui, "\u{2014}"),
    });

    row.muted(7, &age(str_at(t, "updatedAt").unwrap_or_default()));
}

/// The avatar seed for whoever holds the task: a teammate's email, or the
/// agent's own name. `None` means nobody has it.
fn owner(t: &Value, owners: &HashMap<&str, &str>) -> Option<String> {
    match str_at(t, "assigneeKind") {
        Some("person") => owners
            .get(str_at(t, "assigneePersonId")?)
            .map(|email| (*email).to_owned()),
        Some("agent") => Some(str_at(t, "claimedBy").unwrap_or("agent").to_owned()),
        _ => None,
    }
}

/// The blocker's title when it is in this payload, otherwise an honest count.
fn blocker(t: &Value, known: &HashMap<&str, &str>) -> String {
    let named = t
        .get("blockedBy")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|id| known.get(id).copied());
    match named {
        Some(title) => title.to_owned(),
        None => plural(outstanding(t) as usize, "other task"),
    }
}

// -------------------------------------------------------------------- pieces

/// The state a row is filed under. Unfinished blockers beat the status field:
/// a task marked `in_progress` that is waiting on someone else is not in
/// progress, and filing it as though it were hides the thing to go and unblock.
fn bucket(t: &Value) -> &'static str {
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

/// The donut's hue per status. Deliberately not `status_colour`: a ring needs
/// its unfinished remainder to read as the track, so "open" is a line colour.
/// The rest match the chips, so a slice and a row agree on what green means.
fn donut_tint(status: &str) -> Color32 {
    match status {
        "shipped" => colour::OK,
        "completed" => colour::INFO,
        "in_progress" => colour::ACCENT,
        "handoff" => colour::AGENT,
        "blocked" => colour::DANGER,
        _ => colour::LINE_STRONG,
    }
}

/// A discipline's bar colour, matching the chip it wears everywhere else.
fn discipline_tint(discipline: &str) -> Color32 {
    match c::discipline_tone(discipline) {
        c::Tone::Agent => colour::AGENT,
        c::Tone::Info => colour::INFO,
        c::Tone::Ok => colour::OK,
        _ => colour::LINE_STRONG,
    }
}

/// "In progress" from "in progress". The vocabulary still comes from
/// `status_label`; this only decides where the sentence starts.
fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
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
