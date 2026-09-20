use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::project::Project;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBody {
    pub key: String,
    pub name: String,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/user/projects", post(create).get(list))
}

async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<CreateProjectBody>,
) -> AppResult<ApiResponse<Project>> {
    caller.can_mutate()?;
    let project = controllers::project::create(&state, &caller.actor, body.key, body.name).await?;
    Ok(ApiResponse::ok(project))
}

async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<Project>>> {
    caller.require("read")?;
    let projects = controllers::project::list(&state).await?;
    Ok(ApiResponse::ok(projects))
}
