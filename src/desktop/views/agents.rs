//! Agents: the programs on your own machine that take the tasks you hand off.
//!
//! An agent is a row with its own token, not a token with a label. This page
//! lists yours, connects a new one, rotates a token and revokes. Connecting
//! and rotating both end the same way: the server returns a one-time setup
//! prompt, with the token inside it, and this is the only time it is shown.
//! It lives in this view's local state — never in the net cache — and is
//! forgotten the moment the dialog closes.

use std::cell::RefCell;

use egui::RichText;
use serde_json::{json, Value};

use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{cards as c, colour, pad, radius, shell, size, space, text, theme, viz, widgets as w};
use crate::desktop::App;

/// My agents. Other views read it too: the task page, to offer a hand-off.
pub const AGENTS_KEY: &str = "agents:mine";
/// Every create, rotate and revoke goes out under this one key: only one can be
/// in flight, because each one is started from a dialog.
const ACTION_KEY: &str = "agents:action";
const TABLE: &str = "agents:table";

/// The runtimes the server knows, wire value first.
pub const RUNTIMES: [(&str, &str); 4] =
    [("hermes", "Hermes"), ("claude-code", "Claude Code"), ("codex", "Codex"), ("other", "Other")];

pub fn runtime_label(runtime: &str) -> &str {
    RUNTIMES.iter().find(|(v, _)| *v == runtime).map_or(runtime, |(_, l)| l)
}

/// A delegated task's `agentState`, in words.
pub fn state_words(state: &str) -> &str {
    match state {
        "handed_off" => "Handed off",
        "acknowledged" => "Acknowledged",
        "working" => "Working",
        "needs_input" => "Needs your input",
        "in_review" => "In review",
        "done" => "Done",
        "stopped" => "Stopped",
        other => other,
    }
}

/// Amber where the agent is waiting on you, purple while it waits on your
/// review, the task table's colours otherwise.
pub fn state_tone(state: &str) -> c::Tone {
    match state {
        "working" => c::Tone::Info,
        "needs_input" => c::Tone::Running,
        "in_review" => c::Tone::Agent,
        "done" => c::Tone::Ok,
        "stopped" => c::Tone::Quiet,
        _ => c::Tone::Neutral,
    }
}

fn status_words(status: &str) -> (&'static str, c::Tone) {
    match status {
        "connected" => ("Connected", c::Tone::Ok),
        "revoked" => ("Revoked", c::Tone::Quiet),
        _ => ("Waiting for first contact", c::Tone::Running),
    }
}

