//! The action menus, one per kind of thing. A row's right-click and the
//! page's "⋯" draw the same one, so what the two offer cannot drift apart.
//!
//! What the viewer may not do is still listed, greyed, with the reason on
//! hover: a missing item teaches nobody that it exists.

use serde_json::{json, Value};

use super::agents::AGENTS_KEY;
use super::board::{after_archive, archived, str_at, Act};
use crate::desktop::design::{space, viz, widgets as w};
use crate::desktop::net::Net;
use crate::desktop::App;

/// What was picked. Copies are done on the spot and never come back.
pub(super) enum Pick {
    Open,
    Act(Act),
    Handoff(String, String),
    TakeBack,
    Priority(i64),
    /// A project-less task's pinned folder, by name — `None` to use the
    /// viewer's default instead.
    Folder(Option<String>),
    /// Out of triage: into the intake project it was filed in (`None`), or
    /// moved into another (id, name).
    Accept(Option<(String, String)>),
    /// Out of triage, dropped, with a reason asked for first.
    Dismiss,
}

/// Who is looking, as far as a menu needs to know.
#[derive(Clone)]
pub(super) struct Viewer {
    pub me: String,
    pub can_write: bool,
    pub admin: bool,
    /// The viewer's agents that can still take work: (id, name).
    pub agents: Vec<(String, String)>,
    /// Projects a triaged task may be accepted into: (id, name). Empty until
    /// a view with triage on it has asked for them.
    pub projects: Vec<(String, String)>,
    /// The viewer's own folders, for a project-less task's Folder submenu.
    /// Empty until a view that offers it has asked (`settings::want_folders`).
    pub folders: Vec<Value>,
}

