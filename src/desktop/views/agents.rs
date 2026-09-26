//! Agents: the programs on your own machine that take the tasks you hand off.
//!
//! An agent is a row with its own token, not a token with a label. This page
//! shows yours as cards — who it is, what it is doing, whether its setup took
//! — connects a new one, rotates a token and revokes.
//!
//! Connecting is one dialog in three steps: name it, paste the prompt, wait
//! for it to say hello. The server returns the prompt once, with the token
//! inside it; it lives in this view's local state — never in the net cache —
//! and is forgotten when the dialog moves on. The last step can be left and
//! picked up again from the agent's card.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use egui::RichText;
use serde_json::{json, Value};

use crate::desktop::design::agent::{self as face, Presence, Step};
use crate::desktop::design::{
    cards as c, colour, glyph, motion, pad, radius, shell, size, space, text, theme, viz,
    widgets as w,
};
use crate::desktop::net::Net;
use crate::desktop::{App, Tab};

/// My agents. Other views read it too: the task page, to offer a hand-off.
pub const AGENTS_KEY: &str = "agents:mine";
/// Every create, rotate and revoke goes out under this one key: only one can be
/// in flight, because each one is started from a dialog.
const ACTION_KEY: &str = "agents:action";

/// The runtimes the server knows, wire value first, then how the connect
/// flow describes each in a line.
pub const RUNTIMES: [(&str, &str); 4] = [
    ("hermes", "Hermes"),
    ("claude-code", "Claude Code"),
    ("codex", "Codex"),
    ("other", "Other"),
];

const RUNTIME_LINES: [&str; 4] = [
    "Wakes by itself when a task needs it.",
    "Anthropic\u{2019}s coding agent, in your terminal.",
    "OpenAI\u{2019}s coding agent, in your terminal.",
    "Anything that can call an HTTP API.",
];

/// How often the last step asks whether the agent has said hello.
const HELLO_POLL: Duration = Duration::from_secs(2);

pub fn runtime_label(runtime: &str) -> &str {
    RUNTIMES
        .iter()
        .find(|(v, _)| *v == runtime)
        .map_or(runtime, |(_, l)| l)
}

/// A delegated task's `agentState`, in words.
pub fn state_words(state: &str) -> &str {
    match state {
        "handed_off" => "Handed off",
        "acknowledged" => "Acknowledged",
        "plan_review" => "Plan review",
        "working" => "Working",
        "needs_input" => "Needs input",
        "in_review" => "In review",
        "done" => "Done",
        "stopped" => "Stopped",
        other => other,
    }
}

/// Amber where the agent is waiting on you — a question or a plan — purple
/// while it waits on your review, the task table's colours otherwise.
pub fn state_tone(state: &str) -> c::Tone {
    match state {
        "working" => c::Tone::Info,
        "needs_input" | "plan_review" => c::Tone::Running,
        "in_review" => c::Tone::Agent,
        "done" => c::Tone::Ok,
        "stopped" => c::Tone::Quiet,
        _ => c::Tone::Neutral,
    }
}

pub(super) fn status_words(status: &str) -> (&'static str, c::Tone) {
    match status {
        "connected" => ("Connected", c::Tone::Ok),
        "revoked" => ("Revoked", c::Tone::Quiet),
        _ => ("Waiting for first contact", c::Tone::Running),
    }
}

/// An agent's two roles, each switched on or off by its owner.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    /// Takes the tasks its owner hands off (`canWork`).
    Work,
    /// Files tasks for its owner from what it reads (`canIntake`).
    Intake,
}

/// Whether an agent has a role. A payload from before roles has no
/// `canWork`: every agent then took hand-offs.
pub fn has(a: &Value, role: Role) -> bool {
    match role {
        Role::Work => a.get("canWork").and_then(Value::as_bool).unwrap_or(true),
        Role::Intake => a.get("canIntake").and_then(Value::as_bool).unwrap_or(false),
    }
}

/// An agent a task can be handed to: live, and one that works on tasks. The
/// server refuses the rest; every hand-off picker lists only these.
pub fn takes_work(a: &Value) -> bool {
    str_of(a, "status") != Some("revoked") && has(a, Role::Work)
}

/// Its roles as chips: "Works on tasks", "Files tasks".
pub(super) fn role_chips(ui: &mut egui::Ui, a: &Value) {
    if has(a, Role::Work) {
        c::chip(ui, "Works on tasks", c::Tone::Neutral, false)
            .on_hover_text("Takes the tasks you hand off and reports back on them.");
    }
    if has(a, Role::Intake) {
        c::chip(ui, "Files tasks", c::Tone::Info, false)
            .on_hover_text("Creates tasks for you from what it reads; they land in your Triage.");
    }
}

/// The ⋯ menu's role switches, for the owner. Returns the role flipped and
/// whether it is now on; the server refuses what would leave it no role.
pub(super) fn role_items(ui: &mut egui::Ui, a: &Value) -> Option<(Role, bool)> {
    let mut asked = None;
    for (role, on_label, off_label) in [
        (Role::Work, "Stop taking hand-offs", "Take tasks I hand off"),
        (
            Role::Intake,
            "Stop creating tasks",
            "Allow it to create tasks",
        ),
    ] {
        let on = has(a, role);
        if viz::menu_item(ui, if on { on_label } else { off_label }, false, None) {
            asked = Some((role, !on));
        }
    }
    asked
}