/// A handle as the server accepts it: lowercase letters, digits and dashes,
/// starting with a letter or digit.
pub fn valid_handle(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 40
        && !h.starts_with('-')
        && h.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

// ------------------------------------------------------------------ state

struct Draft {
    handle: String,
    name: String,
    runtime: String,
}

impl Default for Draft {
    fn default() -> Self {
        Self { handle: String::new(), name: String::new(), runtime: "hermes".into() }
    }
}

/// The one-time prompt, and who it is for.
struct Reveal {
    name: String,
    prompt: String,
    copied: bool,
}

#[derive(Clone)]
struct Confirm {
    id: String,
    name: String,
    revoke: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Pending {
    Create,
    Rotate,
    Revoke,
}

#[derive(Default)]
struct Local {
    connect: Option<Draft>,
    reveal: Option<Reveal>,
    confirm: Option<Confirm>,
    pending: Option<Pending>,
    /// The last action's failure, in the server's words.
    error: Option<String>,
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

// ------------------------------------------------------------------- page

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    LOCAL.with(|cell| render(app, ui, &mut cell.borrow_mut()));
}

const COLS: [Col; 6] = [
    Col::fill("Agent", 180.0),
    Col::left("Runtime", 96.0).rank(2),
    Col::left("Status", 168.0),
    Col::right("Last seen", 104.0).rank(1),
    Col::right("Active tasks", 84.0).rank(3),
    Col::right("", 176.0),
];

fn render(app: &mut App, ui: &mut egui::Ui, local: &mut Local) {
    let net = app.net.as_mut().expect("chrome runs signed in");
    net.get_once(AGENTS_KEY, "/api/user/agents");
    settle(net, local);

    let list = net.shared(AGENTS_KEY);
    let rows: &[Value] = list.as_deref().and_then(Value::as_array).map_or(&[], Vec::as_slice);
    let mut connect = false;

    shell::page_title(
        ui,
        "Agents",
        "Programs on your own machine that take the tasks you hand them.",
        |ui| {
            if !rows.is_empty() && w::primary(ui, "Connect an agent", true).clicked() {
                connect = true;
            }
        },
    );

    // Failures of the row actions. A failed create stays in its dialog.
    if local.connect.is_none() {
        if let Some(err) = &local.error {
            w::error(ui, err);
            ui.add_space(space::MD);
        }
    }

    if let Some(err) = net.error(AGENTS_KEY) {
        w::error(ui, &format!("Could not load your agents. {err}"));
    } else if list.is_none() {
        w::loading(ui, "Loading your agents");
    } else if rows.is_empty() {
        connect |= empty_state(ui);
    } else {
        let mut ask: Option<Confirm> = None;
        table::show(ui, TABLE, &COLS, rows.len(), |row, i| {
            if let Some(c) = agent_row(row, &rows[i]) {
                ask = Some(c);
            }
        });
        if ask.is_some() {
            local.error = None;
            local.confirm = ask;
        }
    }
    ui.add_space(space::XXL);

    if connect {
        local.error = None;
        local.connect = Some(Draft::default());
    }

    let ctx = ui.ctx().clone();
    connect_dialog(&ctx, net, local);
    confirm_dialog(&ctx, net, local);
}

/// No agents yet: what one is, in a sentence, and the way to connect one.
fn empty_state(ui: &mut egui::Ui) -> bool {
    let mut clicked = false;
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        ui.add_space(space::XL);
        ui.vertical_centered(|ui| {
            w::agent_mark(ui, size::AVATAR_LG);
            ui.add_space(space::MD);
            ui.label(
                RichText::new("No agents yet")
                    .size(text::HEADING)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
            ui.add_space(space::XS);
            ui.label(
                RichText::new(
                    "An agent is Hermes, Claude Code or Codex running on your machine: \
                     it takes the tasks you hand off and reports back on them here.",
                )
                .size(text::SMALL)
                .color(colour::TEXT_MUTED),
            );
            ui.add_space(space::LG);
            clicked = w::primary(ui, "Connect an agent", true).clicked();
        });
        ui.add_space(space::XL);
    });
    clicked
}

/// One agent. Returns a confirmation to ask for when an action was clicked.
fn agent_row(row: &mut table::Cells<'_, '_, '_>, a: &Value) -> Option<Confirm> {
    let status = str_of(a, "status").unwrap_or("waiting");
    let revoked = status == "revoked";
    let name = str_of(a, "name").unwrap_or("Agent");
    let mut ask = None;

    row.at(0, |ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        w::agent_mark(ui, size::AVATAR_SM - space::XS);
        // The handle is the agent's identity, so the name gives way first.
        let handle = str_of(a, "handle").unwrap_or_default();
        let handle_w = ui
            .painter()
            .layout_no_wrap(handle.to_owned(), egui::FontId::monospace(text::CAPTION), colour::TEXT_FAINT)
            .size()
            .x;
        let room = (ui.available_width() - handle_w - space::XL).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(room, table::ROW_H),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| table::strong_label(ui, name, if revoked { colour::TEXT_MUTED } else { colour::TEXT }),
        );
        w::mono_caption(ui, handle);
    });
    row.muted(1, runtime_label(str_of(a, "runtime").unwrap_or("other")));
    row.at(2, |ui| {
        let (words, tone) = status_words(status);
        c::chip(ui, words, tone, true);
    });
    let seen = str_of(a, "lastSeenAt").map(super::task::ago).unwrap_or_default();
    row.muted(3, &seen);
    let active = a.get("activeTasks").and_then(Value::as_i64).unwrap_or(0);
    row.text(4, &active.to_string(), if active > 0 { colour::TEXT } else { colour::TEXT_FAINT });
    row.at(5, |ui| {
        if revoked {
            return;
        }
        ui.spacing_mut().item_spacing.x = space::XS;
        let id = str_of(a, "id").unwrap_or_default().to_owned();
        if w::ghost(ui, "Revoke").clicked() {
            ask = Some(Confirm { id: id.clone(), name: name.to_owned(), revoke: true });
        }
        if w::ghost(ui, "Rotate token").clicked() {
            ask = Some(Confirm { id, name: name.to_owned(), revoke: false });
        }
    });
    ask
}

// ---------------------------------------------------------------- actions

