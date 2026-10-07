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

use chrono::{DateTime, Local as LocalTz, Utc};
use egui::RichText;
use serde_json::{json, Value};

use super::agent_session::{self as session, Session};
use super::agents::AGENTS_KEY;
use super::menus::{task_items, Pick, Viewer};
use super::projects::{label_badge, label_picker, person_option, LABELS_KEY, PEOPLE_KEY, PROSE_W};
use crate::desktop::design::agent::{self as face, Presence};
use crate::desktop::design::{
    avatar, cards as c, colour, motion, radius, shell, size, space, status_label, text, theme,
    viz, widgets as w,
};
use crate::desktop::net::memo;
use crate::desktop::{App, Tab};

const TASK_KEY: &str = "task:one";
const PATCH_KEY: &str = "task:patch";
const ARTIFACTS_KEY: &str = "task:artifacts";
const ATTACH_KEY: &str = "task:artifact:new";
const NOTES_KEY: &str = "task:notes";
/// This delegation's plan revisions, owner-only.
const PLANS_KEY: &str = "task:plans";
const NOTE_KEY: &str = "task:note:new";
const DETAILS_KEY: &str = "task:details";
/// A label made from the rail; its reply joins the task's set.
const NEW_LABEL_KEY: &str = "task:new-label";
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
    /// The kinds this panel will accept. The first is the default, so it is
    /// the picker's resting label rather than one of its options.
    kinds: Vec<Kind>,
    /// `viz::select`'s slot, holding an api value. `None` means the default.
    kind: Option<String>,
    /// The first field: a URL for most kinds, a hash for a commit.
    value: String,
    title: String,
}

impl Prompt {
    fn new(kinds: Vec<Kind>) -> Self {
        Self {
            kinds,
            kind: None,
            value: String::new(),
            title: String::new(),
        }
    }

    fn default_kind(&self) -> Kind {
        self.kinds.first().copied().unwrap_or(Kind::Link)
    }

    fn chosen(&self) -> Kind {
        self.kind
            .as_deref()
            .and_then(Kind::from_api)
            .unwrap_or_else(|| self.default_kind())
    }
}

/// Everything the detail view remembers between frames. Scoped to one task id;
/// opening a different task resets it.
struct Local {
    task_id: String,
    /// The agent session's drafts and its step log.
    session: session::State,
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
    /// Files picked or dropped, waiting to go up one at a time.
    uploads: Vec<Held>,
    intake: Intake,
    /// The file going up now.
    uploading: Option<Held>,
    /// The agent action in flight — its past tense, for the notice.
    agent_busy: Option<String>,
    /// A pick from the title's menu, acted on once the page is drawn.
    pick: Option<Pick>,
    /// A task action from a menu is out.
    archiving: bool,
    /// This task's Accept or Dismiss is out.
    deciding: bool,
    /// The labels just picked, shown until the save lands and the task is
    /// read again.
    labels: Option<Vec<String>>,
}

impl Local {
    fn new(task_id: String) -> Self {
        Self {
            task_id,
            session: session::State::default(),
            patching: false,
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
            uploads: Vec::new(),
            intake: Intake::default(),
            uploading: None,
            agent_busy: None,
            pick: None,
            archiving: false,
            deciding: false,
            labels: None,
        }
    }
}

thread_local! {
    static LOCAL: RefCell<Option<Local>> = const { RefCell::new(None) };
}

