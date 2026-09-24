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

use chrono::{DateTime, Utc};
use egui::RichText;
use serde_json::{json, Value};

use super::agent_session::{self as session, Session};
use super::agents::{state_tone, state_words, AGENTS_KEY};
use super::menus::{task_items, Pick, Viewer};
use super::projects::{person_option, PEOPLE_KEY, PROSE_W};
use crate::desktop::design::agent::{self as face, Presence};
use crate::desktop::design::{
    avatar, cards as c, colour, radius, shell, size, space, status_label, text, theme, viz, widgets as w,
};
use crate::desktop::net::memo;
use crate::desktop::{App, Tab};

const TASK_KEY: &str = "task:one";
const PATCH_KEY: &str = "task:patch";
const ARTIFACTS_KEY: &str = "task:artifacts";
const ATTACH_KEY: &str = "task:artifact:new";
const NOTES_KEY: &str = "task:notes";
const NOTE_KEY: &str = "task:note:new";
const DETAILS_KEY: &str = "task:details";
const REMOVE_KEY: &str = "task:artifact:remove";
/// Hand-off, take-back, answer and review all go out under this one key: they
/// are started from one page, one at a time.
const AGENT_KEY: &str = "task:agent";
/// The server's table of legal moves per track. Under `__`, not `task:`: it
/// does not change while the app runs, so nothing here invalidates it.
const TRACKS_KEY: &str = "__tracks";

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
    /// The agent session's drafts and its step log.
    session: session::State,
    patching: bool,
    /// The status the page showed when the move was asked for. Sent as
    /// `expectedStatus`, so a move made from a stale page is refused rather
    /// than undoing a teammate's.
    move_from: String,
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
    /// A details PATCH is out; `saving_text` when it carries the title and
    /// description draft, which is only thrown away once the save lands.
    saving: bool,
    saving_text: bool,
    /// The resource whose "Remove this link?" row is showing.
    confirm_remove: Option<String>,
    removing: bool,
    /// A failed remove, shown over the list it failed in rather than up by
    /// the title where the move notices live.
    resource_error: Option<String>,
    /// The agent action in flight — its past tense, for the notice.
    agent_busy: Option<String>,
    /// A pick from the title's menu, acted on once the page is drawn.
    pick: Option<Pick>,
    /// A task action from a menu is out.
    archiving: bool,
}

