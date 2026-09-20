use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::run_log::RunLogLine;

/// Run logs are operational output, not editorial state, so appending does
/// **not** write a `change` row. Like claims, this is a deliberate exception to
/// "every mutation writes a change" — a log line records what an agent did, it
/// does not alter anything a human would review.
///
/// `seq` is allocated server-side under a per-task advisory lock, never
/// supplied by the caller. The lock serialises concurrent appenders for the
/// duration of the transaction, so two writers cannot read the same `MAX(seq)`
/// and collide on `UNIQUE (task_id, seq)`.
// ponytail: one lock per task, taken for the length of a single INSERT. If a
// single task ever has enough concurrent appenders for that to hurt, move to a
// per-task sequence counter column; a retry loop is not the upgrade path.
pub async fn append(
    state: &AppState,
    caller_label: &str,
    task_id: Uuid,
    lines: Vec<String>,
) -> AppResult<i64> {
    if lines.is_empty() {
        return Err(AppError::BadRequest("lines must not be empty".into()));
    }

    let mut tx = state.db.begin().await?;

    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1::text))")
        .bind(task_id.to_string())
        .execute(&mut *tx)
        .await?;

    // Only the worker currently holding the lease may write to the log.
    let held: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM task WHERE id = $1 AND claimed_by = $2 AND claim_expires_at > now()",
    )
    .bind(task_id)
    .bind(caller_label)
    .fetch_optional(&mut *tx)
    .await?;

    if held.is_none() {
        let exists: Option<i32> = sqlx::query_scalar("SELECT 1 FROM task WHERE id = $1")
            .bind(task_id)
            .fetch_optional(&mut *tx)
            .await?;
        return Err(match exists {
            Some(_) => AppError::Forbidden("you do not hold this task's lease".into()),
            None => AppError::NotFound("task not found".into()),
        });
    }

    let last_seq: i64 = sqlx::query_scalar(
        "INSERT INTO run_log_line (task_id, seq, text)
         SELECT $1,
                (SELECT COALESCE(MAX(seq), 0) FROM run_log_line WHERE task_id = $1) + ord,
                line
         FROM unnest($2::text[]) WITH ORDINALITY AS l(line, ord)
         RETURNING seq",
    )
    .bind(task_id)
    .bind(&lines)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .max()
    .unwrap_or(0);

    tx.commit().await?;
    Ok(last_seq)
}

pub async fn read(state: &AppState, task_id: Uuid, after_seq: i64) -> AppResult<Vec<RunLogLine>> {
    let lines = sqlx::query_as(
        "SELECT id, task_id, seq, text, created_at FROM run_log_line
         WHERE task_id = $1 AND seq > $2 ORDER BY seq",
    )
    .bind(task_id)
    .bind(after_seq)
    .fetch_all(&state.db)
    .await?;

    Ok(lines)
}