/// Fold in the reply to the action started on an earlier frame.
fn settle(net: &mut crate::desktop::net::Net, local: &mut Local) {
    let Some(pending) = local.pending else { return };
    if net.is_loading(ACTION_KEY) {
        return;
    }
    match net.peek(ACTION_KEY) {
        Some(Ok(v)) => {
            local.error = None;
            if pending != Pending::Revoke {
                let prompt = str_of(v, "prompt").unwrap_or_default().to_owned();
                let name = v.get("agent").and_then(|a| str_of(a, "name")).unwrap_or("your agent").to_owned();
                local.connect = None;
                local.reveal = Some(Reveal { name, prompt, copied: false });
            }
        }
        Some(Err(e)) => local.error = Some(e.to_string()),
        None => {}
    }
    local.pending = None;
    // The reply holds the token; it is not kept anywhere but the dialog.
    net.invalidate(ACTION_KEY);
    net.invalidate(AGENTS_KEY);
    if pending == Pending::Revoke {
        // Revoking takes back every task the agent held.
        net.invalidate_prefix("task:");
        net.invalidate_prefix("board:");
        net.invalidate_prefix("mytasks");
        net.invalidate("home");
        net.invalidate(super::chrome::COUNTS);
    }
}

/// The dialogs' shared frame: the palette's surface, a little more padding.
pub(super) fn dialog(ctx: &egui::Context, id: &str, width: f32, add: impl FnOnce(&mut egui::Ui)) -> egui::ModalResponse<()> {
    egui::Modal::new(egui::Id::new(id))
        .backdrop_color(colour::CANVAS.gamma_multiply(0.7))
        .frame(
            egui::Frame::new()
                .fill(colour::SURFACE)
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG))
                .corner_radius(radius::LG)
                .inner_margin(egui::Margin::same(space::XL as i8)),
        )
        .show(ctx, |ui| {
            ui.set_width(width);
            add(ui);
        })
}

pub(super) fn heading(ui: &mut egui::Ui, s: &str) {
    ui.label(
        RichText::new(s)
            .size(text::CARD)
            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
            .color(colour::TEXT),
    );
}

pub(super) const DIALOG_W: f32 = 460.0;

