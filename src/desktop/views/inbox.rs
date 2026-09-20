//! The approval inbox: what agents have proposed, in plain language, with the
//! two buttons that resolve it.
//!
//! The list lives under the key `"inbox"` because the top bar's badge counts
//! that array. Mutations use `inbox:approve` / `inbox:reject`, and a finished
//! mutation invalidates the board and task caches too, since approving a
//! proposal replays a real write behind them.

use egui::{Color32, CornerRadius, Margin, RichText, Stroke};
use serde_json::Value;

use crate::desktop::{theme, App};

const APPROVE: &str = "inbox:approve";
const REJECT: &str = "inbox:reject";

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
        ui.add_space(12.0);
        banner(ui, &format!("That did not go through. {err}"), theme::DANGER);
    }

    if let Some(err) = &list_error {
        ui.add_space(12.0);
        banner(ui, &format!("Could not load the queue. {err}"), theme::DANGER);
        return;
    }

    if changes.is_empty() {
        ui.add_space(12.0);
        if list_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(quiet("Loading the queue"));
            });
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(64.0);
                ui.label(RichText::new("Nothing waiting for review.").size(14.0).color(theme::MUTED));
                ui.add_space(4.0);
                ui.label(quiet("Proposals from agents will appear here."));
            });
        }
        return;
    }

    ui.add_space(14.0);
    for change in &changes {
        card(ui, net, change, can_write, busy);
        ui.add_space(10.0);
    }
}

fn header(ui: &mut egui::Ui, count: usize, can_write: bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Pending approvals").size(18.0).strong());
        if count > 0 {
            ui.add_space(2.0);
            theme::pill(ui, &count.to_string(), theme::ACCENT);
        }
    });
    ui.add_space(2.0);
    let note = if can_write {
        "Proposals wait here until a person decides."
    } else {
        "Read only. This token can see the queue but not decide on it."
    };
    ui.label(quiet(note));
}

fn card(ui: &mut egui::Ui, net: &mut crate::desktop::net::Net, change: &Value, can_write: bool, busy: bool) {
    let id = string(change, "id");
    let target_type = string(change, "targetType");
    let target_id = string(change, "targetId");
    let actor = string(change, "actor");
    let sentence = describe(change);

    egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(1.0, theme::LINE))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.label(RichText::new(sentence).size(15.0).color(theme::TEXT));
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                theme::pill(ui, &target_type, target_colour(&target_type));
                theme::id_label(ui, &target_id);
                ui.label(quiet("·"));
                ui.label(quiet(&format!("proposed by {actor}")));

                if !can_write {
                    return;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let approve = egui::Button::new(
                        RichText::new("Approve").size(12.0).color(Color32::WHITE),
                    )
                    .fill(theme::ACCENT)
                    .corner_radius(CornerRadius::same(6));
                    if ui.add_enabled(!busy, approve).clicked() {
                        net.post(APPROVE, &format!("/api/user/changes/{id}/approve"), Value::Null);
                    }

                    let reject = egui::Button::new(
                        RichText::new("Reject").size(12.0).color(theme::DANGER),
                    )
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, theme::LINE))
                    .corner_radius(CornerRadius::same(6));
                    if ui.add_enabled(!busy, reject).clicked() {
                        net.post(REJECT, &format!("/api/user/changes/{id}/reject"), Value::Null);
                    }
                });
            });
        });
}

fn target_colour(target_type: &str) -> Color32 {
    match target_type {
        "task" => theme::ACCENT,
        "artifact" => theme::AGENT,
        _ => theme::MUTED,
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

fn quiet(text: &str) -> RichText {
    RichText::new(text).size(12.0).color(theme::MUTED)
}

fn banner(ui: &mut egui::Ui, text: &str, colour: Color32) {
    egui::Frame::new()
        .fill(colour.gamma_multiply(0.08))
        .stroke(Stroke::new(1.0, colour.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).size(12.0).color(colour));
        });
}
