use axum::extract::State;
use axum::routing::{get, patch};
use axum::{Json, Router};

use crate::controllers;
use crate::controllers::people::PersonRow;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/people", get(list))
        .route("/api/user/people/me", patch(update_me))
}

async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<PersonRow>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::people::list(&state).await?))
}

/// Nothing on a profile is self-service any more. The department used to be,
/// and a person switching tracks moved their own shipped work into a track
/// that has no such state — so it is an admin action now
/// (`PATCH /api/admin/people/{id}`, `acp-admin set-department`). The route
/// stays so an old client hears why rather than a bare 405.
async fn update_me(
    caller: Caller,
    Json(body): Json<serde_json::Map<String, serde_json::Value>>,
) -> AppResult<ApiResponse<PersonRow>> {
    caller.require("read")?;
    Err(AppError::BadRequest(if body.contains_key("department") {
        "a department is set by an admin; ask one to move you".into()
    } else {
        "nothing on your profile can be changed here".into()
    }))
}
