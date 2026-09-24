use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::run_log::RunLogLine;
use crate::models::task::sees_agent_private;

/// An agent's step log on a task: what it ran and saw, line by line. Written
/// by the agent holding the task (`agent::log`); read only by the task's
/// owner or an admin, like the rest of the agent's private side.
pub async fn read(
    state: &AppState,
    task_id: Uuid,
    viewer: Option<Uuid>,
    after_seq: i64,
) -> AppResult<Vec<RunLogLine>> {
    let allowed: bool = sqlx::query_scalar(&format!(
        "SELECT {} FROM task t WHERE t.id = $1",
        sees_agent_private("$2")
    ))
    .bind(task_id)
    .bind(viewer)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;
    if !allowed {
        return Err(AppError::Forbidden(
            "only the person this task is assigned to, or an admin, can read its agent's step log".into(),
        ));
    }

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
