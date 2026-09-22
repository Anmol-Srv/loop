//! Task detail: the page a human sits on while the work moves.
//!
//! A task belongs to one person, and that person moves it through its life.
//! The server enforces it — `PATCH /api/user/tasks/{id}` answers 403 to anyone
//! who is neither the assignee nor an admin — so this screen shows the moves
//! only to whoever can actually make them, and tells everyone else whose call
//! it is. Offering a button that is going to 403 is worse than offering none.
//!
//! The page is a reading column and a properties rail. The split is the whole
//! layout argument: status, owner and priority are facts you glance at, and
//! sitting them in the reading column turned four sections into one long
//! stripe of words. The rail takes the facts; the column keeps the prose, the
//! evidence, the thread and the log, each behind a rule.
//!
//! The moves come out of one dropdown rather than a row of buttons, plus the
//! single obvious next step beside the title. A row of six buttons makes you
//! read all six; a list of the legal states makes you read the one you want.
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

/// A commit hash is between an abbreviated one and a full SHA-1. Anything
/// outside that is not a hash, whatever else it might be.
const HASH_MIN: usize = 7;
const HASH_MAX: usize = 40;

// ------------------------------------------------------------ evidence kinds

/// What a piece of evidence *is*, and therefore what the form should ask for.
///
/// One "URL" field for every kind was the bug: picking Commit still asked for
/// a URL and hinted at a pull request. A commit is a hash. The kind decides
/// the label, the hint and what counts as valid, so the form can never ask for
/// the wrong shape again.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Pr,
    Commit,
    Figma,
    Doc,
    Link,
}

/// What one kind's form looks like.
struct Fields {
    /// The label over the first input.
    label: &'static str,
    /// Its placeholder — an example of the thing, not a description of it.
    hint: &'static str,
    /// What the second input asks for. A bare hash is unreadable in the
    /// Resources list, so a commit asks what it did rather than offering an
    /// optional name nobody fills in.
    title_hint: &'static str,
    /// The sentence shown when what was typed cannot be what was asked for.
    expects: &'static str,
}

impl Kind {
    /// The wire value. The API's vocabulary, not the screen's.
    fn api(&self) -> &str {
        match self {
            Kind::Pr => "pr",
            Kind::Commit => "commit",
            Kind::Figma => "figma",
            Kind::Doc => "doc",
            Kind::Link => "link",
        }
    }

    fn from_api(api: &str) -> Option<Kind> {
        Some(match api {
            "pr" => Kind::Pr,
            "commit" => Kind::Commit,
            "figma" => Kind::Figma,
            "doc" => Kind::Doc,
            "link" => Kind::Link,
            _ => return None,
        })
    }

    fn label(&self) -> &'static str {
        kind_label(self.api())
    }

    fn fields(&self) -> Fields {
        match self {
            Kind::Pr => Fields {
                label: "URL",
                hint: "https://github.com/org/repo/pull/123",
                title_hint: "Optional \u{2014} \u{201c}Checkout rewrite\u{201d}\u{2026}",
                expects: EXPECTS_URL,
            },
            Kind::Commit => Fields {
                label: "Commit hash",
                hint: "a1b2c3d",
                title_hint: "What it does",
                expects: "Needs 7 to 40 hex characters.",
            },
            Kind::Figma => Fields {
                label: "Figma link",
                hint: "https://figma.com/file/\u{2026}",
                title_hint: "Optional \u{2014} \u{201c}Checkout rewrite\u{201d}\u{2026}",
                expects: EXPECTS_URL,
            },
            Kind::Doc | Kind::Link => Fields {
                label: "URL",
                hint: "https://\u{2026}",
                title_hint: "Optional \u{2014} \u{201c}Checkout rewrite\u{201d}\u{2026}",
                expects: EXPECTS_URL,
            },
        }
    }

    /// Whether what was typed can be the thing asked for. Deliberately shallow:
    /// this catches a hash pasted into a URL field and a URL pasted into a hash
    /// field, which is the mistake the old single field invited. The server
    /// still has the last word on whether the thing exists.
    fn valid(&self, value: &str) -> bool {
        match self {
            Kind::Commit => {
                (HASH_MIN..=HASH_MAX).contains(&value.len())
                    && value.chars().all(|ch| ch.is_ascii_hexdigit())
            }
            _ => value.starts_with("http://") || value.starts_with("https://"),
        }
    }
}

const EXPECTS_URL: &str = "Needs a URL starting http:// or https://.";

