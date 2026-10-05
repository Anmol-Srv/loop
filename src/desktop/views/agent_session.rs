//! An agent at work, as the rest of the app sees it: the task page's "Agent
//! session" section, and Home's "Agents at work" strip.
//!
//! The session reads top to bottom the way the work went: who holds it and
//! where it is (avatar, stepper), what it is doing right now (the now line),
//! what it has said — quiet single lines for the small stuff, a real block
//! for a report, question, answer or note — the report it handed in (a card
//! with its evidence and the review), and — for the owner only — a private
//! line to the agent and its step log.
//!
//! What is private is decided by the server (`canSeeAgentPrivate`, the notes
//! query, the logs endpoint's 403). This view also leaves those parts out for
//! anyone else, so a teammate's page never shows an empty box where the
//! owner's conversation would be.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use egui::{pos2, vec2, RichText};
use serde_json::Value;

use super::agents::state_words;
use super::projects::PROSE_W;
use super::task::{ago, day_label, day_time, elide, exact};
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
/// A .app launched from Finder gets a minimal PATH, so `cmux` alone is often
/// not found even when it is installed; this is where the app puts it.
const CMUX_FALLBACK: &str = "/Applications/cmux.app/Contents/Resources/bin/cmux";

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
    /// The missing-folder hint's link was followed: the page opens the project.
    pub(super) open_project: bool,
    /// "Ask for changes" on the plan card was pressed: its note is open.
    plan_changes: String,
    plan_changes_open: bool,
    /// The full plan text, behind its own disclosure.
    plan_open: bool,
    /// "Earlier plans", collapsed by default.
    plan_history_open: bool,
}

impl State {
    /// A reply landed: whatever was being written was sent.
    pub(super) fn sent(&mut self) {
        self.answer.clear();
        self.changes.clear();
        self.changes_open = false;
        self.instruction.clear();
        self.plan_changes.clear();
        self.plan_changes_open = false;
    }
}

/// What the session asks the page to send.
pub(super) enum Ask {
    Answer(String),
    Approve,
    Changes(String),
    Instruct(String),
    ApprovePlan,
    PlanChanges(String),
}

