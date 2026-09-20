//! The approval inbox: what agents have proposed, in plain language, with the
//! two buttons that resolve it.
//!
//! The list lives under the key `"inbox"` because the top bar's badge counts
//! that array. Mutations use `inbox:approve` / `inbox:reject`, and a finished
//! mutation invalidates the board and task caches too, since approving a
//! proposal replays a real write behind them.

use egui::{Color32, RichText};
use serde_json::Value;

use crate::desktop::design::{avatar, colour, space, text, widgets as w};
use crate::desktop::App;

const APPROVE: &str = "inbox:approve";
const REJECT: &str = "inbox:reject";

/// The avatar in a metadata row. `space::XL` is 24 — the same disc chrome.rs
/// uses beside the signed-in user, so one person is the same size everywhere.
const AVATAR: f32 = space::XL;

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let net = app.net.as_mut().unwrap();

    // A mutation that came back successfully has changed the world underneath
    // every cached view, so drop the lot and let them refetch.
    for key in [APPROVE, REJECT] {
        if net.data(key).is_some() {
            net.invalidate(key);
            net.invalidate("inbox");
            net.invalidate_prefix("board:");
            net.invalidate_prefix("task:");
        }
    }

    net.get_once("inbox", "/api/user/changes/pending");

    let list_loading = net.is_loading("inbox");
    let list_error = net.error("inbox").map(str::to_owned);
    let mutation_error = net.error(APPROVE).or(net.error(REJECT)).map(str::to_owned);
    let busy = net.is_loading(APPROVE) || net.is_loading(REJECT);

    let mut changes: Vec<Value> =
        net.data("inbox").and_then(Value::as_array).cloned().unwrap_or_default();
    changes.reverse(); // the API orders oldest-first; newest-first reads better

    header(ui, changes.len(), can_write);

    if let Some(err) = &mutation_error {
        ui.add_space(space::MD);
        w::error(ui, &format!("That did not go through. {err}"));
    }

    if let Some(err) = &list_error {
        ui.add_space(space::MD);
        w::error(ui, &format!("Could not load the queue. {err}"));
        return;
    }

    if changes.is_empty() {
        ui.add_space(space::MD);
        if list_loading {
            w::loading(ui, "Loading the queue");
        } else {
            w::empty(ui, "Nothing waiting for review.");
            ui.vertical_centered(|ui| {
                w::caption(ui, "Proposals from agents will appear here.");
            });
        }
        return;
    }

    ui.add_space(space::LG);
    for change in &changes {
        card(ui, net, change, can_write, busy);
        ui.add_space(space::MD);
    }
}

fn header(ui: &mut egui::Ui, count: usize, can_write: bool) {
    ui.horizontal(|ui| {
        w::title(ui, "Pending approvals");
        if count > 0 {
            ui.add_space(space::XXS);
            w::pill(ui, &count.to_string(), colour::ACCENT);
        }
    });
    ui.add_space(space::XXS);
    let note = if can_write {
        "Proposals wait here until a person decides."
    } else {
        "Read only. This token can see the queue but not decide on it."
    };
    w::muted(ui, note);
}

fn card(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    change: &Value,
    can_write: bool,
    busy: bool,
) {
    let id = string(change, "id");
    let target_type = string(change, "targetType");
    let target_id = string(change, "targetId");
    let actor = string(change, "actor");
    let sentence = describe(change);

    w::card(ui, |ui| {
        ui.set_width(ui.available_width());

        // The sentence is the decision: loudest thing on the card.
        ui.label(RichText::new(sentence).size(text::BODY).color(colour::TEXT));
        ui.add_space(space::MD);

        ui.horizontal(|ui| {
            w::pill(ui, &target_type, target_colour(&target_type));
            w::id(ui, &target_id);
            ui.add_space(space::SM);
            avatar::small(ui, &actor, AVATAR);
            ui.add_space(space::XS);
            w::muted(ui, &format!("proposed by {actor}"));

            if !can_write {
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::primary(ui, "Approve", !busy).clicked() {
                    net.post(APPROVE, &format!("/api/user/changes/{id}/approve"), Value::Null);
                }
                ui.add_space(space::SM);
                if w::danger(ui, "Reject", !busy).clicked() {
                    net.post(REJECT, &format!("/api/user/changes/{id}/reject"), Value::Null);
                }
            });
        });
    });
}

fn target_colour(target_type: &str) -> Color32 {
    match target_type {
        "task" => colour::ACCENT,
        "artifact" => colour::AGENT,
        _ => colour::TEXT_MUTED,
    }
}

/// Turn a recorded proposal into the sentence a reviewer can decide on.
///
/// The card already shows the target's type and id, so the sentence says "this
/// task" rather than repeating them. An unrecognised shape falls back to
/// something honest: this is prose, never a source of truth, and it must never
/// panic on a patch the app has not met before.
pub fn describe(change: &Value) -> String {
    let patch = &change["patch"];
    let s = |k: &str| string(patch, k);
    let target_type = string(change, "targetType");

    match (target_type.as_str(), string(change, "op").as_str()) {
        ("project", "create") => {
            format!("Create project \"{}\" with key {}.", s("name"), s("key"))
        }
        ("phase", "create") => {
            let mut out = format!(
                "Add phase \"{}\" at position {} to project {}.",
                s("name"),
                scalar(patch, "position"),
                short(&s("project_id")),
            );
            let gate = s("gate");
            if !gate.is_empty() {
                out.push_str(&format!(" Gate: {gate}."));
            }
            out
        }
        ("phase", "update") => format!("Move this phase to {}.", s("status")),
        ("task", "create") => {
            let mut out = format!(
                "Add task \"{}\" to phase {}.",
                s("title"),
                short(&s("phase_id")),
            );
            let priority = scalar(patch, "priority");
            if !priority.is_empty() {
                out.push_str(&format!(" Priority {priority}."));
            }
            out
        }
        ("task", "update") => {
            if patch.get("status").is_some() {
                format!("Move this task to {}.", s("status"))
            } else if let Some(agent) = patch.get("agent_label").and_then(Value::as_str) {
                format!("Assign this task to agent {agent}.")
            } else if let Some(email) = patch.get("person_email").and_then(Value::as_str) {
                format!("Assign this task to {email}.")
            } else if patch.get("person_email").is_some() || patch.get("agent_label").is_some() {
                // Both keys are present-but-null only when clearing the owner.
                "Unassign this task.".to_string()
            } else {
                unrecognised(change)
            }
        }
        ("artifact", "create") => {
            let parent_type = s("parent_type");
            let parent = if parent_type.is_empty() {
                String::new()
            } else {
                format!(" to {parent_type} {}", short(&s("parent_id")))
            };
            let title = s("title");
            let named = if title.is_empty() {
                String::new()
            } else {
                format!(" \"{title}\"")
            };
            format!("Attach a {}{named}: {}{parent}.", s("kind"), s("url"))
        }
        _ => unrecognised(change),
    }
}

/// Honest rather than clever: name the operation, and list the fields it would
/// set so the reviewer can still judge it.
fn unrecognised(change: &Value) -> String {
    let keys: Vec<String> = change["patch"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    let fields = if keys.is_empty() {
        "no fields".to_string()
    } else {
        keys.join(", ")
    };
    format!(
        "An unfamiliar {} on this {}, setting: {fields}.",
        string(change, "op"),
        string(change, "targetType"),
    )
}

fn string(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// A patch value that is not a string — a position, a priority — rendered
/// without the JSON quoting.
fn scalar(value: &Value, key: &str) -> String {
    match value.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn short(id: &str) -> String {
    id.get(..8).unwrap_or(id).to_string()
}
