//! A person's notifications: what happened on their work, newest first.
//! Written by database triggers (migration 20261007000027); read and marked
//! read here.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/notifications", get(list))
        .route("/api/user/notifications/read", post(read))
}

/// The newest 50, with the task's title and project, and how many are unread.
async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Value>> {
    caller.require("read")?;
    let me = caller.person_id()?;
    let items: Value = sqlx::query_scalar(
        "SELECT coalesce(json_agg(x ORDER BY x.id DESC), '[]') FROM (
           SELECT n.id, n.kind, n.task_id AS \"taskId\", t.title AS \"taskTitle\", pr.name AS \"projectName\",
                  n.actor, n.detail, n.created_at AS \"createdAt\", n.read_at IS NULL AS unread
             FROM notification n
             JOIN task t ON t.id = n.task_id
             LEFT JOIN phase ph ON ph.id = t.phase_id
             LEFT JOIN project pr ON pr.id = ph.project_id
            WHERE n.person_id = $1
            ORDER BY n.id DESC LIMIT 50) x",
    )
    .bind(me)
    .fetch_one(&state.db)
    .await?;
    let unread: i64 = sqlx::query_scalar("SELECT count(*) FROM notification WHERE person_id = $1 AND read_at IS NULL")
        .bind(me)
        .fetch_one(&state.db)
        .await?;
    Ok(ApiResponse::ok(json!({ "items": items, "unread": unread })))
}

#[derive(Deserialize, Default)]
pub struct ReadBody {
    /// These ones; every unread one when absent.
    #[serde(default)]
    pub ids: Option<Vec<i64>>,
}

async fn read(State(state): State<AppState>, caller: Caller, Json(b): Json<ReadBody>) -> AppResult<ApiResponse<Value>> {
    caller.require("read")?;
    let me = caller.person_id()?;
    let marked = sqlx::query(
        "UPDATE notification SET read_at = now()
          WHERE person_id = $1 AND read_at IS NULL AND ($2::bigint[] IS NULL OR id = ANY($2))",
    )
    .bind(me)
    .bind(b.ids)
    .execute(&state.db)
    .await?
    .rows_affected();
    Ok(ApiResponse::ok(json!({ "marked": marked })))
}
