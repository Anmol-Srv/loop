//! An agent at work, as the rest of the app sees it: the task page's "Agent
//! session" section, and Home's "Agents at work" strip.
//!
//! The session reads top to bottom the way the work went: who holds it and
//! where it is (avatar, stepper), what it is doing right now (the now line),
//! what it has said (a left-rail timeline, not chat bubbles), the report it
//! handed in (a card with its evidence and the review), and — for the owner
//! only — a private line to the agent and its step log.
//!
//! What is private is decided by the server (`canSeeAgentPrivate`, the notes
//! query, the logs endpoint's 403). This view also leaves those parts out for
//! anyone else, so a teammate's page never shows an empty box where the
//! owner's conversation would be.

use std::time::{Duration, Instant};

use egui::{pos2, vec2, RichText};
use serde_json::Value;

use super::agents::{runtime_label, state_words};
use super::projects::PROSE_W;
use super::task::{ago, exact};
use crate::desktop::design::agent::{self as face, Presence, Step};
use crate::desktop::design::{
    avatar, cards as c, colour, glyph, motion, pad, radius, shell, size, space, status_label, text, theme, widgets as w,
};
use crate::desktop::net::Net;

/// The step log, fetched only while the owner has it open.
const LOG_KEY: &str = "task:logs";
/// How often an open log on a live session is fetched again.
const POLL: Duration = Duration::from_secs(2);
/// Fire slightly early: a repaint scheduled for +2 s can land a hair under it.
const DUE: Duration = Duration::from_millis(1_900);
/// The log well, in rows: tall enough to watch a run, short enough that the
/// composer above it stays on screen.
const LOG_ROWS: f32 = 12.0;
/// The timeline's node column.
const NODE: f32 = 20.0;

/// The session's local state: drafts, and the log transcript. Log lines live
/// here and not in the net cache because the `afterSeq` fetch returns only the
/// tail; the cache holds one reply, this holds the run.
#[derive(Default)]
pub(super) struct State {
    logs_open: bool,
    lines: Vec<(i64, String)>,
    next_seq: i64,
    fired_at: Option<Instant>,
    /// The log reply last folded in, by generation.
    folded: u64,
    log_error: Option<String>,
    answer: String,
    changes: String,
    /// "Request changes" was pressed: the note it needs is open.
    changes_open: bool,
    instruction: String,
}

impl State {
    /// A reply landed: whatever was being written was sent.
    pub(super) fn sent(&mut self) {
        self.answer.clear();
        self.changes.clear();
        self.changes_open = false;
        self.instruction.clear();
    }
}

/// What the session asks the page to send.
pub(super) enum Ask {
    Answer(String),
    Approve,
    Changes(String),
    Instruct(String),
}

/// Everything the section reads.
pub(super) struct Session<'a> {
    pub task_id: &'a str,
    pub task: &'a Value,
    pub delegate: &'a Value,
    /// The task's notes, oldest first, as the server let this viewer see them.
    pub notes: &'a [Value],
    pub notes_loaded: bool,
    pub evidence: &'a [Value],
    /// The viewer is the task's owner: the one who answers, reviews and
    /// instructs.
    pub mine: bool,
    /// The viewer may read the private parts: the owner, or an admin.
    pub private: bool,
    pub busy: bool,
}

/// The kinds of note that belong to the session rather than the team thread.
pub(super) fn session_kind(kind: &str) -> bool {
    matches!(kind, "progress" | "question" | "answer" | "instruction" | "submission" | "review")
}

fn private_kind(kind: &str) -> bool {
    matches!(kind, "question" | "answer" | "instruction")
}