/// A handle as the server accepts it: lowercase letters, digits and dashes,
/// starting with a letter or digit.
pub fn valid_handle(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 40
        && !h.starts_with('-')
        && h.chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

// ------------------------------------------------------------------ state

struct Draft {
    handle: String,
    name: String,
    runtime: String,
    /// It takes the tasks you hand off.
    work: bool,
    /// It may file tasks for you from what it reads (intake).
    intake: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            handle: String::new(),
            name: String::new(),
            runtime: "hermes".into(),
            work: true,
            intake: false,
        }
    }
}

/// Where the connect dialog is.
enum Connect {
    /// 1: name, handle, runtime.
    Details(Draft),
    /// 2: the one-time prompt.
    Prompt {
        id: String,
        name: String,
        prompt: String,
        copied: bool,
    },
    /// 3: waiting for the agent's hello; `heard` is when it arrived (egui
    /// time), which the checklist's entrance is timed from.
    Hello {
        id: String,
        name: String,
        polled: Option<Instant>,
        heard: Option<f64>,
    },
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
    /// A role switched on or off from a card or the agent page.
    Role,
}

/// An agent's page, open over the list: which agent, the tab showing, and
/// where Back goes — `None` for the list, or the tab and task it was opened
/// from elsewhere in the app.
pub(super) struct Opened {
    pub id: String,
    pub back: Option<(Tab, Option<String>)>,
    pub tab: usize,
}

#[derive(Default)]
struct Local {
    /// The agent page on screen, if one is.
    open: Option<Opened>,
    /// Set by `open` from another view; `follow` moves the app to it.
    nav: bool,
    /// An action landed: the open page fetches its agent again, in place.
    refresh_page: bool,
    connect: Option<Connect>,
    confirm: Option<Confirm>,
    pending: Option<Pending>,
    /// The last action's failure, in the server's words.
    error: Option<String>,
    revoked_open: bool,
    /// What to say once a role switch lands.
    role_said: Option<String>,
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

// ------------------------------------------------------------------- page

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    LOCAL.with(|cell| render(app, ui, &mut cell.borrow_mut()));
}

/// Open an agent's page from anywhere — its name on a task, say. The page
/// takes over on the next frame, through `follow`; Back returns to where
/// this was called from.
pub fn open(id: &str) {
    LOCAL.with(|cell| {
        let mut l = cell.borrow_mut();
        l.open = Some(Opened {
            id: id.to_owned(),
            back: None,
            tab: 0,
        });
        l.nav = true;
    });
}

/// Move to a page `open` asked for, remembering where the app was. Called by
/// the chrome once a frame, before any view draws.
pub fn follow(app: &mut App) {
    LOCAL.with(|cell| {
        let mut l = cell.borrow_mut();
        if !std::mem::take(&mut l.nav) {
            return;
        }
        if let Some(o) = l.open.as_mut() {
            o.back = Some((app.tab, app.task.take()));
            app.tab = Tab::Agents;
        }
    });
}

/// Leave any agent page: the sidebar was used.
pub fn close() {
    LOCAL.with(|cell| cell.borrow_mut().open = None);
}

/// The agent page on screen, if any.
pub fn opened() -> Option<String> {
    LOCAL.with(|cell| cell.borrow().open.as_ref().map(|o| o.id.clone()))
}

/// What a card asked for.
enum CardAct {
    Confirm(Confirm),
    Continue(String, String),
    Open(String),
    /// Switch a role: (id, name, role, on).
    Role(String, String, Role, bool),
    /// The agent's page.
    Page(String),
}

