use axum::extract::{Path, Query, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::run_log::RunLogLine;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendBody {
    pub lines: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadQuery {
    #[serde(default)]
    pub after_seq: i64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/tasks/{id}/logs", post(append).get(read))
}

async fn append(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<AppendBody>,
) -> AppResult<ApiResponse<i64>> {
    caller.require("claim")?;
    let last_seq = controllers::run_log::append(&state, &caller.actor.label, id, body.lines).await?;
    Ok(ApiResponse::ok(last_seq))
}

async fn read(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Query(q): Query<ReadQuery>,
) -> AppResult<ApiResponse<Vec<RunLogLine>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::run_log::read(&state, id, q.after_seq).await?))
}