pub fn attaching() -> bool {
    LOCAL.with(|cell| {
        cell.borrow().as_ref().is_some_and(|l| !l.uploads.is_empty() || l.uploading.is_some() || l.intake.working() > 0)
    })
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let Some(task_id) = app.task.clone() else {
        return;
    };

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
    if can_write {
        pasted_files(ui.ctx(), &local.intake);
        local.uploads.extend(local.intake.take());
    }

    local.archiving = app.board.tasks.busy();
    local.deciding = app.board.tasks.deciding.as_deref() == Some(task_id);
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
    if can_write {
        net.get_once(LABELS_KEY, "/api/user/labels");
        super::settings::want_folders(net);
    }

    let task = net.shared(TASK_KEY);
    // Only the assignee hands off, so only the assignee needs their agents.
    let mine_early = task
        .as_ref()
        .is_some_and(|t| !me.is_empty() && str_of(t, "assigneePersonId") == Some(me.as_str()));
    if mine_early {
        net.get_once(AGENTS_KEY, "/api/user/agents");
    }

    let title = task.as_ref().and_then(|t| str_of(t, "title")).unwrap_or_default().to_owned();
    let back = match app.tab {
        Tab::Projects if app.project.is_some() => task
            .as_ref()
            .filter(|t| str_of(t, "projectId") == app.project.as_deref())
            .and_then(|t| str_of(t, "projectName"))
            .unwrap_or("Project")
            .to_owned(),
        Tab::Home => "Home".to_owned(),
        Tab::MyTasks => "My Tasks".to_owned(),
        Tab::AllTasks => "All Tasks".to_owned(),
        Tab::Triage => "Triage".to_owned(),
        Tab::Projects => "Projects".to_owned(),
        Tab::Agents => "Agents".to_owned(),
        Tab::Settings => "Back".to_owned(),
    };
    if shell::crumbs(ui, &back, &title) {
        app.task = None;
        return;
    }
    let mut open_project: Option<String> = None;

    let net = app.net.as_mut().expect("net is live whenever a view runs");
    let Some(task) = task else {
        if let Some(err) = net.error(TASK_KEY) {
            failed(ui, "Could not load the task", err);
        } else if net.is_loading(TASK_KEY) {
            w::loading(ui, "Loading task");
        } else {
            w::empty(
                ui,
                "That task is no longer here",
                "Use Back to return to the board.",
            );
        }
        return;
    };

    let status = str_of(&task, "status").unwrap_or("open").to_string();
    let mine = !me.is_empty() && str_of(&task, "assigneePersonId") == Some(me.as_str());
    let track = Track::of(str_of(&task, "discipline"));
    let can_act = admin || mine;
    let moves = legal_moves(net.data(TRACKS_KEY), track, &status);
    let updated_at = str_of(&task, "updatedAt").map(str::to_owned);
    // Fold in the reply to a move started on an earlier frame. Done here, where
    // `net` is still free, so neither column has to own it.
    settle_move(net, local);
    settle_details(ui.ctx(), net, task_id, local);
    settle_agent(net, local);
    // A label made from the rail exists now, so it joins the set.
    let mut label_patch: Option<Vec<String>> = None;
    if let Some(label) = net.data(NEW_LABEL_KEY).cloned() {
        net.invalidate(NEW_LABEL_KEY);
        net.invalidate(LABELS_KEY);
        let mut picked = local.labels.clone().unwrap_or_else(|| label_ids(&task));
        if let Some(id) = str_of(&label, "id").filter(|id| !picked.iter().any(|p| p == id)) {
            picked.push(id.to_owned());
            label_patch = Some(picked);
        }
    }
    let label_error = net.error(NEW_LABEL_KEY).map(str::to_owned);
    let all_labels = net.shared(LABELS_KEY);
    let all_folders = net.shared(super::settings::FOLDERS_KEY);
    let picked_labels = local.labels.clone();

    let delegate = task.get("delegate").filter(|d| d.is_object());
    let agents = memo(
        ui.ctx(),
        egui::Id::new("task:my-agents"),
        net.generation(AGENTS_KEY),
        || {
            net.data(AGENTS_KEY)
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter(|a| super::agents::takes_work(a))
                        .filter_map(|a| {
                            Some((str_of(a, "id")?.to_owned(), str_of(a, "name")?.to_owned()))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        },
    );
    let handoff = Handoff {
        mine,
        delegated: super::menus::held(&task),
        finished: super::menus::finished(&task),
        agents: &agents,
    };
    // Who may move it between projects: `canArchive` is the same rule.
    let can_move = can_write
        && task
            .get("canArchive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if status == "triage" || can_move {
        super::triage::want_projects(net);
    }
    let viewer = Viewer {
        me: me.clone(),
        can_write,
        admin,
        agents: agents.to_vec(),
        projects: super::triage::projects(net),
        folders: super::settings::folders(net),
    };

    // The rail cannot hold `net` — the content column has it — so it reports
    // what was asked for and the request is made once both closures are gone.
    let mut from_rail: Option<Ask> = None;
    let busy = local.patching || local.attaching || local.saving || local.agent_busy.is_some();
    let people = net.shared(PEOPLE_KEY).filter(|_| can_write);
    let people: &[Value] = people
        .as_deref()
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    // Who may read the owner's conversation with the agent. The server says;
    // without its word, the owner and admins are exactly who it names.
    let private = task
        .get("canSeeAgentPrivate")
        .and_then(Value::as_bool)
        .unwrap_or(mine || admin);

    // The project's repositories as the viewer sees them, for the session's
    // missing-folder hint. Only asked for when there is a session to hint in,
    // under the key the project page reads, so either page warms the other.
    let project_id = str_of(&task, "projectId").unwrap_or_default().to_owned();
    let project_key = format!("board:project:{project_id}");
    if delegate.is_some() && mine && !project_id.is_empty() {
        net.get_once(&project_key, &format!("/api/user/projects/{project_id}"));
    }
    let unset_repo: Option<String> = net
        .data(&project_key)
        .and_then(|p| p.get("repos"))
        .and_then(Value::as_array)
        .and_then(|rows| rows.iter().find(|r| r["myPath"].is_null()))
        .and_then(|r| str_of(r, "name"))
        .map(str::to_owned);
    let mut hint_open = false;

    // The plan card's own data: owner-only, and only worth asking for once
    // there is a delegation to have one.
    if delegate.is_some() && private {
        net.get_once(PLANS_KEY, &format!("/api/user/tasks/{task_id}/plans"));
    }
    let plans = net.shared(PLANS_KEY);
    let plans_loaded = plans.is_some() || net.error(PLANS_KEY).is_some();

    let notes = net.shared(NOTES_KEY);
    let evidence = net.shared(ARTIFACTS_KEY);
    let panel = delegate.map(|d| Session {
        task_id,
        task: &task,
        delegate: d,
        notes: notes
            .as_deref()
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice),
        evidence: evidence
            .as_deref()
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice),
        mine,
        private,
        busy: local.agent_busy.is_some(),
        unset_repo: unset_repo.as_deref(),
        plans: plans
            .as_deref()
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice),
        plans_loaded,
    });

    let mut editing = false;
    shell::with_rail(
        ui,
        |ui, part| match part {
            shell::Part::Header => {
                editing = headline(
                    ui, net, task_id, &task, &status, track, &moves, can_act, can_write,
                    &handoff, &viewer, local,
                );
                if let Some(s) = &panel {
                    if let Some(ask) = session::pinned(ui, s, &mut local.session) {
                        agent_action(net, task_id, ask, local);
                    }
                }
            }
            shell::Part::Body => {
                if !editing {
                    description(ui, &task);
                }
                manual_reason(ui, &task);
                super::triage::source_card(ui, net, &task);
                resources(ui, net, task_id, &task, &me, admin, can_write, local);

                if let Some(s) = &panel {
                    if let Some(ask) = session::show(ui, net, s, &mut local.session) {
                        agent_action(net, task_id, ask, local);
                    }
                    hint_open |= std::mem::take(&mut local.session.open_project);
                }

                shell::divider(ui);
                activity(ui, net, task_id, &task, delegate, private, local);
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
                can_move,
                projects: &viewer.projects,
                all_labels: all_labels
                    .as_deref()
                    .and_then(Value::as_array)
                    .map_or(&[], Vec::as_slice),
                picked_labels: picked_labels.as_deref(),
                label_error: label_error.as_deref(),
                folders: all_folders
                    .as_deref()
                    .and_then(Value::as_array)
                    .map_or(&[], Vec::as_slice),
                private,
            };
            from_rail = rail(ui, &task, &ctx, &mut open_project);
        },
    );

    if hint_open {
        open_project = Some(project_id.clone());
    }
    match from_rail {
        Some(Ask::Move(next)) => {
            local.notice = None;
            start_move(net, task_id, &status, next, local);
        }
        Some(Ask::NewLabel(body)) => net.post(NEW_LABEL_KEY, "/api/user/labels", body),
        Some(Ask::Details(mut body)) => {
            if let Some(ids) = body.get("labelIds").and_then(Value::as_array) {
                local.labels = Some(
                    ids.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                );
            }
            // The rail edits one field the viewer can see, so the task as
            // shown is the version the edit is made against.
            if let Some(at) = &updated_at {
                body["expectedUpdatedAt"] = json!(at);
            }
            save_details(net, task_id, body, false, local);
        }
        None => {}
    }
    if let Some(ids) = label_patch {
        let mut body = json!({ "labelIds": ids });
        if let Some(at) = &updated_at {
            body["expectedUpdatedAt"] = json!(at);
        }
        local.labels = Some(ids);
        save_details(net, task_id, body, false, local);
    }
    // The page's own details edits say how they went where they always
    // have; hand-off, take-back, archive and delete share every other menu's
    // dialog.
    match local.pick.take() {
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
        .filter(|(_, next)| moves.contains(next) && (can_act || anyone_may(next)))
        .filter(|_| !handoff.delegated);

    // The draft lives in egui's temp store under the task id, not in a local:
    // leave for the project and come back, and the half-written words are
    // still there.
    let draft_id = egui::Id::new(("task:draft", task_id));
    let mut draft: Option<Draft> = ui.data(|d| d.get_temp(draft_id));

    let mut go = None;
    let mut close = false;
    let mut recategorise: Option<String> = None;
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
            let label = if local.saving_text {
                "Saving\u{2026}"
            } else {
                "Save"
            };
            let response = w::primary(ui, label, ready);
            if title.trim().is_empty() {
                response
                    .clone()
                    .on_disabled_hover_text("A task needs a title.");
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
                if let Some(at) = d
                    .updated_at
                    .clone()
                    .or_else(|| str_of(task, "updatedAt").map(str::to_owned))
                {
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
        let mut pick: Option<Pick> = None;
        shell::toolbar_trailing(ui, |ui| {
            viz::more(ui, |ui| pick = task_items(ui, task, viewer, false));
            if status == "triage" && super::triage::decides(task, viewer) && !local.deciding && pick.is_none() {
                pick = super::triage::page_actions(ui, task);
            }
            if let Some((copy, next)) = action {
                if w::primary(ui, copy, !busy).clicked() {
                    go = Some(next);
                }
            }
            if status != "triage" {
                if let Some(p) = handoff_control(ui, handoff, busy) {
                    pick = Some(p);
                }
            }
            if can_write && w::ghost(ui, "Edit").clicked() {
                edit = true;
            }
            if busy {
                ui.add(egui::Spinner::new().size(text::BODY));
            }
        });

        // ---- the title, wrapped in full rather than cut off: a prose
        // measure keeps a long one readable instead of a single edge-to-edge
        // line.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            let category = str_of(task, "category");
            // Offered to set only where it means something: a task
            // that has one, or one an intake agent filed.
            let editable = can_write && !busy;
            if category.is_some()
                || (editable && task.get("source").is_some_and(|s| s.is_object()))
            {
                if let Some(c) = super::triage::category_chip(ui, category, editable) {
                    recategorise = Some(c);
                }
            }
            ui.scope(|ui| {
                ui.set_max_width(PROSE_W.min(ui.available_width()));
                let title = ui.add(
                    egui::Label::new(
                        RichText::new(str_of(task, "title").unwrap_or("Untitled"))
                            .size(text::TITLE)
                            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                            .color(colour::TEXT()),
                    )
                    .wrap()
                    .sense(egui::Sense::click()),
                );
                shell::title_seen(ui, title.rect);
                viz::context_menu(&title, |ui| pick = task_items(ui, task, viewer, false));
            });
        });
        local.pick = pick;

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
        start_move(net, task_id, status, next, local);
    }
    if let Some(category) = recategorise {
        let mut body = json!({ "category": category });
        if let Some(at) = str_of(task, "updatedAt") {
            body["expectedUpdatedAt"] = json!(at);
        }
        save_details(net, task_id, body, false, local);
    }
    if status == "triage" && !super::triage::decides(task, viewer) {
        ui.add_space(space::MD);
        let who = str_of(task, "assigneeName")
            .and_then(|n| n.split_whitespace().next())
            .unwrap_or("its owner");
        w::caption(
            ui,
            &format!("In triage \u{2014} waiting on {who} to accept or dismiss it."),
        );
    }


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
    ui.add_space(space::MD);
    let body = str_of(task, "body").unwrap_or("").trim();
    if body.is_empty() {
        w::caption(ui, "No description");
        return;
    }
    ui.scope(|ui| {
        ui.set_max_width(PROSE_W.min(ui.available_width()));
        super::mrkdwn::show(ui, body, colour::TEXT_2());
    });
}

