//! Postgres-backed job queue.
//!
//! There is no Redis here. A `job` table plus `LISTEN`/`NOTIFY` covers what we
//! need, keeps jobs transactional with the data that enqueued them, and leaves
//! the queue inspectable with plain SQL.

pub mod reaper;
pub mod worker;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;

/// Enqueue a job. The `NOTIFY` rides in the same transaction as the insert, so
/// a job is never announced before it is visible to whoever wakes up.
pub async fn enqueue(
    db: &PgPool,
    kind: &str,
    payload: Value,
    run_after: Option<DateTime<Utc>>,
) -> AppResult<Uuid> {
    let mut tx = db.begin().await?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO job (kind, payload, run_after)
         VALUES ($1, $2, COALESCE($3, now()))
         RETURNING id",
    )
    .bind(kind)
    .bind(payload)
    .bind(run_after)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("SELECT pg_notify('acp_jobs', $1)")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(id)
}