fn render(app: &mut App, ui: &mut egui::Ui, local: &mut Local) {
    let me = app
        .net
        .as_ref()
        .and_then(|n| n.data("__me"))
        .cloned()
        .unwrap_or(Value::Null);
    let my_seed = str_of(&me, "email")
        .or_else(|| str_of(&me, "name"))
        .unwrap_or("me")
        .to_owned();
    let my_first = str_of(&me, "name")
        .and_then(|n| n.split_whitespace().next())
        .unwrap_or("Your")
        .to_owned();

    let net = app.net.as_mut().expect("chrome runs signed in");
    net.get_once(AGENTS_KEY, "/api/user/agents");
    settle(ui.ctx(), net, local);

    let list = net.shared(AGENTS_KEY);
    let all: &[Value] = list
        .as_deref()
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);

    if let Some(opened) = local.open.as_mut() {
        let listed = all
            .iter()
            .find(|a| str_of(a, "id") == Some(opened.id.as_str()))
            .cloned();
        let refresh = std::mem::take(&mut local.refresh_page);
        let asked = super::agent_page::show(
            ui,
            net,
            opened,
            listed,
            refresh,
            local.error.as_deref(),
            &my_seed,
        );
        let id = opened.id.clone();
        match asked {
            Some(super::agent_page::Ask::Back) => {
                if let Some((tab, task)) = local.open.take().and_then(|o| o.back) {
                    app.tab = tab;
                    app.task = task;
                }
            }
            Some(super::agent_page::Ask::Task(task)) => {
                app.task = Some(task);
                app.tab = Tab::Agents;
            }
            Some(super::agent_page::Ask::Card(name, act)) => {
                let act = match act {
                    super::agent_page::CardAsk::Continue => CardAct::Continue(id, name),
                    super::agent_page::CardAsk::Role(role, on) => CardAct::Role(id, name, role, on),
                    super::agent_page::CardAsk::Rotate => CardAct::Confirm(Confirm {
                        id,
                        name,
                        revoke: false,
                    }),
                    super::agent_page::CardAsk::Revoke => CardAct::Confirm(Confirm {
                        id,
                        name,
                        revoke: true,
                    }),
                };
                card_act(app, local, act);
            }
            None => {}
        }
        let ctx = ui.ctx().clone();
        let net = app.net.as_mut().expect("chrome runs signed in");
        connect_dialog(&ctx, net, local, &my_seed, &my_first);
        confirm_dialog(&ctx, net, local);
        return;
    }

    let live: Vec<&Value> = all
        .iter()
        .filter(|a| str_of(a, "status") != Some("revoked"))
        .collect();
    let revoked: Vec<&Value> = all
        .iter()
        .filter(|a| str_of(a, "status") == Some("revoked"))
        .collect();
    let mut connect = false;

    shell::page_title(
        ui,
        "Agents",
        "Programs on your own machine that take the tasks you hand them.",
        |ui| {
            if !live.is_empty() && w::primary(ui, "Connect an agent", true).clicked() {
                connect = true;
            }
        },
    );

    // Failures of the card actions. A failed create stays in its dialog.
    if local.connect.is_none() {
        if let Some(err) = &local.error {
            w::error(ui, err);
            ui.add_space(space::MD);
        }
    }

    let mut act: Option<CardAct> = None;
    if let Some(err) = net.error(AGENTS_KEY) {
        w::error(ui, &format!("Could not load your agents. {err}"));
    } else if list.is_none() {
        skeleton_cards(ui);
    } else if live.is_empty() {
        connect |= empty_state(ui, &my_seed);
    } else {
        act = grid(ui, &live, &my_seed);
    }

    if !revoked.is_empty() {
        ui.add_space(space::XL);
        face::disclosure(
            ui,
            egui::Id::new("agents:revoked"),
            "Revoked",
            Some(revoked.len()),
            &mut local.revoked_open,
        );
        if local.revoked_open {
            ui.add_space(space::XS);
            w::card_list(ui, |ui| {
                ui.set_width(ui.available_width());
                for a in &revoked {
                    revoked_row(ui, a, &my_seed);
                }
            });
        }
    }
    ui.add_space(space::XXL);

    if let Some(act) = act {
        card_act(app, local, act);
    }
    if connect {
        local.error = None;
        local.connect = Some(Connect::Details(Draft::default()));
    }

    let ctx = ui.ctx().clone();
    let net = app.net.as_mut().expect("chrome runs signed in");
    connect_dialog(&ctx, net, local, &my_seed, &my_first);
    confirm_dialog(&ctx, net, local);
}

/// Carry out what a card, or the agent page's menu, asked for.
fn card_act(app: &mut App, local: &mut Local, act: CardAct) {
    match act {
        CardAct::Confirm(ask) => {
            local.error = None;
            local.confirm = Some(ask);
        }
        CardAct::Continue(id, name) => {
            local.error = None;
            local.connect = Some(Connect::Hello {
                id,
                name,
                polled: None,
                heard: None,
            });
        }
        CardAct::Open(task) => {
            app.task = Some(task);
            app.tab = Tab::Agents;
        }
        CardAct::Role(id, name, role, on) => {
            let net = app.net.as_mut().expect("chrome runs signed in");
            local.error = None;
            net.invalidate(ACTION_KEY);
            let body = match role {
                Role::Work => json!({ "canWork": on }),
                Role::Intake => json!({ "canIntake": on }),
            };
            net.patch(ACTION_KEY, &format!("/api/user/agents/{id}"), body);
            local.pending = Some(Pending::Role);
            local.role_said = Some(match (role, on) {
                (Role::Work, true) => format!("{name} takes the tasks you hand off now."),
                (Role::Work, false) => format!("{name} no longer takes hand-offs."),
                (Role::Intake, true) => format!("{name} can create tasks for you now."),
                (Role::Intake, false) => format!("{name} no longer creates tasks for you."),
            });
        }
        CardAct::Page(id) => {
            local.error = None;
            local.open = Some(Opened {
                id,
                back: None,
                tab: 0,
            });
        }
    }
}

/// No agents yet: what one is, in a sentence, and the way to connect one.
fn empty_state(ui: &mut egui::Ui, seed: &str) -> bool {
    let mut clicked = false;
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        ui.add_space(space::XL);
        ui.vertical_centered(|ui| {
            face::avatar(ui, seed, face::LG, Presence::Idle, "Your agent");
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

/// Where the cards will be, while they load.
fn skeleton_cards(ui: &mut egui::Ui) {
    let per_row = per_row(ui.available_width());
    ui.columns(per_row, |cols| {
        for col in cols {
            c::surface(col, false, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    face::skeleton(ui, face::LG, face::LG);
                    ui.vertical(|ui| {
                        face::skeleton(ui, 120.0, text::BODY);
                        face::skeleton(ui, 80.0, text::SMALL);
                    });
                });
                ui.add_space(space::LG);
                face::skeleton(ui, 180.0, text::SMALL);
                ui.add_space(space::SM);
                face::skeleton(ui, 140.0, text::SMALL);
            });
        }
    });
}

fn per_row(width: f32) -> usize {
    if width >= 900.0 {
        3
    } else if width >= 560.0 {
        2
    } else {
        1
    }
}

/// The cards, a few to a row, each row one height: a row of cards of four
/// different heights reads as four unrelated things. Last frame's tallest is
/// the floor, as `viz::row` does it.
fn grid(ui: &mut egui::Ui, agents: &[&Value], my_seed: &str) -> Option<CardAct> {
    let n = per_row(ui.available_width());
    let mut act = None;
    for (r, chunk) in agents.chunks(n).enumerate() {
        let id = egui::Id::new(("agents:row-h", n, r));
        let floor: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
        let mut tallest = 0.0_f32;
        ui.columns(n, |cols| {
            for (col, a) in cols.iter_mut().zip(chunk) {
                let (h, asked) = card(col, a, my_seed, floor);
                tallest = tallest.max(h);
                act = act.take().or(asked);
            }
        });
        if (tallest - floor).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(id, tallest));
            ui.ctx().request_repaint();
        }
        ui.add_space(space::MD);
    }
    act
}