/// Why this task finished without a link. A task closed on someone's word
/// rather than on evidence should say so wherever the task is read — quietly,
/// but in the same breath as the description.
fn manual_reason(ui: &mut egui::Ui, task: &Value) {
    let Some(reason) = str_of(task, "manualReason")
        .map(str::trim)
        .filter(|r| !r.is_empty())
    else {
        return;
    };
    ui.add_space(space::SM);
    ui.scope(|ui| {
        ui.set_max_width(PROSE_W.min(ui.available_width()));
        ui.label(
            RichText::new(format!("Completed manually \u{2014} {reason}"))
                .size(text::SMALL)
                .color(colour::TEXT_MUTED()),
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

/// The hand-off control in the title's actions: "Take back" while an agent
/// holds the task, otherwise "Hand off to …" — a button for one agent, a
/// picker for several. Either is the same pick the task's menu makes, so the
/// hand-off dialog and the take-back confirm are the menu's. Nothing at all
/// for anyone but the assignee, or for an assignee with no agent to hand to.
fn handoff_control(ui: &mut egui::Ui, h: &Handoff, busy: bool) -> Option<Pick> {
    if !h.mine {
        return None;
    }
    if h.delegated {
        return w::secondary(ui, "Take back", !busy).clicked().then_some(Pick::TakeBack);
    }
    match h.agents {
        [] => None,
        _ if h.finished => {
            let label = match h.agents {
                [(_, name)] => format!("Hand off to {name}"),
                _ => "Hand off".to_owned(),
            };
            w::secondary(ui, &label, false).on_disabled_hover_text(
                "This task is finished \u{2014} there is nothing left to hand off.",
            );
            None
        }
        [(id, name)] => w::secondary(ui, &format!("Hand off to {name}"), !busy)
            .clicked()
            .then(|| Pick::Handoff(id.clone(), name.clone())),
        many => {
            let options: Vec<(String, String)> = many.to_vec();
            let mut slot: Option<String> = None;
            ui.add_enabled_ui(!busy, |ui| {
                viz::select(ui, "Hand off to\u{2026}", &options, &mut slot);
            });
            let id = slot?;
            let name = many.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()).unwrap_or_default();
            Some(Pick::Handoff(id, name))
        }
    }
}

fn agent_action(
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    ask: session::Ask,
    local: &mut Local,
) {
    let base = format!("/api/user/tasks/{task_id}");
    let (path, body, done) = match ask {
        session::Ask::Answer(body) => (
            format!("{base}/answer"),
            json!({ "body": body }),
            "Answer sent.".to_owned(),
        ),
        session::Ask::Approve => (
            format!("{base}/review"),
            json!({ "decision": "approve" }),
            "Approved.".to_owned(),
        ),
        session::Ask::Changes(body) => (
            format!("{base}/review"),
            json!({ "decision": "changes", "body": body }),
            "Changes requested.".to_owned(),
        ),
        session::Ask::Instruct(body) => (
            format!("{base}/instruct"),
            json!({ "body": body }),
            "Sent \u{2014} the agent hears it at its next step.".to_owned(),
        ),
        session::Ask::ApprovePlan => (
            format!("{base}/plan/approve"),
            json!({}),
            "Plan approved.".to_owned(),
        ),
        session::Ask::PlanChanges(body) => (
            format!("{base}/plan/changes"),
            json!({ "body": body }),
            "Changes requested.".to_owned(),
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
    NewLabel(Value),
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
    /// Whether the viewer may move it into another project or out of one.
    can_move: bool,
    /// Live projects: (id, name).
    projects: &'a [(String, String)],
    /// The shared label vocabulary, for the picker.
    all_labels: &'a [Value],
    /// The set just picked, while its save is out.
    picked_labels: Option<&'a [String]>,
    label_error: Option<&'a str>,
    /// The viewer's own folders, for the picker a project-less task offers.
    folders: &'a [Value],
    private: bool,
}

/// The ids of the labels a task wears.
fn label_ids(task: &Value) -> Vec<String> {
    task.get("labels")
        .and_then(Value::as_array)
        .map(|ls| {
            ls.iter()
                .filter_map(|l| str_of(l, "id"))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
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
        let options: Vec<(String, String)> = r
            .moves
            .iter()
            .map(|s| ((*s).to_owned(), status_label(s).to_owned()))
            .collect();
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
                    c::priority(ui, p);
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
            r.people
                .iter()
                .map(person_option)
                .filter(|(id, _)| Some(id.as_str()) != current),
        );
        let mut slot: Option<String> = None;
        viz::value_select(ui, name.unwrap_or("Unassigned"), &options, &mut slot);
        let Some(picked) = slot else { return };

        let person = r
            .people
            .iter()
            .find(|p| str_of(p, "id") == Some(picked.as_str()));
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
            ui.label(
                RichText::new(question)
                    .size(text::SMALL)
                    .color(colour::TEXT_2()),
            );
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
            let seed = str_of(d, "id").unwrap_or(name);
            face::avatar(ui, seed, face::SM, Presence::of(state, str_of(d, "lastSeenAt")), name);
            let owner = str_of(d, "ownerName").or_else(|| str_of(task, "assigneeName")).unwrap_or("its owner");
            let mut who = format!("{}\u{2019}s agent", owner.split_whitespace().next().unwrap_or(owner));
            if let Some(rt) = str_of(d, "runtime") {
                who += &format!(" \u{00B7} {}", super::agents::runtime_label(rt));
            }
            let label = egui::Label::new(RichText::new(name).size(text::SMALL).color(colour::TEXT())).truncate();
            match str_of(d, "id").filter(|_| r.private) {
                Some(id) => {
                    let link = ui.add(label.sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(who);
                    link.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, format!("Open {name}")));
                    if link.clicked() {
                        super::agents::open(id);
                    }
                }
                None => {
                    ui.add(label).on_hover_text(who);
                }
            }
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
        let name = str_of(task, "projectName").filter(|p| !p.is_empty());
        if r.can_move && !r.busy {
            // The breadcrumb still links to it; here it is a move.
            let here = str_of(task, "projectId");
            let mut options: Vec<(String, String)> = Vec::new();
            if here.is_some() {
                options.push((UNASSIGN.to_owned(), "No project".to_owned()));
            }
            options.extend(
                r.projects
                    .iter()
                    .filter(|(id, _)| Some(id.as_str()) != here)
                    .cloned(),
            );
            let mut slot: Option<String> = None;
            viz::value_select(ui, name.unwrap_or("No project"), &options, &mut slot);
            if let Some(picked) = slot {
                let to = if picked == UNASSIGN {
                    Value::Null
                } else {
                    json!(picked)
                };
                ask = Some(Ask::Details(json!({ "projectId": to })));
            }
            return;
        }
        match name {
            Some(name) => {
                if w::link(ui, name).clicked() {
                    *open_project = str_of(task, "projectId").map(str::to_owned);
                }
            }
            None => faint(ui, "No project"),
        }
    });

    // A project's repo already says where to work; this is only for a task
    // that has none.
    if str_of(task, "projectId").is_none() {
        shell::property(ui, "Folder", |ui| {
            let pinned = str_of(task, "folderName");
            let default_name = super::settings::default_name(r.folders);
            let resting = match (pinned, default_name) {
                (Some(n), _) => n.to_owned(),
                (None, Some(d)) => format!("{d} (default)"),
                (None, None) => "No folder set".to_owned(),
            };
            if !r.can_write || r.busy {
                if pinned.is_some() || default_name.is_some() {
                    value(ui, &resting);
                } else {
                    faint(ui, "No folder set");
                }
                return;
            }
            let mut options: Vec<(String, String)> = Vec::new();
            if pinned.is_some() {
                let label = match default_name {
                    Some(d) => format!("Use default ({d})"),
                    None => "Use default".to_owned(),
                };
                options.push((UNASSIGN.to_owned(), label));
            }
            options.extend(
                r.folders
                    .iter()
                    .filter_map(|f| str_of(f, "name"))
                    .filter(|n| Some(*n) != pinned)
                    .map(|n| (n.to_owned(), n.to_owned())),
            );
            let mut slot: Option<String> = None;
            viz::value_select(ui, &resting, &options, &mut slot);
            if let Some(picked) = slot {
                let to = if picked == UNASSIGN {
                    Value::Null
                } else {
                    json!(picked)
                };
                ask = Some(Ask::Details(json!({ "folderName": to })));
            }
        });
    }

    let chosen: Vec<String> = r
        .picked_labels
        .map_or_else(|| label_ids(task), <[String]>::to_vec);
    shell::property(ui, "Labels", |ui| {
        if !r.can_write || r.busy {
            // The set as it stands, or as just picked while that save is out.
            let shown: Vec<&Value> = chosen
                .iter()
                .filter_map(|id| {
                    r.all_labels
                        .iter()
                        .chain(
                            task.get("labels")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten(),
                        )
                        .find(|l| str_of(l, "id") == Some(id.as_str()))
                })
                .collect();
            if shown.is_empty() {
                faint(ui, "None");
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::XS, space::XS);
                for l in shown {
                    label_badge(ui, l);
                }
            });
            return;
        }
        let mut picked = chosen.clone();
        ui.vertical(|ui| {
            if let Some(body) = label_picker(ui, "Add labels", r.all_labels, &mut picked) {
                ask = Some(Ask::NewLabel(body));
            }
            if let Some(err) = r.label_error {
                ui.label(
                    RichText::new(format!("Could not make that label: {err}"))
                        .size(text::CAPTION)
                        .color(colour::DANGER()),
                );
            }
        });
        if picked != chosen {
            ask = Some(Ask::Details(json!({ "labelIds": picked })));
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

/// The assignee menu's "nobody" row, and the project menu's "No project". Not
/// a uuid, so it cannot collide with one.
const UNASSIGN: &str = "none";

fn value(ui: &mut egui::Ui, s: &str) -> egui::Response {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT()))
}

/// A value that is not there. Fainter than one that is, so an empty rail row
/// reads as absence rather than as content.
fn faint(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT_FAINT()));
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
            Track::Eng => &[
                "open",
                "in_progress",
                "blocked",
                "completed",
                "shipped",
                "dropped",
            ],
            Track::Design => &[
                "open",
                "in_progress",
                "research",
                "completed",
                "handoff",
                "shipped",
                "blocked",
                "dropped",
            ],
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
/// move you almost always came to the page to make. Both tracks end at
/// shipped, so it gets no button — the dropdown is where a reopen lives.
fn primary_move(track: Track, status: &str) -> Option<(&'static str, &'static str)> {
    Some(match (track, status) {
        (_, "open") => ("Start", "in_progress"),
        (_, "in_progress") | (_, "research") => ("Complete", "completed"),
        (Track::Eng, "completed") => ("Mark shipped", "shipped"),
        (Track::Design, "completed") => ("Hand off", "handoff"),
        (_, "handoff") => ("Mark shipped", "shipped"),
        (_, "blocked") => ("Resume", "in_progress"),
        _ => return None,
    })
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


/// Fold in the reply to a move we started earlier.
fn settle_move(net: &mut crate::desktop::net::Net, local: &mut Local) {
    if !local.patching || net.is_loading(PATCH_KEY) {
        return;
    }
    let Some(result) = net.peek(PATCH_KEY) else {
        return;
    };
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

/// Move it. A person may put a task in any state, with or without evidence.
fn start_move(net: &mut crate::desktop::net::Net, task_id: &str, from: &str, next: &'static str, local: &mut Local) {
    patch_status(net, task_id, from, next, None);
    local.patching = true;
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
    net.patch(
        DETAILS_KEY,
        &format!("/api/user/tasks/{task_id}/details"),
        body,
    );
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
    local.labels = None;
    invalidate_after_move(net);
}

/// This task, the board, the dashboard and the personal list all show the
/// status we have just changed.
pub(super) fn invalidate_after_move(net: &mut crate::desktop::net::Net) {
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
                local.prompt = None;
                net.invalidate(ARTIFACTS_KEY);
                local.notice = Some(("Attached.".to_string(), false));
            }
            Some(Err(e)) => {
                local.notice = Some((e.to_string(), true));
                local.attaching = false;
            }
            // The reply was invalidated out from under us; nothing is coming.
            None => local.attaching = false,
        }
    }

    let Some(prompt) = local.prompt.as_mut() else {
        return;
    };
    let busy = local.attaching || local.patching;

    let mut post: Option<Value> = None;
    let mut close = false;

    ui.add_space(space::MD);
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());

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
                egui::Stroke::new(1.0, colour::DANGER()),
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
            if w::primary(ui, "Attach", ready).clicked() {
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
}

// ------------------------------------------------------------------ evidence

/// Everything hanging off this task, near the top where it is looked for:
/// links as tiles grouped by kind — code, design, docs — and the files people
/// attached, screenshots as thumbnails and docs as tiles. Files can be picked
/// or dropped anywhere on the page; they go to the agent with the task.
#[allow(clippy::too_many_arguments)]
fn resources(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    task: &Value,
    me: &str,
    admin: bool,
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
        net.invalidate(TASK_KEY);
    }
    settle_upload(ui.ctx(), net, task_id, local);
    if can_write {
        dropped_files(ui.ctx(), &local.intake);
    }

    let rows = net.shared(ARTIFACTS_KEY);
    let mut links: Vec<&Value> = rows.as_deref().and_then(Value::as_array).map(|r| r.iter().collect()).unwrap_or_default();
    // Code, then design, then everything else; newest first within a kind.
    links.sort_by_key(|r| KIND_ORDER.iter().position(|k| Some(*k) == str_of(r, "kind")).unwrap_or(KIND_ORDER.len()));
    let files: &[Value] = task.get("files").and_then(Value::as_array).map_or(&[], Vec::as_slice);

    let mut add = false;
    let mut pick = false;
    shell::section_count_with(ui, "Resources", links.len() + files.len(), |ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        if can_write {
            if w::ghost(ui, "Upload file").on_hover_text("Screenshots, PDFs or .md files \u{2014} or drop them on the page").clicked() {
                pick = true;
            }
            if w::ghost(ui, "+ Add link").clicked() {
                add = true;
            }
        }
    });
    if add {
        local.prompt = Some(Prompt::new(vec![Kind::Pr, Kind::Figma, Kind::Doc, Kind::Commit, Kind::Link]));
    }
    if pick {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Attach files")
            .add_filter("Screenshots, PDFs and docs", &["png", "jpg", "jpeg", "gif", "webp", "pdf", "md", "markdown", "txt"])
            .pick_files()
        {
            local.intake.spawn(ui.ctx(), move |ctx| upload_bodies(ctx, &paths));
        }
    }
    prompt_panel(ui, net, task_id, local);

    if let Some(err) = &local.resource_error {
        w::error(ui, err);
        ui.add_space(space::SM);
    }
    let held: Vec<&Held> = local.uploading.iter().chain(&local.uploads).collect();
    let going_up: Vec<(String, egui::TextureHandle)> =
        held.iter().filter_map(|h| Some((h.name.clone(), h.preview.clone()?))).collect();
    let unseen = held.len() - going_up.len() + local.intake.working();
    if unseen > 0 {
        w::caption(ui, &format!("Uploading {}\u{2026}", plural(unseen as i64, "file")));
        ui.add_space(space::SM);
    }
    if let Some(err) = net.error(ARTIFACTS_KEY) {
        failed(ui, "Could not load resources", err);
        return;
    }
    if rows.is_none() {
        w::loading(ui, "Loading resources");
        return;
    }
    if links.is_empty() && files.is_empty() && going_up.is_empty() && unseen == 0 {
        let dragging = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        w::caption(
            ui,
            if dragging {
                "Drop to attach."
            } else {
                "Nothing yet \u{2014} link the PR, Figma or doc, or drop screenshots and .md files here for the agent."
            },
        );
        return;
    }

    // ---- links, as tiles two to a row (one when the column is narrow)
    let remove = link_tiles(ui, &links, can_write, &mut local.confirm_remove, local.removing);

    // ---- files people attached
    let mut drop_file: Option<String> = None;
    if !files.is_empty() || !going_up.is_empty() {
        if !links.is_empty() {
            ui.add_space(space::XS);
        }
        let (images, docs): (Vec<&Value>, Vec<&Value>) =
            files.iter().partition(|f| str_of(f, "mime").is_some_and(|m| m.starts_with("image/")));
        let may_remove = |f: &Value| can_write && (admin || str_of(f, "addedById") == Some(me));
        if !images.is_empty() || !going_up.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::SM);
                for f in &images {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = space::XXS;
                        super::triage::thumbnail(ui, net, f);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::XS;
                            faint(ui, &file_caption(f));
                            if may_remove(f) && w::link(ui, "Remove").clicked() {
                                drop_file = str_of(f, "id").map(str::to_owned);
                            }
                        });
                    });
                }
                for (name, tex) in &going_up {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = space::XXS;
                        ui.multiply_opacity(0.55);
                        preview(ui, tex, super::triage::THUMB, name);
                        faint(ui, "Uploading\u{2026}");
                    });
                }
            });
            ui.add_space(space::SM);
        }
        for f in &docs {
            if let Some(id) = doc_tile(ui, net, f, may_remove(f)) {
                drop_file = Some(id);
            }
            ui.add_space(space::XS);
        }
    }
    super::triage::lightbox(ui.ctx(), net);
    super::triage::open_pending(ui.ctx(), net);
    doc_viewer(ui.ctx(), net);

    if let Some(id) = remove {
        local.confirm_remove = None;
        local.resource_error = None;
        net.invalidate(REMOVE_KEY);
        net.send(REMOVE_KEY, reqwest::Method::DELETE, &format!("/api/user/artifacts/{id}"), Value::Null);
        local.removing = true;
    }
    if let Some(id) = drop_file {
        local.resource_error = None;
        net.invalidate(REMOVE_KEY);
        net.send(REMOVE_KEY, reqwest::Method::DELETE, &format!("/api/user/files/{id}"), Value::Null);
        local.removing = true;
    }
}

