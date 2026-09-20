use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::Router;
use uuid::Uuid;

use crate::controllers::approval::{self, ChangeRow};
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/changes/pending", get(pending))
        .route("/api/user/changes/{id}/approve", post(approve))
        .route("/api/user/changes/{id}/reject", post(reject))
}

async fn pending(
    State(state): State<AppState>,
    caller: Caller,
) -> AppResult<ApiResponse<Vec<ChangeRow>>> {
    // Only an approver should see the queue they are expected to act on.
    caller.require("write")?;
    Ok(ApiResponse::ok(approval::list_pending(&state).await?))
}

async fn approve(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<ChangeRow>> {
    caller.require("write")?;
    Ok(ApiResponse::ok(approval::approve(&state, &caller.actor, id).await?))
}

async fn reject(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<ChangeRow>> {
    caller.require("write")?;
    Ok(ApiResponse::ok(approval::reject(&state, &caller.actor, id).await?))
}