impl Local {
    fn new(task_id: String) -> Self {
        Self {
            task_id,
            session: session::State::default(),
            patching: false,
            move_from: String::new(),
            notice: None,
            prompt: None,
            attaching: false,
            note: String::new(),
            posting_note: false,
            note_error: None,
            saving: false,
            saving_text: false,
            confirm_remove: None,
            removing: false,
            resource_error: None,
            agent_busy: None,
            pick: None,
            archiving: false,
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
    let can_write = app.can_write();

    local.archiving = app.board.tasks.busy();
    let net = app.net.as_mut().expect("net is live whenever a view runs");
    net.get_once(TASK_KEY, &format!("/api/user/tasks/{task_id}"));
    net.get_once(
        ARTIFACTS_KEY,
        &format!("/api/user/artifacts?parentType=task&parentId={task_id}"),
    );
    net.get_once(NOTES_KEY, &format!("/api/user/tasks/{task_id}/notes"));
    net.get_once(TRACKS_KEY, "/api/user/tracks");
    // Asked for on the first frame with the rest, not once `__me` has landed
    // and said whether you may write: waiting on it made this a second wave.
    net.get_once(PEOPLE_KEY, "/api/user/people");

    let task = net.shared(TASK_KEY);
    // Only the assignee hands off, so only the assignee needs their agents.
    let mine_early = task.as_ref().is_some_and(|t| !me.is_empty() && str_of(t, "assigneePersonId") == Some(me.as_str()));
    if mine_early {
        net.get_once(AGENTS_KEY, "/api/user/agents");
    }

    // A breadcrumb, not an id: "22222222" told nobody anything, the project
    // name tells you where you are and is the likeliest place to go next.
    let mut leave = false;
    let mut open_project: Option<String> = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        if shell::back(ui, "Back").clicked() {
            leave = true;
        }
        let project = task.as_ref().and_then(|t| {
            Some((str_of(t, "projectName").filter(|n| !n.is_empty())?, str_of(t, "projectId")?))
        });
        if let Some((name, id)) = project {
            faint(ui, "\u{00B7}");
            if w::link(ui, name).clicked() {
                open_project = Some(id.to_owned());
            }
        }
    });
    if leave {
        app.task = None;
        return;
    }
    if let Some(project_id) = open_project {
        app.task = None;
        app.project = Some(project_id);
        app.tab = Tab::Projects;
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
    let moves = legal_moves(net.data(TRACKS_KEY), track, &status);
    let updated_at = str_of(&task, "updatedAt").map(str::to_owned);
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
    settle_details(ui.ctx(), net, task_id, local);
    settle_agent(net, local);

    let delegate = task.get("delegate").filter(|d| d.is_object());
    let agents = memo(ui.ctx(), egui::Id::new("task:my-agents"), net.generation(AGENTS_KEY), || {
        net.data(AGENTS_KEY)
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter(|a| str_of(a, "status") != Some("revoked"))
                    .filter_map(|a| Some((str_of(a, "id")?.to_owned(), str_of(a, "name")?.to_owned())))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let handoff = Handoff { mine, delegated: super::menus::held(&task), finished: super::menus::finished(&task), agents: &agents };
    let viewer = Viewer { me: me.clone(), can_write, agents: agents.to_vec() };

    // The rail cannot hold `net` — the content column has it — so it reports
    // what was asked for and the request is made once both closures are gone.
    let mut from_rail: Option<Ask> = None;
    let busy = local.patching || local.attaching || local.saving || local.agent_busy.is_some();
    let people = net.shared(PEOPLE_KEY).filter(|_| can_write);
    let people: &[Value] = people.as_deref().and_then(Value::as_array).map_or(&[], Vec::as_slice);
    // Who may read the owner's conversation with the agent. The server says;
    // without its word, the owner and admins are exactly who it names.
    let private = task.get("canSeeAgentPrivate").and_then(Value::as_bool).unwrap_or(mine || admin);

    let mut editing = false;
    shell::with_rail(
        ui,
        |ui, part| match part {
            shell::Part::Header => {
                editing = headline(
                    ui, net, task_id, &task, &status, track, &moves, can_act, can_write, &held, &handoff,
                    &viewer, local,
                );
            }
            shell::Part::Body => {
                if !editing {
                    description(ui, &task);
                }
                manual_reason(ui, &task);

                if let Some(d) = delegate {
                    let notes = net.shared(NOTES_KEY);
                    let evidence = net.shared(ARTIFACTS_KEY);
                    let s = Session {
                        task_id,
                        task: &task,
                        delegate: d,
                        notes: notes.as_deref().and_then(Value::as_array).map_or(&[], Vec::as_slice),
                        notes_loaded: notes.is_some() || net.error(NOTES_KEY).is_some(),
                        evidence: evidence.as_deref().and_then(Value::as_array).map_or(&[], Vec::as_slice),
                        mine,
                        private,
                        busy: local.agent_busy.is_some(),
                    };
                    if let Some(ask) = session::show(ui, net, &s, &mut local.session) {
                        agent_action(net, task_id, ask.into(), local);
                    }
                }

                shell::divider(ui);
                resources(ui, net, track, can_write, local);

                shell::divider(ui);
                notes(ui, net, task_id, delegate.is_some(), local);
            }
        },
        |ui| {
            let ctx = Rail {
                task_id,
                status: &status,
                track,
                moves: &moves,
                can_act,
                can_write,
                busy,
                people,
            };
            from_rail = rail(ui, &task, &ctx, &mut open_project);
        },
    );

    match from_rail {
        Some(Ask::Move(next)) => {
            local.notice = None;
            start_move(net, task_id, &status, next, track, &held, local);
        }
        Some(Ask::Details(mut body)) => {
            // The rail edits one field the viewer can see, so the task as
            // shown is the version the edit is made against.
            if let Some(at) = &updated_at {
                body["expectedUpdatedAt"] = json!(at);
            }
            save_details(net, task_id, body, false, local);
        }
        None => {}
    }
    // The page's own hand-off and details edits say how they went where
    // they always have; archive and delete share every other menu's dialog.
    match local.pick.take() {
        Some(Pick::Handoff(id, name)) => agent_action(net, task_id, AgentAsk::Handoff(id, name), local),
        Some(Pick::TakeBack) => agent_action(net, task_id, AgentAsk::TakeBack, local),
        Some(Pick::Priority(p)) => {
            let mut body = json!({ "priority": p });
            if let Some(at) = &updated_at {
                body["expectedUpdatedAt"] = json!(at);
            }
            save_details(net, task_id, body, false, local);
        }
        Some(pick) => app.board.tasks.pick(net, &task, pick),
        None => {}
    }

    // `net`'s borrow of `app` ends above, so navigation happens last.
    if let Some(project_id) = open_project {
        app.task = None;
        app.project = Some(project_id);
        app.tab = Tab::Projects;
    }
}

// ---------------------------------------------------------------- the top

/// The title and description while Edit is open, with what they were when
/// editing began, so a save sends only what the person changed.
#[derive(Clone)]
struct Draft {
    title: String,
    body: String,
    was_title: String,
    was_body: String,
    /// The task's `updatedAt` when editing began. `None` after a refused save:
    /// the next one goes against the task as refetched.
    updated_at: Option<String>,
}

/// The title and the one move that is almost always the right one, or — while
/// editing — the title and description as fields. Returns whether it is
/// editing, so the reading copy of the description is not drawn twice.
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
    moves: &[&'static str],
    can_act: bool,
    can_write: bool,
    held: &[String],
    handoff: &Handoff,
    viewer: &Viewer,
    local: &mut Local,
) -> bool {
    let busy = local.patching
        || local.attaching
        || local.saving
        || local.agent_busy.is_some()
        || local.archiving;
    let action = primary_move(track, status)
        .filter(|(_, next)| moves.contains(next) && (can_act || anyone_may(next)));

    // The draft lives in egui's temp store under the task id, not in a local:
    // leave for the project and come back, and the half-written words are
    // still there.
    let draft_id = egui::Id::new(("task:draft", task_id));
    let mut draft: Option<Draft> = ui.data(|d| d.get_temp(draft_id));

    let mut go = None;
    let mut close = false;
    if let Some(Draft { title, body, .. }) = draft.as_mut() {
        w::field(ui, "Title", title, false, "What needs doing");
        ui.add_space(space::MD);
        w::field_multiline(
            ui,
            "Description",
            body,
            6,
            "Context, constraints, what done looks like\u{2026}",
        );
        ui.add_space(space::MD);
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            let ready = !title.trim().is_empty() && !busy;
            let label = if local.saving_text { "Saving\u{2026}" } else { "Save" };
            let response = w::primary(ui, label, ready);
            if title.trim().is_empty() {
                response.clone().on_disabled_hover_text("A task needs a title.");
            }
            save = response.clicked();
            ui.add_space(space::XS);
            cancel = w::ghost(ui, "Cancel").clicked();
        });
        if save {
            let d = draft.as_ref().expect("editing");
            // Only what was changed, against the version editing began from:
            // a title fix must not write back a description someone else has
            // since rewritten.
            let mut body = json!({});
            if d.title.trim() != d.was_title {
                body["title"] = json!(d.title.trim());
            }
            if d.body.trim() != d.was_body {
                body["body"] = json!(d.body.trim());
            }
            if body.as_object().is_some_and(|b| b.is_empty()) {
                close = true;
            } else {
                // After a refused save the draft is re-sent against the task as
                // it now stands: the person has read the message and chosen.
                if let Some(at) = d.updated_at.clone().or_else(|| str_of(task, "updatedAt").map(str::to_owned)) {
                    body["expectedUpdatedAt"] = json!(at);
                }
                save_details(net, task_id, body, true, local);
            }
        }
        if cancel || close {
            draft = None;
        }
    } else {
        let mut edit = false;
        // Right to left, so the controls take their width first and the title
        // truncates into what is left — and sits flush left, which a sized
        // label centred in its box did not.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                let mut pick = None;
                viz::more(ui, |ui| pick = task_items(ui, task, viewer, false));
                if let Some((copy, next)) = action {
                    if w::primary(ui, copy, !busy).clicked() {
                        go = Some(next);
                    }
                }
                if let Some(ask) = handoff_control(ui, handoff, busy) {
                    agent_action(net, task_id, ask, local);
                }
                if can_write && w::ghost(ui, "Edit").clicked() {
                    edit = true;
                }
                if busy {
                    ui.add(egui::Spinner::new().size(text::BODY));
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let title = ui.add(
                        egui::Label::new(
                            RichText::new(str_of(task, "title").unwrap_or("Untitled"))
                                .size(text::TITLE)
                                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                                .color(colour::TEXT),
                        )
                        .truncate()
                        .sense(egui::Sense::click()),
                    );
                    viz::context_menu(&title, |ui| pick = task_items(ui, task, viewer, false));
                });
                local.pick = pick;
            });
        });
        if edit {
            let title = str_of(task, "title").unwrap_or_default().to_owned();
            let body = str_of(task, "body").unwrap_or_default().to_owned();
            draft = Some(Draft {
                was_title: title.trim().to_owned(),
                was_body: body.trim().to_owned(),
                title,
                body,
                updated_at: str_of(task, "updatedAt").map(str::to_owned),
            });
        }
    }
    let editing = draft.is_some();
    ui.data_mut(|d| match draft {
        Some(draft) => {
            d.insert_temp(draft_id, draft);
        }
        None => d.remove::<Draft>(draft_id),
    });

    if let Some(next) = go {
        local.notice = None;
        start_move(net, task_id, status, next, track, held, local);
    }

    prompt_panel(ui, net, task_id, local);

    if super::board::archived(task) {
        ui.add_space(space::MD);
        w::caption(
            ui,
            if task.get("projectArchivedAt").is_some_and(|v| !v.is_null()) {
                "Archived with its project \u{2014} restoring the project brings it back."
            } else {
                "Archived \u{2014} out of every list until it is restored."
            },
        );
    }

    if let Some((message, is_error)) = &local.notice {
        ui.add_space(space::MD);
        if *is_error {
            w::error(ui, message);
        } else {
            w::caption(ui, message);
        }
    }
    editing
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