/// The section. Returns what to send, if anything was asked.
pub(super) fn show(ui: &mut egui::Ui, net: &mut Net, s: &Session, st: &mut State) -> Option<Ask> {
    let d = s.delegate;
    let state = str_of(d, "state").unwrap_or("handed_off");
    let agent = str_of(d, "name").unwrap_or("The agent");
    let short = short_name(agent);
    let owner = str_of(d, "ownerName").or_else(|| str_of(s.task, "assigneeName")).unwrap_or("its owner");
    let owner_first = first_name(owner);
    let seed = owner_seed(s.task, owner);
    let presence = Presence::of(state, str_of(d, "lastSeenAt"));
    let held = !matches!(state, "done" | "stopped");
    let mut ask = None;

    shell::divider(ui);
    shell::section(ui, "Agent session");

    // ---- who, and since when
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::MD;
        face::avatar(ui, &seed, face::MD, presence, agent);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(
                RichText::new(agent)
                    .size(text::BODY)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
            let mut who = format!("{owner_first}\u{2019}s agent");
            if let Some(rt) = str_of(d, "runtime") {
                who += &format!(" \u{00B7} {}", runtime_label(rt));
            }
            ui.label(RichText::new(who).size(text::SMALL).color(colour::TEXT_MUTED));
        });
        if let Some(at) = str_of(d, "delegatedAt") {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("Started {}", ago(at))).size(text::SMALL).color(colour::TEXT_MUTED))
                    .on_hover_text(exact(at));
            });
        }
    });
    ui.add_space(space::MD);

    // ---- where it is
    let (steps, current, complete) = stages(state, s, owner_first);
    let tone = match state {
        "needs_input" => colour::WARN,
        "in_review" => colour::AGENT,
        _ => colour::INFO,
    };
    face::stepper(ui, &steps, current, complete, tone);
    ui.add_space(space::MD);

    // ---- what it is doing now
    now_line(ui, d, state, s.mine, owner_first, short);

    // ---- what it has said
    let entries: Vec<&Value> = s
        .notes
        .iter()
        .filter(|n| str_of(n, "kind").is_some_and(session_kind))
        .filter(|n| s.private || !str_of(n, "kind").is_some_and(private_kind))
        .collect();
    let latest_submission = entries.iter().rposition(|n| str_of(n, "kind") == Some("submission"));
    let latest_question = entries.iter().rposition(|n| str_of(n, "kind") == Some("question"));

    ui.add_space(space::LG);
    if !s.notes_loaded {
        for wdt in [0.6, 0.45] {
            face::skeleton(ui, PROSE_W * wdt, text::BODY);
            ui.add_space(space::SM);
        }
    } else {
        let mut nodes: Vec<egui::Rect> = Vec::new();
        let rail = ui.painter().add(egui::Shape::Noop);
        ui.spacing_mut().item_spacing.y = space::LG;
        if let Some(at) = str_of(d, "delegatedAt") {
            nodes.push(entry(ui, Node::Person(&seed), owner, &format!("handed this to {short}"), Some(at), false, |_| {}));
        }
        for (i, n) in entries.iter().enumerate() {
            let kind = str_of(n, "kind").unwrap_or("progress");
            let at = str_of(n, "createdAt");
            let body = str_of(n, "body").unwrap_or_default();
            let by_agent = n.get("agent").is_some_and(Value::is_object);
            // The short name: the full one sits once, in the header above.
            let author = if by_agent { short } else { str_of(n, "authorName").unwrap_or(owner) };
            let private = private_kind(kind);

            if Some(i) == latest_submission {
                nodes.push(report(ui, s, st, n, short, owner_first, state, &mut ask));
                continue;
            }
            let (node, verb) = match kind {
                "question" => (Node::Mark(Mark::Question), "asked".to_owned()),
                "answer" => (Node::Person(&seed), "answered".to_owned()),
                "instruction" => (Node::Person(&seed), format!("told {short}")),
                "submission" => (Node::Mark(Mark::Submitted), "submitted for review".to_owned()),
                "review" if approved(&entries, i, state) => (Node::Mark(Mark::Approved), "approved".to_owned()),
                "review" => (Node::Mark(Mark::Changes), "asked for changes".to_owned()),
                _ => (Node::Mark(Mark::Progress), String::new()),
            };
            let open_question = Some(i) == latest_question && state == "needs_input" && s.mine;
            nodes.push(entry(ui, node, author, &verb, at, private, |ui| {
                prose(ui, body, colour::TEXT_2);
                if open_question {
                    ui.add_space(space::SM);
                    if let Some(a) = answer_box(ui, st, short, s.busy) {
                        ask = Some(a);
                    }
                }
            }));
        }
        ui.spacing_mut().item_spacing.y = space::SM;
        // The hairline joining the nodes, drawn under them once they are placed.
        let segments: Vec<egui::Shape> = nodes
            .windows(2)
            .map(|w| {
                let x = w[0].center().x;
                egui::Shape::line_segment(
                    [pos2(x, w[0].bottom() + space::XS), pos2(x, w[1].top() - space::XS)],
                    egui::Stroke::new(1.0, colour::LINE),
                )
            })
            .collect();
        ui.painter().set(rail, egui::Shape::Vec(segments));
        if entries.is_empty() && str_of(d, "delegatedAt").is_none() {
            w::caption(ui, &format!("Nothing from {short} yet \u{2014} its updates, questions and report land here."));
        }
    }

    // ---- a private line to the agent
    if s.mine && held {
        ui.add_space(space::LG);
        if let Some(a) = composer(ui, st, short, s.busy) {
            ask = Some(a);
        }
    }

    // ---- the step log
    if s.private {
        ui.add_space(space::MD);
        logs(ui, net, s.task_id, matches!(state, "working" | "acknowledged"), st);
    }
    ask
}

