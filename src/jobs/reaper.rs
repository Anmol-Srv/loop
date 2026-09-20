//! Frees tasks whose lease lapsed.
//!
//! A claim is a lease, not a lock: there is no unlock command, so a laptop that
//! sleeps or dies must not strand its task. This sweep is what makes that true.

use crate::db::AppState;
use crate::errors::AppResult;

/// Release every task whose lease has expired. Returns how many were freed.
pub async fn sweep(state: &AppState) -> AppResult<u64> {
    let result = sqlx::query(
        "UPDATE task
            SET claimed_by = NULL,
                claim_expires_at = NULL,
                status = 'open',
                updated_at = now()
          WHERE claimed_by IS NOT NULL
            AND claim_expires_at < now()",
    )
    .execute(&state.db)
    .await?;

    Ok(result.rows_affected())
}