/// One agent: face and name, what it is on, and whether it is set up.
fn card(ui: &mut egui::Ui, a: &Value, my_seed: &str, floor: f32) -> (f32, Option<CardAct>) {
    let id = str_of(a, "id").unwrap_or_default().to_owned();
    let name = str_of(a, "name").unwrap_or("Agent").to_owned();
    let status = str_of(a, "status").unwrap_or("waiting");
    let waiting = status != "connected";
    let current = a.get("currentTask").filter(|t| t.is_object());
    let task_state = current.and_then(|t| str_of(t, "state")).unwrap_or("idle");
    let presence = Presence::of(task_state, str_of(a, "lastSeenAt"));
    let intake = has(a, Role::Intake);
    let mut act = None;
    let mut used = 0.0;

    // The whole card opens the agent's page. Registered before what is on it,
    // on last frame's rect, so the menu and the task link inside stay on top
    // and keep their own clicks.
    let rect_id = egui::Id::new(("agents:card", id.as_str()));
    let last: egui::Rect = ui
        .ctx()
        .data(|d| d.get_temp(rect_id))
        .unwrap_or(egui::Rect::NOTHING);
    let whole = ui.interact(last, rect_id.with("click"), egui::Sense::click());
    let open_label = format!("Open {name}");
    whole.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &open_label));
    let whole = motion::operable(ui, whole, radius::LG as f32);
    if whole.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if whole.clicked() {
        act = Some(CardAct::Page(id.clone()));
    }
    let hot = whole.hovered() || whole.has_focus();

    let frame = c::surface(ui, hot, |ui| {
        ui.set_width(ui.available_width());
        // Every card in a row as tall as the tallest.
        ui.set_min_height(floor);
        used = ui
            // Top-down and left-aligned: `ui.columns` hands out a justified
            // layout, which spread a wrapped sentence across its line.
            .with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = space::SM;
                // ---- face, name, menu
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::MD;
                    if waiting {
                        face::avatar_still(ui, my_seed, face::LG, Presence::Waiting, &name);
                    } else {
                        face::avatar(ui, my_seed, face::LG, presence, &name);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        viz::more(ui, |ui| {
                            if waiting && viz::menu_item(ui, "Continue setup", false, None) {
                                act = Some(CardAct::Continue(id.clone(), name.clone()));
                            }
                            if viz::menu_item(ui, "Copy handle", false, None) {
                                ui.ctx().copy_text(str_of(a, "handle").unwrap_or_default().to_owned());
                                w::toast(ui.ctx(), "Copied.", false);
                            }
                            if let Some((role, on)) = role_items(ui, a) {
                                act = Some(CardAct::Role(id.clone(), name.clone(), role, on));
                            }
                            if viz::menu_item(ui, "Rotate token\u{2026}", false, None) {
                                act = Some(CardAct::Confirm(Confirm { id: id.clone(), name: name.clone(), revoke: false }));
                            }
                            viz::menu_rule(ui);
                            if viz::menu_item(ui, "Revoke\u{2026}", true, None) {
                                act = Some(CardAct::Confirm(Confirm { id: id.clone(), name: name.clone(), revoke: true }));
                            }
                        });
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.y = space::XXS;
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&name)
                                        .size(text::BODY)
                                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                                        .color(colour::TEXT),
                                )
                                .truncate(),
                            );
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::XS;
                                w::mono_caption(ui, str_of(a, "handle").unwrap_or_default());
                                w::caption(ui, &format!("\u{00B7} {}", runtime_label(str_of(a, "runtime").unwrap_or("other"))));
                            });
                        });
                    });
                });

                // ---- status and last contact
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::SM;
                    let (words, tone) = status_words(status);
                    c::chip(ui, words, tone, true);
                    role_chips(ui, a);
                    if let Some(at) = str_of(a, "lastSeenAt") {
                        ui.label(RichText::new(format!("seen {}", super::task::ago(at))).size(text::SMALL).color(colour::TEXT_MUTED))
                            .on_hover_text(super::task::exact(at));
                    }
                });

                // ---- what it is on
                ui.add_space(space::XS);
                if waiting {
                    w::muted(ui, "It has not said hello yet. Paste its setup prompt, or pick up where you left off.");
                    if w::secondary(ui, "Continue setup", true).clicked() {
                        act = Some(CardAct::Continue(id.clone(), name.clone()));
                    }
                } else if let Some(t) = current {
                    w::caption(ui, &state_words(task_state).to_string());
                    let title = str_of(t, "title").unwrap_or("Untitled");
                    let open = ui
                        .add(
                            egui::Label::new(RichText::new(title).size(text::SMALL).color(colour::TEXT))
                                .truncate()
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Open the task");
                    if open.clicked() {
                        act = str_of(t, "id").map(|i| CardAct::Open(i.to_owned()));
                    }
                    if let Some(now) = str_of(t, "now").map(str::trim).filter(|n| !n.is_empty()) {
                        ui.add(egui::Label::new(RichText::new(now).size(text::SMALL).color(colour::TEXT_MUTED)).truncate());
                    }
                } else if has(a, Role::Work) {
                    w::caption(ui, "No task right now");
                    ui.label(RichText::new("Hand it one from the task\u{2019}s page.").size(text::SMALL).color(colour::TEXT_MUTED));
                } else {
                    w::caption(ui, "Files tasks; it doesn\u{2019}t take hand-offs");
                }

                if intake {
                    intake_stats(ui, a);
                }

                // ---- activity and setup
                let days: Vec<f32> = a
                    .get("activity")
                    .and_then(Value::as_array)
                    .map(|d| d.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect())
                    .unwrap_or_default();
                let setup = a.get("setup").filter(|s| s.is_object());
                if !days.is_empty() || setup.is_some() {
                    ui.add_space(space::XS);
                    let (rule, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                    ui.painter().hline(rule.x_range(), rule.center().y, egui::Stroke::new(1.0, colour::LINE));
                }
                if !days.is_empty() {
                    ui.horizontal(|ui| {
                        w::caption(ui, &format!("Last {} days", days.len()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            face::activity(ui, &days);
                        });
                    });
                }
                if let Some(s) = setup {
                    setup_checks(ui, s, true);
                }
            })
            .response
            .rect
            .height();
    });
    ui.ctx()
        .data_mut(|d| d.insert_temp(rect_id, frame.response.rect));
    (used, act)
}