/// "Hermes" out of "Hermes (Anmol's Mac)": the name a sentence can carry.
fn short_name(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name).trim()
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// The owner's avatar seed: the same one their person disc uses, so an agent
/// wears exactly its owner's colour.
fn owner_seed(task: &Value, owner: &str) -> String {
    task.get("delegate")
        .and_then(|d| str_of(d, "ownerEmail"))
        .or_else(|| str_of(task, "assigneeEmail"))
        .unwrap_or(owner)
        .to_owned()
}

/// The four stages, which one it is at, and whether it finished. Times come
/// from the notes that marked each one.
fn stages(state: &str, s: &Session, owner_first: &str) -> (Vec<Step>, usize, bool) {
    let first = |kind: &str| s.notes.iter().find(|n| str_of(n, "kind") == Some(kind)).and_then(|n| str_of(n, "createdAt"));
    let last = |kind: &str| s.notes.iter().rev().find(|n| str_of(n, "kind") == Some(kind)).and_then(|n| str_of(n, "createdAt"));
    let when = |at: Option<&str>| at.map(exact).filter(|e| !e.is_empty());
    let third = match state {
        "needs_input" if s.mine => "Needs you".to_owned(),
        "needs_input" => format!("Needs {owner_first}"),
        _ => "In review".to_owned(),
    };
    let third_at = if state == "needs_input" { last("question") } else { last("submission") };
    let fourth = if state == "stopped" { "Taken back" } else { "Done" };
    let steps = vec![
        Step { label: "Handed off".into(), when: when(str_of(s.delegate, "delegatedAt")) },
        Step { label: "Working".into(), when: when(first("progress")) },
        Step { label: third, when: when(third_at) },
        Step { label: fourth.into(), when: when(str_of(s.task, "doneAt").or(last("review"))) },
    ];
    let (current, complete) = match state {
        "handed_off" => (0, false),
        "acknowledged" | "working" => (1, false),
        "needs_input" | "in_review" => (2, false),
        "done" => (3, true),
        _ => (3, false),
    };
    (steps, current, complete)
}

/// The line under the stepper. While the agent works, what it says it is doing
/// and how fresh that is; otherwise whose move it is, in words.
fn now_line(ui: &mut egui::Ui, d: &Value, state: &str, mine: bool, owner_first: &str, short: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        let words = match state {
            "working" | "acknowledged" => {
                face::spinner(ui, text::BODY);
                match str_of(d, "now").map(str::trim).filter(|n| !n.is_empty()) {
                    Some(now) => {
                        let shown = ui.add(egui::Label::new(RichText::new(now).size(text::BODY).color(colour::TEXT_2)).truncate());
                        shown.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, now));
                        if let Some(at) = str_of(d, "nowAt") {
                            ui.label(RichText::new(ago(at)).size(text::SMALL).color(colour::TEXT_FAINT)).on_hover_text(exact(at));
                        }
                        return;
                    }
                    None if state == "acknowledged" => format!("{short} picked this up"),
                    None => "Working \u{2014} no update yet".to_owned(),
                }
            }
            "handed_off" => format!("Waiting for {short} to pick this up"),
            "needs_input" if mine => "Waiting on your answer".to_owned(),
            "needs_input" => format!("Waiting on {owner_first}\u{2019}s answer"),
            "in_review" if mine => "Waiting on your review".to_owned(),
            "in_review" => format!("Waiting on {owner_first}\u{2019}s review"),
            "done" => "Finished \u{2014} the review approved it".to_owned(),
            "stopped" => format!("Taken back from {short}"),
            other => state_words(other).to_owned(),
        };
        let ink = if state == "needs_input" { colour::WARN } else { colour::TEXT_MUTED };
        ui.label(RichText::new(words).size(text::SMALL).color(ink));
    });
}

/// A review is an approval when it ended the session: nothing was submitted
/// after it and the session is done. The note carries no decision field.
fn approved(entries: &[&Value], i: usize, state: &str) -> bool {
    state == "done" && !entries[i + 1..].iter().any(|n| str_of(n, "kind") == Some("submission"))
}

