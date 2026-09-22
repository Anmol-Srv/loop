//! Task detail: the page a human sits on while the work moves.
//!
//! A task belongs to one person, and that person moves it through its life.
//! The server enforces it — `PATCH /api/user/tasks/{id}` answers 403 to anyone
//! who is neither the assignee nor an admin — so this screen shows the moves
//! only to whoever can actually make them, and tells everyone else whose call
//! it is. Offering a button that is going to 403 is worse than offering none.
//!
//! The run log is the other half of the page, so the fetch discipline matters
//! as much as the layout. Two rules shape it:
//!
//! * A finished task must not poll. Polling is gated on `status ==
//!   "in_progress"`, and the tick comes from `request_repaint_after`, so egui
//!   goes back to sleep between ticks instead of spinning at frame rate.
//! * Log lines live here, not in the net cache, because the incremental
//!   `afterSeq` fetch returns only the tail. The cache holds one reply; we hold
//!   the transcript.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use egui::RichText;
use egui_phosphor::thin as icon;
use serde_json::{json, Value};

use super::projects::PROSE_W;
use crate::desktop::design::{
    avatar, cards as c, colour, pad, radius, shell, size, space, status_label, text, theme,
    widgets as w,
};
use crate::desktop::{App, Tab};

const TASK_KEY: &str = "task:one";
const LOG_KEY: &str = "task:logs";
const PATCH_KEY: &str = "task:patch";
const ARTIFACTS_KEY: &str = "task:artifacts";

/// How far out we ask egui to wake us.
const POLL: Duration = Duration::from_secs(2);
/// Fire slightly early: a repaint scheduled for +2s can land a hair under it,
/// and a strict `>= POLL` test would then skip a tick and halve the cadence.
const DUE: Duration = Duration::from_millis(1_900);

/// The log well, in rows rather than pixels — tall enough to watch a run, short
/// enough that the controls above it stay on screen.
const LOG_ROWS: f32 = 11.0;

/// The assignee disc. Sized off the spacing scale so it lines up with the pills
/// beside it instead of inventing a diameter.
const AVATAR: f32 = size::AVATAR_SM;

/// Everything the detail view remembers between frames. Scoped to one task id;
/// opening a different task resets it.
struct Local {
    task_id: String,
    lines: Vec<(i64, String)>,
    /// `afterSeq` for the next request: the highest seq we already hold.
    next_seq: i64,
    fired_at: Instant,
    /// A log request is out and its reply has not been folded in yet.
    pending: bool,
    log_error: Option<String>,
    patching: bool,
    /// The last move's outcome: the message, and whether it failed. A failure
    /// is the server's own sentence — it is the only thing that explains a 403.
    notice: Option<(String, bool)>,
}

impl Local {
    fn new(task_id: String) -> Self {
        Self {
            task_id,
            lines: Vec::new(),
            next_seq: 0,
            fired_at: Instant::now(),
            pending: false,
            log_error: None,
            patching: false,
            notice: None,
        }
    }
}

thread_local! {
    static LOCAL: RefCell<Option<Local>> = const { RefCell::new(None) };
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let Some(task_id) = app.task.clone() else { return };

    LOCAL.with(|cell| {
        let mut slot = cell.borrow_mut();
        // Opening a different task: drop the transcript and the per-task caches.
        if slot.as_ref().map(|s| s.task_id != task_id).unwrap_or(true) {
            *slot = Some(Local::new(task_id.clone()));
            if let Some(net) = app.net.as_mut() {
                net.invalidate_prefix("task:");
            }
        }
        let local = slot.as_mut().expect("just populated");
        render(app, ui, &task_id, local);
    });
}

