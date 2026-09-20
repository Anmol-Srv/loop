use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::task::Task;
use crate::response::ApiResponse;

fn default_limit() -> i64 {
    20
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimableQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaimBody {
    pub task_id: Option<Uuid>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/work/claimable", get(claimable))
        .route("/api/user/work/claim", post(claim))
        .route("/api/user/work/{id}/heartbeat", post(heartbeat))
        .route("/api/user/work/{id}/release", post(release))
}

async fn claimable(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<ClaimableQuery>,
) -> AppResult<ApiResponse<Vec<Task>>> {
    caller.require("claim")?;
    Ok(ApiResponse::ok(controllers::work::claimable(&state, q.limit).await?))
}

async fn claim(
    State(state): State<AppState>,
    caller: Caller,
    body: Option<Json<ClaimBody>>,
) -> AppResult<ApiResponse<Option<Task>>> {
    caller.require("claim")?;
    let task_id = body.and_then(|Json(b)| b.task_id);
    // caller_label, never a client-supplied one: a worker cannot claim as someone else.
    Ok(ApiResponse::ok(controllers::work::claim(&state, &caller.actor.label, task_id).await?))
}

async fn heartbeat(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Task>> {
    caller.require("claim")?;
    Ok(ApiResponse::ok(controllers::work::heartbeat(&state, &caller.actor.label, id).await?))
}

async fn release(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Task>> {
    caller.require("claim")?;
    Ok(ApiResponse::ok(controllers::work::release(&state, &caller.actor.label, id).await?))
}
