//! The agent page's payload, and the route any agent reports a run on.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;
use uuid::Uuid;

use crate::controllers::agent_overview::{self, Run, RunReport};
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/agents/{id}/overview", get(overview))
        .route("/api/agent/runs", post(report))
}

async fn overview(State(state): State<AppState>, Path(id): Path<Uuid>, caller: Caller) -> AppResult<ApiResponse<Value>> {
    if caller.kind != "session" {
        return Err(AppError::Forbidden("only a signed-in person can read an agent's page".into()));
    }
    Ok(ApiResponse::ok(agent_overview::overview(&state, caller.person_id()?, id).await?))
}

async fn report(State(state): State<AppState>, caller: Caller, Json(body): Json<RunReport>) -> AppResult<ApiResponse<Run>> {
    Ok(ApiResponse::ok(agent_overview::report(&state, caller.agent()?, body).await?))
}