/// Everything the section reads.
pub(super) struct Session<'a> {
    pub task_id: &'a str,
    pub task: &'a Value,
    pub delegate: &'a Value,
    /// The task's notes, oldest first, as the server let this viewer see them.
    pub notes: &'a [Value],
    pub evidence: &'a [Value],
    /// The viewer is the task's owner: the one who answers, reviews and
    /// instructs.
    pub mine: bool,
    /// The viewer may read the private parts: the owner, or an admin.
    pub private: bool,
    pub busy: bool,
    /// A repository of the project the owner has not set a folder for, which
    /// the agent will ask about: said to the owner while the agent holds it.
    pub unset_repo: Option<&'a str>,
    /// This delegation's plan revisions, newest first — `plans[0]` is current.
    /// Empty until the owner-only fetch lands (or if no plan was ever sent).
    pub plans: &'a [Value],
    pub plans_loaded: bool,
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
    let held = !matches!(state, "done" | "stopped");
    let mut ask = None;

    shell::divider(ui);
    shell::section(ui, "Agent session");

    // ---- where it is
    let (steps, current, complete) = stages(state, s, owner_first);
    let tone = match state {
        "needs_input" | "plan_review" | "in_review" => colour::ASK(),
        _ => colour::INFO(),
    };
    face::stepper(ui, &steps, current, complete, tone);
    if !matches!(state, "needs_input" | "plan_review" | "in_review") {
        ui.add_space(space::MD);
        now_line(ui, d, state, short);
    }
    if let Some(repo) = s.unset_repo.filter(|_| s.mine && held) {
        ui.add_space(space::SM);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(
                RichText::new(format!("{short} won\u{2019}t know where the code is \u{2014} "))
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED()),
            );
            if w::link(ui, &format!("set the folder for {repo} on the project")).clicked() {
                st.open_project = true;
            }
            ui.label(RichText::new(".").size(text::SMALL).color(colour::TEXT_MUTED()));
        });
    }

    // ---- attach: the owner's local Claude Code session, once the watcher
    // has started one for this task
    if s.private {
        if let Some(session) = s.task.get("agentSession").filter(|v| !v.is_null()) {
            ui.add_space(space::SM);
            cmux_row(ui, session, str_of(s.task, "title").unwrap_or("Untitled"));
        }
    }

    // ---- the owner's note at hand-off
    if let Some(brief) = str_of(s.task, "brief").filter(|_| s.private) {
        ui.add_space(space::MD);
        ui.scope(|ui| {
            ui.set_max_width(PROSE_W.min(ui.available_width()));
            w::muted(ui, "Your note");
            super::mrkdwn::show(ui, brief, colour::TEXT_2());
        });
    }

    if !s.private {
        plan_section(ui, s, st, short, &mut ask);
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

/// What waits on the owner, pinned: a plan to approve, a question to answer,
/// a report to review. Everything else is in Activity, newest first.
pub(super) fn pinned(ui: &mut egui::Ui, s: &Session, st: &mut State) -> Option<Ask> {
    let d = s.delegate;
    let state = str_of(d, "state").unwrap_or("handed_off");
    let agent = str_of(d, "name").unwrap_or("The agent");
    let short = short_name(agent);
    let owner = str_of(d, "ownerName").or_else(|| str_of(s.task, "assigneeName")).unwrap_or("its owner");
    let owner_first = first_name(owner);
    let agent_seed = str_of(d, "id").unwrap_or(agent);
    let mut ask = None;

    if s.private {
        plan_section(ui, s, st, short, &mut ask);
    }
    let visible: Vec<&Value> = s
        .notes
        .iter()
        .filter(|n| str_of(n, "kind").is_some_and(session_kind))
        .filter(|n| s.private || !str_of(n, "kind").is_some_and(private_kind))
        .collect();
    let last_of = |kind: &str| visible.iter().rev().find(|n| str_of(n, "kind") == Some(kind)).copied();
    if state == "needs_input" {
        if let Some(q) = last_of("question") {
            ui.add_space(space::MD);
            pinned_card(ui, colour::ASK(), |ui| {
                let at = str_of(q, "createdAt");
                let body_id = egui::Id::new(("session:pinned", s.task_id, str_of(q, "id").unwrap_or_default()));
                substantive(ui, Node::Agent(agent_seed), short, "asks", at, private_kind("question") && s.private, body_id,
                    str_of(q, "body").unwrap_or_default(), |ui| {
                        if s.mine {
                            ui.add_space(space::SM);
                            if let Some(a) = answer_box(ui, st, short, s.busy) {
                                ask = Some(a);
                            }
                        }
                    });
            });
        }
    }
    if state == "in_review" {
        if let Some(sub) = last_of("submission") {
            ui.add_space(space::MD);
            report(ui, s, st, sub, short, owner_first, state, &mut ask);
        }
    }
    ask
}

// -------------------------------------------------------------------- plan

/// The plan card: the owner sees the summary, the full plan behind a
/// disclosure, and — while one waits — Approve/Ask for changes. Anyone else
/// sees one neutral line, no content, built only from what is already public
/// (the agent's state and whether a plan was ever approved).
fn plan_section(ui: &mut egui::Ui, s: &Session, st: &mut State, short: &str, ask: &mut Option<Ask>) {
    if !s.private {
        let state = str_of(s.delegate, "state");
        let line = if state == Some("plan_review") {
            Some("Planning")
        } else if s.task.get("planApprovedAt").is_some_and(|v| !v.is_null()) {
            Some("Plan approved")
        } else {
            None
        };
        if let Some(line) = line {
            ui.add_space(space::SM);
            w::caption(ui, line);
        }
        return;
    }
    if !s.plans_loaded {
        return;
    }
    let Some(current) = s.plans.first() else {
        return;
    };
    let decision = str_of(current, "decision");
    // Decided plans live in Activity; only one that waits is pinned here.
    if decision.is_some() {
        return;
    }

    ui.add_space(space::MD);
    pinned_card(ui, colour::ASK(), |ui| {
        ui.spacing_mut().item_spacing.y = space::SM;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.label(
                RichText::new("Plan")
                    .size(text::SMALL)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT()),
            );
            let (label, tone) = match decision {
                Some("approved") => ("Approved", c::Tone::Ok),
                Some("changes_requested") => ("Changes requested", c::Tone::Running),
                _ => ("Waiting for your review", c::Tone::Ask),
            };
            c::chip(ui, label, tone, true);
            if let Some(at) = str_of(current, "createdAt") {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(ago(at)).size(text::SMALL).color(colour::TEXT_FAINT())).on_hover_text(exact(at));
                });
            }
        });
        ui.scope(|ui| {
            ui.set_max_width(PROSE_W.min(ui.available_width()));
            super::mrkdwn::show(ui, str_of(current, "summary").unwrap_or_default(), colour::TEXT());
            if decision == Some("changes_requested") {
                if let Some(note) = str_of(current, "changesNote") {
                    ui.add_space(space::XS);
                    w::muted(ui, &format!("You asked: {note}"));
                }
            }
        });

        let plan_id = egui::Id::new(("session:plan", s.task_id));
        face::disclosure(ui, plan_id, "Full plan", None, &mut st.plan_open);
        if st.plan_open {
            ui.scope(|ui| {
                ui.set_max_width(PROSE_W.min(ui.available_width()));
                super::mrkdwn::show(ui, str_of(current, "plan").unwrap_or_default(), colour::TEXT_2());
            });
        }
        let earlier = &s.plans[1..];
        if !earlier.is_empty() {
            let history_id = egui::Id::new(("session:plan:history", s.task_id));
            face::disclosure(ui, history_id, "Earlier plans", Some(earlier.len()), &mut st.plan_history_open);
            if st.plan_history_open {
                for p in earlier {
                    ui.add_space(space::XS);
                    ui.scope(|ui| {
                        ui.set_max_width(PROSE_W.min(ui.available_width()));
                        super::mrkdwn::show(ui, str_of(p, "summary").unwrap_or_default(), colour::TEXT_MUTED());
                    });
                }
            }
        }

        if decision.is_some() {
            return;
        }
        ui.add_space(space::XS);
        let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().hline(rule.x_range(), rule.center().y, egui::Stroke::new(1.0, colour::LINE()));
        if st.plan_changes_open {
            let field = w::field_multiline(
                ui,
                "",
                &mut st.plan_changes,
                2,
                &format!("What should {short} change?  Cmd+Enter to send"),
            );
            let ready = !st.plan_changes.trim().is_empty() && !s.busy;
            if ready && submit_key(ui, &field) {
                *ask = Some(Ask::PlanChanges(st.plan_changes.trim().to_owned()));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            if st.plan_changes_open {
                let ready = !st.plan_changes.trim().is_empty() && !s.busy;
                let r = w::primary(ui, "Send back", ready);
                if st.plan_changes.trim().is_empty() {
                    r.clone().on_disabled_hover_text("Say what needs to change first.");
                }
                if r.clicked() {
                    *ask = Some(Ask::PlanChanges(st.plan_changes.trim().to_owned()));
                }
                if w::ghost(ui, "Cancel").clicked() {
                    st.plan_changes_open = false;
                }
            } else {
                if w::primary(ui, "Approve plan", !s.busy).clicked() {
                    *ask = Some(Ask::ApprovePlan);
                }
                if w::secondary(ui, "Ask for changes", !s.busy).clicked() {
                    st.plan_changes_open = true;
                }
            }
        });
    });
}