/// What an intake agent has filed: triage, accepted, dismissed — the numbers
/// in white, their words muted — and where its filings land.
fn intake_stats(ui: &mut egui::Ui, a: &Value) {
    let stats = a.get("intakeStats").filter(|s| s.is_object());
    let n = |k: &str| {
        stats
            .and_then(|s| s.get(k))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    ui.add_space(space::XS);
    let (rule, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rule.x_range(),
        rule.center().y,
        egui::Stroke::new(1.0, colour::LINE),
    );
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        for (i, (k, word)) in [
            ("triage", "in triage"),
            ("accepted", "accepted"),
            ("dismissed", "dismissed"),
        ]
        .iter()
        .enumerate()
        {
            if i > 0 {
                ui.add_space(space::SM);
            }
            ui.label(
                RichText::new(n(k).to_string())
                    .size(text::BODY)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
            ui.label(
                RichText::new(*word)
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            );
        }
    });
    w::caption(ui, "Files into your Triage, each task labelled Slack.");
}

/// The three things a good setup reports, as ticks, crosses and dashes.
/// `wrap` lays them out in a line for a card; otherwise one per row.
pub(super) fn setup_checks(ui: &mut egui::Ui, setup: &Value, wrap: bool) {
    let skill = setup.get("skill").and_then(Value::as_str);
    let flag = |k: &str| setup.get(k).and_then(Value::as_bool);
    let items = [
        (
            "Skill",
            skill.map(|_| true),
            skill
                .map(|v| format!("v{}", v.trim_start_matches('v')))
                .unwrap_or_default(),
        ),
        ("MCP", flag("mcp"), String::new()),
        ("Watcher", flag("watcher"), String::new()),
    ];
    if wrap {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::MD;
            for (label, ok, detail) in &items {
                face::check(ui, label, *ok, detail);
            }
        });
    } else {
        for (label, ok, detail) in &items {
            face::check(ui, label, *ok, detail);
        }
    }
}

fn revoked_row(ui: &mut egui::Ui, a: &Value, seed: &str) {
    let name = str_of(a, "name").unwrap_or("Agent");
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), size::ROW),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_height(size::ROW);
            ui.spacing_mut().item_spacing.x = space::SM;
            ui.add_space(space::SM);
            face::avatar_still(ui, seed, face::XS, Presence::Offline, name);
            ui.label(
                RichText::new(name)
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            );
            w::mono_caption(ui, str_of(a, "handle").unwrap_or_default());
            if let Some(at) = str_of(a, "lastSeenAt") {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(space::SM);
                    w::caption(ui, &format!("last seen {}", super::task::ago(at)));
                });
            }
        },
    );
}

// ---------------------------------------------------------------- actions

/// Fold in the reply to the action started on an earlier frame.
fn settle(ctx: &egui::Context, net: &mut Net, local: &mut Local) {
    let Some(pending) = local.pending else { return };
    if net.is_loading(ACTION_KEY) {
        return;
    }
    match net.peek(ACTION_KEY) {
        Some(Ok(_)) if pending == Pending::Role => {
            local.error = None;
            if let Some(said) = local.role_said.take() {
                w::toast(ctx, said, false);
            }
        }
        // A refused role switch is a sentence to read, not a page error.
        Some(Err(e)) if pending == Pending::Role => w::toast(ctx, e.to_string(), true),
        Some(Ok(v)) => {
            local.error = None;
            if pending != Pending::Revoke {
                let agent = v.get("agent").cloned().unwrap_or(Value::Null);
                local.connect = Some(Connect::Prompt {
                    id: str_of(&agent, "id").unwrap_or_default().to_owned(),
                    name: str_of(&agent, "name").unwrap_or("your agent").to_owned(),
                    prompt: str_of(v, "prompt").unwrap_or_default().to_owned(),
                    copied: false,
                });
            }
        }
        Some(Err(e)) => local.error = Some(e.to_string()),
        None => {}
    }
    local.pending = None;
    local.role_said = None;
    // The reply holds the token; it is not kept anywhere but the dialog.
    net.invalidate(ACTION_KEY);
    // In place, so the cards stay put under the dialog while it lands.
    net.get(AGENTS_KEY, "/api/user/agents");
    local.refresh_page = true;
    if pending == Pending::Revoke {
        // Revoking takes back every task the agent held.
        net.invalidate_prefix("task:");
        net.invalidate_prefix("board:");
        net.invalidate_prefix("mytasks");
        net.invalidate("home");
        net.invalidate(super::agent_session::ACTIVE_KEY);
        net.invalidate(super::chrome::COUNTS);
    }
}