/// The order links read in: code first, then design, then the rest.
const KIND_ORDER: [&str; 5] = ["pr", "commit", "figma", "doc", "link"];

/// "Added by Anmol · 2 days ago" under a thumbnail.
fn file_caption(f: &Value) -> String {
    let mut words = str_of(f, "addedBy").map(|n| n.split_whitespace().next().unwrap_or(n).to_owned()).unwrap_or_default();
    if let Some(at) = str_of(f, "createdAt") {
        if !words.is_empty() {
            words.push_str(" \u{00B7} ");
        }
        words.push_str(&ago(at));
    }
    words
}

const TILE_H: f32 = 52.0;
const REMOVE_W: f32 = 28.0;

/// A tile's remove control: a small ×, named for screen readers and on hover.
pub(super) fn remove_x(ui: &mut egui::Ui) -> egui::Response {
    let r = ui.add(
        egui::Button::new(RichText::new(egui_phosphor::regular::X).size(text::SMALL).color(colour::TEXT_MUTED()))
            .frame(false)
            .min_size(egui::Vec2::splat(REMOVE_W - space::XS)),
    );
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Remove"));
    r.on_hover_text("Remove")
}

pub(super) fn link_tiles(
    ui: &mut egui::Ui,
    links: &[&Value],
    can_write: bool,
    confirm: &mut Option<String>,
    busy: bool,
) -> Option<String> {
    let mut remove = None;
    let width = ui.available_width().min(PROSE_W + 160.0);
    let per_row = if width > 560.0 { 2 } else { 1 };
    let tile_w = (width - space::SM * (per_row as f32 - 1.0)) / per_row as f32;
    for chunk in links.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            for row in chunk {
                if let Some(id) = link_tile(ui, row, tile_w, can_write, confirm, busy) {
                    remove = Some(id);
                }
            }
        });
        ui.add_space(space::SM);
    }
    remove
}