// ------------------------------------------------------------ the delegate

/// What the page knows about handing this task to one of the viewer's agents.
struct Handoff<'a> {
    /// The viewer is the assignee: the only person who may hand off or take back.
    mine: bool,
    delegated: bool,
    finished: bool,
    /// The viewer's agents that can still take work: (id, name).
    agents: &'a [(String, String)],
}

/// An agent action to send.
enum AgentAsk {
    Handoff(String, String),
    TakeBack,
    Answer(String),
    Approve,
    Changes(String),
    Instruct(String),
}

impl From<session::Ask> for AgentAsk {
    fn from(a: session::Ask) -> Self {
        match a {
            session::Ask::Answer(b) => AgentAsk::Answer(b),
            session::Ask::Approve => AgentAsk::Approve,
            session::Ask::Changes(b) => AgentAsk::Changes(b),
            session::Ask::Instruct(b) => AgentAsk::Instruct(b),
        }
    }
}

/// The hand-off control in the title's actions: "Take back" while an agent
/// holds the task, otherwise "Hand off to …" — a button for one agent, a
/// picker for several. Nothing at all for anyone but the assignee, or for an
/// assignee with no agent to hand to.
fn handoff_control(ui: &mut egui::Ui, h: &Handoff, busy: bool) -> Option<AgentAsk> {
    if !h.mine {
        return None;
    }
    if h.delegated {
        return w::secondary(ui, "Take back", !busy).clicked().then_some(AgentAsk::TakeBack);
    }
    match h.agents {
        [] => None,
        _ if h.finished => {
            let label = match h.agents {
                [(_, name)] => format!("Hand off to {name}"),
                _ => "Hand off".to_owned(),
            };
            w::secondary(ui, &label, false)
                .on_disabled_hover_text("This task is finished \u{2014} there is nothing left to hand off.");
            None
        }
        [(id, name)] => w::secondary(ui, &format!("Hand off to {name}"), !busy)
            .clicked()
            .then(|| AgentAsk::Handoff(id.clone(), name.clone())),
        many => {
            let options: Vec<(String, String)> = many.to_vec();
            let mut slot: Option<String> = None;
            ui.add_enabled_ui(!busy, |ui| {
                viz::select(ui, "Hand off to\u{2026}", &options, &mut slot);
            });
            let id = slot?;
            let name = many.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()).unwrap_or_default();
            Some(AgentAsk::Handoff(id, name))
        }
    }
}