/// Connect an agent, then — on the reply — its one-time prompt. Rotate lands
/// in the second half too.
fn connect_dialog(ctx: &egui::Context, net: &mut crate::desktop::net::Net, local: &mut Local) {
    if let Some(reveal) = local.reveal.as_mut() {
        let mut done = false;
        // No backdrop dismissal here: a stray click would lose the only copy.
        dialog(ctx, "agents:reveal", DIALOG_W, |ui| {
            heading(ui, &format!("Paste this into {}", reveal.name));
            ui.add_space(space::XS);
            ui.label(
                RichText::new("Shown only once \u{2014} it contains the agent\u{2019}s secret token.")
                    .size(text::SMALL)
                    .color(colour::WARN),
            );
            ui.add_space(space::MD);
            egui::Frame::new()
                .fill(colour::LOG_BG)
                .stroke(egui::Stroke::new(1.0, colour::LINE))
                .corner_radius(radius::MD)
                .inner_margin(egui::Margin::symmetric(pad::CARD.0 as i8, pad::CARD.1 as i8))
                .show(ui, |ui| {
                    let mut shown: &str = &reveal.prompt;
                    ui.add(
                        egui::TextEdit::multiline(&mut shown)
                            .id(egui::Id::new("agents:prompt"))
                            .frame(egui::Frame::NONE)
                            .desired_width(f32::INFINITY)
                            .desired_rows(4)
                            .font(egui::FontId::monospace(text::SMALL))
                            .text_color(colour::LOG_TEXT),
                    );
                });
            ui.add_space(space::LG);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                let copy = if reveal.copied { "Copied" } else { "Copy" };
                if w::primary(ui, copy, true).clicked() {
                    ui.ctx().copy_text(reveal.prompt.clone());
                    reveal.copied = true;
                }
                if w::ghost(ui, "Done").clicked() {
                    done = true;
                }
            });
        });
        if done {
            local.reveal = None;
        }
        return;
    }

    let Some(draft) = local.connect.as_mut() else { return };
    let busy = local.pending.is_some();
    let mut create = false;
    let mut close = false;

    let modal = dialog(ctx, "agents:connect", DIALOG_W, |ui| {
        heading(ui, "Connect an agent");
        ui.add_space(space::XS);
        w::muted(ui, "It gets its own token and sees only the tasks you hand it.");
        ui.add_space(space::LG);

        let typed = draft.handle.trim().to_owned();
        let bad = !typed.is_empty() && !valid_handle(&typed);
        let entry = w::field(ui, "Handle", &mut draft.handle, false, "hermes");
        if bad {
            ui.painter().rect_stroke(
                entry.rect,
                radius::SM as f32,
                egui::Stroke::new(1.0, colour::DANGER),
                egui::StrokeKind::Inside,
            );
            ui.add_space(space::XXS);
            w::caption(ui, "Lowercase letters, digits and dashes \u{2014} like hermes or claude-mac.");
        }
        ui.add_space(space::MD);
        w::field(ui, "Display name", &mut draft.name, false, "Hermes (Anmol\u{2019}s Mac)");
        ui.add_space(space::MD);
        w::caption(ui, "Runtime");
        ui.add_space(space::XXS);
        let others: Vec<(String, String)> = RUNTIMES
            .iter()
            .filter(|(v, _)| *v != draft.runtime)
            .map(|(v, l)| ((*v).to_owned(), (*l).to_owned()))
            .collect();
        let mut slot: Option<String> = None;
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.x = size::PICKER_W;
            viz::value_select(ui, runtime_label(&draft.runtime), &others, &mut slot);
        });
        if let Some(rt) = slot {
            draft.runtime = rt;
        }

        if let Some(err) = &local.error {
            ui.add_space(space::MD);
            w::error(ui, err);
        }
        ui.add_space(space::XL);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            let ready = valid_handle(&typed) && !draft.name.trim().is_empty() && !busy;
            let label = if busy { "Creating\u{2026}" } else { "Create" };
            let response = w::primary(ui, label, ready);
            if !busy && !ready {
                response.clone().on_disabled_hover_text("Needs a valid handle and a name.");
            }
            create = response.clicked();
            if w::ghost(ui, "Cancel").clicked() {
                close = true;
            }
        });
    });

    if create {
        local.error = None;
        net.invalidate(ACTION_KEY);
        net.post(
            ACTION_KEY,
            "/api/user/agents",
            json!({
                "handle": draft.handle.trim(),
                "name": draft.name.trim(),
                "runtime": draft.runtime,
            }),
        );
        local.pending = Some(Pending::Create);
    }
    if (close || modal.should_close()) && !busy {
        local.connect = None;
        local.error = None;
    }
}

/// "Are you sure" for rotate and revoke. Both cut off the agent's current
/// token, so neither happens on one click.
fn confirm_dialog(ctx: &egui::Context, net: &mut crate::desktop::net::Net, local: &mut Local) {
    let Some(ask) = local.confirm.clone() else { return };
    let mut go = false;
    let mut close = false;
    let modal = dialog(ctx, "agents:confirm", DIALOG_W * 0.8, |ui| {
        if ask.revoke {
            heading(ui, &format!("Revoke {}?", ask.name));
            ui.add_space(space::XS);
            w::muted(ui, "Its token stops working now, and any task it holds comes back to you.");
        } else {
            heading(ui, &format!("Rotate the token for {}?", ask.name));
            ui.add_space(space::XS);
            w::muted(ui, "The current token stops working now. You get a new setup prompt to paste into the agent.");
        }
        ui.add_space(space::XL);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            go = if ask.revoke {
                w::danger(ui, "Revoke", true).clicked()
            } else {
                w::primary(ui, "Rotate token", true).clicked()
            };
            close = w::ghost(ui, "Cancel").clicked();
        });
    });
    if go {
        net.invalidate(ACTION_KEY);
        if ask.revoke {
            net.send(ACTION_KEY, reqwest::Method::DELETE, &format!("/api/user/agents/{}", ask.id), Value::Null);
            local.pending = Some(Pending::Revoke);
        } else {
            net.post(ACTION_KEY, &format!("/api/user/agents/{}/rotate", ask.id), json!({}));
            local.pending = Some(Pending::Rotate);
        }
        local.confirm = None;
    } else if close || modal.should_close() {
        local.confirm = None;
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::valid_handle;

    #[test]
    fn handles() {
        for ok in ["hermes", "claude-mac", "codex2", "a"] {
            assert!(valid_handle(ok), "{ok}");
        }
        for bad in ["", "Hermes", "my agent", "-x", "under_score", "émile"] {
            assert!(!valid_handle(bad), "{bad}");
        }
    }
}
