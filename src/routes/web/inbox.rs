//! The approval inbox: what agents have proposed, in plain language, with the
//! two buttons that resolve it.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::Value;
use uuid::Uuid;

use crate::controllers::approval::{self, ChangeRow};
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::session::WebCaller;
use crate::routes::web::auth::page;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/inbox", get(inbox))
        .route("/inbox/{id}/approve", post(approve))
        .route("/inbox/{id}/reject", post(reject))
}

/// One pending change, already turned into the words a reviewer reads.
pub struct Card {
    pub id: Uuid,
    pub actor: String,
    pub on_behalf_of: String,
    pub sentence: String,
    pub target_type: String,
    pub target: String,
}

#[derive(Template)]
#[template(path = "inbox.html")]
struct InboxTemplate {
    cards: Vec<Card>,
    can_write: bool,
}

async fn inbox(State(state): State<AppState>, WebCaller(caller): WebCaller) -> AppResult<Response> {
    let mut changes = approval::list_pending(&state).await?;
    changes.reverse(); // the controller orders oldest-first; the inbox reads newest-first

    let labels = labels_for(&state, &changes).await?;
    let label = |id: Option<Uuid>| id.and_then(|i| labels.get(&i)).cloned().unwrap_or_default();

    let cards = changes
        .iter()
        .map(|c| Card {
            id: c.id,
            actor: c.actor.clone(),
            on_behalf_of: label(c.on_behalf_of),
            sentence: describe(c, &label(context_id(c))),
            target_type: c.target_type.clone(),
            target: label(Some(c.target_id)),
        })
        .collect();

    Ok(page(&InboxTemplate {
        cards,
        can_write: caller.has("write"),
    }))
}

async fn approve(
    State(state): State<AppState>,
    WebCaller(caller): WebCaller,
    Path(id): Path<Uuid>,
) -> AppResult<Redirect> {
    caller.require("write")?;
    approval::approve(&state, &caller.actor, id).await?;
    Ok(Redirect::to("/inbox"))
}

async fn reject(
    State(state): State<AppState>,
    WebCaller(caller): WebCaller,
    Path(id): Path<Uuid>,
) -> AppResult<Redirect> {
    caller.require("write")?;
    approval::reject(&state, &caller.actor, id).await?;
    Ok(Redirect::to("/inbox"))
}

/// The entity whose name makes the sentence readable. For an update that is the
/// target itself; for a create the target id is a placeholder, so it is the
/// parent the new row would hang off.
fn context_id(change: &ChangeRow) -> Option<Uuid> {
    let key = match (change.target_type.as_str(), change.op.as_str()) {
        ("project", "create") => return None,
        ("phase", "create") => "project_id",
        ("task", "create") => "phase_id",
        ("artifact", "create") => "parent_id",
        _ => return Some(change.target_id),
    };
    change.patch.get(key).and_then(Value::as_str).and_then(|s| s.parse().ok())
}

/// Display names for every id a card mentions, in one round trip.
///
/// ponytail: a presentation-only lookup, so it lives here rather than growing a
/// controller. If a second screen needs it, move it into one.
async fn labels_for(state: &AppState, changes: &[ChangeRow]) -> AppResult<HashMap<Uuid, String>> {
    let ids: Vec<Uuid> = changes
        .iter()
        .flat_map(|c| [Some(c.target_id), c.on_behalf_of, context_id(c)])
        .flatten()
        .collect();

    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name  FROM project  WHERE id = ANY($1)
         UNION ALL SELECT id, name  FROM phase    WHERE id = ANY($1)
         UNION ALL SELECT id, title FROM task     WHERE id = ANY($1)
         UNION ALL SELECT id, title FROM artifact WHERE id = ANY($1)
         UNION ALL SELECT id, email FROM person   WHERE id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&state.db)
    .await?;

    Ok(rows.into_iter().collect())
}

/// Turn a recorded proposal into the sentence a reviewer can decide on.
///
/// `context` is the display name of the entity the change hangs off — the task
/// being updated, or the phase a new task would join. It may be empty, and an
/// unrecognised patch shape falls back to something honest rather than
/// panicking: this is prose, never a source of truth.
pub fn describe(change: &ChangeRow, context: &str) -> String {
    let p = &change.patch;
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let here = |fallback: &str| {
        if context.is_empty() {
            fallback.to_string()
        } else {
            format!("\"{context}\"")
        }
    };

    match (change.target_type.as_str(), change.op.as_str()) {
        ("project", "create") => format!("Create project \"{}\" with key {}.", s("name"), s("key")),
        ("phase", "create") => format!(
            "Add phase \"{}\" at position {} to project {}.",
            s("name"),
            p.get("position").and_then(Value::as_i64).unwrap_or(0),
            here("an unnamed project"),
        ),
        ("phase", "update") => format!("Move phase {} to {}.", here("(unknown)"), s("status")),
        ("task", "create") => format!(
            "Add task \"{}\" to phase {}.",
            s("title"),
            here("an unnamed phase"),
        ),
        ("task", "update") => {
            let task = here("(unknown task)");
            if p.get("status").is_some() {
                format!("Move task {task} to {}.", s("status"))
            } else if let Some(agent) = p.get("agent_label").and_then(Value::as_str) {
                format!("Assign task {task} to agent {agent}.")
            } else if let Some(email) = p.get("person_email").and_then(Value::as_str) {
                format!("Assign task {task} to {email}.")
            } else if p.get("person_email").is_some() || p.get("agent_label").is_some() {
                format!("Unassign task {task}.")
            } else {
                unrecognised(change, context)
            }
        }
        ("artifact", "create") => format!(
            "Attach {} \"{}\" ({}) to {} {}.",
            s("kind"),
            s("title"),
            s("url"),
            s("parent_type"),
            here("(unknown)"),
        ),
        _ => unrecognised(change, context),
    }
}

fn unrecognised(change: &ChangeRow, context: &str) -> String {
    let what = if context.is_empty() {
        change.target_type.clone()
    } else {
        format!("{} \"{}\"", change.target_type, context)
    };
    format!("Unrecognised {} on {what}: {}", change.op, change.patch)
}
