//! My tasks: everything assigned to the signed-in person, grouped by the state
//! that decides what they do next, and under it the work they could claim.
//!
//! Two fetches, both cached under the `mine:` prefix so a claim can drop the
//! pair with one `invalidate_prefix`. Grouping is by *effective* state, not by
//! the `status` column: a task whose blockers are unfinished is filed under
//! Blocked whatever its status says, because that is what actually stops you.
//!
//! Every assigned task is a card, not a row: chips on top, the title as the
//! loudest thing in it, the phase and the people underneath. The claimable
//! work at the foot is the one exception — a slim card, because a title and a
//! Claim button is the whole content.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::desktop::design::{
    cards as c, colour, shell, space, status_label, text, theme, widgets as w,
};
use crate::desktop::App;

const MINE: &str = "mine:assigned";
const AVAILABLE: &str = "mine:available";
const CLAIM: &str = "mine:claim";

const DISCIPLINES: [&str; 3] = ["design", "frontend", "backend"];

/// The vocabulary of the status filter, and — read top to bottom — the order
/// the sections appear in. In progress first because it is what you are
/// holding; Blocked last because it is what you cannot move.
///
/// `done` and `dropped` are in the list so the filter can ask for them, but a
/// finished task is not shown unless it is asked for: this screen answers
/// "what now", and yesterday's work is noise in that answer.
const SECTIONS: [&str; 6] = ["in_progress", "in_review", "open", "blocked", "done", "dropped"];

/// Filter selections. `None` means "any" on each. The two fixed vocabularies
/// are `&'static str` so a frame that changes nothing allocates nothing; the
/// project list is discovered from the data, so it has to own its id.
#[derive(Default, Clone)]
pub struct State {
    pub discipline: Option<&'static str>,
    pub status: Option<&'static str>,
    pub project: Option<String>,
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    // ponytail: the filters live in egui's temp store rather than a field on
    // `App`, which keeps this screen to one file. Move them onto `App` the day
    // another view needs to read them.
    let slot = egui::Id::new("mine:filters");
    let mut state: State = ui.data(|d| d.get_temp(slot)).unwrap_or_default();

