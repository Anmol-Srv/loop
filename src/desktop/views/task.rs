//! Task detail: the page a human sits on while the work moves.
//!
//! A task belongs to one person, and that person moves it through its life.
//! The server enforces it — `PATCH /api/user/tasks/{id}` answers 403 to anyone
//! who is neither the assignee nor an admin — so this screen shows the moves
//! only to whoever can actually make them, and tells everyone else whose call
//! it is. Offering a button that is going to 403 is worse than offering none.
//!
//! The run log is the other half of the page, so the fetch discipline matters
//! as much as the layout. Two rules shape it:
//!
//! * A finished task must not poll. Polling is gated on `status ==
//!   "in_progress"`, and the tick comes from `request_repaint_after`, so egui
//!   goes back to sleep between ticks instead of spinning at frame rate.
//! * Log lines live here, not in the net cache, because the incremental
//!   `afterSeq` fetch returns only the tail. The cache holds one reply; we hold
//!   the transcript.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use egui::RichText;
use serde_json::{json, Value};

use super::projects::PROSE_W;
use crate::desktop::design::{
    avatar, cards as c, colour, pad, radius, shell, size, space, status_label, text, theme, viz,
    widgets as w,
};
use crate::desktop::{App, Tab};

const TASK_KEY: &str = "task:one";
const LOG_KEY: &str = "task:logs";
const PATCH_KEY: &str = "task:patch";
const ARTIFACTS_KEY: &str = "task:artifacts";
const ATTACH_KEY: &str = "task:artifact:new";
const NOTES_KEY: &str = "task:notes";
const NOTE_KEY: &str = "task:note:new";

/// How far out we ask egui to wake us.
const POLL: Duration = Duration::from_secs(2);
/// Fire slightly early: a repaint scheduled for +2s can land a hair under it,
/// and a strict `>= POLL` test would then skip a tick and halve the cadence.
const DUE: Duration = Duration::from_millis(1_900);

/// The log well, in rows rather than pixels — tall enough to watch a run, short
/// enough that the controls above it stay on screen.
const LOG_ROWS: f32 = 11.0;

/// The assignee disc. Sized off the spacing scale so it lines up with the pills
/// beside it instead of inventing a diameter.
const AVATAR: f32 = size::AVATAR_SM;

/// The evidence prompt: what the screen asks for when a move needs proof the
/// task has none of yet.
///
/// It is a panel and not a modal because the thing it asks about — "did you
/// open a PR?" — is answered by looking at the page behind it.
struct Prompt {
    /// The status to move to once the evidence lands. `None` when the panel
    /// was opened by "+ Add" and no move is waiting on it.
    then: Option<&'static str>,
    /// The kinds this panel will accept, as `viz::select` wants them.
    kinds: Vec<(String, String)>,
    kind: Option<String>,
    url: String,
    title: String,
    /// Set once "or mark it done manually" is taken: the panel swaps the link
    /// form for this one reason. `None` while the link form is showing.
    reason: Option<String>,
}

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
    /// The last move's outcome: the message, and whether it failed. A failure
    /// is the server's own sentence — it is the only thing that explains a 403.
    notice: Option<(String, bool)>,
    prompt: Option<Prompt>,
    /// An artifact POST is out. Its reply is what releases the PATCH behind it.
    attaching: bool,
    note: String,
    posting_note: bool,
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
            prompt: None,
            attaching: false,
            note: String::new(),
            posting_note: false,
        }
    }
}

thread_local! {
    static LOCAL: RefCell<Option<Local>> = const { RefCell::new(None) };
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let Some(task_id) = app.task.clone() else { return };

    LOCAL.with(|cell| {
        let mut slot = cell.borrow_mut();
        // Opening a different task: drop the transcript and the per-task caches.
        if slot.as_ref().map(|s| s.task_id != task_id).unwrap_or(true) {
            *slot = Some(Local::new(task_id.clone()));
            if let Some(net) = app.net.as_mut() {
                net.invalidate_prefix("task:");
            }
        }
        let local = slot.as_mut().expect("just populated");
        render(app, ui, &task_id, local);
    });
}