// ----------------------------------------------------------------- timeline

enum Node<'a> {
    /// A person's disc: their answers, instructions, the hand-off.
    Person(&'a str),
    Mark(Mark),
}

#[derive(Clone, Copy)]
enum Mark {
    Progress,
    Question,
    Submitted,
    Approved,
    Changes,
}

/// One timeline entry: the node in the rail, who and what beside it, then
/// whatever `body` adds. Returns the node's rect, for the rail's hairline.
fn entry(
    ui: &mut egui::Ui,
    node: Node,
    who: &str,
    verb: &str,
    at: Option<&str>,
    private: bool,
    body: impl FnOnce(&mut egui::Ui),
) -> egui::Rect {
    let mut rect = egui::Rect::NOTHING;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::MD;
        let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(NODE), egui::Sense::hover());
        rect = r;
        paint_node(ui, r, &node);
        ui.vertical(|ui| {
            ui.set_max_width(PROSE_W.min(ui.available_width()));
            ui.spacing_mut().item_spacing.y = space::XS;
            ui.horizontal(|ui| {
                ui.set_min_height(NODE);
                ui.spacing_mut().item_spacing.x = space::XS;
                ui.label(
                    RichText::new(who)
                        .size(text::SMALL)
                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                        .color(colour::TEXT),
                );
                if !verb.is_empty() {
                    ui.label(RichText::new(verb).size(text::SMALL).color(colour::TEXT_MUTED));
                }
                if let Some(at) = at {
                    ui.label(RichText::new(ago(at)).size(text::SMALL).color(colour::TEXT_FAINT)).on_hover_text(exact(at));
                }
                if private {
                    ui.add_space(space::XS);
                    face::private_label(ui);
                }
            });
            body(ui);
        });
    });
    rect
}

fn paint_node(ui: &mut egui::Ui, r: egui::Rect, node: &Node) {
    let p = ui.painter();
    let c = r.center();
    match node {
        Node::Person(seed) => avatar::paint(p, r.shrink(1.0), seed),
        Node::Mark(mark) => {
            let (ink, fill) = match mark {
                Mark::Progress => (colour::TEXT_MUTED, colour::SURFACE),
                Mark::Question => (colour::WARN, colour::WARN_BG),
                Mark::Submitted => (colour::AGENT, colour::AGENT_BG),
                Mark::Approved => (colour::OK, colour::OK_BG),
                Mark::Changes => (colour::WARN, colour::WARN_BG),
            };
            p.circle_filled(c, NODE / 2.0, fill);
            p.circle_stroke(c, NODE / 2.0 - 0.5, egui::Stroke::new(1.0, ink.gamma_multiply(0.35)));
            match mark {
                Mark::Progress => {
                    p.circle_filled(c, 2.5, ink);
                }
                Mark::Question => {
                    p.text(c, egui::Align2::CENTER_CENTER, "?", egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::BOLD.into())), ink);
                }
                Mark::Submitted => glyph::arrow_up(p, c, NODE * 0.7, ink),
                Mark::Approved => glyph::tick(p, c, NODE * 0.55, ink),
                Mark::Changes => glyph::back(p, c, NODE * 0.7, ink),
            }
        }
    }
}

fn prose(ui: &mut egui::Ui, body: &str, ink: egui::Color32) {
    for (i, para) in body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).enumerate() {
        if i > 0 {
            ui.add_space(space::XS);
        }
        ui.label(RichText::new(para).size(text::BODY).color(ink));
    }
}

/// Cmd+Enter in a focused box. Read off the key event itself, which carries
/// its modifiers, rather than the frame's modifier state.
fn submit_key(ui: &egui::Ui, field: &egui::Response) -> bool {
    field.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
}

fn answer_box(ui: &mut egui::Ui, st: &mut State, short: &str, busy: bool) -> Option<Ask> {
    let field = w::field_multiline(ui, "", &mut st.answer, 2, &format!("Answer {short}\u{2026}  Cmd+Enter to send"));
    let ready = !st.answer.trim().is_empty() && !busy;
    let mut go = ready && submit_key(ui, &field);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        let response = w::primary(ui, "Send answer", ready);
        if st.answer.trim().is_empty() {
            response.clone().on_disabled_hover_text("Write the answer first.");
        }
        go |= response.clicked();
    });
    go.then(|| Ask::Answer(st.answer.trim().to_owned()))
}

