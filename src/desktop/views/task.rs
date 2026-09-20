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

use serde_json::{json, Value};

use crate::desktop::{theme, App};

const LOG_KEY: &str = "task:logs";
const PATCH_KEY: &str = "task:patch";
const LIST_KEY: &str = "task:all";
const ARTIFACTS_KEY: &str = "task:artifacts";

/// How far out we ask egui to wake us.
const POLL: Duration = Duration::from_secs(2);
/// Fire slightly early: a repaint scheduled for +2s can land a hair under it,
/// and a strict `>= POLL` test would then skip a tick and halve the cadence.
const DUE: Duration = Duration::from_millis(1_900);

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
            waiting(ui, "Loading task");
        } else {
            waiting(ui, "That task is no longer in the list");
        }
        return;
    };

    let status = str_of(&task, "status").unwrap_or("open").to_string();

    header(ui, &task, &status);
    body(ui, &task);

    if can_write {
        status_controls(ui, net, task_id, &status, local);
    }
    if let Some((text, colour)) = &local.notice {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(text).size(12.0).color(*colour));
    }

    artifacts(ui, net);
    run_log(ui, net, task_id, &status, local);
}

// ---------------------------------------------------------------- chrome bits

fn back_row(app: &mut App, ui: &mut egui::Ui, task_id: &str) {
    ui.horizontal(|ui| {
        if ui.small_button("< Board").clicked() {
            app.task = None;
        }
        ui.add_space(4.0);
        theme::id_label(ui, task_id);
    });
    ui.add_space(14.0);
}

fn header(ui: &mut egui::Ui, task: &Value, status: &str) {
    card(ui, |ui| {
        ui.label(
            egui::RichText::new(str_of(task, "title").unwrap_or("Untitled"))
                .size(21.0)
                .strong(),
        );
        ui.add_space(12.0);

        ui.horizontal_wrapped(|ui| {
            theme::pill(ui, &status.replace('_', " "), theme::status(status));

            let kind = str_of(task, "assigneeKind");
            let is_agent = kind == Some("agent");
            let who = str_of(task, "claimedBy")
                .or(kind)
                .unwrap_or("unassigned")
                .to_string();
            let colour = if is_agent {
                theme::AGENT
            } else if kind.is_some() {
                theme::ACCENT
            } else {
                theme::MUTED
            };
            theme::pill(ui, &who, colour);

            if let Some(p) = task.get("priority").and_then(Value::as_i64) {
                let colour = if p <= 1 { theme::WARN } else { theme::MUTED };
                theme::pill(ui, &format!("priority {p}"), colour);
            }
        });

        if let Some(updated) = str_of(task, "updatedAt") {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!("updated {}", stamp(updated)))
                    .size(11.0)
                    .color(theme::MUTED),
            );
        }
    });
}

fn body(ui: &mut egui::Ui, task: &Value) {
    let text = str_of(task, "body").unwrap_or("").trim();
    if text.is_empty() {
        return;
    }
    section(ui, "Description");
    card(ui, |ui| {
        ui.label(egui::RichText::new(text).size(13.0));
    });
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
                    theme::WARN,
                ),
                Ok(_) => ("Status updated.".to_string(), theme::OK),
                Err(e) => (format!("Could not move the task: {e}"), theme::DANGER),
            });
            local.patching = false;
            // The task list, the log and the board all show the old status.
            net.invalidate_prefix("task:");
            net.invalidate_prefix("board:");
        }
    }

    section(ui, "Move to");
    ui.horizontal_wrapped(|ui| {
        for next in ["open", "in_progress", "in_review", "done"] {
            let here = next == status;
            let label = egui::RichText::new(next.replace('_', " "))
                .size(12.0)
                .color(if here { theme::MUTED } else { theme::status(next) });
            let button = egui::Button::new(label);
            if ui
                .add_enabled(!here && !local.patching, button)
                .clicked()
            {
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
            ui.add_space(6.0);
            ui.add(egui::Spinner::new().size(13.0));
        }
    });
}

fn artifacts(ui: &mut egui::Ui, net: &crate::desktop::net::Net) {
    section(ui, "Artifacts");

    if let Some(err) = net.error(ARTIFACTS_KEY) {
        failed(ui, "Could not load artifacts", err);
        return;
    }
    let Some(rows) = net.data(ARTIFACTS_KEY).and_then(Value::as_array) else {
        waiting(ui, "Loading artifacts");
        return;
    };
    if rows.is_empty() {
        empty(ui, "Nothing attached yet");
        return;
    }

    card(ui, |ui| {
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                ui.add_space(7.0);
            }
            ui.horizontal(|ui| {
                theme::pill(ui, str_of(row, "kind").unwrap_or("link"), theme::MUTED);
                ui.add_space(4.0);
                let url = str_of(row, "url").unwrap_or_default();
                let title = match str_of(row, "title").map(str::trim) {
                    Some(t) if !t.is_empty() => t,
                    _ => url,
                };
                ui.hyperlink_to(egui::RichText::new(title).size(13.0), url)
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

    ui.add_space(20.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("RUN LOG")
                .size(10.0)
                .strong()
                .color(theme::MUTED),
        );
        if live {
            ui.add_space(6.0);
            theme::pill(ui, "live", theme::ACCENT);
        }
    });
    ui.add_space(6.0);

    if let Some(err) = &local.log_error {
        failed(ui, "Could not load the run log", err);
    }

    egui::Frame::new()
        .fill(theme::TEXT)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(14, 12))
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
                        .size(12.0)
                        .color(theme::MUTED),
                );
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("task:log:scroll")
                .max_height(340.0)
                .stick_to_bottom(true)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    for (seq, text) in &local.lines {
                        ui.horizontal_top(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{seq:>4}"))
                                    .monospace()
                                    .size(11.5)
                                    .color(theme::MUTED),
                            );
                            ui.add_space(6.0);
                            ui.add(egui::Label::new(
                                egui::RichText::new(text)
                                    .monospace()
                                    .size(11.5)
                                    .color(theme::BG),
                            ));
                        });
                    }
                });
        });
}

// --------------------------------------------------------------------- shared

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(8)
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(20.0);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .size(10.0)
            .strong()
            .color(theme::MUTED),
    );
    ui.add_space(6.0);
}

fn waiting(ui: &mut egui::Ui, what: &str) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(13.0));
        ui.label(egui::RichText::new(what).size(12.0).color(theme::MUTED));
    });
}

fn empty(ui: &mut egui::Ui, what: &str) {
    ui.label(egui::RichText::new(what).size(12.0).color(theme::MUTED));
}

fn failed(ui: &mut egui::Ui, what: &str, err: &str) {
    card(ui, |ui| {
        ui.label(egui::RichText::new(what).size(13.0).color(theme::DANGER));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(err).size(11.0).color(theme::MUTED));
    });
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