/// A card for something waiting on the owner, edged in the colour of what it
/// asks.
fn pinned_card(ui: &mut egui::Ui, edge: egui::Color32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(colour::SURFACE())
        .stroke(egui::Stroke::new(1.0, edge.gamma_multiply(0.5)))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::symmetric(pad::CARD.0 as i8, pad::CARD.1 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

// ----------------------------------------------------------------- activity

/// Everything that happened on a task, newest first, in one list: the hand-off,
/// each plan and its decision, the agent's updates, questions and reports, the
/// owner's answers and instructions, and the team's notes. Read-only — what
/// needs acting on is pinned under the page's header.
pub(super) struct Feed<'a> {
    pub task_id: &'a str,
    pub task: &'a Value,
    /// The current delegation, if any: its hand-off and agent name.
    pub delegate: Option<&'a Value>,
    /// Every note the viewer may see, oldest first, as the server sends them.
    pub notes: &'a [Value],
    /// Plan revisions, newest first; empty for anyone but the owner and admins.
    pub plans: &'a [Value],
    /// The viewer may read the private parts.
    pub private: bool,
}

enum Item<'a> {
    Handoff(&'a str),
    Plan(&'a Value),
    Decision(&'a Value),
    Note(&'a Value),
}

fn parse(at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(at).map_or(0, |t| t.timestamp_micros())
}

pub(super) fn feed(ui: &mut egui::Ui, f: &Feed) {
    let agent = f.delegate.and_then(|d| str_of(d, "name")).unwrap_or("The agent");
    let short = short_name(agent);
    let agent_seed = f.delegate.and_then(|d| str_of(d, "id")).unwrap_or(agent);
    let state = f.delegate.and_then(|d| str_of(d, "state")).unwrap_or("");
    let owner = f
        .delegate
        .and_then(|d| str_of(d, "ownerName"))
        .or_else(|| str_of(f.task, "assigneeName"))
        .unwrap_or("Someone");
    let owner_seed = f
        .delegate
        .and_then(|d| str_of(d, "ownerEmail"))
        .or_else(|| str_of(f.task, "assigneeEmail"))
        .unwrap_or(owner)
        .to_owned();

    let notes: Vec<&Value> = f
        .notes
        .iter()
        .filter(|n| f.private || !str_of(n, "kind").is_some_and(private_kind))
        .collect();

    let mut items: Vec<(i64, Item)> = Vec::new();
    if let Some(at) = f.delegate.and_then(|d| str_of(d, "delegatedAt")) {
        items.push((parse(at), Item::Handoff(at)));
    }
    for p in f.plans {
        if let Some(at) = str_of(p, "createdAt") {
            items.push((parse(at), Item::Plan(p)));
        }
        if let Some(at) = str_of(p, "decidedAt") {
            items.push((parse(at), Item::Decision(p)));
        }
    }
    for n in &notes {
        items.push((str_of(n, "createdAt").map_or(0, parse), Item::Note(n)));
    }
    // Newest first; equal times keep the server's order reversed.
    items.reverse();
    items.sort_by(|a, b| b.0.cmp(&a.0));

    if items.is_empty() {
        w::caption(ui, "Nothing yet \u{2014} notes, an agent\u{2019}s updates, plans and questions land here, newest first.");
        return;
    }

    ui.spacing_mut().item_spacing.y = 0.0;
    let mut day = String::new();
    let mut drawn = false;
    let mut prev_minor = false;
    for (i, (_, item)) in items.iter().enumerate() {
        match item {
            Item::Handoff(at) => {
                mark_day(ui, Some(at), &mut day, &mut drawn, &mut prev_minor);
                ui.add_space(timeline_gap(drawn, prev_minor, true));
                minor_line(ui, Node::Person(&owner_seed), owner, &format!("handed this to {short}"), Some(at));
                prev_minor = true;
            }
            Item::Decision(p) => {
                let at = str_of(p, "decidedAt");
                mark_day(ui, at, &mut day, &mut drawn, &mut prev_minor);
                ui.add_space(timeline_gap(drawn, prev_minor, true));
                let (mark, rest) = match str_of(p, "decision") {
                    Some("approved") => (Mark::Approved, "approved the plan".to_owned()),
                    _ => (
                        Mark::Changes,
                        match str_of(p, "changesNote") {
                            Some(note) => format!("asked for changes to the plan: {note}"),
                            None => "asked for changes to the plan".to_owned(),
                        },
                    ),
                };
                minor_line(ui, Node::Mark(mark), owner, &rest, at);
                prev_minor = true;
            }
            Item::Plan(p) => {
                let at = str_of(p, "createdAt");
                mark_day(ui, at, &mut day, &mut drawn, &mut prev_minor);
                ui.add_space(timeline_gap(drawn, prev_minor, false));
                let id = str_of(p, "id").unwrap_or_default();
                entry(ui, Node::Agent(agent_seed), short, "sent a plan", at, true, |ui| {
                    let (label, tone) = match str_of(p, "decision") {
                        Some("approved") => ("Approved", c::Tone::Ok),
                        Some(_) => ("Changes requested", c::Tone::Running),
                        None => ("Waiting for review", c::Tone::Ask),
                    };
                    c::chip(ui, label, tone, true);
                    super::mrkdwn::show(ui, str_of(p, "summary").unwrap_or_default(), colour::TEXT());
                    let open_id = egui::Id::new(("feed:plan", f.task_id, id));
                    let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
                    face::disclosure(ui, open_id.with("d"), "Full plan", None, &mut open);
                    ui.data_mut(|d| d.insert_temp(open_id, open));
                    if open {
                        super::mrkdwn::show(ui, str_of(p, "plan").unwrap_or_default(), colour::TEXT_2());
                    }
                });
                prev_minor = false;
            }
            Item::Note(n) => {
                let kind = str_of(n, "kind").unwrap_or("note");
                let at = str_of(n, "createdAt");
                let body = str_of(n, "body").unwrap_or_default();
                let note_agent = n.get("agent").filter(|a| a.is_object());
                let author = match note_agent {
                    Some(a) => short_name(str_of(a, "name").unwrap_or(agent)),
                    None => str_of(n, "authorName").unwrap_or("Someone"),
                };
                let seed_owned;
                let node = match note_agent {
                    Some(a) => Node::Agent(str_of(a, "id").unwrap_or(agent_seed)),
                    None => {
                        seed_owned = str_of(n, "authorEmail").unwrap_or(author).to_owned();
                        Node::Person(&seed_owned)
                    }
                };
                let pos = notes.iter().position(|m| std::ptr::eq(*m, *n));
                mark_day(ui, at, &mut day, &mut drawn, &mut prev_minor);
                match kind {
                    "progress" => {
                        let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
                        let rest = if flat.is_empty() { "posted an update".to_owned() } else { flat };
                        ui.add_space(timeline_gap(drawn, prev_minor, true));
                        minor_line(ui, node, author, &rest, at);
                        prev_minor = true;
                    }
                    "review" => {
                        let (mark, verb) = if pos.is_some_and(|at| approved(&notes, at, state)) {
                            (Mark::Approved, "approved the work")
                        } else {
                            (Mark::Changes, "asked for changes")
                        };
                        ui.add_space(timeline_gap(drawn, prev_minor, true));
                        minor_line(ui, Node::Mark(mark), author, verb, at);
                        prev_minor = true;
                    }
                    _ => {
                        let label = match kind {
                            "question" => "asked",
                            "answer" => "answered",
                            "instruction" => "told the agent",
                            "submission" => "submitted for review",
                            _ => "noted",
                        };
                        let node = match kind {
                            "submission" => Node::Mark(Mark::Submitted),
                            _ => node,
                        };
                        ui.add_space(timeline_gap(drawn, prev_minor, false));
                        let body_id = egui::Id::new(("feed:body", f.task_id, str_of(n, "id").unwrap_or_default(), i));
                        substantive(ui, node, author, label, at, private_kind(kind), body_id, body, |_| {});
                        prev_minor = false;
                    }
                }
            }
        }
        drawn = true;
    }
}

// ------------------------------------------------------------------- attach

/// The owner's way back into the agent's Claude Code session on their own
/// Mac: a cmux workspace attached to it, or — failing that — the attach
/// command on the clipboard. Owner-only: the session lives on their machine,
/// not the dashboard's.
fn cmux_row(ui: &mut egui::Ui, session: &Value, title: &str) {
    let Some(session_id) = str_of(session, "sessionId") else { return };
    let cwd = str_of(session, "cwd").unwrap_or_default();
    let path = tildify(cwd);
    // Wrapped, and the path elided to whatever is left on its line: a
    // worktree path is the one string here with nowhere to break on its own.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::SM, space::XS);
        if w::secondary(ui, "Open in cmux", true).clicked() {
            open_cmux(ui.ctx(), title, cwd, session_id);
        }
        w::muted(ui, "Claude session \u{00B7}");
        w::id(ui, session_id);
        w::muted(ui, "\u{00B7}");
        let fitted = elide(ui, &path, ui.available_width());
        ui.label(RichText::new(fitted).size(text::SMALL).color(colour::TEXT_MUTED())).on_hover_text(&path);
        if w::link(ui, "Copy").clicked() {
            ui.ctx().copy_text(attach_command(cwd, session_id));
            w::toast(ui.ctx(), "Attach command copied.", false);
        }
    });
    w::caption(ui, "While it\u{2019}s open, the agent waits and won\u{2019}t run in the background.");
}

/// `cmux new-workspace`, non-blocking: `cmux` on PATH first, then its
/// absolute install path. Either succeeding hands the terminal to cmux; if
/// neither spawns, the attach command goes to the clipboard instead.
///
/// ponytail: "available" is judged only by whether the process spawned, not
/// whether the workspace actually opened (that would mean waiting on it,
/// which blocks the UI thread). If cmux starts but its own command fails,
/// that shows up in cmux's window, not here.
fn open_cmux(ctx: &egui::Context, title: &str, cwd: &str, session_id: &str) {
    let resume = resume_command(session_id);
    let spawn = |bin: &str| {
        Command::new(bin)
            .args(["new-workspace", "--name", title, "--cwd", cwd, "--command", resume.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    };
    if spawn("cmux").or_else(|_| spawn(CMUX_FALLBACK)).is_ok() {
        return;
    }
    ctx.copy_text(attach_command(cwd, session_id));
    w::toast(ctx, "cmux isn\u{2019}t available \u{2014} copied the attach command instead.", true);
}

fn attach_command(cwd: &str, session_id: &str) -> String {
    format!("cd '{cwd}' && {}", resume_command(session_id))
}

/// The command that resumes the session: the agent's own attach script when
/// it is installed — it carries the MCP wiring, token and rules a bare
/// `claude --resume` skips, which is what lets the resumed session keep
/// talking back to the dashboard — a bare resume otherwise.
fn resume_command(session_id: &str) -> String {
    let script = std::env::var("HOME").ok().map(|home| format!("{home}/.airtribe-agent/attach.sh"));
    match script {
        Some(path) if std::path::Path::new(&path).exists() => format!("'{path}' {session_id}"),
        _ => format!("claude --resume {session_id}"),
    }
}

/// The home folder as `~`, the way a person reads their own path.
fn tildify(path: &str) -> String {
    std::env::var("HOME")
        .ok()
        .and_then(|home| path.strip_prefix(home.as_str()).map(|rest| format!("~{rest}")))
        .unwrap_or_else(|| path.to_owned())
}

/// "Hermes" out of "Hermes (Anmol's Mac)": the name a sentence can carry.
pub(super) fn short_name(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name).trim()
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// The owner's avatar seed: the same one their person disc uses, so an agent
/// wears exactly its owner's colour.

/// The four stages, which one it is at, and whether it finished. Times come
/// from the notes that marked each one.
fn stages(state: &str, s: &Session, owner_first: &str) -> (Vec<Step>, usize, bool) {
    let first = |kind: &str| s.notes.iter().find(|n| str_of(n, "kind") == Some(kind)).and_then(|n| str_of(n, "createdAt"));
    let last = |kind: &str| s.notes.iter().rev().find(|n| str_of(n, "kind") == Some(kind)).and_then(|n| str_of(n, "createdAt"));
    let when = |at: Option<&str>| at.map(exact).filter(|e| !e.is_empty());
    let third = match state {
        "needs_input" | "plan_review" if s.mine => "Needs you".to_owned(),
        "needs_input" | "plan_review" => format!("Needs {owner_first}"),
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
        "needs_input" | "in_review" | "plan_review" => (2, false),
        "done" => (3, true),
        _ => (3, false),
    };
    (steps, current, complete)
}

/// The line under the stepper. While the agent works, what it says it is doing
/// and how fresh that is; otherwise whose move it is, in words.
fn now_line(ui: &mut egui::Ui, d: &Value, state: &str, short: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        let words = match state {
            "working" | "acknowledged" => {
                face::spinner(ui, text::BODY);
                match str_of(d, "now").map(str::trim).filter(|n| !n.is_empty()) {
                    Some(now) => {
                        let at = str_of(d, "nowAt");
                        let when = at.map(|a| ui.painter().layout_no_wrap(ago(a), egui::FontId::proportional(text::SMALL), colour::TEXT_FAINT()));
                        let room = ui.available_width() - when.as_ref().map_or(0.0, |g| g.size().x + space::SM);
                        let line = w::truncated(ui, now, egui::FontId::proportional(text::BODY), colour::TEXT_2(), room.max(0.0));
                        let shown = ui.add(egui::Label::new(line));
                        shown.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, now));
                        if let (Some(a), Some(g)) = (at, when) {
                            ui.add(egui::Label::new(g)).on_hover_text(exact(a));
                        }
                        return;
                    }
                    None if state == "acknowledged" => format!("{short} picked this up"),
                    None => "Working \u{2014} no update yet".to_owned(),
                }
            }
            "handed_off" => format!("Waiting for {short} to pick this up"),
            "done" => "Finished \u{2014} the review approved it".to_owned(),
            "stopped" => format!("Taken back from {short}"),
            other => state_words(other).to_owned(),
        };
        ui.label(RichText::new(words).size(text::SMALL).color(colour::TEXT_MUTED()));
    });
}