/// "Tell Hermes…": a private instruction, heard by the agent at its next step.
fn composer(ui: &mut egui::Ui, st: &mut State, short: &str, busy: bool) -> Option<Ask> {
    let mut go = false;
    ui.scope(|ui| {
        ui.set_max_width((PROSE_W + NODE + space::MD).min(ui.available_width()));
        let field = w::field_multiline(ui, "", &mut st.instruction, 2, &format!("Tell {short}\u{2026}  Cmd+Enter to send"));
        let ready = !st.instruction.trim().is_empty() && !busy;
        go = ready && submit_key(ui, &field);
        ui.horizontal(|ui| {
            face::private_label(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = w::secondary(ui, &format!("Send to {short}"), ready);
                if st.instruction.trim().is_empty() {
                    response.clone().on_disabled_hover_text("Write the instruction first.");
                }
                go |= response.clicked();
            });
        });
    });
    go.then(|| Ask::Instruct(st.instruction.trim().to_owned()))
}

// ------------------------------------------------------------------- report

/// The submission, as a report: what was done, the evidence, where approving
/// moves the task, and — for the owner, while it waits — the review.
#[allow(clippy::too_many_arguments)]
fn report(
    ui: &mut egui::Ui,
    s: &Session,
    st: &mut State,
    note: &Value,
    short: &str,
    owner_first: &str,
    state: &str,
    ask: &mut Option<Ask>,
) -> egui::Rect {
    let mut node = egui::Rect::NOTHING;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::MD;
        let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(NODE), egui::Sense::hover());
        node = r;
        paint_node(ui, r, &Node::Mark(Mark::Submitted));
        // A frame takes its parent's layout; the card reads top to bottom.
        ui.vertical(|ui| egui::Frame::new()
            .fill(colour::SURFACE)
            .stroke(egui::Stroke::new(1.0, colour::LINE))
            .corner_radius(radius::LG)
            .inner_margin(egui::Margin::symmetric(pad::CARD.0 as i8, pad::CARD.1 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = space::SM;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    ui.label(
                        RichText::new("Report")
                            .size(text::SMALL)
                            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                            .color(colour::TEXT),
                    );
                    ui.label(RichText::new(format!("\u{00B7} {short} submitted this for review")).size(text::SMALL).color(colour::TEXT_MUTED));
                    if let Some(at) = str_of(note, "createdAt") {
                        ui.label(RichText::new(ago(at)).size(text::SMALL).color(colour::TEXT_FAINT)).on_hover_text(exact(at));
                    }
                });
                ui.scope(|ui| {
                    ui.set_max_width(PROSE_W.min(ui.available_width()));
                    prose(ui, str_of(note, "body").unwrap_or_default(), colour::TEXT);
                });

                let evidence: Vec<&Value> =
                    s.evidence.iter().filter(|r| matches!(str_of(r, "kind"), Some("pr" | "commit" | "figma"))).collect();
                ui.add_space(space::XS);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(space::SM, space::SM);
                    if evidence.is_empty() {
                        ui.label(RichText::new("No evidence attached").size(text::SMALL).color(colour::TEXT_FAINT));
                    }
                    for row in &evidence {
                        evidence_chip(ui, row);
                    }
                });
                let target = str_of(s.task, "reviewTarget").unwrap_or("completed");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::SM;
                    w::muted(ui, if state == "in_review" { "Approving moves it to" } else { "Asked to move it to" });
                    c::chip(ui, status_label(target), c::status_tone(target), true);
                });

                if state != "in_review" {
                    return;
                }
                if !s.mine {
                    w::caption(ui, &format!("Waiting on {owner_first}\u{2019}s review."));
                    return;
                }
                ui.add_space(space::XS);
                let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), egui::Sense::hover());
                ui.painter().hline(rule.x_range(), rule.center().y, egui::Stroke::new(1.0, colour::LINE));
                if let Some(a) = review_controls(ui, st, short, s.busy) {
                    *ask = Some(a);
                }
            }));
    });
    node
}