/// The evidence prompt: what the screen asks for when a move needs proof the
/// task has none of yet.
///
/// It is a panel and not a modal because the thing it asks about — "did you
/// open a PR?" — is answered by looking at the page behind it.
struct Prompt {
    /// The status to move to once the evidence lands. `None` when the panel
    /// was opened by "+ Add" and no move is waiting on it.
    then: Option<&'static str>,
    /// The kinds this panel will accept. The first is the default, so it is
    /// the picker's resting label rather than one of its options.
    kinds: Vec<Kind>,
    /// `viz::select`'s slot, holding an api value. `None` means the default.
    kind: Option<String>,
    /// The first field: a URL for most kinds, a hash for a commit.
    value: String,
    title: String,
    /// Set once "or mark it done manually" is taken: the panel swaps the link
    /// form for this one reason. `None` while the link form is showing.
    reason: Option<String>,
}

impl Prompt {
    fn new(then: Option<&'static str>, kinds: Vec<Kind>) -> Self {
        Self { then, kinds, kind: None, value: String::new(), title: String::new(), reason: None }
    }

    fn default_kind(&self) -> Kind {
        self.kinds.first().copied().unwrap_or(Kind::Link)
    }

    fn chosen(&self) -> Kind {
        self.kind.as_deref().and_then(Kind::from_api).unwrap_or_else(|| self.default_kind())
    }
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
    /// A failed note POST. Kept apart from `notice`, which belongs to a move.
    note_error: Option<String>,
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
            note_error: None,
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
            w::empty(ui, "That task is no longer here", "Use Back to return to the board.");
        }
        return;
    };

    let status = str_of(&task, "status").unwrap_or("open").to_string();
    let mine = !me.is_empty() && str_of(&task, "assigneePersonId") == Some(me.as_str());
    let track = Track::of(str_of(&task, "discipline"));
    let can_act = admin || mine;
    // Read out of the cache before the closures borrow `net` mutably: the whole
    // question the gate asks of the list is "is the required kind already here?".
    let held: Vec<String> = net
        .data(ARTIFACTS_KEY)
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(|r| str_of(r, "kind")).map(str::to_owned).collect())
        .unwrap_or_default();

    // Fold in the reply to a move started on an earlier frame. Done here, where
    // `net` is still free, so neither column has to own it.
    settle_move(net, local);

    // The rail cannot hold `net` — the content column has it — so it reports
    // what was asked for and the move is made once both closures are gone.
    let mut from_rail: Option<&'static str> = None;
    let mut open_project: Option<String> = None;
    let busy = local.patching || local.attaching;

    shell::with_rail(
        ui,
        |ui| {
            headline(ui, net, task_id, &task, &status, track, can_act, &held, local);
            description(ui, &task);
            manual_reason(ui, &task);

            shell::divider(ui);
            resources(ui, net, track, local);

            shell::divider(ui);
            notes(ui, net, task_id, local);

            shell::divider(ui);
            run_log(ui, net, task_id, &status, local);
        },
        |ui| {
            from_rail = rail(ui, &task, &status, track, can_act, busy, &mut open_project);
        },
    );

    if let Some(next) = from_rail {
        local.notice = None;
        start_move(net, task_id, next, track, &held, local);
    }

    // `net`'s borrow of `app` ends above, so navigation happens last.
    if let Some(project_id) = open_project {
        app.task = None;
        app.project = Some(project_id);
        app.tab = Tab::Projects;
    }
}

// ---------------------------------------------------------------- the top