// ----------------------------------------------------------------- timeline

pub(super) enum Node<'a> {
    /// A person's disc: their answers, instructions, the hand-off.
    Person(&'a str),
    Agent(&'a str),
    Mark(Mark),
}

#[derive(Clone, Copy)]
pub(super) enum Mark {
    Progress,
    Question,
    Submitted,
    Approved,
    Changes,
    /// An intake agent filed a task from something it read.
    Filed,
}

/// One timeline entry: the node in the rail, who and what beside it, then
/// whatever `body` adds. Returns the node's rect, for the rail's hairline.
pub(super) fn entry(
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
            ui.scope(|ui| {
                ui.spacing_mut().interact_size.y = NODE;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    ui.label(
                        RichText::new(who)
                            .size(text::SMALL)
                            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                            .color(colour::TEXT()),
                    );
                    if !verb.is_empty() {
                        ui.label(RichText::new(verb).size(text::SMALL).color(colour::TEXT_MUTED()));
                    }
                    if let Some(at) = at {
                        ui.label(RichText::new(day_time(at)).size(text::SMALL).color(colour::TEXT_FAINT())).on_hover_text(exact(at));
                    }
                    if private {
                        ui.add_space(space::XS);
                        face::private_label(ui);
                    }
                });
            });
            body(ui);
        });
    });
    rect
}