/// One link as a tile: its kind's mark in a tinted well, the name, and under
/// it the kind, where it goes and who added it. The whole tile opens it; a
/// commit has nowhere to go, so its tile is still. Returns the id to remove
/// once "Remove" has been confirmed.
fn link_tile(ui: &mut egui::Ui, row: &Value, width: f32, can_write: bool, confirm: &mut Option<String>, busy: bool) -> Option<String> {
    let id = str_of(row, "id").unwrap_or_default().to_owned();
    let kind = str_of(row, "kind").unwrap_or("link");
    let url = str_of(row, "url").unwrap_or_default();
    let title = str_of(row, "title").map(str::trim).filter(|t| !t.is_empty());
    let place = if kind == "commit" {
        link_label(url).unwrap_or_else(|| url.chars().take(7).collect())
    } else {
        link_label(url).unwrap_or_else(|| host_path(url).to_owned())
    };
    let name = title.map(str::to_owned).unwrap_or_else(|| place.clone());
    let opens = kind != "commit" && !url.is_empty();
    let confirming = confirm.as_deref() == Some(id.as_str());
    let removable = can_write && row["canRemove"].as_bool() == Some(true);

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, TILE_H),
        if opens { egui::Sense::click() } else { egui::Sense::hover() },
    );
    let response = if opens { motion::operable(ui, response, radius::MD as f32) } else { response };
    let hot = opens && (response.hovered() || response.has_focus());
    let ink = kind_ink(kind);
    let p = ui.painter().clone();
    p.rect_filled(rect, radius::MD as f32, if hot { colour::SURFACE_HOVER() } else { colour::SURFACE() });
    p.rect_stroke(
        rect,
        radius::MD as f32,
        egui::Stroke::new(1.0, if hot { colour::LINE_STRONG() } else { colour::LINE() }),
        egui::StrokeKind::Inside,
    );
    let well = egui::Rect::from_center_size(
        egui::pos2(rect.left() + space::MD + 16.0, rect.center().y),
        egui::Vec2::splat(32.0),
    );
    p.rect_filled(well, radius::SM as f32, ink.gamma_multiply(0.14));
    crate::desktop::design::glyph::evidence(&p, well.center(), 18.0, kind, ink);

    // The right edge: Remove (or its confirmation) sits over the tile, so a
    // click on it is its own and not the tile's.
    let mut removed = None;
    let right_w = if confirming { 150.0 } else if removable { REMOVE_W } else { 0.0 };
    if right_w > 0.0 {
        let slot = egui::Rect::from_min_max(
            egui::pos2(rect.right() - right_w - space::SM, rect.top()),
            egui::pos2(rect.right() - space::SM, rect.bottom()),
        );
        // A child, not a scope: it must not move the row's cursor, or the
        // next tile in the row starts short.
        let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(slot).layout(egui::Layout::right_to_left(egui::Align::Center)));
        {
            ui.spacing_mut().item_spacing.x = space::XS;
            if confirming {
                if w::danger(ui, "Remove", !busy).clicked() {
                    removed = Some(id.clone());
                }
                if w::ghost(ui, "Keep").clicked() {
                    *confirm = None;
                }
            } else if remove_x(ui).clicked() {
                *confirm = Some(id.clone());
            }
        }
    }

    let text_left = well.right() + space::MD;
    let text_w = (rect.right() - right_w - space::MD - text_left).max(0.0);
    let title_g = w::truncated(
        ui,
        &name,
        egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::SEMIBOLD.into())),
        colour::TEXT(),
        text_w,
    );
    let mut sub = kind_label(kind).to_owned();
    if title.is_some() {
        sub += &format!(" \u{00B7} {place}");
    }
    if let Some(who) = super::board::added_by(row) {
        sub += &format!(" \u{00B7} {who}");
    }
    let sub_g = w::truncated(ui, &sub, egui::FontId::proportional(text::CAPTION), colour::TEXT_MUTED(), text_w);
    let total = title_g.size().y + space::XXS + sub_g.size().y;
    let top = rect.center().y - total / 2.0;
    p.galley(egui::pos2(text_left, top), title_g.clone(), colour::TEXT());
    p.galley(egui::pos2(text_left, top + title_g.size().y + space::XXS), sub_g, colour::TEXT_MUTED());

    let response = response.on_hover_text(if url.is_empty() { name.as_str() } else { url });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, opens, format!("{}: {name}", kind_label(kind))));
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if opens && response.clicked() && removed.is_none() && !confirming {
        super::mrkdwn::open(ui.ctx(), url);
    }
    removed
}