fn review_controls(ui: &mut egui::Ui, st: &mut State, short: &str, busy: bool) -> Option<Ask> {
    let mut ask = None;
    if st.changes_open {
        let field = w::field_multiline(ui, "", &mut st.changes, 2, &format!("What should {short} change?  Cmd+Enter to send"));
        if !field.has_focus() && st.changes.is_empty() && !ui.ctx().memory(|m| m.focused().is_some()) {
            field.request_focus();
        }
        if field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            st.changes_open = false;
        }
        let ready = !st.changes.trim().is_empty() && !busy;
        if ready && submit_key(ui, &field) {
            ask = Some(Ask::Changes(st.changes.trim().to_owned()));
        }
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        if st.changes_open {
            let ready = !st.changes.trim().is_empty() && !busy;
            let r = w::primary(ui, "Send back", ready);
            if st.changes.trim().is_empty() {
                r.clone().on_disabled_hover_text("Say what needs to change first.");
            }
            if r.clicked() {
                ask = Some(Ask::Changes(st.changes.trim().to_owned()));
            }
            if w::ghost(ui, "Cancel").clicked() {
                st.changes_open = false;
            }
        } else {
            if w::primary(ui, "Approve", !busy).clicked() {
                ask = Some(Ask::Approve);
            }
            if w::secondary(ui, "Request changes", !busy).clicked() {
                st.changes_open = true;
            }
        }
    });
    ask
}

/// A piece of evidence: its kind's mark and its name. Opens the link; a commit
/// is a hash with nowhere to go, so it is a plain chip.
fn evidence_chip(ui: &mut egui::Ui, row: &Value) {
    let kind = str_of(row, "kind").unwrap_or("link");
    let url = str_of(row, "url").unwrap_or_default();
    let title = str_of(row, "title").map(str::trim).filter(|t| !t.is_empty());
    let (label, mono) = match (kind, title) {
        ("commit", Some(t)) => (format!("{} {t}", &url[..url.len().min(7)]), false),
        ("commit", None) => (url[..url.len().min(7)].to_owned(), true),
        (_, Some(t)) => (t.to_owned(), false),
        _ => (host_path(url).to_owned(), false),
    };
    let font = if mono { egui::FontId::monospace(text::CAPTION) } else { egui::FontId::proportional(text::SMALL) };
    let galley = w::truncated(ui, &label, font, colour::TEXT_2, 260.0);
    let icon = text::BODY;
    let (rect, response) = ui.allocate_exact_size(
        vec2(space::SM + icon + space::XS + galley.size().x + space::SM, size::CONTROL - space::XS),
        if kind == "commit" { egui::Sense::hover() } else { egui::Sense::click() },
    );
    let name = format!("{}: {label}", kind_word(kind));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    let response = if kind == "commit" { response } else { motion::operable(ui, response, radius::SM as f32) };
    let hot = kind != "commit" && (response.hovered() || response.has_focus());
    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, if hot { colour::SURFACE_HOVER } else { colour::INSET });
    p.rect_stroke(rect, radius::SM as f32, egui::Stroke::new(1.0, if hot { colour::LINE_STRONG } else { colour::LINE }), egui::StrokeKind::Inside);
    glyph::evidence(p, pos2(rect.left() + space::SM + icon / 2.0, rect.center().y), icon, kind, kind_ink(kind));
    p.galley(pos2(rect.left() + space::SM + icon + space::XS, rect.center().y - galley.size().y / 2.0), galley, if hot { colour::TEXT } else { colour::TEXT_2 });
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let response = response.on_hover_text(url);
    if response.clicked() && kind != "commit" && !url.is_empty() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

fn kind_word(kind: &str) -> &'static str {
    match kind {
        "pr" => "Pull request",
        "commit" => "Commit",
        "figma" => "Figma",
        "doc" => "Doc",
        _ => "Link",
    }
}

fn kind_ink(kind: &str) -> egui::Color32 {
    match kind {
        "pr" => colour::INFO,
        "commit" => colour::OK,
        "figma" => colour::AGENT,
        _ => colour::TEXT_MUTED,
    }
}

fn host_path(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.strip_prefix("www.").unwrap_or(rest).trim_end_matches('/')
}

// --------------------------------------------------------------------- logs