/// The dialogs' shared frame: the palette's surface, a little more padding.
pub(super) fn dialog(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    add: impl FnOnce(&mut egui::Ui),
) -> egui::ModalResponse<()> {
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
/// The connect flow is wider: its first step lays the runtimes out two by two.
const CONNECT_W: f32 = 520.0;

pub(super) fn short(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name).trim()
}

/// The three steps, for the dialog's indicator.
fn connect_steps(at: usize, done: bool) -> impl FnOnce(&mut egui::Ui) {
    move |ui| {
        let steps = ["Name it", "Paste the prompt", "First contact"].map(|l| Step {
            label: l.into(),
            when: None,
        });
        face::stepper(ui, &steps, at, done, colour::INFO);
    }
}

/// The connect flow. Escape and Cancel are safe at every step: nothing is
/// created until step 1's Continue, and an agent left at step 2 or 3 waits on
/// its card with "Continue setup".
fn connect_dialog(
    ctx: &egui::Context,
    net: &mut Net,
    local: &mut Local,
    my_seed: &str,
    my_first: &str,
) {
    let busy = local.pending.is_some();
    let mut next: Option<Option<Connect>> = None;
    let mut rotate: Option<Confirm> = None;

    // Step 3 asks, every two seconds, whether the agent has said hello —
    // refetched in place, so the card behind the dialog does not blink.
    if let Some(Connect::Hello { polled, heard, .. }) = local.connect.as_mut() {
        if heard.is_none() {
            ctx.request_repaint_after(HELLO_POLL);
            if polled.is_none_or(|t| t.elapsed() >= HELLO_POLL - Duration::from_millis(100))
                && !net.is_loading(AGENTS_KEY)
            {
                net.get(AGENTS_KEY, "/api/user/agents");
                *polled = Some(Instant::now());
            }
        }
    }
    let agents = net.shared(AGENTS_KEY);

    let Some(step) = local.connect.as_mut() else {
        return;
    };
    let modal = match step {
        Connect::Details(draft) => dialog(ctx, "agents:connect", CONNECT_W, |ui| {
            heading(ui, "Connect an agent");
            ui.add_space(space::XS);
            w::muted(
                ui,
                "It gets its own token and sees only the tasks you hand it.",
            );
            ui.add_space(space::LG);
            connect_steps(0, false)(ui);
            ui.add_space(space::LG);

            w::caption(ui, "Runtime");
            ui.add_space(space::XXS);
            runtime_cards(ui, &mut draft.runtime);
            ui.add_space(space::MD);

            let label = runtime_label(&draft.runtime).to_owned();
            let name_field = w::field(
                ui,
                "Display name",
                &mut draft.name,
                false,
                &format!("{label} ({my_first}\u{2019}s Mac)"),
            );
            ui.add_space(space::MD);
            let typed = draft.handle.trim().to_owned();
            let bad = !typed.is_empty() && !valid_handle(&typed);
            let hint = if draft.runtime == "other" {
                "my-agent"
            } else {
                draft.runtime.as_str()
            };
            let entry = w::field(ui, "Handle", &mut draft.handle, false, hint);
            if bad {
                ui.painter().rect_stroke(
                    entry.rect,
                    radius::SM as f32,
                    egui::Stroke::new(1.0, colour::DANGER),
                    egui::StrokeKind::Inside,
                );
                ui.add_space(space::XXS);
                w::caption(
                    ui,
                    "Lowercase letters, digits and dashes \u{2014} like hermes or claude-mac.",
                );
            } else {
                ui.add_space(space::XXS);
                w::caption(ui, "How the agent is named in the API and its logs.");
            }

            ui.add_space(space::LG);
            w::caption(ui, "Roles");
            ui.add_space(space::XS);
            w::switch(
                ui,
                "Takes tasks I hand off",
                "It works on the tasks you hand it and reports back on each one.",
                &mut draft.work,
            );
            ui.add_space(space::SM);
            w::switch(
                ui,
                "Creates tasks for me",
                "It files what it reads \u{2014} a Slack thread, say \u{2014} as tasks in your Triage, for you to accept or dismiss.",
                &mut draft.intake,
            );
            let no_role = !draft.work && !draft.intake;
            if no_role {
                ui.add_space(space::XS);
                ui.label(
                    RichText::new("Turn on at least one role.")
                        .size(text::SMALL)
                        .color(colour::WARN),
                );
            }

            if let Some(err) = &local.error {
                ui.add_space(space::MD);
                w::error(ui, err);
            }
            ui.add_space(space::XL);
            let ready = valid_handle(&typed) && !draft.name.trim().is_empty() && !no_role && !busy;
            // Enter from either field submits: egui drops a single-line field's
            // focus on Enter, which is how it can be told from Enter on a button.
            let entered = (name_field.lost_focus() || entry.lost_focus())
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut create = ready && entered;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                let response = w::primary(
                    ui,
                    if busy { "Creating\u{2026}" } else { "Continue" },
                    ready,
                );
                if !busy && !ready {
                    let why = if no_role {
                        "Turn on at least one role."
                    } else {
                        "Needs a name and a valid handle."
                    };
                    response.clone().on_disabled_hover_text(why);
                }
                create |= response.clicked();
                if w::ghost(ui, "Cancel").clicked() && !busy {
                    next = Some(None);
                }
            });
            if create {
                local.error = None;
                net.invalidate(ACTION_KEY);
                net.post(
                    ACTION_KEY,
                    "/api/user/agents",
                    json!({ "handle": typed, "name": draft.name.trim(), "runtime": draft.runtime, "canWork": draft.work, "canIntake": draft.intake }),
                );
                local.pending = Some(Pending::Create);
            }
        }),

        Connect::Prompt {
            id,
            name,
            prompt,
            copied,
        } => dialog(ctx, "agents:connect", CONNECT_W, |ui| {
            heading(ui, &format!("Paste this into {}", short(name)));
            ui.add_space(space::LG);
            connect_steps(1, false)(ui);
            ui.add_space(space::LG);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                let (r, _) = ui.allocate_exact_size(
                    egui::Vec2::splat(text::SMALL + 1.0),
                    egui::Sense::hover(),
                );
                glyph::lock(ui.painter(), r.center(), r.width(), colour::WARN);
                ui.label(
                    RichText::new(
                        "Shown only once \u{2014} it contains the agent\u{2019}s secret token.",
                    )
                    .size(text::SMALL)
                    .color(colour::WARN),
                );
            });
            ui.add_space(space::SM);
            egui::Frame::new()
                .fill(colour::LOG_BG)
                .stroke(egui::Stroke::new(1.0, colour::LINE))
                .corner_radius(radius::MD)
                .inner_margin(egui::Margin::symmetric(
                    pad::CARD.0 as i8,
                    pad::CARD.1 as i8,
                ))
                .show(ui, |ui| {
                    let mut shown: &str = prompt;
                    egui::ScrollArea::vertical()
                        .max_height(size::ROW * 6.0)
                        .show(ui, |ui| {
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
                });
            ui.add_space(space::SM);
            w::muted(
                ui,
                &format!(
                    "Paste it into {} on your Mac. It sets itself up and says hello here.",
                    short(name)
                ),
            );
            ui.add_space(space::XL);
            let mut closed = false;
            button_row(
                ui,
                |ui| {
                    // One filled button: Copy until it is copied, then the way on.
                    let pasted = if *copied {
                        w::primary(ui, "I\u{2019}ve pasted it", true)
                    } else {
                        w::secondary(ui, "I\u{2019}ve pasted it", true)
                    };
                    if pasted.clicked() {
                        next = Some(Some(Connect::Hello {
                            id: id.clone(),
                            name: name.clone(),
                            polled: None,
                            heard: None,
                        }));
                    }
                    let copy = if *copied {
                        w::secondary(ui, "Copied", true)
                    } else {
                        w::primary(ui, "Copy", true)
                    };
                    if copy.clicked() {
                        ui.ctx().copy_text(prompt.clone());
                        *copied = true;
                    }
                },
                |ui| {
                    closed = w::ghost(ui, "Close").clicked();
                },
            );
            if closed {
                next = Some(None);
            }
        }),

        Connect::Hello {
            id, name, heard, ..
        } => {
            let agent = agents
                .as_deref()
                .and_then(Value::as_array)
                .and_then(|a| a.iter().find(|a| str_of(a, "id") == Some(id.as_str())));
            let connected = agent.is_some_and(|a| str_of(a, "status") == Some("connected"));
            let now = ctx.input(|i| i.time);
            if connected && heard.is_none() {
                *heard = Some(now);
            }
            dialog(ctx, "agents:connect", CONNECT_W, |ui| {
                let s = short(name);
                heading(
                    ui,
                    &if connected {
                        format!("{s} is connected")
                    } else {
                        format!("Waiting for {s} to say hello\u{2026}")
                    },
                );
                ui.add_space(space::LG);
                connect_steps(2, connected)(ui);
                ui.add_space(space::XL);
                ui.vertical_centered(|ui| {
                    let presence = if connected {
                        Presence::Idle
                    } else {
                        Presence::Waiting
                    };
                    face::avatar(ui, my_seed, face::XXL, presence, name);
                    ui.add_space(space::MD);
                    if connected {
                        w::muted(
                            ui,
                            "Said hello just now. Hand it a task from any task\u{2019}s page.",
                        );
                    } else {
                        w::muted(
                            ui,
                            "This page updates by itself, usually within a minute of pasting.",
                        );
                    }
                });
                if let (true, Some(at)) = (connected, *heard) {
                    ui.add_space(space::MD);
                    let setup = agent
                        .and_then(|a| a.get("setup"))
                        .filter(|s| s.is_object())
                        .cloned()
                        .unwrap_or(json!({}));
                    checklist(ui, &setup, now - at);
                }
                ui.add_space(space::XL);
                button_row(
                    ui,
                    |ui| {
                        if connected {
                            if w::primary(ui, "Done", true).clicked() {
                                next = Some(None);
                            }
                        } else if w::ghost(ui, "Close").clicked() {
                            next = Some(None);
                        }
                    },
                    |ui| {
                        if !connected && w::link(ui, "Lost the prompt? Get a new one").clicked() {
                            rotate = Some(Confirm {
                                id: id.clone(),
                                name: name.clone(),
                                revoke: false,
                            });
                        }
                    },
                );
            })
        }
    };

    if modal.should_close() && !busy {
        next = Some(None);
    }
    if let Some(n) = next {
        local.connect = n;
        local.error = None;
    }
    if let Some(r) = rotate {
        local.connect = None;
        local.confirm = Some(r);
    }
}

