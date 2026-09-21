//! Home: the one screen that answers "what should I do now?".
//!
//! Everything on it arrives in a single `GET /api/user/home`, cached under the
//! key `"home"` — chrome.rs reads that same key for the My Tasks badge, so the
//! name is load-bearing. The team roster is the one extra fetch, under
//! `"home:people"`, because capacity is the only section the home payload has
//! nothing to say about.
//!
//! Top to bottom the page goes: who you are and when, a tab strip, four stat
//! tiles that summarise the day, then the work in the order you act on it —
//! yours, the decisions waiting on you, what nobody has taken, and finally
//! where the team is.
//!
//! Mutations (approve, reject, claim) invalidate `"home"`, the board and the
//! task caches, because each one changes a row some other view has cached.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Local, Timelike, Utc};
use egui::{Align, Layout, RichText};
use serde_json::Value;

use crate::desktop::design::{
    avatar, cards as c, colour, shell, size, space, status_label, text, theme, widgets as w,
};
use crate::desktop::views::inbox::describe;
use crate::desktop::{App, Tab};

const HOME: &str = "home";
const PEOPLE: &str = "home:people";
const APPROVE: &str = "home:approve";
const REJECT: &str = "home:reject";
const CLAIM: &str = "home:claim";

/// How many of my tasks the home screen shows before deferring to My Tasks.
const MY_TASKS_PREVIEW: usize = 5;