/// Paints into whatever size `r` happens to be — the full `NODE` column or a
/// minor line's smaller mark — rather than assuming one fixed size.
fn paint_node(ui: &mut egui::Ui, r: egui::Rect, node: &Node) {
    let p = ui.painter();
    let c = r.center();
    let side = r.width();
    match node {
        Node::Person(seed) => avatar::paint(p, r.shrink(1.0), seed),
        Node::Agent(seed) => face::paint_still(p, r, seed),
        Node::Mark(mark) => {
            let (ink, fill) = match mark {
                Mark::Progress => (colour::TEXT_MUTED(), colour::SURFACE()),
                Mark::Question => (colour::ASK(), colour::ASK_BG()),
                Mark::Submitted => (colour::AGENT(), colour::AGENT_BG()),
                Mark::Approved => (colour::OK(), colour::OK_BG()),
                Mark::Changes => (colour::WARN(), colour::WARN_BG()),
                Mark::Filed => (colour::INFO(), colour::INFO_BG()),
            };
            p.circle_filled(c, side / 2.0, fill);
            p.circle_stroke(c, side / 2.0 - 0.5, egui::Stroke::new(1.0, ink.gamma_multiply(0.35)));
            match mark {
                Mark::Progress => {
                    p.circle_filled(c, (side * 0.12).max(2.0), ink);
                }
                Mark::Question => {
                    p.text(c, egui::Align2::CENTER_CENTER, "?", egui::FontId::new(text::SMALL.min(side * 0.7), egui::FontFamily::Name(theme::BOLD.into())), ink);
                }
                Mark::Submitted => glyph::arrow_up(p, c, side * 0.7, ink),
                Mark::Approved => glyph::tick(p, c, side * 0.55, ink),
                Mark::Changes => glyph::back(p, c, side * 0.7, ink),
                Mark::Filed => glyph::hash(p, c, side * 0.55, ink),
            }
        }
    }
}

