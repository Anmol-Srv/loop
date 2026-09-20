//! Home: the one screen that answers "what should I do now?".
//!
//! Everything on it arrives in a single `GET /api/user/home`, cached under the
//! key `"home"` — chrome.rs reads that same key for the My Tasks badge, so the
//! name is load-bearing. Four sections, in the order you act on them: the work
//! that is yours, the decisions waiting on you, the work nobody has taken, and
//! finally where the team is.
//!
//! Mutations (approve, reject, claim) all invalidate `"home"` and the board,
//! because each one changes a row some other view has cached.

use std::collections::HashMap;

use chrono::{DateTime, Local, Timelike, Utc};
use egui::{Align, Layout, RichText};
use egui_phosphor::thin as icon;
use serde_json::Value;

use crate::desktop::design::{
    avatar, colour, size, space, status_colour, status_label, text, widgets as w,
};
use crate::desktop::views::inbox::describe;
use crate::desktop::{App, Tab};

const HOME: &str = "home";
const APPROVE: &str = "home:approve";
const REJECT: &str = "home:reject";
const CLAIM: &str = "home:claim";

/// How many of my tasks the home screen shows before deferring to My Tasks.
const MY_TASKS_PREVIEW: usize = 5;

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let greeting = greeting(app);

    let net = app.net.as_mut().unwrap();

    // A finished mutation has changed rows every other view has cached.
    for key in [APPROVE, REJECT, CLAIM] {
        if net.data(key).is_some() {
            net.invalidate(key);
            net.invalidate(HOME);
            net.invalidate_prefix("board:");
        }
    }

    net.get_once(HOME, "/api/user/home");

    let loading = net.is_loading(HOME);
    let error = net.error(HOME).map(str::to_owned);
    let busy = net.is_loading(APPROVE) || net.is_loading(REJECT) || net.is_loading(CLAIM);
    let mutation_error = net
        .error(APPROVE)
        .or(net.error(REJECT))
        .or(net.error(CLAIM))
        .map(str::to_owned);
    let home = net.data(HOME).cloned().unwrap_or(Value::Null);

    let my_tasks = list(&home, "myTasks");
    let waiting = list(&home, "waitingOnMe");
    let available = list(&home["available"], "first");
    let available_count = home["available"]
        .get("count")
        .and_then(Value::as_i64)
        .unwrap_or(available.len() as i64);
    let projects = list(&home, "projects");

    // Blocker ids resolve to titles only if the blocking task happens to be on
    // this payload. It usually is — you are blocked by work in your own list.
    let titles: HashMap<&str, &str> = my_tasks
        .iter()
        .chain(available.iter())
        .filter_map(|t| Some((str_at(t, "id")?, str_at(t, "title")?)))
        .collect();

    // ---- greeting
    ui.label(
        RichText::new(greeting)
            .size(text::TITLE)
            .family(egui::FontFamily::Name(
                crate::desktop::design::theme::BOLD.into(),
            ))
            .color(colour::TEXT),
    );
    ui.add_space(space::XXS);
    w::muted(ui, &subline(&my_tasks));

    if let Some(err) = &mutation_error {
        ui.add_space(space::MD);
        w::error(ui, &format!("That did not go through. {err}"));
    }

    let mut open_task: Option<String> = None;
    let mut view_all = false;

    // ---- my tasks
    section(ui, "My tasks", my_tasks.len(), |ui| {
        if w::link(ui, &format!("View all {}", icon::ARROW_RIGHT)).clicked() {
            view_all = true;
        }
    });
    if body(
        ui,
        loading,
        error.as_deref(),
        my_tasks.is_empty(),
        "Nothing assigned to you.",
        "Claim something below and it will show up here.",
    ) {
        w::card(ui, |ui| {
            ui.set_width(ui.available_width());
            for t in my_tasks.iter().take(MY_TASKS_PREVIEW) {
                if task_row(ui, t, &titles).clicked() {
                    open_task = str_at(t, "id").map(str::to_owned);
                }
            }
        });
    }

    // ---- waiting on you
    section(ui, "Waiting on you", waiting.len(), |_ui| {});
    if body(
        ui,
        loading,
        error.as_deref(),
        waiting.is_empty(),
        "Nothing waiting on you. You are clear.",
        "Proposals from agents land here when they need a person.",
    ) {
        for (i, change) in waiting.iter().enumerate() {
            change_card(ui, net, change, can_write, busy, i == 0);
            ui.add_space(space::SM);
        }
    }

    // ---- available to claim
    section(ui, "Available to claim", available_count as usize, |ui| {
        // /api/user/me carries no disciplines, so there is no match to name.
        w::muted(ui, "unclaimed across every project");
    });
    if body(
        ui,
        loading,
        error.as_deref(),
        available.is_empty(),
        "Nothing unclaimed.",
        "Every open task already has someone on it.",
    ) {
        w::card(ui, |ui| {
            ui.set_width(ui.available_width());
            for t in &available {
                let Some(id) = str_at(t, "id") else { continue };
                w::row(ui, |ui| {
                    w::discipline(ui, str_at(t, "discipline").unwrap_or_default());
                    ui.add_space(space::XS);
                    w::body(ui, str_at(t, "title").unwrap_or_default());

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if can_write && w::ghost(ui, "Claim").clicked() {
                            net.post(CLAIM, &format!("/api/user/tasks/{id}/claim"), Value::Null);
                        }
                        ui.add_space(space::SM);
                        w::muted(ui, str_at(t, "projectName").unwrap_or_default());
                    });
                });
            }
        });
    }

    // ---- across the team
    section(ui, "Across the team", projects.len(), |_ui| {});
    if body(
        ui,
        loading,
        error.as_deref(),
        projects.is_empty(),
        "No projects yet.",
        "Create one with: acp project new <key> <name>",
    ) {
        for pair in projects.chunks(2) {
            ui.columns(2, |cols| {
                for (i, p) in pair.iter().enumerate() {
                    project_card(&mut cols[i], p);
                }
            });
            ui.add_space(space::MD);
        }
    }

    if let Some(id) = open_task {
        app.task = Some(id);
    }
    if view_all {
        app.tab = Tab::MyTasks;
    }
}