/// Days in the "done this week" sparkline. Seven, because the label says week.
const WEEK: usize = 7;

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let greeting = greeting(app);
    let my_disciplines = me_list(app, "disciplines");
    let my_person_id = me_str(app, "personId");
    let my_email = me_str(app, "email");

    let net = app.net.as_mut().unwrap();

    // A finished mutation has changed rows every other view has cached.
    for key in [APPROVE, REJECT, CLAIM] {
        if net.data(key).is_some() {
            net.invalidate(key);
            net.invalidate(HOME);
            net.invalidate_prefix("board:");
            net.invalidate_prefix("task:");
        }
    }

    net.get_once(HOME, "/api/user/home");
    net.get_once(PEOPLE, "/api/user/people");

    let loading = net.is_loading(HOME);
    let error = net.error(HOME).map(str::to_owned);
    let people_loading = net.is_loading(PEOPLE);
    let people_error = net.error(PEOPLE).map(str::to_owned);
    let busy = net.is_loading(APPROVE) || net.is_loading(REJECT) || net.is_loading(CLAIM);
    let mutation_error = net
        .error(APPROVE)
        .or(net.error(REJECT))
        .or(net.error(CLAIM))
        .map(str::to_owned);

    let home = net.data(HOME).cloned().unwrap_or(Value::Null);
    let people = net.data(PEOPLE).cloned().unwrap_or(Value::Null);
    let people: Vec<&Value> = people.as_array().map(|a| a.iter().collect()).unwrap_or_default();

    // A change names the person it is on behalf of by id; the roster is the
    // only thing on this screen that can turn that back into a name.
    let names: HashMap<&str, &str> = people
        .iter()
        .filter_map(|p| Some((str_at(p, "id")?, str_at(p, "name")?)))
        .collect();

    let my_tasks = list(&home, "myTasks");
    let waiting = list(&home, "waitingOnMe");
    let available = list(&home["available"], "first");
    let available_count = home["available"]
        .get("count")
        .and_then(Value::as_i64)
        .unwrap_or(available.len() as i64) as usize;

    // Blocker ids resolve to a title or a discipline only if the blocking task
    // happens to be on this payload. It usually is — you are blocked by work in
    // your own list.
    let known: HashMap<&str, &Value> = my_tasks
        .iter()
        .chain(available.iter())
        .filter_map(|t| Some((str_at(t, "id")?, *t)))
        .collect();

    // Every task id that something else is waiting on. Whoever holds one of
    // these is blocking somebody.
    let blocking_ids: HashSet<&str> = my_tasks
        .iter()
        .chain(available.iter())
        .filter_map(|t| t.get("blockedBy").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect();

    // ---- greeting and tabs
    shell::page_title(ui, &greeting, &Local::now().format("%A, %-d %B").to_string(), |_| {});
    // Tab wiring is out of scope: the strip renders, the selection is fixed.
    let _ = c::tabs(ui, &["Overview", "My work", "Team"], 0);

    if let Some(err) = &mutation_error {
        w::error(ui, &format!("That did not go through. {err}"));
        ui.add_space(space::MD);
    }

    // ---- the day in four numbers
    let done_total = my_tasks.iter().filter(|t| str_at(t, "status") == Some("done")).count();
    let spark = done_by_day(&my_tasks);
    let proposers = proposers(&waiting);
    let blocked: Vec<&&Value> = my_tasks.iter().filter(|t| is_blocked(t)).collect();
    let blocked_note = blocked_note(&blocked, &known);
    let matching = matching_note(&my_disciplines);

    ui.columns(4, |cols| {
        c::stat(&mut cols[0], "Done this week", &done_total.to_string(), None, "", |ui| {
            c::sparkline(ui, &spark, colour::ACCENT);
        });
        c::stat(&mut cols[1], "Awaiting your review", &waiting.len().to_string(), None, "", |ui| {
            for seed in &proposers {
                avatar::small(ui, seed, size::AVATAR_SM);
            }
        });
        c::stat(&mut cols[2], "Blocked", &blocked.len().to_string(), None, &blocked_note, |_| {});
        c::stat(
            &mut cols[3],
            "Available to claim",
            &available_count.to_string(),
            None,
            &matching,
            |_| {},
        );
    });

    let mut open_task: Option<String> = None;
    let mut claim: Option<String> = None;
    let mut view_all = false;

    // ---- my day
    shell::section_count_with(ui, "My day", my_tasks.len(), |ui| {
        if w::ghost(ui, "View all").clicked() {
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
        for t in my_tasks.iter().take(MY_TASKS_PREVIEW) {
            let chips = task_chips(t);
            let trailing = trailing_chips(t);
            let people = match str_at(t, "assigneeKind") {
                Some("person") if !my_email.is_empty() => vec![my_email.clone()],
                _ => Vec::new(),
            };
            let card = c::TaskCard {
                title: str_at(t, "title").unwrap_or_default(),
                chips: &chips,
                context: &context(t, &known),
                trailing_chips: &trailing,
                people: &people,
            };
            if c::task_card(ui, &card).clicked() {
                open_task = str_at(t, "id").map(str::to_owned);
            }
        }
    }

    // ---- waiting on you
    shell::section_count(ui, "Waiting on you", waiting.len());
    if body(
        ui,
        loading,
        error.as_deref(),
        waiting.is_empty(),
        "Nothing waiting on you. You are clear.",
        "Proposals from agents land here when they need a person.",
    ) {
        for (i, change) in waiting.iter().enumerate() {
            change_card(ui, net, change, &names, can_write, busy, i == 0);
        }
    }

    // ---- available to claim
    shell::section_count_with(ui, "Available to claim", available_count, |ui| {
        c::chip(ui, &matching, c::Tone::Neutral, false);
    });
    if body(
        ui,
        loading,
        error.as_deref(),
        available.is_empty(),
        "Nothing unclaimed.",
        "Every open task already has someone on it.",
    ) {
        for t in &available {
            let Some(id) = str_at(t, "id") else { continue };
            let discipline = str_at(t, "discipline").unwrap_or_default();
            c::slim_card(ui, |ui| {
                c::chip(ui, discipline, c::discipline_tone(discipline), false);
                ui.add_space(space::SM);
                ui.label(
                    RichText::new(str_at(t, "title").unwrap_or_default())
                        .size(text::BODY)
                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                        .color(colour::TEXT),
                );
                ui.add_space(space::SM);
                w::muted(ui, str_at(t, "projectName").unwrap_or_default());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if can_write && w::ghost(ui, "Claim").clicked() {
                        claim = Some(id.to_owned());
                    }
                });
            });
        }
    }

    // ---- team capacity. Ambient rather than actionable, so it sits last.
    shell::section_count(ui, "Team capacity", people.len());
    if body(
        ui,
        people_loading,
        people_error.as_deref(),
        people.is_empty(),
        "No one on the team yet.",
        "Add people with: acp-admin seed-team <file>",
    ) {
        for chunk in people.chunks(3) {
            ui.columns(3, |cols| {
                for (i, p) in chunk.iter().enumerate() {
                    let seed = str_at(p, "email").unwrap_or_default();
                    let (open, review, blocking) =
                        load_of(p, &my_tasks, &blocking_ids, &my_person_id);
                    c::capacity(
                        &mut cols[i],
                        seed,
                        str_at(p, "name").unwrap_or(seed),
                        &joined(p, "disciplines"),
                        open,
                        review,
                        blocking,
                    );
                }
            });
            ui.add_space(space::MD);
        }
    }
    ui.add_space(space::XXL);

    if let Some(id) = claim {
        net.post(CLAIM, &format!("/api/user/tasks/{id}/claim"), Value::Null);
    }
    if let Some(id) = open_task {
        app.task = Some(id);
    }
    if view_all {
        app.tab = Tab::MyTasks;
    }
}

