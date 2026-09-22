use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::controllers;
use crate::controllers::label::Label;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelBody {
    pub name: String,
    #[serde(default)]
    pub colour: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/user/labels", get(list).post(create))
}

async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<Label>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::label::list(&state).await?))
}

/// Idempotent: creating a label that already exists returns the existing one,
/// because the form lets you type a new label inline and typing one that
/// exists should attach it rather than fail.
async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<LabelBody>,
) -> AppResult<ApiResponse<Label>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(
        controllers::label::ensure(&state, body.name, body.colour).await?,
    ))
}