/// A file that is not an image — a markdown doc, a PDF — as a tile. A doc
/// opens in the app, read as markdown; anything else opens with the Mac's
/// own viewer. Returns the id to remove when Remove is clicked.
fn doc_tile(ui: &mut egui::Ui, net: &mut crate::desktop::net::Net, f: &Value, may_remove: bool) -> Option<String> {
    let id = str_of(f, "id").unwrap_or_default().to_owned();
    let name = str_of(f, "name").unwrap_or("file");
    let mime = str_of(f, "mime").unwrap_or_default();
    let readable = mime.starts_with("text/");
    let width = ui.available_width().min(PROSE_W + 160.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, TILE_H), egui::Sense::click());
    let response = motion::operable(ui, response, radius::MD as f32);
    let hot = response.hovered() || response.has_focus();
    let p = ui.painter().clone();
    p.rect_filled(rect, radius::MD as f32, if hot { colour::SURFACE_HOVER() } else { colour::SURFACE() });
    p.rect_stroke(rect, radius::MD as f32, egui::Stroke::new(1.0, if hot { colour::LINE_STRONG() } else { colour::LINE() }), egui::StrokeKind::Inside);
    let well = egui::Rect::from_center_size(egui::pos2(rect.left() + space::MD + 16.0, rect.center().y), egui::Vec2::splat(32.0));
    p.rect_filled(well, radius::SM as f32, colour::TEXT_MUTED().gamma_multiply(0.14));
    crate::desktop::design::glyph::evidence(&p, well.center(), 18.0, "doc", colour::TEXT_2());

    let mut removed = None;
    let right_w = if may_remove { REMOVE_W } else { 0.0 };
    if may_remove {
        let slot = egui::Rect::from_min_max(egui::pos2(rect.right() - right_w - space::SM, rect.top()), egui::pos2(rect.right() - space::SM, rect.bottom()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(slot).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if remove_x(&mut child).clicked() {
            removed = Some(id.clone());
        }
    }
    let text_left = well.right() + space::MD;
    let text_w = (rect.right() - right_w - space::MD - text_left).max(0.0);
    let title_g = w::truncated(ui, name, egui::FontId::new(text::SMALL, egui::FontFamily::Name(theme::SEMIBOLD.into())), colour::TEXT(), text_w);
    let mut sub = if mime == "text/markdown" { "Markdown".to_owned() } else if mime == "application/pdf" { "PDF".to_owned() } else { "Text".to_owned() };
    if let Some(size) = f.get("size").and_then(Value::as_i64) {
        sub += &format!(" \u{00B7} {}", super::triage::file_size(size));
    }
    let caption = file_caption(f);
    if !caption.is_empty() {
        sub += &format!(" \u{00B7} {caption}");
    }
    let sub_g = w::truncated(ui, &sub, egui::FontId::proportional(text::CAPTION), colour::TEXT_MUTED(), text_w);
    let total = title_g.size().y + space::XXS + sub_g.size().y;
    let top = rect.center().y - total / 2.0;
    p.galley(egui::pos2(text_left, top), title_g.clone(), colour::TEXT());
    p.galley(egui::pos2(text_left, top + title_g.size().y + space::XXS), sub_g, colour::TEXT_MUTED());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Open {name}")));
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() && removed.is_none() && !id.is_empty() {
        super::triage::want(net, &id);
        let key = if readable { DOC_VIEW } else { "source:opening" };
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(key), (id.clone(), name.to_owned())));
    }
    removed
}