/// The step log, behind a disclosure. Nothing is fetched until it is opened;
/// while it is open on a live session it is fetched again every two seconds,
/// and the well follows the tail.
fn logs(ui: &mut egui::Ui, net: &mut Net, task_id: &str, live: bool, st: &mut State) {
    let id = egui::Id::new(("session:logs", task_id));
    ui.horizontal(|ui| {
        let count = (!st.lines.is_empty()).then_some(st.lines.len());
        face::disclosure(ui, id, "Logs", count, &mut st.logs_open);
        if st.logs_open && live {
            w::pill(ui, "live", colour::INFO);
        }
        if st.logs_open && !st.lines.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::link(ui, "Copy log").clicked() {
                    let all: Vec<&str> = st.lines.iter().map(|(_, l)| l.as_str()).collect();
                    ui.ctx().copy_text(all.join("\n"));
                    w::toast(ui.ctx(), "Log copied.", false);
                }
            });
        }
    });
    if !st.logs_open {
        return;
    }

    // Fold in each reply once: the fetch asks only for lines after the last
    // one held, so every new payload is a tail to append.
    let generation = net.generation(LOG_KEY);
    if generation != 0 && generation != st.folded {
        st.folded = generation;
        st.log_error = None;
        for row in net.data(LOG_KEY).and_then(Value::as_array).into_iter().flatten() {
            let seq = row.get("seq").and_then(Value::as_i64).unwrap_or(0);
            if seq > st.next_seq {
                st.next_seq = seq;
                st.lines.push((seq, str_of(row, "text").unwrap_or_default().to_owned()));
            }
        }
    }
    if let Some(err) = net.error(LOG_KEY) {
        st.log_error = Some(err.to_owned());
    }
    // Fetch on opening; then, while open and live, again every two seconds,
    // in place. `request_repaint_after` is the whole clock, so a closed log,
    // a finished run or a refusal costs nothing.
    let path = format!("/api/user/tasks/{task_id}/logs?afterSeq={}", st.next_seq);
    let loading = net.is_loading(LOG_KEY);
    if net.peek(LOG_KEY).is_none() && !loading {
        net.get(LOG_KEY, &path);
        st.fired_at = Some(Instant::now());
    } else if live && st.log_error.is_none() {
        ui.ctx().request_repaint_after(POLL);
        if !loading && st.fired_at.is_none_or(|t| t.elapsed() >= DUE) {
            net.get(LOG_KEY, &path);
            st.fired_at = Some(Instant::now());
        }
    }

    ui.add_space(space::XS);
    if let Some(err) = &st.log_error {
        w::error(ui, &format!("Could not load the log: {err}"));
        return;
    }
    egui::Frame::new()
        .fill(colour::LOG_BG)
        .stroke(egui::Stroke::new(1.0, colour::LINE_SOFT))
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(pad::CARD.0 as i8, pad::CARD.1 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if st.lines.is_empty() {
                if net.is_loading(LOG_KEY) && st.log_error.is_none() {
                    for wdt in [0.5, 0.7, 0.35] {
                        face::skeleton(ui, ui.available_width() * wdt, text::SMALL);
                        ui.add_space(space::XS);
                    }
                } else {
                    let note = if live { "No lines yet \u{2014} they appear here as the agent logs its steps." } else { "No log was recorded." };
                    ui.label(RichText::new(note).monospace().size(text::SMALL).color(colour::LOG_SEQ));
                }
                return;
            }
            let gutter = format!("{}", st.lines.last().map_or(0, |(s, _)| *s)).len().max(3);
            // Sized to the lines it holds, up to the cap: a scroll area sized
            // from last frame's content showed a new run cropped to its tail.
            let line_h = ui.fonts_mut(|f| f.row_height(&egui::FontId::monospace(text::SMALL))) + space::XXS;
            let height = (st.lines.len() as f32 * line_h).min(line_h * LOG_ROWS);
            egui::ScrollArea::vertical()
                .id_salt(("session:logs:scroll", task_id))
                .min_scrolled_height(height)
                .max_height(height)
                .stick_to_bottom(true)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = space::XXS;
                    for (seq, line) in &st.lines {
                        ui.horizontal_top(|ui| {
                            ui.label(RichText::new(format!("{seq:>gutter$}")).monospace().size(text::SMALL).color(colour::LOG_SEQ));
                            ui.add_space(space::SM);
                            ui.label(RichText::new(line).monospace().size(text::SMALL).color(colour::LOG_TEXT));
                        });
                    }
                });
        });
}

// --------------------------------------------------------------- home strip

/// Everyone's agents holding a task right now. Team-visible.
pub(super) const ACTIVE_KEY: &str = "agents:active";