// ------------------------------------------------------------------ sections

/// A section heading with its count beside the label and something optional on
/// the right. `shell::section_with` right-aligns everything it is given, and
/// the count belongs next to the label, so the count is folded into the label.
fn section(ui: &mut egui::Ui, label: &str, count: usize, trailing: impl FnOnce(&mut egui::Ui)) {
    crate::desktop::design::shell::section_with(ui, &format!("{label}   {count}"), trailing);
}

/// Loading, error and empty in one place. Returns true when the caller should
/// draw the real thing. One request feeds every section, so they share a
/// spinner and an error, and differ only in what "empty" means.
fn body(
    ui: &mut egui::Ui,
    loading: bool,
    error: Option<&str>,
    is_empty: bool,
    empty_message: &str,
    empty_detail: &str,
) -> bool {
    if let Some(err) = error {
        w::error(ui, err);
        return false;
    }
    if !is_empty {
        return true;
    }
    if loading {
        w::loading(ui, "Loading");
    } else {
        w::empty(ui, empty_message, empty_detail);
    }
    false
}

// ---------------------------------------------------------------------- rows

fn task_row(ui: &mut egui::Ui, t: &Value, titles: &HashMap<&str, &str>) -> egui::Response {
    let status = str_at(t, "status").unwrap_or_default();
    let done = t.get("blockersDone").and_then(Value::as_i64).unwrap_or(0);
    let total = t.get("blockersTotal").and_then(Value::as_i64).unwrap_or(0);

    w::row(ui, |ui| {
        w::dot(ui, status_colour(status));
        ui.add_space(space::XS);
        w::discipline(ui, str_at(t, "discipline").unwrap_or_default());
        ui.add_space(space::XS);
        w::body(ui, str_at(t, "title").unwrap_or_default());

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // A blocked row is in a relationship, not a state: name what it
            // waits on instead of showing a status it cannot leave.
            if done < total {
                w::blocked_by(ui, &blocker_name(t, titles, total - done));
            } else {
                w::pill(ui, status_label(status), status_colour(status));
                ui.add_space(space::SM);
                w::muted(ui, str_at(t, "projectName").unwrap_or_default());
            }
        });
    })
}

/// The blocker's title when it is on this payload, otherwise an honest count.
fn blocker_name(t: &Value, titles: &HashMap<&str, &str>, outstanding: i64) -> String {
    let named = t
        .get("blockedBy")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|id| titles.get(id).copied());

    match named {
        Some(title) => title.to_string(),
        None if outstanding == 1 => "1 other task".to_string(),
        None => format!("{outstanding} other tasks"),
    }
}

