use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::task::{Task, TASK_COLUMNS};

/// Claims are operational state, not editorial state: they say who is holding a
/// task right now, not what the task *is*. So, deliberately, nothing in this
/// module writes a `change` row. This is the one exception to "every mutation
/// writes a change" — it is not an oversight.
const CLAIMABLE_WHERE: &str = "assignee_kind = 'agent'
     AND status IN ('open', 'in_progress')
     AND (claimed_by IS NULL OR claim_expires_at < now())";

pub async fn claimable(state: &AppState, limit: i64) -> AppResult<Vec<Task>> {
    let tasks = sqlx::query_as(&format!(
        "SELECT {TASK_COLUMNS} FROM task
         WHERE {CLAIMABLE_WHERE}
         ORDER BY priority, created_at
         LIMIT $1"
    ))
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(tasks)
}

/// Claims `task_id`, or the next claimable task when `None`. The `SELECT ... FOR
/// UPDATE SKIP LOCKED` and the `UPDATE` share one transaction, so two workers
/// polling at the same instant can never be handed the same row: the loser skips
/// the locked row rather than blocking on it.
pub async fn claim(
    state: &AppState,
    caller_label: &str,
    task_id: Option<Uuid>,
) -> AppResult<Option<Task>> {
    let mut tx = state.db.begin().await?;

    let picked: Option<Uuid> = sqlx::query_scalar(&format!(
        "SELECT id FROM task
         WHERE ($1::uuid IS NULL OR id = $1)
           AND {CLAIMABLE_WHERE}
         ORDER BY priority, created_at
         FOR UPDATE SKIP LOCKED
         LIMIT 1"
    ))
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(id) = picked else {
        return Ok(None);
    };

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task
         SET claimed_by = $2,
             claim_expires_at = now() + interval '5 minutes',
             status = 'in_progress',
             updated_at = now()
         WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(caller_label)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(task))
}

/// Extends the lease by another 5 minutes. Only the holder may do this — a stale
/// worker must not be able to keep a task alive after someone else took it.
pub async fn heartbeat(state: &AppState, caller_label: &str, task_id: Uuid) -> AppResult<Task> {
    let task: Task = sqlx::query_as(&format!(
        "UPDATE task
         SET claim_expires_at = now() + interval '5 minutes', updated_at = now()
         WHERE id = $1 AND claimed_by = $2 RETURNING {TASK_COLUMNS}"
    ))
    .bind(task_id)
    .bind(caller_label)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(not_holder)?;

    Ok(task)
}

/// Drops the lease and returns the task to `open` so another worker can take it.
pub async fn release(state: &AppState, caller_label: &str, task_id: Uuid) -> AppResult<Task> {
    let task: Task = sqlx::query_as(&format!(
        "UPDATE task
         SET claimed_by = NULL, claim_expires_at = NULL, status = 'open', updated_at = now()
         WHERE id = $1 AND claimed_by = $2 RETURNING {TASK_COLUMNS}"
    ))
    .bind(task_id)
    .bind(caller_label)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(not_holder)?;

    Ok(task)
}

/// The update matched no row: the task is gone, or someone else holds the lease.
/// Both answers are "not yours", so both are 403.
fn not_holder() -> AppError {
    AppError::Forbidden("you do not hold the lease on this task".into())
}