fn render(app: &mut App, ui: &mut egui::Ui, task_id: &str, local: &mut Local) {
    // Who is looking. The assignee moves the task; an admin overrides, which is
    // what an admin is for. Read before `net` is borrowed mutably below.
    let me = me_str(app, "personId");
    let admin = me_str(app, "role") == "admin";

    let net = app.net.as_mut().expect("net is live whenever a view runs");
    net.get_once(TASK_KEY, &format!("/api/user/tasks/{task_id}"));
    net.get_once(
        ARTIFACTS_KEY,
        &format!("/api/user/artifacts?parentType=task&parentId={task_id}"),
    );
    net.get_once(NOTES_KEY, &format!("/api/user/tasks/{task_id}/notes"));

    let task = net.data(TASK_KEY).cloned();

    let mut leave = false;
    ui.horizontal(|ui| {
        if shell::back(ui, "Back").clicked() {
            leave = true;
        }
        w::id(ui, task_id);
    });
    if leave {
        app.task = None;
        return;
    }
    ui.add_space(space::XS);

    let net = app.net.as_mut().expect("net is live whenever a view runs");
    let Some(task) = task else {
        if let Some(err) = net.error(TASK_KEY) {
            failed(ui, "Could not load the task", err);
        } else if net.is_loading(TASK_KEY) {
            w::loading(ui, "Loading task");
        } else {
            w::empty(ui, "That task is no longer here", "It may have been dropped or moved.");
        }
        return;
    };

    let status = str_of(&task, "status").unwrap_or("open").to_string();
    let mine = !me.is_empty() && str_of(&task, "assigneePersonId") == Some(me.as_str());
    let track = Track::of(str_of(&task, "discipline"));
    // Read out of the cache before `actions` borrows `net` mutably: the whole
    // question it asks of the list is "is the required kind already here?".
    let held: Vec<String> = net
        .data(ARTIFACTS_KEY)
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(|r| str_of(r, "kind")).map(str::to_owned).collect())
        .unwrap_or_default();

    heading(ui, &task, &status);
    let open_project = meta_line(ui, &task);
    description(ui, &task);
    manual_reason(ui, &task);
    actions(ui, net, task_id, &task, &status, track, admin || mine, &held, local);
    resources(ui, net, local);
    notes(ui, net, task_id, local);
    run_log(ui, net, task_id, &status, local);

    // `net`'s borrow of `app` ends above, so navigation happens last.
    if let Some(project_id) = open_project {
        app.task = None;
        app.project = Some(project_id);
        app.tab = Tab::Projects;
    }
}

// ---------------------------------------------------------------- the top

/// The title, with the state it is in beside it. No card: this is the page's
/// own heading, and a box around a heading is a box around nothing.
fn heading(ui: &mut egui::Ui, task: &Value, status: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(str_of(task, "title").unwrap_or("Untitled"))
                .size(text::TITLE)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        ui.add_space(space::SM);
        c::chip(ui, &sentence(status_label(status)), c::status_tone(status), true);
    });
    ui.add_space(space::XXS);
}

/// Everything a task *is*, on one line: where it lives, who holds it, how
/// urgent, what kind of work, how old.
///
/// A line, not a card and not a label grid. Six facts in a two-column table
/// would be the tallest thing on the page and the least read; set out along
/// one line, a fact's width apart, they are one glance. Returns the project id
/// when the project name is clicked.
fn meta_line(ui: &mut egui::Ui, task: &Value) -> Option<String> {
    let mut open_project = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;

        let project = str_of(task, "projectName").unwrap_or_default();
        if !project.is_empty() && w::link(ui, project).clicked() {
            open_project = str_of(task, "projectId").map(str::to_owned);
        }

        ui.add_space(space::XL);
        match str_of(task, "assigneeName") {
            Some(name) if !name.is_empty() => {
                let seed = str_of(task, "assigneeEmail").unwrap_or(name);
                avatar::small(ui, seed, AVATAR);
                value(ui, name);
            }
            _ => label(ui, "Unassigned"),
        }

        if let Some(p) = task.get("priority").and_then(Value::as_i64) {
            ui.add_space(space::XL);
            c::chip(ui, &format!("P{p}"), priority_tone(p), false);
        }

        let discipline = str_of(task, "discipline").unwrap_or_default();
        if !discipline.is_empty() {
            ui.add_space(space::XL);
            w::discipline(ui, discipline);
        }

        if let Some(created) = str_of(task, "createdAt") {
            ui.add_space(space::XL);
            label(ui, "Created");
            value(ui, &ago(created));
        }
        if let Some(done) = str_of(task, "doneAt") {
            ui.add_space(space::XL);
            label(ui, "Done");
            value(ui, &ago(done));
        }
    });
    open_project
}