impl Viewer {
    pub(super) fn of(app: &mut App) -> Self {
        let can_write = app.can_write();
        let net = app.net.as_mut().expect("net is live whenever a view runs");
        net.get_once(AGENTS_KEY, "/api/user/agents");
        let me = net
            .data("__me")
            .map(|m| str_at(m, "personId").to_owned())
            .unwrap_or_default();
        let admin = net
            .data("__me")
            .is_some_and(|m| str_at(m, "role") == "admin");
        let projects = super::triage::projects(net);
        let folders = super::settings::folders(net);
        let agents = net
            .data(AGENTS_KEY)
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter(|a| super::agents::takes_work(a))
                    .map(|a| (str_at(a, "id").to_owned(), str_at(a, "name").to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            me,
            can_write,
            admin,
            agents,
            projects,
            folders,
        }
    }
}

const READ_ONLY: &str = "Your access is read-only.";

/// A project's menu. `row` adds Open, which on the project's own page would
/// go nowhere.
pub(super) fn project_items(
    ui: &mut egui::Ui,
    p: &Value,
    can_write: bool,
    row: bool,
) -> Option<Pick> {
    let mut pick = None;
    if row && viz::menu_item(ui, "Open", false, None) {
        pick = Some(Pick::Open);
    }
    copy(ui, "Copy name", str_at(p, "name"));
    copy(ui, "Copy ID", str_at(p, "id"));
    viz::menu_rule(ui);
    pick.or(lifecycle(ui, p, can_write, "project", None))
}

/// A task's menu: the same hand-off the task page offers, its priority, the
/// copies, then archive and delete.
pub(super) fn task_items(ui: &mut egui::Ui, t: &Value, viewer: &Viewer, row: bool) -> Option<Pick> {
    let mut pick = None;
    if row && viz::menu_item(ui, "Open", false, None) {
        pick = Some(Pick::Open);
    }
    if str_at(t, "status") == "triage" {
        if let Some(p) = triage_items(ui, t, viewer) {
            pick = Some(p);
        }
        viz::menu_rule(ui);
    }
    // Only the assignee hands off or takes back — the task page's rule.
    if !viewer.me.is_empty() && str_at(t, "assigneePersonId") == viewer.me {
        if held(t) {
            if viz::menu_item(ui, "Take back", false, None) {
                pick = Some(Pick::TakeBack);
            }
        } else {
            match viewer.agents.as_slice() {
                [] => {}
                _ if finished(t) => {
                    viz::menu_item(
                        ui,
                        "Hand off",
                        false,
                        Some("This task is finished \u{2014} there is nothing left to hand off."),
                    );
                }
                [(id, name)] => {
                    if viz::menu_item(ui, &format!("Hand off to {name}"), false, None) {
                        pick = Some(Pick::Handoff(id.clone(), name.clone()));
                    }
                }
                many => viz::submenu(ui, "Hand off to", None, |ui| {
                    for (id, name) in many {
                        if viz::menu_item(ui, name, false, None) {
                            pick = Some(Pick::Handoff(id.clone(), name.clone()));
                        }
                    }
                }),
            }
        }
    }
    let current = t.get("priority").and_then(Value::as_i64);
    viz::submenu(
        ui,
        "Priority",
        (!viewer.can_write).then_some(READ_ONLY),
        |ui| {
            for p in 0..=4 {
                if viz::menu_choice(ui, &format!("P{p}"), current == Some(p)) && current != Some(p)
                {
                    pick = Some(Pick::Priority(p));
                }
            }
        },
    );
    // Only for a task with no project: its own repo already says where to
    // work.
    if str_at(t, "projectId").is_empty()
        && (!viewer.folders.is_empty() || t.get("folderName").is_some_and(|v| !v.is_null()))
    {
        let pinned = t.get("folderName").and_then(Value::as_str);
        let default_label = match super::settings::default_name(&viewer.folders) {
            Some(d) => format!("Use default ({d})"),
            None => "Use default".to_owned(),
        };
        viz::submenu(
            ui,
            "Folder",
            (!viewer.can_write).then_some(READ_ONLY),
            |ui| {
                if pinned.is_some() && viz::menu_choice(ui, &default_label, false) {
                    pick = Some(Pick::Folder(None));
                }
                for f in &viewer.folders {
                    let name = str_at(f, "name");
                    if viz::menu_choice(ui, name, pinned == Some(name)) && pinned != Some(name) {
                        pick = Some(Pick::Folder(Some(name.to_owned())));
                    }
                }
            },
        );
    }
    copy(ui, "Copy title", str_at(t, "title"));
    copy(ui, "Copy ID", str_at(t, "id"));
    viz::menu_rule(ui);
    // A task archived along with its project has no Restore of its own.
    let with_project = t.get("projectArchivedAt").is_some_and(|v| !v.is_null());
    let why = with_project
        .then_some("Archived with its project \u{2014} restoring the project brings it back.");
    pick.or(lifecycle(ui, t, viewer.can_write, "task", why))
}

/// Accept, Accept into, Dismiss — for a task an intake agent filed. Greyed,
/// with the rule on hover, for anyone but its owner or an admin.
fn triage_items(ui: &mut egui::Ui, t: &Value, viewer: &Viewer) -> Option<Pick> {
    let mine = !viewer.me.is_empty() && str_at(t, "assigneePersonId") == viewer.me;
    let why = (!mine && !viewer.admin).then_some(super::triage::NOT_YOURS);
    let mut pick = None;
    if viz::menu_item(ui, "Accept", false, why) {
        pick = Some(Pick::Accept(None));
    }
    let here = str_at(t, "projectId");
    let others: Vec<&(String, String)> = viewer
        .projects
        .iter()
        .filter(|(id, _)| id != here)
        .collect();
    let why_into = why.or(others
        .is_empty()
        .then_some("No other project to move it into."));
    viz::submenu(ui, "Accept into", why_into, |ui| {
        for (id, name) in others {
            if viz::menu_item(ui, name, false, None) {
                pick = Some(Pick::Accept(Some((id.clone(), name.clone()))));
            }
        }
    });
    if viz::menu_item(ui, "Dismiss\u{2026}", false, why) {
        pick = Some(Pick::Dismiss);
    }
    pick
}

/// An agent still holds it: it can be taken back. The delegate stays on a
/// finished task as history.
pub(super) fn held(t: &Value) -> bool {
    t.get("delegate")
        .filter(|d| d.is_object())
        .is_some_and(|d| !matches!(str_at(d, "state"), "done" | "stopped"))
}

pub(super) fn finished(t: &Value) -> bool {
    t.get("doneAt").is_some_and(|v| !v.is_null()) || str_at(t, "status") == "dropped"
}

fn copy(ui: &mut egui::Ui, label: &str, text: &str) {
    if viz::menu_item(ui, label, false, None) {
        ui.ctx().copy_text(text.to_owned());
        w::toast(ui.ctx(), "Copied.", false);
    }
}

/// Archive or Restore, then Delete — the group that changes what exists, so
/// it comes last, under a rule.
fn lifecycle(
    ui: &mut egui::Ui,
    v: &Value,
    can_write: bool,
    what: &str,
    restore_why: Option<&str>,
) -> Option<Pick> {
    let mut pick = None;
    let (act, label, verb) = if archived(v) {
        (Act::Restore, "Restore", "restore")
    } else {
        (Act::Archive, "Archive\u{2026}", "archive")
    };
    let why = restore_why
        .filter(|_| act == Act::Restore)
        .map(str::to_owned)
        .or_else(|| refusal(v, can_write, verb, what));
    if viz::menu_item(ui, label, false, why.as_deref()) {
        pick = Some(Pick::Act(act));
    }
    if viz::menu_item(
        ui,
        "Delete\u{2026}",
        true,
        refusal(v, can_write, "delete", what).as_deref(),
    ) {
        pick = Some(Pick::Act(Act::Delete));
    }
    pick
}

/// Why the viewer may not `verb` it, or `None` if they may. The rows do not
/// carry who created the thing, so the sentence names the rule, not the
/// person; the server's refusal names them.
fn refusal(v: &Value, can_write: bool, verb: &str, what: &str) -> Option<String> {
    if !can_write {
        return Some(READ_ONLY.to_owned());
    }
    let flag = if verb == "delete" {
        "canDelete"
    } else {
        "canArchive"
    };
    if v.get(flag).and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    Some(match what {
        "project" => format!("Only whoever created this project, or an admin, can {verb} it."),
        _ => format!("Only whoever created this task or its project, or an admin, can {verb} it."),
    })
}

// ------------------------------------------------------------ task actions

/// Not under `task:`: a success sweeps that prefix, and the reply has to be
/// read first.
const KEY: &str = "tasks:action";

/// A task archive or delete waiting on a yes.
#[derive(Clone)]
struct Ask {
    id: String,
    title: String,
    act: Act,
}

/// The task actions a menu starts, from any row or the task page: the one
/// waiting on a yes, and the one sent.
#[derive(Default)]
pub struct Tasks {
    ask: Option<Ask>,
    /// A dismiss waiting on its (optional) reason: id, title, what is typed.
    dismiss: Option<(String, String, String)>,
    /// The task a triage decision is out for, so its row can say so.
    pub(super) deciding: Option<String>,
    /// What to say when the reply lands, and the task's id if it deletes it.
    sent: Option<(String, Option<String>)>,
}

impl Tasks {
    pub(super) fn busy(&self) -> bool {
        self.sent.is_some()
    }