    let can_write = app.can_write();
    // Every task on this screen is assigned to the signed-in person, so the
    // avatar on a card is always theirs — the row itself carries no seed.
    let me = app
        .net
        .as_ref()
        .and_then(|n| n.data("__me"))
        .and_then(|m| m.get("email"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let net = app.net.as_mut().unwrap();

    // A finished claim has changed both lists and the home aggregate.
    if net.data(CLAIM).is_some() {
        net.invalidate_prefix("mine:");
        net.invalidate("home");
    }

    net.get_once(MINE, "/api/user/tasks/mine");
    net.get_once(AVAILABLE, "/api/user/tasks/available");

    let mine = array(net.data(MINE));
    let available = array(net.data(AVAILABLE));
    let loading = net.is_loading(MINE) || net.is_loading(AVAILABLE);
    let list_error = net.error(MINE).or(net.error(AVAILABLE)).map(str::to_owned);
    let claim_error = net.error(CLAIM).map(str::to_owned);
    let busy = net.is_loading(CLAIM);

    // Blockers arrive as ids. Any blocker that is itself on one of these lists
    // can be named; the rest fall back to their short id, which is still
    // something you can paste into `acp task show`.
    let mut titles: HashMap<String, String> = HashMap::new();
    for t in mine.iter().chain(available.iter()) {
        titles.insert(text_at(t, "id"), text_at(t, "title"));
    }

    // Everything assigned, minus the finished work nobody asked to see.
    let shown: Vec<&Value> = mine
        .iter()
        .filter(|t| keep(t, &state))
        .filter(|t| matches!(state.status, Some(s) if s == bucket(t)) || !finished(bucket(t)))
        .collect();
    let claimable: Vec<&Value> = available.iter().filter(|t| keep(t, &state)).collect();

    shell::page_title(
        ui,
        "My tasks",
        &format!(
            "{} assigned · {} available to claim",
            shown.len(),
            claimable.len()
        ),
        |_| {},
    );

    filters(ui, &mut state, &mine, &available);

    if let Some(err) = &claim_error {
        ui.add_space(space::MD);
        w::error(ui, &format!("That claim did not go through. {err}"));
    }

    if let Some(err) = &list_error {
        ui.add_space(space::MD);
        w::error(ui, &format!("Could not load your work. {err}"));
        ui.data_mut(|d| d.insert_temp(slot, state));
        return;
    }

    if mine.is_empty() && available.is_empty() {
        ui.add_space(space::MD);
        if loading {
            w::loading(ui, "your work");
        } else {
            w::empty(
                ui,
                "Nothing is assigned to you, and nothing matches your disciplines yet.",
                "Ask for a discipline on your profile, or open Projects to see what the team is building.",
            );
        }
        ui.data_mut(|d| d.insert_temp(slot, state));
        return;
    }

    let mut open: Option<String> = None;
    let mut claim: Option<String> = None;

    for name in SECTIONS {
        let rows: Vec<&&Value> = shown.iter().filter(|t| bucket(t) == name).collect();
        if rows.is_empty() {
            continue;
        }
        shell::section_count(ui, &capitalise(status_label(name)), rows.len());
        for t in rows {
            if let Some(id) = assigned_card(ui, t, &titles, &me) {
                open = Some(id);
            }
        }
    }

    if !claimable.is_empty() {
        let mut kinds: Vec<&str> = Vec::new();
        for t in &claimable {
            let d = t.get("discipline").and_then(Value::as_str).unwrap_or("");
            if !d.is_empty() && !kinds.contains(&d) {
                kinds.push(d);
            }
        }
        let note = format!("matching {}", kinds.join(", "));
        // Other people's unclaimed work is a different kind of thing from the
        // status sections above it. At the same pitch the screen reads as a
        // run of interchangeable slabs; a page-margin gap says "and separately".
        ui.add_space(space::XXL);
        shell::section_count_with(ui, "Available to claim", claimable.len(), |ui| {
            if !kinds.is_empty() {
                w::muted(ui, &note);
            }
        });

        for t in &claimable {
            match claimable_card(ui, t, can_write && !busy) {
                Row::Open(id) => open = Some(id),
                Row::Claim(id) => claim = Some(id),
                Row::Idle => {}
            }
        }
    }

    if shown.is_empty() && claimable.is_empty() {
        w::empty(ui, "Nothing matches these filters.", "Clear one to see the rest of your work.");
    }

    if let Some(id) = claim {
        app.net
            .as_mut()
            .unwrap()
            .post(CLAIM, &format!("/api/user/tasks/{id}/claim"), Value::Null);
    } else if let Some(id) = open {
        app.task = Some(id);
    }

    ui.data_mut(|d| d.insert_temp(slot, state));
}

// --------------------------------------------------------------------- cards

/// One assigned task. Returns its id when clicked.
fn assigned_card(
    ui: &mut egui::Ui,
    t: &Value,
    titles: &HashMap<String, String>,
    me: &str,
) -> Option<String> {
    let id = text_at(t, "id");
    let title = text_at(t, "title");
    let discipline = text_at(t, "discipline");
    let project = text_at(t, "projectName");
    let is_agent = text_at(t, "assigneeKind") == "agent";
    let claimed_by = text_at(t, "claimedBy");
    let state = bucket(t);

    // Age first and quiet: on a screen about what to do next, the task that
    // has not moved in four days is the news. Then discipline, then the state
    // with its dot, then which project it belongs to.
    let mut chips: Vec<(String, c::Tone, bool)> = vec![(age(t), c::Tone::Quiet, false)];
    if !discipline.is_empty() {
        chips.push((discipline.clone(), c::discipline_tone(&discipline), false));
    }
    chips.push((capitalise(status_label(state)), c::status_tone(state), true));
    if !project.is_empty() {
        chips.push((project.clone(), c::Tone::Neutral, false));
    }

    // A blocked card spends its context line naming the blocker: the project
    // and phase are not what you need from it.
    let context = if state == "blocked" {
        waiting_on(&blocker_name(t, titles))
    } else {
        format!("{project} · {}", text_at(t, "phaseName"))
    };

    let trailing: Vec<(String, c::Tone)> = if is_agent {
        let label = if claimed_by.is_empty() { "agent".to_owned() } else { claimed_by };
        vec![(label, c::Tone::Agent)]
    } else {
        Vec::new()
    };
    // The agent chip already says who holds it; a second face beside it is noise.
    let people: Vec<String> =
        if is_agent || me.is_empty() { Vec::new() } else { vec![me.to_owned()] };

    c::task_card(
        ui,
        &c::TaskCard {
            title: &title,
            chips: &chips,
            context: &context,
            trailing_chips: &trailing,
            people: &people,
        },
    )
    .clicked()
    .then_some(id)
}

enum Row {
    Idle,
    Open(String),
    Claim(String),
}

/// One claimable task: no status, because every row here is open, and a Claim
/// button where the assigned cards carry their people.
fn claimable_card(ui: &mut egui::Ui, t: &Value, can_claim: bool) -> Row {
    let id = text_at(t, "id");
    let title = text_at(t, "title");
    let discipline = text_at(t, "discipline");
    let project = text_at(t, "projectName");

    let mut claimed = false;
    let hit = c::slim_card(ui, |ui| {
        if !discipline.is_empty() {
            c::chip(ui, &discipline, c::discipline_tone(&discipline), false);
            ui.add_space(space::XS);
        }
        ui.label(
            egui::RichText::new(&title)
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            claimed = w::button(ui, "Claim", w::Emphasis::Ghost, can_claim).clicked();
            ui.add_space(space::SM);
            w::muted(ui, &project);
        });
    })
    .interact(egui::Sense::click());

    if claimed {
        Row::Claim(id)
    } else if hit.clicked() {
        Row::Open(id)
    } else {
        Row::Idle
    }
}

// ------------------------------------------------------------------- filters

fn filters(ui: &mut egui::Ui, state: &mut State, mine: &[Value], available: &[Value]) {
    // Only projects that actually have a row here: a dropdown listing projects
    // this person has no work in filters to nothing.
    let mut projects: Vec<(String, String)> = Vec::new();
    for t in mine.iter().chain(available.iter()) {
        let id = text_at(t, "projectId");
        if !id.is_empty() && !projects.iter().any(|(p, _)| *p == id) {
            projects.push((id, text_at(t, "projectName")));
        }
    }

    ui.horizontal(|ui| {
        fixed(ui, "mine:f:discipline", "All disciplines", &DISCIPLINES, &mut state.discipline);
        ui.add_space(space::SM);
        fixed(ui, "mine:f:status", "Any status", &SECTIONS, &mut state.status);
        ui.add_space(space::SM);
        project_filter(ui, &projects, &mut state.project);

        if state.discipline.is_some() || state.status.is_some() || state.project.is_some() {
            ui.add_space(space::SM);
            if w::secondary(ui, "Clear", true).clicked() {
                *state = State::default();
            }
        }
    });
}

/// A dropdown over a fixed vocabulary; `None` is the leading "any" entry.
fn fixed(
    ui: &mut egui::Ui,
    id: &str,
    any: &str,
    options: &[&'static str],
    slot: &mut Option<&'static str>,
) {
    let selected = slot.map(status_label).unwrap_or(any).to_owned();
    egui::ComboBox::from_id_salt(id)
        .selected_text(egui::RichText::new(selected).size(text::BODY))
        .show_ui(ui, |ui| {
            if ui.selectable_label(slot.is_none(), egui::RichText::new(any).size(text::BODY)).clicked() {
                *slot = None;
            }
            for opt in options {
                let label = egui::RichText::new(status_label(opt)).size(text::BODY);
                if ui.selectable_label(*slot == Some(*opt), label).clicked() {
                    *slot = Some(*opt);
                }
            }
        });
}

fn project_filter(ui: &mut egui::Ui, projects: &[(String, String)], slot: &mut Option<String>) {
    let selected = slot
        .as_ref()
        .and_then(|id| projects.iter().find(|(p, _)| p == id))
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| "All projects".to_owned());

    egui::ComboBox::from_id_salt("mine:f:project")
        .selected_text(egui::RichText::new(selected).size(text::BODY))
        .show_ui(ui, |ui| {
            let any = egui::RichText::new("All projects").size(text::BODY);
            if ui.selectable_label(slot.is_none(), any).clicked() {
                *slot = None;
            }
            for (id, name) in projects {
                let label = egui::RichText::new(name).size(text::BODY);
                if ui.selectable_label(slot.as_deref() == Some(id), label).clicked() {
                    *slot = Some(id.clone());
                }
            }
        });
}

// --------------------------------------------------------------------- rules

/// The state a row is filed under. Unfinished blockers beat the status column:
/// a task marked `in_progress` that is waiting on someone else is not in
/// progress, and filing it as though it were hides the thing to go and unblock.
fn bucket(t: &Value) -> &'static str {
    let total = num(t, "blockersTotal");
    if num(t, "blockersDone") < total {
        return "blocked";
    }
    let status = t.get("status").and_then(Value::as_str).unwrap_or("open");
    SECTIONS.iter().copied().find(|s| *s == status).unwrap_or("open")
}

