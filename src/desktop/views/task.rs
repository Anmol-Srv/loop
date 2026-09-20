//! Task detail: the page a human sits on while an agent works.
//!
//! The run log is the centre of it, so the fetch discipline matters more than
//! the layout. Two rules shape this file:
//!
//! * A finished task must not poll. Polling is gated on `status ==
//!   "in_progress"`, and the tick comes from `request_repaint_after`, so egui
//!   goes back to sleep between ticks instead of spinning at frame rate.
//! * Log lines live here, not in the net cache, because the incremental
//!   `afterSeq` fetch returns only the tail. The cache holds one reply; we hold
//!   the transcript.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use egui_phosphor::thin as icon;
use serde_json::{json, Value};

use crate::desktop::design::{
    avatar, colour, pad, radius, shell, size, space, status_colour, status_label, text,
    widgets as w,
};
use crate::desktop::App;

const LOG_KEY: &str = "task:logs";
const PATCH_KEY: &str = "task:patch";
const LIST_KEY: &str = "task:all";
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
const AVATAR: f32 = space::XL;

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
    notice: Option<(String, egui::Color32)>,
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
    let can_write = app.can_write();

    LOCAL.with(|cell| {
        let mut slot = cell.borrow_mut();
        // Opening a different task: drop the transcript and the per-task caches.
        if slot.as_ref().map(|s| s.task_id != task_id).unwrap_or(true) {
            *slot = Some(Local::new(task_id.clone()));
            if let Some(net) = app.net.as_mut() {
                net.invalidate(LOG_KEY);
                net.invalidate(ARTIFACTS_KEY);
                net.invalidate(PATCH_KEY);
            }
        }
        let local = slot.as_mut().expect("just populated");
        render(app, ui, &task_id, can_write, local);
    });
}

fn render(app: &mut App, ui: &mut egui::Ui, task_id: &str, can_write: bool, local: &mut Local) {
    let net = app.net.as_mut().expect("net is live whenever a view runs");

    // There is no get-task-by-id endpoint, so we read the full list and pick
    // ours out of it. The list is shared with the board, hence a stable key.
    net.get_once(LIST_KEY, "/api/user/tasks");
    net.get_once(
        ARTIFACTS_KEY,
        &format!("/api/user/artifacts?parentType=task&parentId={task_id}"),
    );

    let task = net
        .data(LIST_KEY)
        .and_then(Value::as_array)
        .and_then(|rows| rows.iter().find(|t| str_of(t, "id") == Some(task_id)))
        .cloned();

    back_row(app, ui, task_id);

    let net = app.net.as_mut().expect("net is live whenever a view runs");
    let Some(task) = task else {
        if let Some(err) = net.error(LIST_KEY) {
            failed(ui, "Could not load the task", err);
        } else if net.is_loading(LIST_KEY) {
            w::loading(ui, "Loading task");
        } else {
            w::empty(ui, "That task is no longer in the list");
        }
        return;
    };

    let status = str_of(&task, "status").unwrap_or("open").to_string();

    header(ui, &task, &status);
    body(ui, &task);

    if can_write {
        status_controls(ui, net, task_id, &status, local);
    }
    if let Some((message, colour)) = &local.notice {
        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new(message)
                .size(text::SMALL)
                .color(*colour),
        );
    }

    artifacts(ui, net);
    run_log(ui, net, task_id, &status, local);
}

// ---------------------------------------------------------------- chrome bits

fn back_row(app: &mut App, ui: &mut egui::Ui, task_id: &str) {
    ui.horizontal(|ui| {
        if w::link(ui, &format!("{} Board", icon::ARROW_LEFT)).clicked() {
            app.task = None;
        }
        ui.add_space(space::XS);
        w::id(ui, task_id);
    });
    ui.add_space(space::MD);
}

fn header(ui: &mut egui::Ui, task: &Value, status: &str) {
    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        w::title(ui, str_of(task, "title").unwrap_or("Untitled"));
        ui.add_space(space::MD);

        ui.horizontal_wrapped(|ui| {
            w::pill(ui, status_label(status), status_colour(status));

            let kind = str_of(task, "assigneeKind");
            let is_agent = kind == Some("agent");
            let claimed_by = str_of(task, "claimedBy");
            let who = claimed_by.or(kind).unwrap_or("unassigned").to_string();

            // An agent-held task gets a face: the run log below is that
            // agent's output, and the two should read as one thing.
            if is_agent {
                if let Some(seed) = claimed_by {
                    ui.add_space(space::XS);
                    avatar::small(ui, seed, AVATAR);
                    ui.add_space(space::XS);
                }
            }

            let colour = if is_agent {
                colour::AGENT
            } else if kind.is_some() {
                colour::ACCENT
            } else {
                colour::TEXT_MUTED
            };
            w::pill(ui, &who, colour);

            if let Some(p) = task.get("priority").and_then(Value::as_i64) {
                let tint = if p <= 1 { colour::WARN } else { colour::TEXT_MUTED };
                w::pill(ui, &format!("priority {p}"), tint);
            }
        });

        if let Some(updated) = str_of(task, "updatedAt") {
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new(format!("{} updated {}", icon::CLOCK, stamp(updated)))
                    .size(text::CAPTION)
                    .color(colour::TEXT_FAINT),
            );
        }
    });
}

fn body(ui: &mut egui::Ui, task: &Value) {
    let description = str_of(task, "body").unwrap_or("").trim();
    if description.is_empty() {
        return;
    }
    shell::section(ui, "Description");
    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        w::body(ui, description);
    });
}

/// The move a human is most likely to make from here. It gets the one filled
/// button on the screen; everything else stays outlined.
fn likely_next(status: &str) -> &'static str {
    match status {
        "in_progress" => "in_review",
        "in_review" => "done",
        "done" => "open",
        _ => "in_progress",
    }
}

fn status_controls(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    status: &str,
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
                    colour::WARN,
                ),
                Ok(_) => ("Status updated.".to_string(), colour::OK),
                Err(e) => (format!("Could not move the task: {e}"), colour::DANGER),
            });
            local.patching = false;
            // The task list, the log and the board all show the old status.
            net.invalidate_prefix("task:");
            net.invalidate_prefix("board:");
        }
    }

    shell::section(ui, "Move to");
    let suggested = likely_next(status);
    ui.horizontal_wrapped(|ui| {
        for next in ["open", "in_progress", "in_review", "done"] {
            let here = next == status;
            let enabled = !here && !local.patching;
            let label = status_label(next);
            let clicked = if next == suggested && enabled {
                w::primary(ui, label, true).clicked()
            } else {
                w::secondary(ui, label, enabled).clicked()
            };
            if clicked {
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
        w::empty(ui, "Nothing attached yet");
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

    // A section heading with a pill beside it. `shell::section` owns the label
    // alone, so the row is hand-painted here in the same type and colour.
    ui.add_space(space::LG);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Run log")
                .size(text::SMALL)
                .family(egui::FontFamily::Name(
                    crate::desktop::design::theme::MEDIUM.into(),
                ))
                .color(colour::TEXT_MUTED),
        );
        if live {
            ui.add_space(space::SM);
            w::pill(ui, "live", colour::ACCENT);
        }
    });
    ui.add_space(space::SM);

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

/// `2026-09-20T11:04:09.123Z` reads better as `2026-09-20 11:04`. ponytail: a
/// string trim, not a chrono parse; nobody needs the local-time conversion to
/// see how stale a task is.
fn stamp(raw: &str) -> String {
    raw.get(..16).unwrap_or(raw).replace('T', " ")
}