// ------------------------------------------------------------------ sections

/// Loading, error and empty in one place. Returns true when the caller should
/// draw the real thing. One request feeds most sections, so they share a
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

/// The pending change as a card: who proposed it, the sentence it amounts to,
/// and the two buttons that resolve it.
///
/// Not `c::task_card` — that has no slot for controls, and the buttons belong
/// inside the card rather than floating under it.
fn change_card(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    change: &Value,
    names: &HashMap<&str, &str>,
    can_write: bool,
    busy: bool,
    first: bool,
) {
    let id = str_at(change, "id").unwrap_or_default().to_string();
    let actor = str_at(change, "actor").unwrap_or_default().to_string();
    let on_behalf = str_at(change, "onBehalfOf")
        .map(|id| names.get(id).copied().unwrap_or(id).to_string())
        .unwrap_or_default();
    let when = ago(str_at(change, "createdAt").unwrap_or_default());

    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            if !actor.is_empty() {
                c::chip(ui, &actor, c::Tone::Agent, false);
            }
            if !on_behalf.is_empty() {
                c::chip(ui, &format!("on behalf of {on_behalf}"), c::Tone::Neutral, false);
            }
            if !when.is_empty() {
                c::chip(ui, &when, c::Tone::Quiet, false);
            }
        });
        ui.add_space(space::SM);

        // The sentence is the decision: its own line, at full width, so it
        // never competes with the buttons for room.
        ui.label(
            RichText::new(describe(change))
                .size(text::CARD)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );

        if !can_write {
            return;
        }
        ui.add_space(space::MD);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // One filled button on the screen: the decision most likely to be
            // made, on the oldest thing waiting. The rest are outlined.
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
    ui.add_space(space::SM);
}

// --------------------------------------------------------------------- chips

/// Leading chips, in the order they read: how stale it is, whose craft it is,
/// where it stands, and which project it belongs to.
fn task_chips(t: &Value) -> Vec<(String, c::Tone, bool)> {
    let status = str_at(t, "status").unwrap_or_default();
    let discipline = str_at(t, "discipline").unwrap_or_default();
    let age = ago(str_at(t, "updatedAt").unwrap_or_default());

    let mut chips = vec![(
        if age.is_empty() { "no activity".to_string() } else { age },
        c::Tone::Quiet,
        false,
    )];
    if !discipline.is_empty() {
        chips.push((discipline.to_string(), c::discipline_tone(discipline), false));
    }
    if !status.is_empty() {
        chips.push((status_label(status).to_string(), c::status_tone(status), true));
    }
    let project = str_at(t, "projectName").unwrap_or_default();
    if !project.is_empty() {
        chips.push((project.to_string(), c::Tone::Neutral, false));
    }
    chips
}

/// The agent holding the task, when one is.
fn trailing_chips(t: &Value) -> Vec<(String, c::Tone)> {
    if str_at(t, "assigneeKind") != Some("agent") {
        return Vec::new();
    }
    let label = str_at(t, "claimedBy").unwrap_or("agent");
    vec![(label.to_string(), c::Tone::Agent)]
}

/// The muted line under a title: the phase, or — when the task is blocked —
/// what it is waiting on, in `w::blocked_by`'s wording.
fn context(t: &Value, known: &HashMap<&str, &Value>) -> String {
    if is_blocked(t) {
        return format!("\u{2933} waiting on \u{201c}{}\u{201d}", blocker_name(t, known));
    }
    // The payload carries a phase name but no phase number, so there is no
    // "Phase 5 ·" to put in front of it.
    str_at(t, "phaseName").unwrap_or_default().to_string()
}

/// The blocker's title when it is on this payload, otherwise an honest count.
fn blocker_name(t: &Value, known: &HashMap<&str, &Value>) -> String {
    let named = blocked_by(t).find_map(|id| known.get(id).and_then(|b| str_at(b, "title")));
    match named {
        Some(title) => title.to_string(),
        None => {
            let outstanding = outstanding(t);
            if outstanding == 1 {
                "1 other task".to_string()
            } else {
                format!("{outstanding} other tasks")
            }
        }
    }
}

// --------------------------------------------------------------------- stats

