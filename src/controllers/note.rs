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
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const NOTE_COLUMNS: &str = "n.id, n.task_id, n.author_id, p.name AS author_name, n.body, n.created_at";

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
    let body = body.trim();
    if body.is_empty() {
        return Err(AppError::BadRequest("a note needs something in it".into()));
    }

    let id: Uuid = sqlx::query_scalar("INSERT INTO note (task_id, author_id, body) VALUES ($1, $2, $3) RETURNING id")
        .bind(task_id)
        .bind(author_id)
        .bind(body)
        .fetch_one(&state.db)
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
    .fetch_one(&state.db)
    .await
    .map_err(Into::into)
}