fn render(app: &mut App, ui: &mut egui::Ui, task_id: &str, local: &mut Local) {
    // Who is looking. The assignee moves the task; an admin overrides, which is
    // what an admin is for. Read before `net` is borrowed mutably below.
    let me = me_str(app, "personId");
    let admin = me_str(app, "role") == "admin";

    let net = app.net.as_mut().expect("net is live whenever a view runs");
    net.get_once(TASK_KEY, &format!("/api/user/tasks/{task_id}"));
    net.get_once(
        ARTIFACTS_KEY,
        &format!("/api/user/artifacts?parentType=task&parentId={task_id}"),
    );

    let task = net.data(TASK_KEY).cloned();

    let mut leave = false;
    ui.horizontal(|ui| {
        if shell::back(ui, "Back").clicked() {
            leave = true;
        }
        w::id(ui, task_id);
    });
    if leave {
        app.task = None;
        return;
    }
    ui.add_space(space::XS);

    let net = app.net.as_mut().expect("net is live whenever a view runs");
    let Some(task) = task else {
        if let Some(err) = net.error(TASK_KEY) {
            failed(ui, "Could not load the task", err);
        } else if net.is_loading(TASK_KEY) {
            w::loading(ui, "Loading task");
        } else {
            w::empty(ui, "That task is no longer here", "It may have been dropped or moved.");
        }
        return;
    };

    let status = str_of(&task, "status").unwrap_or("open").to_string();
    let mine = !me.is_empty() && str_of(&task, "assigneePersonId") == Some(me.as_str());

    heading(ui, &task, &status);
    let open_project = meta_line(ui, &task);
    description(ui, &task);
    actions(ui, net, task_id, &task, &status, admin || mine, local);
    artifacts(ui, net);
    run_log(ui, net, task_id, &status, local);

    // `net`'s borrow of `app` ends above, so navigation happens last.
    if let Some(project_id) = open_project {
        app.task = None;
        app.project = Some(project_id);
        app.tab = Tab::Projects;
    }
}

// ---------------------------------------------------------------- the top

/// The title, with the state it is in beside it. No card: this is the page's
/// own heading, and a box around a heading is a box around nothing.
fn heading(ui: &mut egui::Ui, task: &Value, status: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(str_of(task, "title").unwrap_or("Untitled"))
                .size(text::TITLE)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        ui.add_space(space::SM);
        c::chip(ui, &sentence(status_label(status)), c::status_tone(status), true);
    });
    ui.add_space(space::XXS);
}

/// Everything a task *is*, on one line: where it lives, who holds it, how
/// urgent, what kind of work, how old.
///
/// A line, not a card and not a label grid. Six facts in a two-column table
/// would be the tallest thing on the page and the least read; set out along
/// one line, a fact's width apart, they are one glance. Returns the project id
/// when the project name is clicked.
fn meta_line(ui: &mut egui::Ui, task: &Value) -> Option<String> {
    let mut open_project = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;

        let project = str_of(task, "projectName").unwrap_or_default();
        if !project.is_empty() && w::link(ui, project).clicked() {
            open_project = str_of(task, "projectId").map(str::to_owned);
        }

        ui.add_space(space::XL);
        match str_of(task, "assigneeName") {
            Some(name) if !name.is_empty() => {
                let seed = str_of(task, "assigneeEmail").unwrap_or(name);
                avatar::small(ui, seed, AVATAR);
                value(ui, name);
            }
            _ => label(ui, "Unassigned"),
        }

        if let Some(p) = task.get("priority").and_then(Value::as_i64) {
            ui.add_space(space::XL);
            c::chip(ui, &format!("P{p}"), priority_tone(p), false);
        }

        let discipline = str_of(task, "discipline").unwrap_or_default();
        if !discipline.is_empty() {
            ui.add_space(space::XL);
            w::discipline(ui, discipline);
        }

        if let Some(created) = str_of(task, "createdAt") {
            ui.add_space(space::XL);
            label(ui, "Created");
            value(ui, &ago(created));
        }
        if let Some(done) = str_of(task, "doneAt") {
            ui.add_space(space::XL);
            label(ui, "Done");
            value(ui, &ago(done));
        }
    });
    open_project
}

/// P0 shouts and P4 whispers, in the same chip vocabulary as status — the meta
/// line should read as one row of tokens, not two competing systems.
fn priority_tone(p: i64) -> c::Tone {
    match p {
        0 => c::Tone::Blocked,
        1 => c::Tone::Running,
        2 => c::Tone::Neutral,
        _ => c::Tone::Quiet,
    }
}

fn label(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT_MUTED));
}

fn value(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT));
}