const DOC_VIEW: &str = "task:doc-view";

/// A markdown or text file, read in the app.
fn doc_viewer(ctx: &egui::Context, net: &crate::desktop::net::Net) {
    let key = egui::Id::new(DOC_VIEW);
    let Some((id, name)) = ctx.data(|d| d.get_temp::<(String, String)>(key)) else {
        return;
    };
    let mut close = false;
    let modal = super::agents::dialog(ctx, DOC_VIEW, PROSE_W, |ui| {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = w::ghost(ui, "Close").clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(
                        RichText::new(&name).size(text::HEADING).family(egui::FontFamily::Name(theme::SEMIBOLD.into())).color(colour::TEXT()),
                    ).truncate());
                });
            });
        });
        ui.add_space(space::MD);
        match net.bytes(&super::triage::file_key(&id)) {
            Some(Ok(bytes)) => {
                egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.65).show(ui, |ui| {
                    super::mrkdwn::show(ui, &String::from_utf8_lossy(bytes), colour::TEXT());
                    shell::edge_scroll(ui);
                });
            }
            Some(Err(e)) => w::error(ui, e),
            None => w::loading(ui, "Loading"),
        }
    });
    if close || modal.should_close() {
        ctx.data_mut(|d| d.remove::<(String, String)>(key));
    }
}

/// The wire type for a file, from its extension; `None` for one the server
/// will not take.
fn mime_of(path: &std::path::Path) -> Option<&'static str> {
    Some(match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        _ => return None,
    })
}

/// Read picked, dropped or pasted files for upload, refusing what the
/// server would refuse anyway with a sentence now rather than a 415 later.
pub(super) fn upload_bodies(ctx: &egui::Context, paths: &[std::path::PathBuf]) -> Vec<Held> {
    let mut out = Vec::new();
    for path in paths {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_owned();
        let Some(mime) = mime_of(path) else {
            w::toast(ctx, format!("{name} can\u{2019}t be attached \u{2014} images, PDFs and .md or .txt files only."), true);
            continue;
        };
        if std::fs::metadata(path).is_ok_and(|m| over_cap(ctx, &name, m.len())) {
            continue;
        }
        match std::fs::read(path) {
            Ok(bytes) => out.extend(upload_body(ctx, name, mime, &bytes, None)),
            Err(e) => w::toast(ctx, format!("Could not read {name}: {e}"), true),
        }
    }
    out
}

pub(super) struct Held {
    pub(super) name: String,
    pub(super) body: Value,
    pub(super) preview: Option<egui::TextureHandle>,
}

const MAX_UPLOAD: u64 = 8 * 1024 * 1024;

fn over_cap(ctx: &egui::Context, name: &str, len: u64) -> bool {
    let over = len > MAX_UPLOAD;
    if over {
        w::toast(ctx, format!("{name} is over 8 MB; link it instead."), true);
    }
    over
}

fn upload_body(ctx: &egui::Context, name: String, mime: &str, bytes: &[u8], image: Option<image::DynamicImage>) -> Option<Held> {
    use base64::Engine;
    if over_cap(ctx, &name, bytes.len() as u64) {
        return None;
    }
    let image = image.or_else(|| mime.starts_with("image/").then(|| decode(bytes)).flatten());
    let preview = image.map(|img| thumb(ctx, &name, img));
    let body = json!({
        "name": name,
        "mime": mime,
        "dataBase64": base64::engine::general_purpose::STANDARD.encode(bytes),
    });
    Some(Held { name, body, preview })
}

const PREVIEW_PX: u32 = 480;
const DECODE_MAX_ALLOC: u64 = 256 * 1024 * 1024;

fn decode(bytes: &[u8]) -> Option<image::DynamicImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(DECODE_MAX_ALLOC);
    reader.limits(limits);
    reader.decode().ok()
}

fn thumb(ctx: &egui::Context, name: &str, img: image::DynamicImage) -> egui::TextureHandle {
    let img = if img.width().max(img.height()) > PREVIEW_PX { img.thumbnail(PREVIEW_PX, PREVIEW_PX) } else { img };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    ctx.load_texture(format!("held:{name}"), egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), egui::TextureOptions::LINEAR)
}

pub(super) fn preview(ui: &mut egui::Ui, tex: &egui::TextureHandle, max: egui::Vec2, name: &str) -> egui::Response {
    let natural = tex.size_vec2();
    let shown = (natural * (max.x / natural.x).min(max.y / natural.y).min(1.0)).max(egui::Vec2::splat(space::XXL));
    let (rect, response) = ui.allocate_exact_size(shown, egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, name));
    egui::Image::new((tex.id(), shown)).corner_radius(radius::MD).paint_at(ui, rect);
    ui.painter().rect_stroke(rect, radius::MD as f32, egui::Stroke::new(1.0, colour::LINE_SOFT()), egui::StrokeKind::Inside);
    response.on_hover_text(name)
}

#[derive(Clone, Default)]
pub(super) struct Intake(std::sync::Arc<std::sync::Mutex<Prepared>>);

#[derive(Default)]
struct Prepared {
    ready: Vec<Held>,
    working: usize,
}

impl Intake {
    fn lock(&self) -> std::sync::MutexGuard<'_, Prepared> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn take(&self) -> Vec<Held> {
        std::mem::take(&mut self.lock().ready)
    }

    pub(super) fn working(&self) -> usize {
        self.lock().working
    }

    pub(super) fn spawn(&self, ctx: &egui::Context, job: impl FnOnce(&egui::Context) -> Vec<Held> + Send + 'static) {
        self.lock().working += 1;
        let (intake, ctx) = (self.clone(), ctx.clone());
        std::thread::spawn(move || {
            let held = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&ctx))).unwrap_or_else(|_| {
                w::toast(&ctx, "Could not add that file.", true);
                Vec::new()
            });
            let mut prepared = intake.lock();
            prepared.working -= 1;
            prepared.ready.extend(held);
            drop(prepared);
            ctx.request_repaint();
        });
    }
}

pub(super) fn pasted_files(ctx: &egui::Context, intake: &Intake) {
    let pasted_text = |e: &egui::Event| matches!(e, egui::Event::Paste(_));
    let asked = crate::desktop::menu::pasted(ctx) || ctx.input(|i| i.events.iter().any(pasted_text));
    if !asked {
        return;
    }
    let Ok(mut clip) = arboard::Clipboard::new() else { return };
    let files = clip.get().file_list().unwrap_or_default();
    if !files.is_empty() {
        intake.spawn(ctx, move |ctx| upload_bodies(ctx, &files));
    } else if ctx.text_edit_focused() && clip.get_text().is_ok_and(|t| !t.trim().is_empty()) {
        return;
    } else if let Ok(image) = clip.get_image() {
        let image = arboard::ImageData { width: image.width, height: image.height, bytes: image.bytes.into_owned().into() };
        intake.spawn(ctx, move |ctx| pasted_image(ctx, image));
    } else {
        return;
    }
    ctx.input_mut(|i| i.events.retain(|e| !pasted_text(e)));
}