/// Home's "Agents at work": one compact row per agent holding a task. Absent
/// when there are none — and when the server does not have the endpoint yet,
/// rather than an error on the dashboard. Returns a task to open.
pub(super) fn at_work(ui: &mut egui::Ui, net: &mut Net) -> Option<String> {
    net.get_once(ACTIVE_KEY, "/api/user/agents/active");
    let rows = net.shared(ACTIVE_KEY)?;
    let rows = rows.as_array().filter(|r| !r.is_empty())?;
    // The owner's colour comes from their email, as on their disc in the
    // table below; the row carries only a name, so borrow it from the task.
    let tasks = net.shared("home:tasks");
    let email_of = |task: &str| {
        tasks.as_deref().and_then(Value::as_array).and_then(|t| {
            t.iter().find(|r| str_of(r, "id") == Some(task)).and_then(|r| str_of(r, "assigneeEmail")).map(str::to_owned)
        })
    };
    let mut open = None;
    shell::section_count(ui, "Agents at work", rows.len());
    w::card_list(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in rows {
            let task = row.get("task").and_then(|t| str_of(t, "id")).unwrap_or_default();
            if active_row(ui, row, email_of(task)) {
                open = row.get("task").and_then(|t| str_of(t, "id")).map(str::to_owned);
            }
        }
    });
    open
}

fn active_row(ui: &mut egui::Ui, row: &Value, email: Option<String>) -> bool {
    let empty = Value::Null;
    let agent = row.get("agent").unwrap_or(&empty);
    let owner = row.get("owner").unwrap_or(&empty);
    let task = row.get("task").unwrap_or(&empty);
    let name = str_of(agent, "name").unwrap_or("Agent");
    let short = short_name(name);
    let owner_name = str_of(owner, "name").unwrap_or_default();
    let owner_first = first_name(owner_name);
    let seed = str_of(owner, "email").map(str::to_owned).or(email).unwrap_or_else(|| owner_name.to_owned());
    let state = str_of(row, "state").unwrap_or("working");
    let presence = Presence::of(state, str_of(row, "lastSeenAt"));
    let title = str_of(task, "title").unwrap_or("Untitled");
    let now = str_of(row, "now").map(str::trim).filter(|n| !n.is_empty());
    let since = str_of(row, "delegatedAt").map(elapsed).unwrap_or_default();

    let response = w::row(ui, |ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        face::avatar(ui, &seed, face::SM, presence, name);
        let whole = ui.available_width();
        let who_w = (whole * 0.22).clamp(120.0, 200.0);
        let since_w = 44.0;
        let title_w = ((whole - who_w - since_w) * 0.45).max(80.0);
        fixed(ui, who_w, |ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.label(
                RichText::new(short)
                    .size(text::SMALL)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
            ui.add(egui::Label::new(RichText::new(format!("\u{00B7} {owner_first}")).size(text::SMALL).color(colour::TEXT_MUTED)).truncate());
        });
        fixed(ui, title_w, |ui| {
            ui.add(egui::Label::new(RichText::new(title).size(text::SMALL).color(colour::TEXT)).truncate());
        });
        let rest = (ui.available_width() - since_w - space::SM).max(0.0);
        fixed(ui, rest, |ui| {
            let (words, ink) = match (state, now) {
                ("working" | "acknowledged", Some(now)) => (now.to_owned(), colour::TEXT_MUTED),
                ("needs_input", _) => (format!("Waiting on {owner_first}"), colour::WARN),
                ("in_review", _) => ("In review".to_owned(), colour::AGENT),
                ("acknowledged", None) => ("Picked up".to_owned(), colour::TEXT_MUTED),
                _ => (state_words(state).to_owned(), colour::TEXT_MUTED),
            };
            ui.add(egui::Label::new(RichText::new(words).size(text::SMALL).color(ink)).truncate());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(since).size(text::SMALL).color(colour::TEXT_FAINT));
        });
    });
    let label = format!("{short}, {owner_first}\u{2019}s agent, on {title}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

fn fixed(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(vec2(width, size::ROW), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_width(width);
        add(ui);
    });
}

/// "2h", "3d": how long the agent has held it.
fn elapsed(raw: &str) -> String {
    let Ok(then) = chrono::DateTime::parse_from_rfc3339(raw) else { return String::new() };
    match (chrono::Utc::now() - then.with_timezone(&chrono::Utc)).num_seconds().max(0) {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_and_reviews() {
        assert_eq!(short_name("Hermes (Anmol's Mac)"), "Hermes");
        assert_eq!(short_name("Codex"), "Codex");
        let (sub, rev) = (json!({"kind": "submission"}), json!({"kind": "review"}));
        // A review followed by another submission sent it back.
        assert!(!approved(&[&sub, &rev, &sub], 1, "in_review"));
        // The last review on a finished session approved it.
        assert!(approved(&[&sub, &rev, &sub, &rev], 3, "done"));
        assert!(!approved(&[&sub, &rev, &sub, &rev], 1, "done"));
    }
}