/// "frontend waiting on design" when both disciplines are on the payload,
/// otherwise the count, which is the most we actually know.
fn blocked_note(blocked: &[&&Value], known: &HashMap<&str, &Value>) -> String {
    if blocked.is_empty() {
        return String::new();
    }
    let pairing = blocked.iter().find_map(|t| {
        let mine = str_at(t, "discipline")?;
        let theirs = blocked_by(t)
            .find_map(|id| known.get(id).and_then(|b| str_at(b, "discipline")))?;
        Some(format!("{mine} waiting on {theirs}"))
    });
    pairing.unwrap_or_else(|| {
        let n: i64 = blocked.iter().map(|t| outstanding(t)).sum();
        if n == 1 { "1 blocker outstanding".to_string() } else { format!("{n} blockers outstanding") }
    })
}

/// "matching frontend, backend" — or an honest nothing when `/api/user/me`
/// says this person has claimed no disciplines.
fn matching_note(disciplines: &[String]) -> String {
    if disciplines.is_empty() {
        return "unclaimed across every project".to_string();
    }
    format!("matching {}", disciplines.join(", "))
}

/// Tasks finished on each of the last seven days, oldest bucket first. Only
/// tasks whose last touch falls in the window land in one, which is as close
/// to a completion date as the payload gets.
fn done_by_day(my_tasks: &[&Value]) -> Vec<f32> {
    let today = Local::now().date_naive();
    let mut buckets = vec![0.0_f32; WEEK];
    for t in my_tasks.iter().filter(|t| str_at(t, "status") == Some("done")) {
        let Some(raw) = str_at(t, "updatedAt") else { continue };
        let Ok(when) = DateTime::parse_from_rfc3339(raw) else { continue };
        let days = (today - when.with_timezone(&Local).date_naive()).num_days();
        if (0..WEEK as i64).contains(&days) {
            buckets[WEEK - 1 - days as usize] += 1.0;
        }
    }
    buckets
}

/// Who proposed the things waiting on you, deduplicated and in order.
fn proposers(waiting: &[&Value]) -> Vec<String> {
    let mut seen = HashSet::new();
    waiting
        .iter()
        .filter_map(|c| str_at(c, "actor"))
        .filter(|a| seen.insert(a.to_string()))
        .map(str::to_owned)
        .collect()
}

/// One person's load, from the tasks this screen actually holds.
///
/// The home payload carries *my* tasks and the unassigned pool, and nothing
/// else — so every number here is honest for me and zero for everybody else.
/// A team-wide count needs an endpoint that returns team-wide tasks; inventing
/// a shape for the bar was the worst thing the previous version did.
fn load_of(
    person: &Value,
    my_tasks: &[&Value],
    blocking_ids: &HashSet<&str>,
    my_person_id: &str,
) -> (usize, usize, usize) {
    if str_at(person, "id").unwrap_or_default() != my_person_id || my_person_id.is_empty() {
        return (0, 0, 0);
    }
    let live = || {
        my_tasks
            .iter()
            .filter(|t| !matches!(str_at(t, "status"), Some("done") | Some("dropped")))
    };
    let open = live().filter(|t| str_at(t, "status") != Some("in_review")).count();
    let review = live().filter(|t| str_at(t, "status") == Some("in_review")).count();
    let blocking = live()
        .filter(|t| str_at(t, "id").is_some_and(|id| blocking_ids.contains(id)))
        .count();
    (open, review, blocking)
}

// -------------------------------------------------------------------- pieces

/// "Good evening, Anmol" — from the local clock and whoever `__me` says we are.
/// `__me` is fetched once by the app itself, so this only ever reads it.
fn greeting(app: &App) -> String {
    let who = me(app)
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
        Some(ch) => ch.to_uppercase().collect::<String>() + name.as_str(),
        None => who.clone(),
    };
    format!("{part}, {capitalised}")
}

fn me(app: &App) -> Option<&Value> {
    app.net.as_ref().and_then(|n| n.data("__me"))
}

fn me_str(app: &App, key: &str) -> String {
    me(app)
        .and_then(|m| m.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn me_list(app: &App, key: &str) -> Vec<String> {
    me(app)
        .and_then(|m| m.get(key))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// "frontend · backend", for the faint line beside a name.
fn joined(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" \u{b7} ")
        })
        .unwrap_or_default()
}

fn is_blocked(t: &Value) -> bool {
    outstanding(t) > 0
}

/// Blockers still unfinished.
fn outstanding(t: &Value) -> i64 {
    let done = t.get("blockersDone").and_then(Value::as_i64).unwrap_or(0);
    let total = t.get("blockersTotal").and_then(Value::as_i64).unwrap_or(0);
    (total - done).max(0)
}

fn blocked_by(t: &Value) -> impl Iterator<Item = &str> {
    t.get("blockedBy")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// "4m ago". Coarse on purpose: the exact second is never the thing you are
/// deciding on.
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