/// The task's own words, at a prose measure. Paragraphs split on a blank line,
/// because that is how whoever filed it typed them.
fn description(ui: &mut egui::Ui, task: &Value) {
    shell::section(ui, "Description");
    let body = str_of(task, "body").unwrap_or("").trim();
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

// ---------------------------------------------------------------- the moves

/// Where a task can go from here, and how loudly each move is offered.
///
/// One filled button per state — the move that is almost always the right one —
/// then the alternatives outlined, then the way out in ghost. The set is
/// deliberately small: a task in review has two honest futures, and a row of
/// six buttons makes you read all six to find them.
fn moves(status: &str) -> &'static [(&'static str, w::Emphasis, &'static str)] {
    match status {
        "open" => &[
            ("Start", w::Emphasis::Primary, "in_progress"),
            ("Close", w::Emphasis::Ghost, "dropped"),
        ],
        "in_progress" => &[
            ("Complete", w::Emphasis::Primary, "done"),
            ("Send to review", w::Emphasis::Secondary, "in_review"),
            ("Close", w::Emphasis::Ghost, "dropped"),
        ],
        "in_review" => &[
            ("Complete", w::Emphasis::Primary, "done"),
            ("Back to work", w::Emphasis::Secondary, "in_progress"),
        ],
        "blocked" => &[
            ("Resume", w::Emphasis::Primary, "in_progress"),
            ("Close", w::Emphasis::Ghost, "dropped"),
        ],
        "done" => &[("Reopen", w::Emphasis::Secondary, "in_progress")],
        "dropped" => &[("Reopen", w::Emphasis::Secondary, "open")],
        _ => &[],
    }
}

fn actions(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    task: &Value,
    status: &str,
    can_act: bool,
    local: &mut Local,
) {
    // Fold in the reply to a move we started earlier.
    if local.patching && !net.is_loading(PATCH_KEY) {
        if let Some(result) = net.peek(PATCH_KEY) {
            local.notice = Some(match result {
                Ok(v) if str_of(v, "status") == Some("proposed") => (
                    "Awaiting approval: you do not hold write on this project, so the \
                     move was recorded as a proposed change."
                        .to_string(),
                    false,
                ),
                Ok(_) => ("Moved.".to_string(), false),
                Err(e) => (e.to_string(), true),
            });
            local.patching = false;
            // This task, the board, the dashboard and the personal list all
            // show the status we have just changed.
            net.invalidate_prefix("task:");
            net.invalidate_prefix("board:");
            net.invalidate_prefix("mytasks");
            net.invalidate("home");
        }
    }

    shell::section(ui, "Actions");

    if !can_act {
        match str_of(task, "assigneeName").filter(|n| !n.is_empty()) {
            Some(name) => w::caption(ui, &format!("Only {name} can move this task.")),
            None => w::caption(ui, "Unassigned \u{2014} nobody can move it yet."),
        }
        return;
    }

    ui.horizontal_wrapped(|ui| {
        for (copy, emphasis, next) in moves(status) {
            if w::button(ui, copy, *emphasis, !local.patching).clicked() {
                net.patch(
                    PATCH_KEY,
                    &format!("/api/user/tasks/{task_id}"),
                    json!({ "status": next }),
                );
                local.patching = true;
                local.notice = None;
            }
        }
        if local.patching {
            ui.add_space(space::SM);
            ui.add(egui::Spinner::new().size(text::BODY));
        }
    });

    if let Some((message, is_error)) = &local.notice {
        ui.add_space(space::SM);
        if *is_error {
            w::error(ui, message);
        } else {
            w::caption(ui, message);
        }
    }
}

fn artifacts(ui: &mut egui::Ui, net: &crate::desktop::net::Net) {
    shell::section(ui, "Artifacts");

    if let Some(err) = net.error(ARTIFACTS_KEY) {
        failed(ui, "Could not load artifacts", err);
        return;
    }
    let Some(rows) = net.data(ARTIFACTS_KEY).and_then(Value::as_array) else {
        w::loading(ui, "Loading artifacts");
        return;
    };
    if rows.is_empty() {
        w::empty(ui, "Nothing attached yet", "Link a PR or a doc with: acp link <task-id> --pr <url>");
        return;
    }

    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                ui.add_space(space::SM);
            }
            ui.horizontal(|ui| {
                w::pill(ui, str_of(row, "kind").unwrap_or("link"), colour::TEXT_MUTED);
                ui.add_space(space::XS);
                let url = str_of(row, "url").unwrap_or_default();
                let title = match str_of(row, "title").map(str::trim) {
                    Some(t) if !t.is_empty() => t,
                    _ => url,
                };
                ui.hyperlink_to(
                    egui::RichText::new(format!("{} {title}", icon::LINK)).size(text::BODY),
                    url,
                )
                .on_hover_text(url);
            });
        }
    });
}