fn finished(bucket: &str) -> bool {
    bucket == "done" || bucket == "dropped"
}

fn keep(t: &Value, state: &State) -> bool {
    if let Some(d) = state.discipline {
        if text_at(t, "discipline") != d {
            return false;
        }
    }
    if let Some(s) = state.status {
        if bucket(t) != s {
            return false;
        }
    }
    if let Some(p) = &state.project {
        if &text_at(t, "projectId") != p {
            return false;
        }
    }
    true
}

/// What the blocked card names. The first unfinished blocker is the one to go
/// and chase; its title when we have it, its short id when we do not.
fn blocker_name(t: &Value, titles: &HashMap<String, String>) -> String {
    let first = t
        .get("blockedBy")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or_default();

    match titles.get(first) {
        Some(title) => title.clone(),
        None => first.get(..8).unwrap_or(first).to_owned(),
    }
}

// -------------------------------------------------------------------- pieces

/// `w::blocked_by`'s wording, as a string — a card's context line is text, not
/// a widget.
/// ponytail: two spellings of one sentence. Fold them together the day a third
/// screen needs it.
fn waiting_on(what: &str) -> String {
    format!("\u{2933} waiting on \u{201c}{what}\u{201d}")
}

/// How long since the task last moved, for the quiet leading chip.
fn age(t: &Value) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(&text_at(t, "updatedAt")) else {
        return "no activity".to_owned();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn array(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn text_at(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn num(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}