fn pasted_image(ctx: &egui::Context, image: arboard::ImageData<'static>) -> Vec<Held> {
    let name = format!("Pasted image {}.png", chrono::Local::now().format("%H.%M.%S"));
    match png(image) {
        Ok((bytes, rgba)) => upload_body(ctx, name, "image/png", &bytes, Some(image::DynamicImage::ImageRgba8(rgba))).into_iter().collect(),
        Err(e) => {
            w::toast(ctx, format!("Could not paste the image: {e}"), true);
            Vec::new()
        }
    }
}

fn png(image: arboard::ImageData<'_>) -> Result<(Vec<u8>, image::RgbaImage), String> {
    let rgba = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())
        .ok_or("the clipboard image is malformed")?;
    let mut out = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok((out.into_inner(), rgba))
}

pub(super) fn dropped_files(ctx: &egui::Context, intake: &Intake) {
    let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
    ctx.input_mut(|i| i.raw.dropped_files.clear());
    if !dropped.is_empty() {
        intake.spawn(ctx, move |ctx| upload_bodies(ctx, &dropped));
    }
}

const UPLOAD_KEY: &str = "task:upload";

/// One upload at a time: fold in the reply, then send the next queued file.
fn settle_upload(ctx: &egui::Context, net: &mut crate::desktop::net::Net, task_id: &str, local: &mut Local) {
    if let Some(name) = local.uploading.as_ref().map(|h| h.name.clone()) {
        if net.is_loading(UPLOAD_KEY) {
            return;
        }
        match net.peek(UPLOAD_KEY) {
            Some(Ok(_)) => w::toast(ctx, format!("Attached {name}."), false),
            Some(Err(e)) => local.resource_error = Some(format!("{name}: {e}")),
            None => {}
        }
        local.uploading = None;
        net.invalidate(TASK_KEY);
    }
    if local.uploads.is_empty() {
        return;
    }
    let mut next = local.uploads.remove(0);
    let body = std::mem::take(&mut next.body);
    local.uploading = Some(next);
    net.invalidate(UPLOAD_KEY);
    net.post(UPLOAD_KEY, &format!("/api/user/tasks/{task_id}/files"), body);
}

/// One hue per kind, so a list of five resources is scannable rather than read.
fn kind_ink(kind: &str) -> egui::Color32 {
    match kind {
        "pr" => colour::INFO(),
        "commit" => colour::OK(),
        "figma" => colour::AGENT(),
        "doc" => colour::WARN(),
        _ => colour::TEXT_MUTED(),
    }
}

/// A GitHub commit or pull request named the way people say it:
/// "mycohort-api · 6f54d7d", "mycohort-api #4821". `None` for anything else.
pub(super) fn link_label(url: &str) -> Option<String> {
    let rest = host_path(url).strip_prefix("github.com/")?;
    let mut parts = rest.split('/');
    let (_org, repo, what, id) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    match what {
        "commit" => Some(format!("{repo} \u{b7} {}", id.chars().take(7).collect::<String>())),
        "pull" => Some(format!("{repo} #{id}")),
        _ => None,
    }
}

/// "github.com/airtribe/mycohort-api/pull/4821" from the full URL: where a
/// link goes, without the scheme nobody reads.
fn host_path(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.strip_prefix("www.")
        .unwrap_or(rest)
        .trim_end_matches('/')
}

// --------------------------------------------------------------------- notes

/// Everything that happened on the task, newest first: the team's notes and,
/// when an agent has held it, its plans, updates, questions and reports. The
/// note box sits on top, where the newest entry lands. Anyone may post — a
/// note needs no write scope, because the point of it is that the person who
/// noticed something can say so.
fn activity(
    ui: &mut egui::Ui,
    net: &mut crate::desktop::net::Net,
    task_id: &str,
    task: &Value,
    delegate: Option<&Value>,
    private: bool,
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
    shell::section(ui, "Activity");

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
        if ready && box_.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
            send = true;
        }
        if let Some(err) = &local.note_error {
            ui.add_space(space::XS);
            w::error(ui, err);
        }
        // Only once something is typed: an empty box with a dead button under
        // it was half the section's height for nothing.
        if !local.note.trim().is_empty() || local.posting_note {
            ui.add_space(space::SM);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let label = if local.posting_note { "Posting\u{2026}" } else { "Post" };
                if w::primary(ui, label, ready).clicked() {
                    send = true;
                }
            });
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
    ui.add_space(space::LG);

    if let Some(err) = net.error(NOTES_KEY) {
        failed(ui, "Could not load activity", err);
        return;
    }
    let plans = net.shared(PLANS_KEY);
    let plans: &[Value] = plans.as_deref().and_then(Value::as_array).map_or(&[], Vec::as_slice);
    session::feed(
        ui,
        &session::Feed { task_id, task, delegate, notes: all, plans, private },
    );
}

// --------------------------------------------------------------------- shared

fn failed(ui: &mut egui::Ui, what: &str, err: &str) {
    w::error(ui, &format!("{what}: {err}"));
}

/// `s` cut to `width`, with an ellipsis where it was cut. The cut point is
/// estimated from the full string's measure rather than fitted glyph by glyph;
/// the whole value is on the hover text either way.
pub(super) fn elide(ui: &egui::Ui, s: &str, width: f32) -> String {
    let font = egui::FontId::proportional(text::SMALL);
    let full = ui
        .painter()
        .layout_no_wrap(s.to_owned(), font, colour::TEXT())
        .size()
        .x;
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
        Ok(t) => t
            .with_timezone(&chrono::Local)
            .format("%-d %b %Y, %H:%M")
            .to_string(),
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

pub(super) fn day_time(raw: &str) -> String {
    let Ok(t) = DateTime::parse_from_rfc3339(raw) else {
        return String::new();
    };
    let local = t.with_timezone(&LocalTz);
    if local.date_naive() == LocalTz::now().date_naive() {
        ago(raw)
    } else {
        local.format("%H:%M").to_string()
    }
}

/// "Today", "Yesterday", or "Mon 22 Sep", in local time — a quiet separator
/// for a list that reads top to bottom by time.
pub(super) fn day_label(raw: &str) -> String {
    let Ok(t) = DateTime::parse_from_rfc3339(raw) else {
        return String::new();
    };
    let day = t.with_timezone(&LocalTz).date_naive();
    let today = LocalTz::now().date_naive();
    match (today - day).num_days() {
        0 => "Today".to_owned(),
        1 => "Yesterday".to_owned(),
        _ => day.format("%a %-d %b").to_string(),
    }
}

#[cfg(test)]
mod link_label_tests {
    use super::{link_label, png};

    #[test]
    fn names_commits_and_prs() {
        assert_eq!(
            link_label("https://github.com/airtribe-live/mycohort-api/commit/6f54d7dd1e2a").as_deref(),
            Some("mycohort-api \u{b7} 6f54d7d")
        );
        assert_eq!(
            link_label("https://github.com/airtribe-live/mycohort-api/pull/4821").as_deref(),
            Some("mycohort-api #4821")
        );
        assert_eq!(link_label("https://figma.com/design/abc"), None);
    }

    #[test]
    fn a_clipboard_image_becomes_a_png() {
        let image = arboard::ImageData { width: 2, height: 1, bytes: vec![255, 0, 0, 255, 0, 0, 255, 255].into() };
        let decoded = image::load_from_memory(&png(image).unwrap().0).unwrap().to_rgba8();
        assert_eq!((decoded.width(), decoded.height()), (2, 1));
        assert_eq!(decoded.get_pixel(1, 0).0, [0, 0, 255, 255]);
        let short = arboard::ImageData { width: 4, height: 4, bytes: vec![0; 3].into() };
        assert!(png(short).is_err());
    }
}