fn agent_action(net: &mut crate::desktop::net::Net, task_id: &str, ask: AgentAsk, local: &mut Local) {
    let base = format!("/api/user/tasks/{task_id}");
    let (path, body, done) = match ask {
        AgentAsk::Handoff(id, name) => (format!("{base}/handoff"), json!({ "agentId": id }), format!("Handed off to {name}.")),
        AgentAsk::TakeBack => (format!("{base}/takeback"), json!({}), "Taken back \u{2014} the agent no longer has this task.".to_owned()),
        AgentAsk::Answer(body) => (format!("{base}/answer"), json!({ "body": body }), "Answer sent.".to_owned()),
        AgentAsk::Approve => (format!("{base}/review"), json!({ "decision": "approve" }), "Approved.".to_owned()),
        AgentAsk::Changes(body) => (
            format!("{base}/review"),
            json!({ "decision": "changes", "body": body }),
            "Changes requested.".to_owned(),
        ),
        AgentAsk::Instruct(body) => (
            format!("{base}/instruct"),
            json!({ "body": body }),
            "Sent \u{2014} the agent hears it at its next step.".to_owned(),
        ),
    };
    local.notice = None;
    net.invalidate(AGENT_KEY);
    net.post(AGENT_KEY, &path, body);
    local.agent_busy = Some(done);
}

/// Fold in the reply to an agent action.
fn settle_agent(net: &mut crate::desktop::net::Net, local: &mut Local) {
    if local.agent_busy.is_none() || net.is_loading(AGENT_KEY) {
        return;
    }
    let done = local.agent_busy.take().unwrap_or_default();
    match net.peek(AGENT_KEY) {
        Some(Ok(_)) => {
            local.notice = Some((done, false));
            local.session.sent();
        }
        Some(Err(e)) => local.notice = Some((e.to_string(), true)),
        None => {}
    }
    invalidate_after_move(net);
    net.invalidate(AGENTS_KEY);
}

// ---------------------------------------------------------------- the rail