// -------------------------------------------------------------------- run log

fn run_log(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    status: &str,
    local: &mut Local,
) {
    let live = status == "in_progress";

    // 1. Fold in whatever came back, appending to the transcript we hold.
    if local.pending && !net.is_loading(LOG_KEY) {
        match net.peek(LOG_KEY) {
            Some(Ok(v)) => {
                local.log_error = None;
                for row in v.as_array().into_iter().flatten() {
                    let seq = row.get("seq").and_then(Value::as_i64).unwrap_or(0);
                    if seq > local.next_seq {
                        local.next_seq = seq;
                        local
                            .lines
                            .push((seq, str_of(row, "text").unwrap_or_default().to_string()));
                    }
                }
                local.pending = false;
            }
            Some(Err(e)) => {
                local.log_error = Some(e.to_string());
                local.pending = false;
            }
            None => {}
        }
    }

    // 2. Tick. `request_repaint_after` is the whole clock: it wakes the app
    //    once, two seconds out, and nothing else keeps the loop hot. A task
    //    that is not in progress asks for no repaint at all, so the view is
    //    static and free.
    if live {
        ui.ctx().request_repaint_after(POLL);
        if !local.pending && local.fired_at.elapsed() >= DUE {
            net.invalidate(LOG_KEY);
        }
    }

    // 3. Fire, if nothing is cached and nothing is out. Covers both the first
    //    load and the tick above, and cannot double-fire: `pending` is only
    //    cleared by step 1.
    if !local.pending && net.peek(LOG_KEY).is_none() {
        net.get(
            LOG_KEY,
            &format!("/api/user/tasks/{task_id}/logs?afterSeq={}", local.next_seq),
        );
        local.pending = true;
        local.fired_at = Instant::now();
    }

    // The heading carries a live pill, which is what `section_with`'s trailing
    // slot is for — it was hand-painted here before that existed.
    shell::section_with(ui, "Run log", |ui| {
        if live {
            w::pill(ui, "live", colour::ACCENT);
        }
    });

    if let Some(err) = &local.log_error {
        failed(ui, "Could not load the run log", err);
    }

    egui::Frame::new()
        .fill(colour::LOG_BG)
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(
            pad::CARD.0 as i8,
            pad::CARD.1 as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            if local.lines.is_empty() {
                let note = if local.pending {
                    "Loading output"
                } else if live {
                    "Waiting for the agent's first line"
                } else {
                    "No output was recorded"
                };
                ui.label(
                    egui::RichText::new(note)
                        .monospace()
                        .size(text::SMALL)
                        .color(colour::LOG_SEQ),
                );
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("task:log:scroll")
                .max_height(size::ROW * LOG_ROWS)
                .stick_to_bottom(true)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = space::XXS;
                    for (seq, line) in &local.lines {
                        ui.horizontal_top(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{seq:>4}"))
                                    .monospace()
                                    .size(text::SMALL)
                                    .color(colour::LOG_SEQ),
                            );
                            ui.add_space(space::SM);
                            ui.add(egui::Label::new(
                                egui::RichText::new(line)
                                    .monospace()
                                    .size(text::SMALL)
                                    .color(colour::LOG_TEXT),
                            ));
                        });
                    }
                });
        });
}

// --------------------------------------------------------------------- shared

fn failed(ui: &mut egui::Ui, what: &str, err: &str) {
    w::error(ui, &format!("{what}: {err}"));
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
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

/// "3 days ago". The meta line has room for words where a table column has
/// room for "3d", and this is the one place on the screen that reads as prose.
fn ago(raw: &str) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(raw) else {
        return String::new();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => plural(s / 60, "minute"),
        s if s < 86_400 => plural(s / 3600, "hour"),
        s => plural(s / 86_400, "day"),
    }
}

fn plural(n: i64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
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