/// P0 shouts and P4 whispers, in the same chip vocabulary as status — the meta
/// line should read as one row of tokens, not two competing systems.
fn priority_tone(p: i64) -> c::Tone {
    match p {
        0 => c::Tone::Blocked,
        1 => c::Tone::Running,
        2 => c::Tone::Neutral,
        _ => c::Tone::Quiet,
    }
}

fn label(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT_MUTED));
}

fn value(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT));
}

/// The task's own words, at a prose measure. Paragraphs split on a blank line,
/// because that is how whoever filed it typed them.
fn description(ui: &mut egui::Ui, task: &Value) {
    shell::section(ui, "Description");
    let body = str_of(task, "body").unwrap_or("").trim();
    if body.is_empty() {
        w::caption(ui, "No description");
        return;
    }
    ui.scope(|ui| {
        ui.set_max_width(PROSE_W.min(ui.available_width()));
        for (i, para) in body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).enumerate() {
            if i > 0 {
                ui.add_space(space::MD);
            }
            ui.label(RichText::new(para).size(text::BODY).color(colour::TEXT_2));
        }
    });
}

/// Why this task finished without a link. A task closed on someone's word
/// rather than on evidence should say so wherever the task is read — quietly,
/// but in the same breath as the description.
fn manual_reason(ui: &mut egui::Ui, task: &Value) {
    let Some(reason) = str_of(task, "manualReason").map(str::trim).filter(|r| !r.is_empty()) else {
        return;
    };
    ui.add_space(space::SM);
    ui.scope(|ui| {
        ui.set_max_width(PROSE_W.min(ui.available_width()));
        ui.label(
            RichText::new(format!("Completed manually \u{2014} {reason}"))
                .size(text::SMALL)
                .color(colour::TEXT_MUTED),
        );
    });
}

// ---------------------------------------------------------------- the moves

/// Which life a task leads. It is the assignee's department that decides:
/// design work ends at a handoff, engineering work ends at a deploy.
#[derive(Clone, Copy, PartialEq)]
enum Track {
    Eng,
    Design,
}

impl Track {
    /// An unassigned task has no department, so it has no track yet. It takes
    /// the engineering one, which is the only place its legal first move
    /// (`in_progress`) and the eng flow agree — and the server has the last
    /// word on anything further anyway.
    fn of(discipline: Option<&str>) -> Self {
        match discipline {
            Some("design") => Track::Design,
            _ => Track::Eng,
        }
    }

    /// The artifact kinds that count as evidence on this track, in the order
    /// the picker should offer them.
    fn evidence(self) -> &'static [&'static str] {
        match self {
            Track::Eng => &["pr", "commit"],
            Track::Design => &["figma"],
        }
    }
}