/// A subtle day separator — "Today", "Yesterday" — drawn once per day the
/// timeline crosses, so a list that keeps the server's order still reads as
/// one that moves through time.
fn day_marker(ui: &mut egui::Ui, label: &str) {
    ui.label(
        RichText::new(label)
            .size(text::CAPTION)
            .family(egui::FontFamily::Name(theme::MEDIUM.into()))
            .color(colour::TEXT_MUTED()),
    );
}

/// Draws the separator the moment `at` crosses into a new day; a no-op
/// otherwise. Resets `prev_minor` so the item right after one never looks
/// like it is bunched under the last group.
fn mark_day(ui: &mut egui::Ui, at: Option<&str>, day: &mut String, drawn: &mut bool, prev_minor: &mut bool) {
    let Some(label) = at.map(day_label).filter(|l| !l.is_empty() && l.as_str() != day.as_str()) else { return };
    if *drawn {
        ui.add_space(space::XL);
    }
    day_marker(ui, &label);
    ui.add_space(space::XS);
    *day = label;
    *drawn = false;
    *prev_minor = false;
}

/// The vertical gap before the next timeline item: none before the first,
/// tight between two minor lines, roomier whenever a block is involved.
fn timeline_gap(drawn: bool, prev_minor: bool, minor: bool) -> f32 {
    if !drawn {
        0.0
    } else if prev_minor && minor {
        space::XXS
    } else {
        space::MD
    }
}

/// A single low-signal line: a small mark, who did it in ink, the rest
/// muted, the time inline after — `entry`'s who/verb/time, at one line's
/// height instead of a block's. No card, nothing to expand — the kind of
/// event a reader's eye should pass over rather than stop at.
fn minor_line(ui: &mut egui::Ui, node: Node, who: &str, rest: &str, at: Option<&str>) {
    // Painted by hand into one pre-measured rect, the way `disclosure` and
    // `pill` do: nesting a right-aligned layout inside a left-to-right one
    // has the parent's cursor jump to the row's far right the moment
    // anything is placed there, leaving nothing for a sibling after it.
    let height = face::XS.max(text::SMALL * 1.3);
    // The same measure `entry` reads at: NODE for the mark column, MD to its
    // text, then a prose line — capped the same way, so a run of minor lines
    // and the blocks between them share one left and right edge.
    let width = (NODE + space::MD + PROSE_W).min(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let label = if rest.is_empty() { who.to_owned() } else { format!("{who} {rest}") };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    if let Some(a) = at {
        response.on_hover_text(exact(a));
    }

    // The mark centred in a NODE-wide slot, not flush left of it: `entry`'s
    // text starts at NODE + MD, and a smaller mark flush left of that same
    // column read as a ragged edge against it.
    let mark = egui::Rect::from_center_size(
        egui::pos2(rect.left() + NODE / 2.0, rect.center().y),
        egui::Vec2::splat(face::XS),
    );
    paint_node(ui, mark, &node);

    let p = ui.painter();
    let who_galley = p.layout_no_wrap(
        who.to_owned(),
        egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::SEMIBOLD.into())),
        colour::TEXT(),
    );
    let mut x = rect.left() + NODE + space::MD;
    p.galley(egui::pos2(x, rect.center().y - who_galley.size().y / 2.0), who_galley.clone(), colour::TEXT());
    x += who_galley.size().x;

    // Measured up front so the rest of the line truncates around it rather
    // than running under it — the same order `entry`'s row reads in, just
    // painted by hand instead of laid out by egui.
    let time = at.map(|a| p.layout_no_wrap(day_time(a), egui::FontId::proportional(text::CAPTION), colour::TEXT_FAINT()));
    if !rest.is_empty() {
        x += space::XS;
        let time_w = time.as_ref().map_or(0.0, |g| g.size().x + space::SM);
        let max_w = (rect.right() - time_w - x).max(0.0);
        let rest_galley = w::truncated(ui, rest, egui::FontId::proportional(text::SMALL), colour::TEXT_MUTED(), max_w);
        p.galley(egui::pos2(x, rect.center().y - rest_galley.size().y / 2.0), rest_galley.clone(), colour::TEXT_MUTED());
        x += rest_galley.size().x + space::SM;
    }
    if let Some(g) = time {
        p.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g.clone(), colour::TEXT_FAINT());
    }
}

