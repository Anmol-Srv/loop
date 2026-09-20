//! The job loop.
//!
//! Claims jobs with the same `FOR UPDATE SKIP LOCKED` shape the work queue
//! uses, so two server instances can run safely. Waits on `LISTEN acp_jobs`
//! with a timeout: the notification makes new work immediate, and the timeout
//! is what lets `run_after` scheduling work without polling tightly.

use std::time::Duration;

use sqlx::postgres::PgListener;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::AppResult;
use crate::jobs;

const LEASE_SWEEP: &str = "lease_sweep";
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ATTEMPTS: i32 = 5;

/// Run one job if any is due. Returns the kind that ran, so a test can drive
/// the loop a step at a time instead of spawning it and sleeping.
pub async fn step(state: &AppState) -> AppResult<Option<String>> {
    let mut tx = state.db.begin().await?;

    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, kind FROM job
          WHERE run_after <= now() AND locked_by IS NULL
          ORDER BY run_after
          FOR UPDATE SKIP LOCKED
          LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some((id, kind)) = row else {
        tx.commit().await?;
        return Ok(None);
    };

    sqlx::query("UPDATE job SET locked_by = 'worker', locked_at = now(), attempts = attempts + 1 WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let outcome = run(state, &kind).await;

    match outcome {
        Ok(()) => {
            sqlx::query("DELETE FROM job WHERE id = $1").bind(id).execute(&state.db).await?;
        }
        Err(e) => {
            tracing::error!(job = %id, kind = %kind, error = %e, "job failed");
            // Back off and let it retry, until it has clearly stopped working.
            sqlx::query(
                "UPDATE job
                    SET locked_by = NULL, locked_at = NULL,
                        run_after = now() + (attempts * interval '10 seconds')
                  WHERE id = $1 AND attempts < $2",
            )
            .bind(id)
            .bind(MAX_ATTEMPTS)
            .execute(&state.db)
            .await?;
        }
    }

    Ok(Some(kind))
}

async fn run(state: &AppState, kind: &str) -> AppResult<()> {
    match kind {
        LEASE_SWEEP => {
            let freed = jobs::reaper::sweep(state).await?;
            if freed > 0 {
                tracing::info!(freed, "released lapsed leases");
            }
            // Recurring: schedule the next sweep before finishing this one.
            jobs::enqueue(
                &state.db,
                LEASE_SWEEP,
                serde_json::json!({}),
                Some(chrono::Utc::now() + chrono::Duration::seconds(60)),
            )
            .await?;
            Ok(())
        }
        other => Err(crate::errors::AppError::Internal(format!("unknown job kind '{other}'"))),
    }
}

/// Ensure exactly one sweep is scheduled. Safe to call on every boot.
pub async fn ensure_lease_sweep(state: &AppState) -> AppResult<()> {
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM job WHERE kind = $1")
        .bind(LEASE_SWEEP)
        .fetch_one(&state.db)
        .await?;

    if pending == 0 {
        jobs::enqueue(&state.db, LEASE_SWEEP, serde_json::json!({}), None).await?;
    }
    Ok(())
}

/// The loop. Spawned from `main`; ends when `shutdown` flips.
pub async fn run_loop(state: AppState, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    let mut listener = match PgListener::connect_with(&state.db).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, "job worker could not listen; not starting");
            return;
        }
    };

    if let Err(e) = listener.listen("acp_jobs").await {
        tracing::error!(error = %e, "job worker could not subscribe; not starting");
        return;
    }

    loop {
        // Drain everything due before going back to sleep.
        loop {
            match step(&state).await {
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(e) => {
                    tracing::error!(error = %e, "job step failed");
                    break;
                }
            }
        }

        tokio::select! {
            _ = shutdown.changed() => {
                tracing::info!("job worker shutting down");
                return;
            }
            _ = listener.recv() => {}
            _ = tokio::time::sleep(IDLE_TIMEOUT) => {}
        }
    }
}