/// Where a task can go from here, and how loudly each move is offered.
///
/// One filled button per state — the move that is almost always the right one —
/// then the alternatives outlined, then the way out in ghost. The set is
/// deliberately small: a task in handoff has two honest futures, and a row of
/// six buttons makes you read all six to find them.
fn moves(track: Track, status: &str) -> &'static [(&'static str, w::Emphasis, &'static str)] {
    match (track, status) {
        (_, "open") => &[
            ("Start", w::Emphasis::Primary, "in_progress"),
            ("Close", w::Emphasis::Ghost, "dropped"),
        ],
        (Track::Eng, "in_progress") => &[
            ("Complete", w::Emphasis::Primary, "completed"),
            ("Block", w::Emphasis::Secondary, "blocked"),
            ("Close", w::Emphasis::Ghost, "dropped"),
        ],
        (Track::Eng, "completed") => &[
            ("Ship", w::Emphasis::Primary, "shipped"),
            ("Reopen", w::Emphasis::Secondary, "in_progress"),
        ],
        (Track::Eng, "shipped") => &[("Reopen", w::Emphasis::Secondary, "in_progress")],
        (Track::Design, "in_progress") => &[
            ("Hand off", w::Emphasis::Primary, "handoff"),
            ("Block", w::Emphasis::Secondary, "blocked"),
            ("Close", w::Emphasis::Ghost, "dropped"),
        ],
        (Track::Design, "handoff") => &[
            ("Complete", w::Emphasis::Primary, "completed"),
            ("Back to work", w::Emphasis::Secondary, "in_progress"),
        ],
        (Track::Design, "completed") => &[("Reopen", w::Emphasis::Secondary, "in_progress")],
        (_, "blocked") => &[
            ("Resume", w::Emphasis::Primary, "in_progress"),
            ("Close", w::Emphasis::Ghost, "dropped"),
        ],
        (_, "dropped") => &[("Reopen", w::Emphasis::Secondary, "open")],
        _ => &[],
    }
}

/// The one move on each track the server will not let you make on your word
/// alone. Everything else is a state change; this is a claim about the world.
fn needs_evidence(track: Track, next: &str) -> bool {
    matches!((track, next), (Track::Eng, "completed") | (Track::Design, "handoff"))
}

/// Shipping is the exception to "only the assignee moves it": whoever put the
/// build out knows it went out, and that is rarely the person who wrote it.
fn anyone_may(next: &str) -> bool {
    next == "shipped"
}

/// The label a kind wears in the picker and on its chip.
fn kind_label(kind: &str) -> &'static str {
    match kind {
        "pr" => "PR",
        "commit" => "Commit",
        "figma" => "Figma link",
        "doc" => "Doc",
        _ => "Link",
    }
}

/// One hue per kind, so a list of five resources is scannable rather than read.
fn kind_tone(kind: &str) -> c::Tone {
    match kind {
        "pr" => c::Tone::Info,
        "commit" => c::Tone::Ok,
        "figma" => c::Tone::Agent,
        _ => c::Tone::Quiet,
    }
}

fn kind_options(kinds: &[&str]) -> Vec<(String, String)> {
    kinds.iter().map(|k| ((*k).to_owned(), kind_label(k).to_owned())).collect()
}

/// What the two buttons in the prompt say. Copy follows the move it is
/// standing in for, so nobody has to translate "attach" into "hand off".
fn prompt_verbs(then: Option<&str>) -> (&'static str, &'static str) {
    match then {
        Some("handoff") => ("Attach and hand off", "Hand off without a link"),
        Some(_) => ("Attach and complete", "Complete without a link"),
        None => ("Attach", ""),
    }
}