/// How many lines of a body show before "Show more" — long enough to be
/// useful, short enough that a report can't turn the whole timeline into a
/// wall of text.
const CLAMP_LINES: usize = 6;

/// A markdown body, cut to `CLAMP_LINES` once it runs past them, with "Show
/// more" to read the rest. Expansion is remembered per entry via `id`.
fn clamped_body(ui: &mut egui::Ui, id: egui::Id, body: &str, ink: egui::Color32) {
    let long = body.lines().count() > CLAMP_LINES || body.chars().count() > 640;
    let open = !long || ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if open {
        super::mrkdwn::show(ui, body, ink);
    } else {
        let preview = body.lines().take(CLAMP_LINES).collect::<Vec<_>>().join("\n");
        super::mrkdwn::show(ui, &preview, ink);
    }
    if long {
        ui.add_space(space::XXS);
        if w::link(ui, if open { "Show less" } else { "Show more" }).clicked() {
            ui.data_mut(|d| d.insert_temp(id, !open));
        }
    }
}

/// A block for something with real content: an author's avatar at `SM`, what
/// kind of entry it is, when, and its body as markdown.
#[allow(clippy::too_many_arguments)]
fn substantive(
    ui: &mut egui::Ui,
    node: Node,
    author: &str,
    kind_label: &str,
    at: Option<&str>,
    private: bool,
    body_id: egui::Id,
    body: &str,
    extra: impl FnOnce(&mut egui::Ui),
) {
    entry(ui, node, author, kind_label, at, private, |ui| {
        clamped_body(ui, body_id, body, colour::TEXT_2());
        extra(ui);
    });
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
) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::MD;
        let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(NODE), egui::Sense::hover());
        paint_node(ui, r, &Node::Mark(Mark::Submitted));
        // A frame takes its parent's layout; the card reads top to bottom.
        ui.vertical(|ui| pinned_card(ui, colour::ASK(), |ui| {
            ui.spacing_mut().item_spacing.y = space::SM;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                ui.label(
                    RichText::new("Report")
                        .size(text::SMALL)
                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                        .color(colour::TEXT()),
                );
                ui.label(RichText::new(format!("\u{00B7} {short} submitted this for review")).size(text::SMALL).color(colour::TEXT_MUTED()));
                if let Some(at) = str_of(note, "createdAt") {
                    ui.label(RichText::new(ago(at)).size(text::SMALL).color(colour::TEXT_FAINT())).on_hover_text(exact(at));
                }
            });
            ui.scope(|ui| {
                ui.set_max_width(PROSE_W.min(ui.available_width()));
                let body_id = egui::Id::new(("session:body", s.task_id, str_of(note, "id").unwrap_or_default()));
                clamped_body(ui, body_id, str_of(note, "body").unwrap_or_default(), colour::TEXT());
            });

            let evidence: Vec<&Value> =
                s.evidence.iter().filter(|r| matches!(str_of(r, "kind"), Some("pr" | "commit" | "figma"))).collect();
            ui.add_space(space::XS);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(space::SM, space::SM);
                if evidence.is_empty() {
                    ui.label(RichText::new("No evidence attached").size(text::SMALL).color(colour::TEXT_FAINT()));
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
            ui.painter().hline(rule.x_range(), rule.center().y, egui::Stroke::new(1.0, colour::LINE()));
            if let Some(a) = review_controls(ui, st, short, s.busy) {
                *ask = Some(a);
            }
        }));
    });
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
    let galley = w::truncated(ui, &label, font, colour::TEXT_2(), 260.0);
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
    p.rect_filled(rect, radius::SM as f32, if hot { colour::SURFACE_HOVER() } else { colour::INSET() });
    p.rect_stroke(rect, radius::SM as f32, egui::Stroke::new(1.0, if hot { colour::LINE_STRONG() } else { colour::LINE() }), egui::StrokeKind::Inside);
    glyph::evidence(p, pos2(rect.left() + space::SM + icon / 2.0, rect.center().y), icon, kind, kind_ink(kind));
    p.galley(pos2(rect.left() + space::SM + icon + space::XS, rect.center().y - galley.size().y / 2.0), galley, if hot { colour::TEXT() } else { colour::TEXT_2() });
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let response = response.on_hover_text(url);
    if response.clicked() && kind != "commit" && !url.is_empty() {
        super::mrkdwn::open(ui.ctx(), url);
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
        "pr" => colour::INFO(),
        "commit" => colour::OK(),
        "figma" => colour::AGENT(),
        _ => colour::TEXT_MUTED(),
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
            w::pill(ui, "live", colour::INFO());
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
        .fill(colour::LOG_BG())
        .stroke(egui::Stroke::new(1.0, colour::LINE_SOFT()))
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
                    ui.label(RichText::new(note).monospace().size(text::SMALL).color(colour::LOG_SEQ()));
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
                // The cap, not the estimate: a wrapped line is taller than one row.
                .max_height(line_h * LOG_ROWS)
                .stick_to_bottom(true)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = space::XXS;
                    for (seq, line) in &st.lines {
                        ui.horizontal_top(|ui| {
                            ui.add(
                                egui::Label::new(RichText::new(format!("{seq:>gutter$}")).monospace().size(text::SMALL).color(colour::LOG_SEQ()))
                                    .selectable(false),
                            );
                            ui.add_space(space::SM);
                            // Wrapped to what is left of the row: a long step
                            // (a path, a command) ran off the window.
                            shell::selectable(
                                ui,
                                egui::Label::new(RichText::new(line).monospace().size(text::SMALL).color(colour::LOG_TEXT())).wrap(),
                            );
                        });
                    }
                    shell::edge_scroll(ui);
                });
        });
}