    /// Act on a pick from `t`'s menu. Open is the caller's: only it knows
    /// where it is.
    pub(super) fn pick(&mut self, net: &mut Net, t: &Value, pick: Pick) {
        let id = str_at(t, "id").to_owned();
        let path = format!("/api/user/tasks/{id}");
        net.invalidate(KEY);
        let done = match pick {
            Pick::Open => return,
            Pick::Act(act @ (Act::Archive | Act::Delete)) => {
                self.ask = Some(Ask {
                    id,
                    title: str_at(t, "title").to_owned(),
                    act,
                });
                return;
            }
            // Undone as easily as it was done, so it does not ask.
            Pick::Act(Act::Restore) => {
                net.post(KEY, &format!("{path}/restore"), json!({}));
                "Restored.".to_owned()
            }
            Pick::Handoff(agent, name) => {
                net.post(KEY, &format!("{path}/handoff"), json!({ "agentId": agent }));
                format!("Handed off to {name}.")
            }
            Pick::TakeBack => {
                net.post(KEY, &format!("{path}/takeback"), json!({}));
                "Taken back \u{2014} the agent no longer has this task.".to_owned()
            }
            Pick::Dismiss => {
                self.dismiss = Some((id, str_at(t, "title").to_owned(), String::new()));
                return;
            }
            Pick::Accept(into) => {
                let body = match &into {
                    Some((project, _)) => json!({ "projectId": project }),
                    None => json!({}),
                };
                net.post(KEY, &format!("{path}/accept"), body);
                self.deciding = Some(id);
                match into {
                    Some((_, name)) => format!("Accepted into {name}."),
                    None => "Accepted \u{2014} it is an open task now.".to_owned(),
                }
            }
            Pick::Priority(p) => {
                // Against the task as the row showed it, like every details edit.
                let at = str_at(t, "updatedAt");
                let mut body = json!({ "priority": p });
                if !at.is_empty() {
                    body["expectedUpdatedAt"] = json!(at);
                }
                net.patch(KEY, &format!("{path}/details"), body);
                format!("Priority set to P{p}.")
            }
            Pick::Folder(name) => {
                let at = str_at(t, "updatedAt");
                let mut body = json!({ "folderName": name });
                if !at.is_empty() {
                    body["expectedUpdatedAt"] = json!(at);
                }
                net.patch(KEY, &format!("{path}/details"), body);
                match &name {
                    Some(n) => format!("Folder set to {n}."),
                    None => "Folder set to your default.".to_owned(),
                }
            }
        };
        self.sent = Some((done, None));
    }