/// A dialog's foot: `right` flush right, `left` flush left, one row high.
/// A right-to-left layout on its own takes the rest of a modal's height and
/// centres its buttons in it.
fn button_row(
    ui: &mut egui::Ui,
    right: impl FnOnce(&mut egui::Ui),
    left: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.set_height(size::CONTROL);
        left(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            right(ui);
        });
    });
}

/// The runtimes as a two-by-two set of tiles: the name and a line on what it
/// is. The chosen one is raised and ticked — not outlined in blue.
fn runtime_cards(ui: &mut egui::Ui, runtime: &mut String) {
    let gap = space::SM;
    let tile_w = (ui.available_width() - gap) / 2.0;
    for pair in RUNTIMES
        .iter()
        .zip(RUNTIME_LINES)
        .collect::<Vec<_>>()
        .chunks(2)
    {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for ((value, label), line) in pair {
                let on = runtime == value;
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(tile_w, 56.0), egui::Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, *label)
                });
                let response = motion::operable(ui, response, radius::MD as f32);
                if response.clicked() {
                    *runtime = (*value).to_owned();
                }
                let hot = response.hovered() || response.has_focus();
                let fill = if on {
                    colour::SURFACE_ACTIVE
                } else {
                    motion::hover_fill(
                        ui,
                        response.id.with("fill"),
                        hot,
                        colour::INSET,
                        colour::SURFACE_HOVER,
                    )
                };
                let p = ui.painter();
                p.rect_filled(rect, radius::MD as f32, fill);
                p.rect_stroke(
                    rect,
                    radius::MD as f32,
                    egui::Stroke::new(
                        1.0,
                        if on || hot {
                            colour::LINE_STRONG
                        } else {
                            colour::LINE
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                let x = rect.left() + space::MD;
                p.text(
                    egui::pos2(x, rect.top() + space::MD),
                    egui::Align2::LEFT_TOP,
                    *label,
                    egui::FontId::new(text::BODY, egui::FontFamily::Name(theme::SEMIBOLD.into())),
                    if on { colour::TEXT } else { colour::TEXT_2 },
                );
                let galley = w::truncated(
                    ui,
                    line,
                    egui::FontId::proportional(text::CAPTION),
                    colour::TEXT_MUTED,
                    rect.width() - space::MD * 2.0,
                );
                ui.painter().galley(
                    egui::pos2(x, rect.bottom() - space::MD - galley.size().y),
                    galley,
                    colour::TEXT_MUTED,
                );
                if on {
                    let c =
                        egui::pos2(rect.right() - space::MD - 5.0, rect.top() + space::MD + 7.0);
                    glyph::tick(ui.painter(), c, 11.0, colour::TEXT);
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
        });
        ui.add_space(gap - ui.spacing().item_spacing.y);
    }
}

