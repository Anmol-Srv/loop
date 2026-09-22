use axum::extract::State;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::Deserialize;

use crate::controllers;
use crate::controllers::people::PersonRow;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepartmentBody {
    pub department: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/people", get(list))
        .route("/api/user/people/me", patch(set_department))
}

async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<PersonRow>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::people::list(&state).await?))
}

/// `read` is the right scope here: the only row you can touch is your own, and
/// choosing what you work on is not an edit to the board.
async fn set_department(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<DepartmentBody>,
) -> AppResult<ApiResponse<PersonRow>> {
    caller.require("read")?;
    let person_id = caller.person_id()?;
    Ok(ApiResponse::ok(
        controllers::people::set_department(&state, person_id, body.department).await?,
    ))
}