#[allow(clippy::too_many_arguments)]
fn actions(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    task: &Value,
    status: &str,
    track: Track,
    can_act: bool,
    held: &[String],
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
                    false,
                ),
                Ok(_) => ("Moved.".to_string(), false),
                Err(e) => (e.to_string(), true),
            });
            local.patching = false;
            invalidate_after_move(net);
        }
    }

    shell::section(ui, "Actions");

    // Everything the viewer is allowed to press. `shipped` escapes the gate:
    // the server lets anyone set it, so hiding it would be this screen lying.
    let offered: Vec<_> = moves(track, status)
        .iter()
        .filter(|(_, _, next)| can_act || anyone_may(next))
        .collect();

    if offered.is_empty() {
        match str_of(task, "assigneeName").filter(|n| !n.is_empty()) {
            Some(name) => w::caption(ui, &format!("Only {name} can move this task.")),
            None => w::caption(ui, "Unassigned \u{2014} nobody can move it yet."),
        }
        return;
    }

    let busy = local.patching || local.attaching;
    let mut go: Option<&'static str> = None;
    ui.horizontal_wrapped(|ui| {
        for (copy, emphasis, next) in offered {
            if w::button(ui, copy, *emphasis, !busy).clicked() {
                go = Some(next);
            }
        }
        if busy {
            ui.add_space(space::SM);
            ui.add(egui::Spinner::new().size(text::BODY));
        }
    });

    if let Some(next) = go {
        local.notice = None;
        // The gate is a missing *fact*, not a missing permission: if the PR is
        // already attached the button is just a button.
        if needs_evidence(track, next) && !track.evidence().iter().any(|k| held.iter().any(|h| h == k))
        {
            local.prompt = Some(Prompt {
                then: Some(next),
                kinds: kind_options(track.evidence()),
                kind: track.evidence().first().map(|k| (*k).to_owned()),
                url: String::new(),
                title: String::new(),
                reason: None,
            });
        } else {
            patch_status(net, task_id, next, None);
            local.patching = true;
        }
    }

    prompt_panel(ui, net, task_id, local);

    if let Some((message, is_error)) = &local.notice {
        ui.add_space(space::SM);
        if *is_error {
            w::error(ui, message);
        } else {
            w::caption(ui, message);
        }
    }
}

fn patch_status(
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    next: &str,
    reason: Option<&str>,
) {
    let mut body = json!({ "status": next });
    if let Some(reason) = reason {
        body["manualReason"] = json!(reason);
    }
    // Drop the previous attempt's reply so the notice belongs to this one.
    net.invalidate(PATCH_KEY);
    net.patch(PATCH_KEY, &format!("/api/user/tasks/{task_id}"), body);
}

/// This task, the board, the dashboard and the personal list all show the
/// status we have just changed.
fn invalidate_after_move(net: &mut crate::desktop::net::Net) {
    net.invalidate_prefix("task:");
    net.invalidate_prefix("board:");
    net.invalidate_prefix("mytasks");
    net.invalidate("home");
}

/// The evidence prompt, and the two-step it runs.
///
/// Attaching is one gesture but two requests, and they are strictly ordered:
/// POST the artifact, and only once *that* comes back Ok, PATCH the status.
/// Firing both at once would race the server's own check and could leave the
/// task moved with nothing attached — the exact state the check exists to
/// prevent. The POST's reply is what releases the PATCH.
fn prompt_panel(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    local: &mut Local,
) {
    // Step two: the artifact landed, so make the move it was standing in for.
    if local.attaching && !net.is_loading(ATTACH_KEY) {
        match net.peek(ATTACH_KEY) {
            Some(Ok(_)) => {
                local.attaching = false;
                let then = local.prompt.as_ref().and_then(|p| p.then);
                local.prompt = None;
                net.invalidate(ARTIFACTS_KEY);
                match then {
                    Some(next) => {
                        patch_status(net, task_id, next, None);
                        local.patching = true;
                    }
                    None => local.notice = Some(("Attached.".to_string(), false)),
                }
            }
            Some(Err(e)) => {
                local.notice = Some((e.to_string(), true));
                local.attaching = false;
            }
            // The reply was invalidated out from under us; nothing is coming.
            None => local.attaching = false,
        }
    }

    let Some(prompt) = local.prompt.as_mut() else { return };
    let (attach_verb, manual_verb) = prompt_verbs(prompt.then);
    let busy = local.attaching || local.patching;

    let mut post: Option<Value> = None;
    let mut manual: Option<String> = None;
    let mut close = false;

    ui.add_space(space::MD);
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());

        if let Some(reason) = prompt.reason.as_mut() {
            w::field(ui, "Why it is done without a link", reason, false, "Pairing, a verbal sign-off, a deploy someone else made");
            ui.add_space(space::LG);
            let ready = !reason.trim().is_empty() && !busy;
            ui.horizontal(|ui| {
                if w::primary(ui, manual_verb, ready).clicked() {
                    manual = Some(reason.trim().to_owned());
                }
                ui.add_space(space::XS);
                if w::ghost(ui, "Cancel").clicked() {
                    close = true;
                }
            });
            return;
        }

        ui.horizontal(|ui| {
            viz::select(ui, "Kind", &prompt.kinds, &mut prompt.kind);
        });
        ui.add_space(space::MD);
        w::field(ui, "URL", &mut prompt.url, false, "https://github.com/org/repo/pull/1");
        ui.add_space(space::MD);
        w::field(ui, "Title", &mut prompt.title, false, "Optional");
        ui.add_space(space::LG);

        let ready = prompt.kind.is_some() && !prompt.url.trim().is_empty() && !busy;
        ui.horizontal(|ui| {
            if w::primary(ui, attach_verb, ready).clicked() {
                post = Some(json!({
                    "parentType": "task",
                    "parentId": task_id,
                    "kind": prompt.kind.clone(),
                    "url": prompt.url.trim(),
                    "title": prompt.title.trim(),
                }));
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                close = true;
            }
            // Only offered where a move is waiting: with no move behind it,
            // "done manually" is not a thing to be.
            if prompt.then.is_some() {
                ui.add_space(space::SM);
                if w::link(ui, "or mark it done manually").clicked() {
                    prompt.reason = Some(String::new());
                }
            }
        });
    });

    if close {
        local.prompt = None;
        return;
    }
    if let Some(body) = post {
        net.invalidate(ATTACH_KEY);
        net.post(ATTACH_KEY, "/api/user/artifacts", body);
        local.attaching = true;
    }
    if let Some(reason) = manual {
        let then = local.prompt.as_ref().and_then(|p| p.then);
        local.prompt = None;
        if let Some(next) = then {
            patch_status(net, task_id, next, Some(&reason));
            local.patching = true;
        }
    }
}