// --------------------------------------------------------------- home strip

/// Everyone's agents holding a task right now. Team-visible.
pub(super) const ACTIVE_KEY: &str = "agents:active";

/// Home's "Agents at work": one compact row per agent holding a task. Absent
/// when there are none — and when the server does not have the endpoint yet,
/// rather than an error on the dashboard. Returns a task to open.
pub(super) fn at_work(ui: &mut egui::Ui, net: &mut Net, me: &str) -> Option<String> {
    net.get_once(ACTIVE_KEY, "/api/user/agents/active");
    let rows = net.shared(ACTIVE_KEY)?;
    let rows = rows.as_array().filter(|r| !r.is_empty())?;
    let mut open = None;
    shell::section_count(ui, "Agents at work", rows.len());
    w::card_list(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in rows {
            if active_row(ui, row, me) {
                open = row.get("task").and_then(|t| str_of(t, "id")).map(str::to_owned);
            }
        }
    });
    open
}

fn active_row(ui: &mut egui::Ui, row: &Value, me: &str) -> bool {
    let empty = Value::Null;
    let agent = row.get("agent").unwrap_or(&empty);
    let owner = row.get("owner").unwrap_or(&empty);
    let task = row.get("task").unwrap_or(&empty);
    let name = str_of(agent, "name").unwrap_or("Agent");
    let short = short_name(name);
    let owner_name = str_of(owner, "name").unwrap_or_default();
    let owner_first = first_name(owner_name);
    // The agent's own id, not its owner's: two agents at work for the same
    // person must read as two different agents, not one repeated.
    let seed = str_of(agent, "id").unwrap_or(name);
    let state = str_of(row, "state").unwrap_or("working");
    let presence = Presence::of(state, str_of(row, "lastSeenAt"));
    let title = str_of(task, "title").unwrap_or("Untitled");
    let now = str_of(row, "now").map(str::trim).filter(|n| !n.is_empty());
    let since = str_of(row, "delegatedAt").map(elapsed).unwrap_or_default();

    let response = w::row(ui, |ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        face::avatar(ui, seed, face::SM, presence, name);
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
                    .color(colour::TEXT()),
            );
            ui.add(egui::Label::new(RichText::new(format!("\u{00B7} {owner_first}")).size(text::SMALL).color(colour::TEXT_MUTED())).truncate());
        });
        fixed(ui, title_w, |ui| {
            ui.add(egui::Label::new(RichText::new(title).size(text::SMALL).color(colour::TEXT())).truncate());
        });
        let rest = (ui.available_width() - since_w - space::SM).max(0.0);
        fixed(ui, rest, |ui| {
            let (words, ink) = match (state, now) {
                ("working" | "acknowledged", Some(now)) => (now.to_owned(), colour::TEXT_MUTED()),
                ("needs_input", _) if !me.is_empty() && str_of(owner, "id") == Some(me) => ("Waiting on you".to_owned(), colour::ASK()),
                ("needs_input", _) => (format!("Waiting on {owner_first}"), colour::ASK()),
                ("plan_review", _) => ("Plan review".to_owned(), colour::ASK()),
                ("in_review", _) => ("In review".to_owned(), colour::ASK()),
                ("acknowledged", None) => ("Picked up".to_owned(), colour::TEXT_MUTED()),
                _ => (state_words(state).to_owned(), colour::TEXT_MUTED()),
            };
            ui.add(egui::Label::new(RichText::new(words).size(text::SMALL).color(ink)).truncate());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(since).size(text::SMALL).color(colour::TEXT_FAINT()));
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

/// A review approved the work when nothing was submitted after it and the
/// session ended — the note carries no decision of its own.
fn approved(notes: &[&Value], at: usize, state: &str) -> bool {
    let last = |kind: &str| notes.iter().rposition(|n| str_of(n, "kind") == Some(kind));
    state == "done" && last("review") == Some(at) && last("submission").is_none_or(|s| s < at)
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