fn change_card(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    change: &Value,
    can_write: bool,
    busy: bool,
    first: bool,
) {
    let id = str_at(change, "id").unwrap_or_default().to_string();
    let actor = str_at(change, "actor").unwrap_or_default().to_string();
    let on_behalf = str_at(change, "onBehalfOf").unwrap_or_default().to_string();
    let when = ago(str_at(change, "createdAt").unwrap_or_default());

    let mut meta = actor.clone();
    if !on_behalf.is_empty() {
        meta.push_str(&format!(" \u{b7} on behalf of {on_behalf}"));
    }
    if !when.is_empty() {
        meta.push_str(&format!(" \u{b7} {when}"));
    }

    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            avatar::small(ui, &actor, size::AVATAR_MD);
            ui.add_space(space::SM);
            ui.vertical(|ui| {
                w::body(ui, &describe(change));
                ui.add_space(space::XXS);
                w::muted(ui, &meta);
            });

            if !can_write {
                return;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // One filled button on the screen: the decision most likely to
                // be made, on the oldest thing waiting. The rest are outlined.
                let approve = if first {
                    w::primary(ui, "Approve", !busy)
                } else {
                    w::secondary(ui, "Approve", !busy)
                };
                if approve.clicked() {
                    net.post(APPROVE, &format!("/api/user/changes/{id}/approve"), Value::Null);
                }
                ui.add_space(space::SM);
                if w::secondary(ui, "Reject", !busy).clicked() {
                    net.post(REJECT, &format!("/api/user/changes/{id}/reject"), Value::Null);
                }
            });
        });
    });
}

fn project_card(ui: &mut egui::Ui, p: &Value) {
    let done = p.get("done").and_then(Value::as_i64).unwrap_or(0);
    let total = p.get("total").and_then(Value::as_i64).unwrap_or(0);
    let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };

    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            w::heading(ui, str_at(p, "name").unwrap_or_default());
            ui.add_space(space::SM);
            w::mono_caption(ui, str_at(p, "key").unwrap_or_default());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                w::muted(ui, &format!("{done} / {total}"));
            });
        });
        ui.add_space(space::SM);
        w::progress(ui, fraction, ui.available_width(), colour::ACCENT);
        ui.add_space(space::SM);
        // The payload has no phase or headcount, so the second line carries
        // what it does have: where the work sits by discipline.
        w::muted(ui, &disciplines(p));
    });
}

/// "frontend 3/5 · backend 1/4", or the project's own state when it has none.
fn disciplines(p: &Value) -> String {
    let parts: Vec<String> = p
        .get("disciplines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|d| {
            format!(
                "{} {}/{}",
                str_at(d, "discipline").unwrap_or("unlabelled"),
                d.get("done").and_then(Value::as_i64).unwrap_or(0),
                d.get("total").and_then(Value::as_i64).unwrap_or(0),
            )
        })
        .collect();

    if parts.is_empty() {
        return status_label(str_at(p, "status").unwrap_or_default()).to_string();
    }
    parts.join(" \u{b7} ")
}

// -------------------------------------------------------------------- pieces

/// "Good evening, Anmol" — from the local clock and whoever `__me` says we are.
/// `__me` is fetched once by the app itself, so this only ever reads it.
fn greeting(app: &App) -> String {
    let who = app
        .net
        .as_ref()
        .and_then(|n| n.data("__me"))
        .and_then(|m| {
            m.get("email")
                .and_then(Value::as_str)
                .or_else(|| m.get("label").and_then(Value::as_str))
        })
        .unwrap_or("")
        .split(['@', '.', ' '])
        .next()
        .unwrap_or("")
        .to_string();

    let part = match Local::now().hour() {
        5..=11 => "Good morning",
        12..=16 => "Good afternoon",
        _ => "Good evening",
    };

    if who.is_empty() {
        return part.to_string();
    }
    let mut name = who.chars();
    let capitalised = match name.next() {
        Some(c) => c.to_uppercase().collect::<String>() + name.as_str(),
        None => who.clone(),
    };
    format!("{part}, {capitalised}")
}

/// "Saturday, 20 September · 4 open, 1 in review".
fn subline(my_tasks: &[&Value]) -> String {
    let count = |status: &str| {
        my_tasks.iter().filter(|t| str_at(t, "status") == Some(status)).count()
    };
    let in_review = count("in_review");
    let open = my_tasks
        .iter()
        .filter(|t| !matches!(str_at(t, "status"), Some("done") | Some("dropped") | Some("in_review")))
        .count();

    format!(
        "{} \u{b7} {open} open, {in_review} in review",
        Local::now().format("%A, %-d %B")
    )
}

/// "4m ago". Coarse on purpose: the exact second of a proposal is never the
/// thing you are deciding on.
fn ago(raw: &str) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(raw) else {
        return String::new();
    };
    let seconds = (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0);
    match seconds {
        s if s < 60 => "just now".to_string(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
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