// ------------------------------------------------------------------ evidence

/// Everything hanging off this task: the PR that closed it, the Figma it came
/// from, the doc that explains it. One list, because the question a reader has
/// is "where is the work", not "what kind of link is it".
fn resources(ui: &mut egui::Ui, net: &crate::desktop::net::Net, local: &mut Local) {
    let rows = net.data(ARTIFACTS_KEY).and_then(Value::as_array);
    let mut add = false;
    shell::section_count_with(ui, "Resources", rows.map_or(0, Vec::len), |ui| {
        if w::ghost(ui, "+ Add").clicked() {
            add = true;
        }
    });
    if add {
        local.prompt = Some(Prompt {
            then: None,
            kinds: kind_options(&["pr", "commit", "figma", "doc", "link"]),
            kind: None,
            url: String::new(),
            title: String::new(),
            reason: None,
        });
    }

    if let Some(err) = net.error(ARTIFACTS_KEY) {
        failed(ui, "Could not load resources", err);
        return;
    }
    let Some(rows) = rows else {
        w::loading(ui, "Loading resources");
        return;
    };
    if rows.is_empty() {
        w::empty(ui, "Nothing attached yet.", "PRs, commits, Figma files and docs live here.");
        return;
    }

    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                ui.add_space(space::SM);
            }
            ui.horizontal(|ui| {
                let kind = str_of(row, "kind").unwrap_or("link");
                c::chip(ui, kind_label(kind), kind_tone(kind), false);
                ui.add_space(space::SM);
                let url = str_of(row, "url").unwrap_or_default();
                let title = match str_of(row, "title").map(str::trim) {
                    Some(t) if !t.is_empty() => t,
                    _ => url,
                };
                if w::link(ui, title).on_hover_text(url).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            });
        }
    });
}

// --------------------------------------------------------------------- notes