/// What the rail asked for. It cannot hold `net`, so it says what it wants and
/// `render` sends it once the closures are gone.
enum Ask {
    Move(&'static str),
    Details(Value),
}

/// What the rail needs to know about the viewer and the page.
struct Rail<'a> {
    task_id: &'a str,
    status: &'a str,
    track: Track,
    moves: &'a [&'static str],
    can_act: bool,
    can_write: bool,
    busy: bool,
    people: &'a [Value],
}

/// Everything a task *is*, as a column of labelled facts — and, for anyone
/// with write, the three of them that can be changed.
///
/// These were a single wrapped line under the title, which put six glanceable
/// values in the middle of the reading column and made them the hardest thing
/// on the page to find.
fn rail(
    ui: &mut egui::Ui,
    task: &Value,
    r: &Rail,
    open_project: &mut Option<String>,
) -> Option<Ask> {
    let mut ask = None;
    let status = r.status;

    shell::property(ui, "Status", |ui| {
        if !r.can_act || r.busy || r.moves.is_empty() {
            // A viewer with no moves gets the fact, not a control that 403s.
            c::chip(ui, status_label(status), c::status_tone(status), true);
            return;
        }
        // Every move the server allows from here in one menu, the current
        // state resting at the top — and nothing it would refuse.
        let options: Vec<(String, String)> =
            r.moves.iter().map(|s| ((*s).to_owned(), status_label(s).to_owned())).collect();
        let mut slot: Option<String> = None;
        viz::value_select(ui, status_label(status), &options, &mut slot);
        if let Some(next) = slot.as_deref() {
            ask = r.moves.iter().copied().find(|s| *s == next).map(Ask::Move);
        }
    });

    let priority = task.get("priority").and_then(Value::as_i64);
    shell::property(ui, "Priority", |ui| {
        if !r.can_write || r.busy {
            match priority {
                Some(p) => {
                    c::chip(ui, &format!("P{p}"), priority_tone(p), false);
                }
                None => faint(ui, "\u{2014}"),
            }
            return;
        }
        let options: Vec<(String, String)> = (0..=4)
            .filter(|p| Some(*p) != priority)
            .map(|p| (p.to_string(), format!("P{p}")))
            .collect();
        let mut slot: Option<String> = None;
        let current = priority.map_or_else(|| "No priority".to_owned(), |p| format!("P{p}"));
        viz::value_select(ui, &current, &options, &mut slot);
        if let Some(p) = slot.and_then(|s| s.parse::<i64>().ok()) {
            ask = Some(Ask::Details(json!({ "priority": p })));
        }
    });

    // A reassignment that would drag the task across tracks waits here for a
    // yes. Kept in the temp store so the rail, which holds nothing, can
    // remember it between frames: (who, the question to ask).
    let confirm_id = egui::Id::new(("task:reassign", r.task_id));
    let current = str_of(task, "assigneePersonId");
    shell::property(ui, "Assignee", |ui| {
        let name = str_of(task, "assigneeName").filter(|n| !n.is_empty());
        let editable = r.can_write && !r.busy;
        // The face only when the name is plain text. Beside a picker it
        // pushed the control out of the column every other rail control
        // starts and ends on, and the picker names the person anyway.
        if let Some(name) = name.filter(|_| !editable) {
            avatar::small(ui, str_of(task, "assigneeEmail").unwrap_or(name), AVATAR);
        }
        if !editable {
            match name {
                Some(name) => {
                    value(ui, name);
                }
                None => faint(ui, "Unassigned"),
            }
            return;
        }
        let mut options: Vec<(String, String)> = Vec::new();
        if current.is_some() {
            options.push((UNASSIGN.to_owned(), "Unassigned".to_owned()));
        }
        options.extend(
            r.people.iter().map(person_option).filter(|(id, _)| Some(id.as_str()) != current),
        );
        let mut slot: Option<String> = None;
        viz::value_select(ui, name.unwrap_or("Unassigned"), &options, &mut slot);
        let Some(picked) = slot else { return };

        let person = r.people.iter().find(|p| str_of(p, "id") == Some(picked.as_str()));
        let id = person.map(|_| picked.clone());
        let to = Track::of(person.and_then(|p| str_of(p, "department")));
        if to != r.track && !to.states().contains(&status) {
            let question = match person.and_then(|p| str_of(p, "name")) {
                Some(who) => format!(
                    "Reassign to {who}? This moves the task to the {} track and back to Open.",
                    to.word()
                ),
                None => "Unassign this task? It goes back to Open.".to_owned(),
            };
            ui.data_mut(|d| d.insert_temp(confirm_id, (id, question)));
        } else {
            ask = Some(Ask::Details(json!({ "assigneeId": id })));
        }
    });

    if let Some((id, question)) = ui.data(|d| d.get_temp::<(Option<String>, String)>(confirm_id)) {
        let mut done = false;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = space::XS;
            ui.label(RichText::new(question).size(text::SMALL).color(colour::TEXT_2));
            ui.horizontal(|ui| {
                if w::secondary(ui, "Reassign", !r.busy).clicked() {
                    ask = Some(Ask::Details(json!({ "assigneeId": id })));
                    done = true;
                }
                if w::ghost(ui, "Keep").clicked() {
                    done = true;
                }
            });
        });
        ui.add_space(space::SM);
        if done {
            ui.data_mut(|d| d.remove::<(Option<String>, String)>(confirm_id));
        }
    }

    if let Some(d) = task.get("delegate").filter(|d| d.is_object()) {
        let state = str_of(d, "state").unwrap_or("handed_off");
        shell::property(ui, "Delegate", |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            let name = str_of(d, "name").unwrap_or("Agent");
            let seed = str_of(task, "assigneeEmail").or_else(|| str_of(task, "assigneeName")).unwrap_or(name);
            // Still: the session's header carries the one moving ring.
            face::avatar_still(ui, seed, face::SM, Presence::of(state, str_of(d, "lastSeenAt")), name);
            ui.add(egui::Label::new(RichText::new(name).size(text::SMALL).color(colour::TEXT)).truncate());
        });
        shell::property(ui, "Agent state", |ui| {
            c::chip(ui, state_words(state), state_tone(state), true);
        });
        shell::property(ui, "Last seen", |ui| match str_of(d, "lastSeenAt") {
            Some(at) => {
                value(ui, &ago(at)).on_hover_text(exact(at));
            }
            None => faint(ui, "Not yet"),
        });
    }

    shell::property(ui, "Department", |ui| {
        match str_of(task, "discipline").filter(|d| !d.is_empty()) {
            // The same chip a department wears in every table, not the
            // monospace tag it used to be here alone.
            Some(d) => {
                c::chip(ui, d, c::discipline_tone(d), false);
            }
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

    if !r.can_act {
        ui.add_space(space::SM);
        match str_of(task, "assigneeName").filter(|n| !n.is_empty()) {
            Some(name) => w::caption(ui, &format!("Only {name} can move this task.")),
            None => w::caption(ui, "Unassigned \u{2014} assign it and the moves open up."),
        }
    }

    ask
}

/// The assignee menu's "nobody" row. Not a uuid, so it cannot collide with one.
const UNASSIGN: &str = "none";

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

    /// The track's key in the server's table of moves.
    fn key(self) -> &'static str {
        match self {
            Track::Eng => "eng",
            Track::Design => "design",
        }
    }

    /// The track's name in a sentence.
    fn word(self) -> &'static str {
        match self {
            Track::Eng => "engineering",
            Track::Design => "design",
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

/// Where a task on `track` may go from `status`, from `GET /api/user/tracks` —
/// the server's own table, so the menu never offers a move it would refuse.
/// Until the table arrives (or from a server that lacks it) every other state
/// on the track is offered, and the server has the last word.
fn legal_moves(table: Option<&Value>, track: Track, status: &str) -> Vec<&'static str> {
    let all = track.states().iter().copied().filter(|s| *s != status);
    match table.and_then(|t| t.get(track.key())) {
        Some(moves) => {
            let listed: Vec<&str> = moves
                .get(status)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            all.filter(|s| listed.contains(s)).collect()
        }
        None => all.collect(),
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
                Some(next) => format!("Moved to {}.", status_label(next)),
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
    from: &str,
    next: &'static str,
    track: Track,
    held: &[String],
    local: &mut Local,
) {
    local.move_from = from.to_owned();
    if needs_evidence(track, next)
        && !track.evidence().iter().any(|k| held.iter().any(|h| h == k.api()))
    {
        local.prompt = Some(Prompt::new(Some(next), track.evidence().to_vec()));
    } else {
        patch_status(net, task_id, from, next, None);
        local.patching = true;
    }
}

fn patch_status(
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    from: &str,
    next: &str,
    reason: Option<&str>,
) {
    let mut body = json!({ "status": next, "expectedStatus": from });
    if let Some(reason) = reason {
        body["manualReason"] = json!(reason);
    }
    // Drop the previous attempt's reply so the notice belongs to this one.
    net.invalidate(PATCH_KEY);
    net.patch(PATCH_KEY, &format!("/api/user/tasks/{task_id}"), body);
}

/// Send a details edit. `text` marks the title-and-description draft, which
/// is kept until the save lands so a failed one loses nothing.
fn save_details(
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    body: Value,
    text: bool,
    local: &mut Local,
) {
    local.notice = None;
    net.invalidate(DETAILS_KEY);
    net.patch(DETAILS_KEY, &format!("/api/user/tasks/{task_id}/details"), body);
    local.saving = true;
    local.saving_text = text;
}

/// Fold in the reply to a details edit.
fn settle_details(
    ctx: &egui::Context,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    local: &mut Local,
) {
    if !local.saving || net.is_loading(DETAILS_KEY) {
        return;
    }
    let (notice, ok) = match net.peek(DETAILS_KEY) {
        Some(Ok(v)) if str_of(v, "status") == Some("proposed") => (
            "Awaiting approval: you do not hold write on this project, so the \
             edit was recorded as a proposed change."
                .to_string(),
            true,
        ),
        Some(Ok(_)) => ("Saved.".to_string(), true),
        Some(Err(e)) => (e.to_string(), false),
        None => (String::new(), false),
    };
    if !notice.is_empty() {
        local.notice = Some((notice, !ok));
    }
    let draft_id = egui::Id::new(("task:draft", task_id));
    if local.saving_text {
        ctx.data_mut(|d| {
            if ok {
                d.remove::<Draft>(draft_id);
            } else if let Some(mut draft) = d.get_temp::<Draft>(draft_id) {
                // Kept, and re-based: the refetch shows what changed, and the
                // next Save is the person's answer to it.
                draft.updated_at = None;
                d.insert_temp(draft_id, draft);
            }
        });
    }
    local.saving = false;
    local.saving_text = false;
    invalidate_after_move(net);
}

/// This task, the board, the dashboard and the personal list all show the
/// status we have just changed.
fn invalidate_after_move(net: &mut crate::desktop::net::Net) {
    net.invalidate_prefix("task:");
    net.invalidate_prefix("board:");
    net.invalidate_prefix("mytasks");
    net.invalidate("home");
    net.invalidate(super::chrome::COUNTS);
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
                        patch_status(net, task_id, &local.move_from, next, None);
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
            patch_status(net, task_id, &local.move_from, next, Some(&reason));
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
    net: &mut crate::desktop::net::Net,
    track: Track,
    can_write: bool,
    local: &mut Local,
) {
    // Fold in a remove. Either way the list is refetched: a 404 means someone
    // else already removed it, and the list should say so by not having it.
    if local.removing && !net.is_loading(REMOVE_KEY) {
        if let Some(Err(e)) = net.peek(REMOVE_KEY) {
            local.resource_error = Some(e.to_string());
        }
        local.removing = false;
        net.invalidate(ARTIFACTS_KEY);
    }

    let rows = net.shared(ARTIFACTS_KEY);
    let rows: Option<&Vec<Value>> = rows.as_deref().and_then(Value::as_array);
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

    if let Some(err) = &local.resource_error {
        w::error(ui, err);
        ui.add_space(space::SM);
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
        w::empty(ui, "No resources yet \u{2014} PRs, docs and Figma files live here.", "");
        return;
    }

    let mut remove: Option<String> = None;
    w::card_list(ui, |ui| {
        ui.set_width(ui.available_width());
        for row in rows {
            if let Some(id) = resource_row(ui, row, can_write, local) {
                remove = Some(id);
            }
        }
    });
    if let Some(id) = remove {
        local.confirm_remove = None;
        local.resource_error = None;
        net.invalidate(REMOVE_KEY);
        net.send(REMOVE_KEY, reqwest::Method::DELETE, &format!("/api/user/artifacts/{id}"), Value::Null);
        local.removing = true;
    }
}

/// One resource: kind, what it is, where it goes. Returns the id to remove
/// once "Remove" has been confirmed.
///
/// The whole row opens the link, because a title-sized hit target on a
/// full-width row made the rest of the row look dead. A commit is a hash with
/// nowhere to open, so its row is not clickable and its hash stays mono.
fn resource_row(
    ui: &mut egui::Ui,
    row: &Value,
    can_write: bool,
    local: &mut Local,
) -> Option<String> {
    let id = str_of(row, "id").unwrap_or_default().to_owned();
    let kind = str_of(row, "kind").unwrap_or("link");
    let url = str_of(row, "url").unwrap_or_default();
    let title = str_of(row, "title").map(str::trim).filter(|t| !t.is_empty());
    let confirming = local.confirm_remove.as_deref() == Some(id.as_str());
    let mut removed = None;

    let mut body = |ui: &mut egui::Ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The right edge first, so what it takes is known before the
            // title and address are cut to fit what is left.
            if confirming {
                if w::danger(ui, "Remove", !local.removing).clicked() {
                    removed = Some(id.clone());
                }
                if w::ghost(ui, "Keep").clicked() {
                    local.confirm_remove = None;
                }
                faint(ui, "Remove this link?");
            } else {
                // Always drawn, never hover-only: a control that appears
                // under the pointer is one the keyboard can never reach, and
                // the project page's resources show it the same way. Only
                // whoever added it may remove it; the server says who that is.
                if can_write && row["canRemove"].as_bool() == Some(true) && w::ghost(ui, "Remove").clicked() {
                    local.confirm_remove = Some(id.clone());
                }
                if let Some(who) = super::board::added_by(row) {
                    faint(ui, &who);
                }
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                c::chip(ui, kind_label(kind), kind_tone(kind), false);
                if kind == "commit" {
                    w::mono_caption(ui, url);
                    if let Some(title) = title {
                        let fitted = elide(ui, title, ui.available_width());
                        ui.label(RichText::new(fitted).size(text::SMALL).color(colour::TEXT));
                    }
                    return;
                }
                let place = host_path(url);
                let name = elide(ui, title.unwrap_or(place), ui.available_width());
                ui.label(RichText::new(name).size(text::SMALL).color(colour::TEXT));
                // An untitled link already shows its address as its name.
                if title.is_some() {
                    let fitted = elide(ui, place, ui.available_width());
                    ui.label(RichText::new(fitted).size(text::SMALL).color(colour::TEXT_MUTED));
                }
            });
        });
    };

    if kind == "commit" || url.is_empty() {
        let w = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(w, size::ROW),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(w, size::ROW));
                ui.add_space(space::SM);
                body(ui);
            },
        );
    } else {
        // Remove sits on top of the row, so a click on it lands on it and not
        // on the row: egui gives the later widget the pointer.
        let response = w::row(ui, body).on_hover_text(url);
        if response.clicked() && removed.is_none() && !confirming {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
    removed
}

/// "github.com/airtribe/mycohort-api/pull/4821" from the full URL: where a
/// link goes, without the scheme nobody reads.
fn host_path(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.strip_prefix("www.").unwrap_or(rest).trim_end_matches('/')
}

// --------------------------------------------------------------------- notes

/// The thread. Anyone may post — a note needs no write scope, because the
/// point of it is that the person who noticed something can say so.
fn notes(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    with_session: bool,
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

    let all = net.shared(NOTES_KEY);
    let all: &[Value] = all.as_deref().and_then(Value::as_array).map_or(&[], Vec::as_slice);
    // With an agent on the task, its updates, questions and report are the
    // session's timeline; the thread keeps what people said to each other.
    let rows: Vec<&Value> =
        all.iter().filter(|n| !with_session || !str_of(n, "kind").is_some_and(session::session_kind)).collect();
    shell::section_count(ui, "Notes", rows.len());

    if let Some(err) = net.error(NOTES_KEY) {
        failed(ui, "Could not load notes", err);
    } else if rows.is_empty() {
        w::caption(ui, "No notes yet \u{2014} questions, decisions and heads-ups for the team go here.");
    } else {
        ui.scope(|ui| {
            ui.set_max_width(PROSE_W.min(ui.available_width()));
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    ui.add_space(space::MD);
                }
                // An agent's entry is signed with the agent's name and mark;
                // a person's with theirs.
                let agent = row.get("agent").and_then(|a| str_of(a, "name"));
                let author = agent.or_else(|| str_of(row, "authorName")).unwrap_or("Someone");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::SM;
                    if agent.is_some() {
                        w::agent_mark(ui, AVATAR);
                    } else {
                        avatar::small(ui, author, AVATAR);
                    }
                    value(ui, author);
                    if let Some((word, ink)) = note_kind(str_of(row, "kind").unwrap_or("note")) {
                        ui.label(RichText::new(word).size(text::SMALL).color(ink));
                    }
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
        let box_ = w::field_multiline(
            ui,
            "",
            &mut local.note,
            2,
            "Leave a note for the team\u{2026} Cmd+Enter to post",
        );
        let ready = !local.note.trim().is_empty() && !local.posting_note;
        if ready
            && box_.has_focus()
            && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
        {
            send = true;
        }
        if let Some(err) = &local.note_error {
            ui.add_space(space::XS);
            w::error(ui, err);
        }
        ui.add_space(space::SM);
        // Flush with the box's right edge, where a comment box keeps its send:
        // a disabled button has no fill, so on the left its label floated a
        // padding's width in from the box's edge.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let label = if local.posting_note { "Posting\u{2026}" } else { "Post" };
            let response = w::primary(ui, label, ready);
            if local.note.trim().is_empty() {
                response.clone().on_disabled_hover_text("Write a note first.");
            }
            if response.clicked() {
                send = true;
            }
        });
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

/// What kind of entry a note is, when it is more than a note: the word beside
/// its author, and its ink. Colour only where it asked something of a person.
fn note_kind(kind: &str) -> Option<(&'static str, egui::Color32)> {
    Some(match kind {
        "progress" => ("progress", colour::TEXT_MUTED),
        "question" => ("asked", colour::WARN),
        "answer" => ("answered", colour::TEXT_MUTED),
        "submission" => ("submitted for review", colour::AGENT),
        "review" => ("reviewed", colour::INFO),
        _ => return None,
    })
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
pub(super) fn ago(raw: &str) -> String {
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
pub(super) fn exact(raw: &str) -> String {
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
