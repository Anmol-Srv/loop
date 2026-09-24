use uuid::Uuid;

use crate::db::AppState;
use crate::errors::AppResult;
use crate::models::run_log::RunLogLine;

/// Run logs were written by agents holding a lease. Leases are gone — agents
/// report through notes now — so what is here stays readable and nothing new
/// is appended.
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