/// The title and the one move that is almost always the right one.
///
/// One button, not a row: the alternatives all live in the rail's status
/// dropdown, so the heading can carry the obvious next step alone and the eye
/// has somewhere to land.
#[allow(clippy::too_many_arguments)]
fn headline(
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
    let busy = local.patching || local.attaching;
    let action = primary_move(track, status).filter(|(_, next)| can_act || anyone_may(next));

    let mut go = None;
    ui.horizontal(|ui| {
        // The button is laid out after the title, so the title has to leave it
        // room: unbounded, a long one takes the whole row and pushes it off.
        let reserve = match action {
            Some((copy, _)) => {
                ui.painter()
                    .layout_no_wrap(
                        copy.to_owned(),
                        egui::FontId::proportional(text::BODY),
                        colour::TEXT,
                    )
                    .size()
                    .x
                    + pad::BUTTON.0 * 2.0
                    + space::MD
            }
            None => 0.0,
        };
        ui.add_sized(
            [(ui.available_width() - reserve).max(size::ROW), size::ROW],
            egui::Label::new(
                RichText::new(str_of(task, "title").unwrap_or("Untitled"))
                    .size(text::TITLE)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            )
            .truncate()
            .halign(egui::Align::LEFT),
        );
        if let Some((copy, next)) = action {
            ui.add_space(space::MD);
            if w::primary(ui, copy, !busy).clicked() {
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
        start_move(net, task_id, next, track, held, local);
    }

    prompt_panel(ui, net, task_id, local);

    if let Some((message, is_error)) = &local.notice {
        ui.add_space(space::MD);
        if *is_error {
            w::error(ui, message);
        } else {
            w::caption(ui, message);
        }
    }
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

// ---------------------------------------------------------------- the rail

/// Everything a task *is*, as a column of labelled facts.
///
/// These were a single wrapped line under the title, which put six glanceable
/// values in the middle of the reading column and made them the hardest thing
/// on the page to find. Returns the status the viewer asked to move to.
fn rail(
    ui: &mut egui::Ui,
    task: &Value,
    status: &str,
    track: Track,
    can_act: bool,
    busy: bool,
    open_project: &mut Option<String>,
) -> Option<&'static str> {
    let mut go = None;

    shell::property(ui, "Status", |ui| {
        if !can_act || busy {
            // A viewer with no moves gets the fact, not a control that 403s.
            c::chip(ui, &sentence(status_label(status)), c::status_tone(status), true);
            return;
        }
        // Every legal state on this track in one menu, the current one resting
        // at the top: the whole vocabulary, instead of the two or three moves a
        // button row had room to offer.
        let options: Vec<(String, String)> = track
            .states()
            .iter()
            .filter(|s| **s != status)
            .map(|s| ((*s).to_owned(), sentence(status_label(s))))
            .collect();
        let mut slot: Option<String> = None;
        viz::select(ui, &sentence(status_label(status)), &options, &mut slot);
        if let Some(next) = slot.as_deref() {
            go = track.states().iter().copied().find(|s| *s == next);
        }
    });

    shell::property(ui, "Priority", |ui| match task.get("priority").and_then(Value::as_i64) {
        Some(p) => {
            c::chip(ui, &format!("P{p}"), priority_tone(p), false);
        }
        None => faint(ui, "\u{2014}"),
    });

    shell::property(ui, "Assignee", |ui| match str_of(task, "assigneeName") {
        Some(name) if !name.is_empty() => {
            let seed = str_of(task, "assigneeEmail").unwrap_or(name);
            avatar::small(ui, seed, AVATAR);
            value(ui, name);
        }
        _ => faint(ui, "Unassigned"),
    });

    shell::property(ui, "Department", |ui| {
        match str_of(task, "discipline").filter(|d| !d.is_empty()) {
            Some(d) => w::discipline(ui, d),
            None => faint(ui, "\u{2014}"),
        }
    });

    shell::property(ui, "Project", |ui| {
        match str_of(task, "projectName").filter(|p| !p.is_empty()) {
            Some(name) => {
                if w::link(ui, name).clicked() {
                    *open_project = str_of(task, "projectId").map(str::to_owned);
                }
            }
            None => faint(ui, "\u{2014}"),
        }
    });

    if let Some(created) = str_of(task, "createdAt") {
        shell::property(ui, "Created", |ui| {
            value(ui, &ago(created)).on_hover_text(exact(created));
        });
    }
    // No row at all rather than an em dash: an unfinished task has no done
    // date, and a blank line for it is a fact about nothing.
    if let Some(done) = str_of(task, "doneAt") {
        shell::property(ui, "Done", |ui| {
            value(ui, &ago(done)).on_hover_text(exact(done));
        });
    }

    if !can_act {
        ui.add_space(space::SM);
        match str_of(task, "assigneeName").filter(|n| !n.is_empty()) {
            Some(name) => w::caption(ui, &format!("Only {name} can move this task.")),
            None => w::caption(ui, "Unassigned \u{2014} assign it and the moves open up."),
        }
    }

    go
}

/// P0 shouts and P4 whispers, in the same chip vocabulary as status — the rail
/// should read as one column of tokens, not two competing systems.
fn priority_tone(p: i64) -> c::Tone {
    match p {
        0 => c::Tone::Blocked,
        1 => c::Tone::Running,
        2 => c::Tone::Neutral,
        _ => c::Tone::Quiet,
    }
}

fn value(ui: &mut egui::Ui, s: &str) -> egui::Response {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT))
}

/// A value that is not there. Fainter than one that is, so an empty rail row
/// reads as absence rather than as content.
fn faint(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT_FAINT));
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

    /// Every state this track's tasks can be in, in life order. This is what
    /// the status dropdown offers — the two tracks differ by exactly one state
    /// each, and showing a designer "Shipped" would be offering a move their
    /// flow does not have.
    fn states(self) -> &'static [&'static str] {
        match self {
            Track::Eng => &["open", "in_progress", "blocked", "completed", "shipped", "dropped"],
            Track::Design => &["open", "in_progress", "blocked", "handoff", "completed", "dropped"],
        }
    }

    /// The artifact kinds that count as evidence on this track, in the order
    /// the picker should offer them.
    fn evidence(self) -> &'static [Kind] {
        match self {
            Track::Eng => &[Kind::Pr, Kind::Commit],
            Track::Design => &[Kind::Figma],
        }
    }
}