/// The thread. Anyone may post — a note needs no write scope, because the
/// point of it is that the person who noticed something can say so.
fn notes(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    local: &mut Local,
) {
    if local.posting_note && !net.is_loading(NOTE_KEY) {
        match net.peek(NOTE_KEY) {
            Some(Ok(_)) => {
                local.posting_note = false;
                local.note.clear();
                net.invalidate(NOTES_KEY);
            }
            Some(Err(e)) => {
                local.notice = Some((e.to_string(), true));
                local.posting_note = false;
            }
            None => local.posting_note = false,
        }
    }

    let rows: Vec<Value> =
        net.data(NOTES_KEY).and_then(Value::as_array).cloned().unwrap_or_default();
    shell::section_count(ui, "Notes", rows.len());

    if let Some(err) = net.error(NOTES_KEY) {
        failed(ui, "Could not load notes", err);
    } else if rows.is_empty() {
        w::caption(ui, "No notes yet.");
    } else {
        ui.scope(|ui| {
            ui.set_max_width(PROSE_W.min(ui.available_width()));
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    ui.add_space(space::MD);
                }
                let author = str_of(row, "authorName").unwrap_or("Someone");
                ui.horizontal(|ui| {
                    avatar::small(ui, author, AVATAR);
                    ui.add_space(space::XS);
                    value(ui, author);
                    ui.add_space(space::SM);
                    if let Some(at) = str_of(row, "createdAt") {
                        label(ui, &ago(at));
                    }
                });
                ui.add_space(space::XXS);
                ui.label(
                    RichText::new(str_of(row, "body").unwrap_or_default())
                        .size(text::BODY)
                        .color(colour::TEXT_2),
                );
            }
        });
    }

    ui.add_space(space::MD);
    let mut send = false;
    ui.scope(|ui| {
        ui.set_max_width(PROSE_W.min(ui.available_width()));
        w::field_multiline(ui, "", &mut local.note, 2, "Leave a note for the team");
        ui.add_space(space::SM);
        let ready = !local.note.trim().is_empty() && !local.posting_note;
        if w::primary(ui, if local.posting_note { "Posting\u{2026}" } else { "Post" }, ready)
            .clicked()
        {
            send = true;
        }
    });
    if send {
        net.invalidate(NOTE_KEY);
        net.post(
            NOTE_KEY,
            &format!("/api/user/tasks/{task_id}/notes"),
            json!({ "body": local.note.trim() }),
        );
        local.posting_note = true;
    }
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

    // The heading carries a live pill, which is what `section_with`'s trailing
    // slot is for — it was hand-painted here before that existed.
    shell::section_with(ui, "Run log", |ui| {
        if live {
            w::pill(ui, "live", colour::ACCENT);
        }
    });

    if let Some(err) = &local.log_error {
        failed(ui, "Could not load the run log", err);
    }

    egui::Frame::new()
        .fill(colour::LOG_BG)
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(
            pad::CARD.0 as i8,
            pad::CARD.1 as i8,
        ))
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
                        .size(text::SMALL)
                        .color(colour::LOG_SEQ),
                );
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("task:log:scroll")
                .max_height(size::ROW * LOG_ROWS)
                .stick_to_bottom(true)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = space::XXS;
                    for (seq, line) in &local.lines {
                        ui.horizontal_top(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{seq:>4}"))
                                    .monospace()
                                    .size(text::SMALL)
                                    .color(colour::LOG_SEQ),
                            );
                            ui.add_space(space::SM);
                            ui.add(egui::Label::new(
                                egui::RichText::new(line)
                                    .monospace()
                                    .size(text::SMALL)
                                    .color(colour::LOG_TEXT),
                            ));
                        });
                    }
                });
        });
}

// --------------------------------------------------------------------- shared

fn failed(ui: &mut egui::Ui, what: &str, err: &str) {
    w::error(ui, &format!("{what}: {err}"));
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn me_str(app: &App, key: &str) -> String {
    app.net
        .as_ref()
        .and_then(|n| n.data("__me"))
        .and_then(|m| m.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// "3 days ago". The meta line has room for words where a table column has
/// room for "3d", and this is the one place on the screen that reads as prose.
fn ago(raw: &str) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(raw) else {
        return String::new();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => plural(s / 60, "minute"),
        s if s < 86_400 => plural(s / 3600, "hour"),
        s => plural(s / 86_400, "day"),
    }
}

fn plural(n: i64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

/// "In review" from "in review". The vocabulary still comes from
/// `status_label`; this only decides where the sentence starts.
fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
