//! Notes: the team talking on a task.
//!
//! Deliberately not a `change` row. The `change` table is the audit trail for
//! things that alter the board — a note alters nothing, and putting comments
//! through the approval machinery would mean an agent's question to a
//! teammate needed approving before anyone could read it.

use serde::Serialize;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: Uuid,
    pub task_id: Uuid,
    pub author_id: Option<Uuid>,
    /// Resolved here so a client never has to join the roster to render a
    /// thread. `None` when an agent wrote it.
    pub author_name: Option<String>,
    pub body: String,
    /// note, progress, question, answer, submission or review. Everything a
    /// person types is a `note` unless it answers or reviews their agent.
    pub kind: String,
    /// `{id, name}` when an agent wrote it.
    pub agent: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const NOTE_COLUMNS: &str = "n.id, n.task_id, n.author_id, p.name AS author_name, n.body, n.kind,
    (SELECT json_build_object('id', ag.id, 'name', ag.name) FROM agent ag WHERE ag.id = n.agent_id) AS agent,
    n.created_at";

pub async fn list(state: &AppState, task_id: Uuid) -> AppResult<Vec<Note>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {NOTE_COLUMNS} FROM note n
           LEFT JOIN person p ON p.id = n.author_id
          WHERE n.task_id = $1 ORDER BY n.created_at"
    ))
    .bind(task_id)
    .fetch_all(&state.db)
    .await?)
}

/// Anyone on the team can write on any task. Notes are how a blocker gets
/// unblocked, so restricting them to the assignee would defeat the point.
pub async fn add(
    state: &AppState,
    author_id: Option<Uuid>,
    task_id: Uuid,
    body: String,
) -> AppResult<Note> {
    let mut tx = state.db.begin().await?;
    let note = insert(&mut tx, task_id, Author::Person(author_id), "note", &body).await?;
    tx.commit().await?;
    Ok(note)
}

/// Who wrote a note: a person (or nobody, for a credential tied to no one),
/// or an agent.
pub enum Author {
    Person(Option<Uuid>),
    Agent(Uuid),
}

/// A note, written inside the caller's transaction — agent routes and owner
/// reviews write one alongside the state change it explains.
pub async fn insert(
    tx: &mut sqlx::PgTransaction<'_>,
    task_id: Uuid,
    author: Author,
    kind: &str,
    body: &str,
) -> AppResult<Note> {
    let body = body.trim();
    if body.is_empty() {
        return Err(AppError::BadRequest("a note needs something in it".into()));
    }
    let (author_id, agent_id) = match author {
        Author::Person(p) => (p, None),
        Author::Agent(a) => (None, Some(a)),
    };

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO note (task_id, author_id, agent_id, kind, body) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(task_id)
    .bind(author_id)
    .bind(agent_id)
    .bind(kind)
    .bind(body)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::NotFound("task not found".into())
        }
        _ => AppError::Database(e),
    })?;

    sqlx::query_as(&format!(
        "SELECT {NOTE_COLUMNS} FROM note n
           LEFT JOIN person p ON p.id = n.author_id
          WHERE n.id = $1"
    ))
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .map_err(Into::into)
}