    fn send(&mut self, net: &mut Net, a: Ask) {
        let path = format!("/api/user/tasks/{}", a.id);
        net.invalidate(KEY);
        match a.act {
            Act::Delete => net.send(KEY, reqwest::Method::DELETE, &path, Value::Null),
            _ => net.post(KEY, &format!("{path}/archive"), json!({})),
        }
        self.sent = Some(match a.act {
            Act::Delete => ("Deleted.".to_owned(), Some(a.id)),
            _ => ("Archived.".to_owned(), None),
        });
    }
}

/// Fold in the reply to a task action and ask for the yes one needs. Once a
/// frame, from the shell, so the dialog sits over whichever page asked.
/// Returns the id of a task just deleted, so its page can leave.
pub(super) fn settle(ctx: &egui::Context, net: &mut Net, s: &mut Tasks) -> Option<String> {
    let mut gone = None;
    if let Some((done, deletes)) = s.sent.take_if(|_| !net.is_loading(KEY)) {
        s.deciding = None;
        match net.peek(KEY) {
            Some(Ok(v)) if str_at(v, "status") == "proposed" => w::toast(
                ctx,
                "Awaiting approval: you do not hold write on this project, so it was recorded as a proposed change.",
                false,
            ),
            Some(Ok(_)) => {
                w::toast(ctx, done, false);
                gone = deletes;
            }
            Some(Err(e)) => w::toast(ctx, e.clone(), true),
            None => {}
        }
        net.invalidate(KEY);
        after_archive(net);
        net.invalidate(AGENTS_KEY);
    }

    dismiss_dialog(ctx, net, s);
    let Some(a) = s.ask.clone() else { return gone };
    let (mut go, mut close) = (false, false);
    let delete = a.act == Act::Delete;
    let modal = super::agents::dialog(ctx, "task:confirm", super::agents::DIALOG_W * 0.8, |ui| {
        let verb = if delete { "Delete" } else { "Archive" };
        super::agents::heading(ui, &format!("{verb} \u{201c}{}\u{201d}?", a.title));
        ui.add_space(space::XS);
        w::muted(
            ui,
            if delete {
                "Its notes, run log and links go with it. This cannot be undone \u{2014} archive it instead to keep it."
            } else {
                "It leaves the board, Home and everyone\u{2019}s task lists. Restore it from its menu any time."
            },
        );
        ui.add_space(space::XL);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            go = if delete {
                w::danger(ui, "Delete task", true)
            } else {
                w::primary(ui, "Archive", true)
            }
            .clicked();
            close = w::ghost(ui, "Cancel").clicked();
        });
    });
    if go {
        s.ask = None;
        s.send(net, a);
    } else if close || modal.should_close() {
        s.ask = None;
    }
    gone
}

/// "Dismiss this?" with room for why. The reason is optional and lands on the
/// task as a note; Enter in the box dismisses, Escape leaves it in triage.
fn dismiss_dialog(ctx: &egui::Context, net: &mut Net, s: &mut Tasks) {
    let Some((id, title, reason)) = s.dismiss.as_mut() else {
        return;
    };
    let (mut go, mut close) = (false, false);
    let modal = super::agents::dialog(ctx, "task:dismiss", super::agents::DIALOG_W * 0.8, |ui| {
        super::agents::heading(ui, &format!("Dismiss \u{201c}{title}\u{201d}?"));
        ui.add_space(space::XS);
        w::muted(
            ui,
            "It leaves triage as dropped. The agent will not file this message again.",
        );
        ui.add_space(space::LG);
        let field = w::field(
            ui,
            "Reason (optional)",
            reason,
            false,
            "Already fixed, not ours, duplicate\u{2026}",
        );
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            go = true;
        }
        ui.add_space(space::XL);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            go |= w::primary(ui, "Dismiss", true).clicked();
            close = w::ghost(ui, "Cancel").clicked();
        });
    });
    if go {
        let reason = reason.trim();
        let body = if reason.is_empty() {
            json!({})
        } else {
            json!({ "reason": reason })
        };
        net.invalidate(KEY);
        net.post(KEY, &format!("/api/user/tasks/{id}/dismiss"), body);
        s.deciding = Some(std::mem::take(id));
        s.sent = Some(("Dismissed.".to_owned(), None));
        s.dismiss = None;
    } else if close || modal.should_close() {
        s.dismiss = None;
    }
}