/// What the agent reported at hello, one line at a time: each fades in a beat
/// after the last. With reduced motion they are simply there.
fn checklist(ui: &mut egui::Ui, setup: &Value, since: f64) {
    const STAGGER: f64 = 0.12;
    const FADE: f64 = 0.18;
    let still = ui.style().animation_time <= f32::EPSILON;
    let skill = setup.get("skill").and_then(Value::as_str);
    let flag = |k: &str| setup.get(k).and_then(Value::as_bool);
    let items = [
        (
            "Skill installed",
            skill.map(|_| true),
            skill
                .map(|v| format!("v{}", v.trim_start_matches('v')))
                .unwrap_or_default(),
        ),
        ("MCP server registered", flag("mcp"), String::new()),
        ("Watcher running", flag("watcher"), String::new()),
    ];
    let mut animating = false;
    // A plain list, not a card: the dialog is the surface.
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = space::SM;
        for (i, (label, ok, detail)) in items.iter().enumerate() {
            let t = if still {
                1.0
            } else {
                ((since - i as f64 * STAGGER) / FADE).clamp(0.0, 1.0) as f32
            };
            animating |= t < 1.0;
            ui.scope(|ui| {
                ui.set_opacity(egui::emath::easing::cubic_out(t));
                face::check(ui, label, *ok, detail);
            });
        }
        if items.iter().any(|(_, ok, _)| ok.is_none()) {
            w::caption(
                ui,
                "A dash is something this agent\u{2019}s kit does not report yet.",
            );
        }
    });
    if animating {
        ui.ctx().request_repaint();
    }
}

/// "Are you sure" for rotate and revoke. Both cut off the agent's current
/// token, so neither happens on one click.
fn confirm_dialog(ctx: &egui::Context, net: &mut Net, local: &mut Local) {
    let Some(ask) = local.confirm.clone() else {
        return;
    };
    let mut go = false;
    let mut close = false;
    let modal = dialog(ctx, "agents:confirm", DIALOG_W * 0.8, |ui| {
        if ask.revoke {
            heading(ui, &format!("Revoke {}?", ask.name));
            ui.add_space(space::XS);
            w::muted(
                ui,
                "Its token stops working now, and any task it holds comes back to you.",
            );
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
            net.send(
                ACTION_KEY,
                reqwest::Method::DELETE,
                &format!("/api/user/agents/{}", ask.id),
                Value::Null,
            );
            local.pending = Some(Pending::Revoke);
        } else {
            net.post(
                ACTION_KEY,
                &format!("/api/user/agents/{}/rotate", ask.id),
                json!({}),
            );
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
    use super::{per_row, valid_handle};

    #[test]
    fn handles() {
        for ok in ["hermes", "claude-mac", "codex2", "a"] {
            assert!(valid_handle(ok), "{ok}");
        }
        for bad in ["", "Hermes", "my agent", "-x", "under_score", "émile"] {
            assert!(!valid_handle(bad), "{bad}");
        }
    }

    #[test]
    fn cards_per_row() {
        assert_eq!(per_row(1000.0), 3);
        assert_eq!(per_row(700.0), 2);
        assert_eq!(per_row(400.0), 1);
    }
}