/// The single next step from here, or nothing when the state is terminal.
///
/// Anything else a task could do is in the rail's dropdown; this is only the
/// move you almost always came to the page to make. Design's `completed` is
/// the end of design's road, so it gets no button — the dropdown is where a
/// reopen lives.
fn primary_move(track: Track, status: &str) -> Option<(&'static str, &'static str)> {
    Some(match (track, status) {
        (_, "open") => ("Start", "in_progress"),
        (Track::Eng, "in_progress") => ("Complete", "completed"),
        (Track::Design, "in_progress") => ("Hand off", "handoff"),
        (Track::Eng, "completed") => ("Ship", "shipped"),
        (_, "handoff") => ("Complete", "completed"),
        (_, "blocked") => ("Resume", "in_progress"),
        _ => return None,
    })
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

/// What the two buttons in the prompt say. Copy follows the move it is
/// standing in for, so nobody has to translate "attach" into "hand off".
fn prompt_verbs(then: Option<&str>) -> (&'static str, &'static str) {
    match then {
        Some("handoff") => ("Attach and hand off", "Hand off without a link"),
        Some(_) => ("Attach and complete", "Complete without a link"),
        None => ("Attach", ""),
    }
}

/// Fold in the reply to a move we started earlier.
fn settle_move(net: &mut crate::desktop::net::Net, local: &mut Local) {
    if !local.patching || net.is_loading(PATCH_KEY) {
        return;
    }
    let Some(result) = net.peek(PATCH_KEY) else { return };
    local.notice = Some(match result {
        Ok(v) if str_of(v, "status") == Some("proposed") => (
            "Awaiting approval: you do not hold write on this project, so the \
             move was recorded as a proposed change."
                .to_string(),
            false,
        ),
        Ok(v) => (
            match str_of(v, "status") {
                Some(next) => format!("Moved to {}.", sentence(status_label(next))),
                None => "Moved.".to_string(),
            },
            false,
        ),
        Err(e) => (e.to_string(), true),
    });
    local.patching = false;
    invalidate_after_move(net);
}

/// Ask for a state change, or ask for the evidence it depends on first.
///
/// The gate is a missing *fact*, not a missing permission: if the PR is already
/// attached, the move is just a move.
fn start_move(
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    next: &'static str,
    track: Track,
    held: &[String],
    local: &mut Local,
) {
    if needs_evidence(track, next)
        && !track.evidence().iter().any(|k| held.iter().any(|h| h == k.api()))
    {
        local.prompt = Some(Prompt::new(Some(next), track.evidence().to_vec()));
    } else {
        patch_status(net, task_id, next, None);
        local.patching = true;
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
            w::field(ui, "Why it is done without a link", reason, false, "Pairing, a verbal sign-off, a deploy someone else made\u{2026}");
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

        // The first kind is the picker's resting label rather than one of its
        // rows, so the form always has a kind and never a "pick one" state.
        let default = prompt.default_kind();
        let options: Vec<(String, String)> = prompt.kinds[1.min(prompt.kinds.len())..]
            .iter()
            .map(|k| (k.api().to_owned(), k.label().to_owned()))
            .collect();
        w::caption(ui, "Kind");
        ui.add_space(space::XXS);
        viz::select(ui, default.label(), &options, &mut prompt.kind);

        let kind = prompt.chosen();
        let fields = kind.fields();
        ui.add_space(space::MD);
        let typed = prompt.value.trim().to_owned();
        let bad = !typed.is_empty() && !kind.valid(&typed);
        let entry = w::field(ui, fields.label, &mut prompt.value, false, fields.hint);
        if bad {
            // The border carries it at a glance; the caption is for the person
            // who cannot tell this red from the line around every other field.
            ui.painter().rect_stroke(
                entry.rect,
                radius::SM as f32,
                egui::Stroke::new(1.0, colour::DANGER),
                egui::StrokeKind::Inside,
            );
            ui.add_space(space::XXS);
            w::caption(ui, fields.expects);
        }
        ui.add_space(space::MD);
        w::field(ui, "Title", &mut prompt.title, false, fields.title_hint);
        ui.add_space(space::LG);

        let ready = !typed.is_empty() && !bad && !busy;
        ui.horizontal(|ui| {
            if w::primary(ui, attach_verb, ready).clicked() {
                post = Some(json!({
                    "parentType": "task",
                    "parentId": task_id,
                    "kind": kind.api(),
                    "url": typed,
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
fn resources(
    ui: &mut egui::Ui,
    net: &crate::desktop::net::Net,
    track: Track,
    local: &mut Local,
) {
    let rows = net.data(ARTIFACTS_KEY).and_then(Value::as_array);
    let mut add = false;
    shell::section_count_with(ui, "Resources", rows.map_or(0, Vec::len), |ui| {
        if w::ghost(ui, "+ Add").clicked() {
            add = true;
        }
    });
    if add {
        // The track's own evidence leads, because that is what is usually being
        // attached; the rest follow.
        let mut kinds = track.evidence().to_vec();
        for k in [Kind::Pr, Kind::Commit, Kind::Figma, Kind::Doc, Kind::Link] {
            if !kinds.contains(&k) {
                kinds.push(k);
            }
        }
        local.prompt = Some(Prompt::new(None, kinds));
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
                let title = str_of(row, "title").map(str::trim).filter(|t| !t.is_empty());

                // A commit has no URL to open — it is a hash. Drawing it as a
                // link would promise a destination that does not exist, so it
                // is set in mono and the title carries the meaning.
                if kind == "commit" {
                    w::mono_caption(ui, url);
                    if let Some(title) = title {
                        ui.add_space(space::SM);
                        let fitted = elide(ui, title, ui.available_width() - space::SM);
                        ui.label(
                            RichText::new(fitted).size(text::SMALL).color(colour::TEXT_2),
                        );
                    }
                    return;
                }

                // `w::link` lays its label out no-wrap, so a long title — or the
                // raw URL standing in for a missing one — would run past the card.
                let fitted = elide(ui, title.unwrap_or(url), ui.available_width() - space::SM);
                if w::link(ui, &fitted).on_hover_text(url).clicked() {
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
                local.note_error = Some(e.to_string());
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
                        ui.label(
                            RichText::new(ago(at)).size(text::SMALL).color(colour::TEXT_MUTED),
                        );
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
        w::field_multiline(ui, "", &mut local.note, 2, "Leave a note for the team\u{2026}");
        if let Some(err) = &local.note_error {
            ui.add_space(space::XS);
            w::error(ui, err);
        }
        ui.add_space(space::SM);
        let ready = !local.note.trim().is_empty() && !local.posting_note;
        if w::primary(ui, if local.posting_note { "Posting\u{2026}" } else { "Post" }, ready)
            .clicked()
        {
            send = true;
        }
    });
    if send {
        local.note_error = None;
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
                    "Waiting for the agent’s first line"
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

/// `s` cut to `width`, with an ellipsis where it was cut. The cut point is
/// estimated from the full string's measure rather than fitted glyph by glyph;
/// the whole value is on the hover text either way.
fn elide(ui: &egui::Ui, s: &str, width: f32) -> String {
    let font = egui::FontId::proportional(text::SMALL);
    let full = ui.painter().layout_no_wrap(s.to_owned(), font, colour::TEXT).size().x;
    if full <= width || full <= 0.0 {
        return s.to_owned();
    }
    let keep = (s.chars().count() as f32 * (width / full)) as usize;
    s.chars().take(keep.saturating_sub(1)).collect::<String>() + "\u{2026}"
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

/// "3 days ago". The rail has room for words where a table column has room for
/// "3d", and a relative date is the one you can read without arithmetic.
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

/// The date itself, for the hover. "9 days ago" is the right answer to "how
/// long", and the wrong one to "which Tuesday".
fn exact(raw: &str) -> String {
    match DateTime::parse_from_rfc3339(raw) {
        Ok(t) => t.with_timezone(&chrono::Local).format("%-d %b %Y, %H:%M").to_string(),
        Err(_) => String::new(),
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
