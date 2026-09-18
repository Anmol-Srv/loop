use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::models::change::Actor;
use crate::models::project::Project;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBody {
    pub key: String,
    pub name: String,
}

// P0 has no authentication. The P1 plan replaces this with the authenticated
// caller extracted from the request.
fn system_actor() -> Actor {
    Actor { label: "system".into(), person_id: None, can_apply: true }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/user/projects", post(create).get(list))
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateProjectBody>,
) -> AppResult<ApiResponse<Project>> {
    let project = controllers::project::create(&state, &system_actor(), body.key, body.name).await?;
    Ok(ApiResponse::ok(project))
}

async fn list(State(state): State<AppState>) -> AppResult<ApiResponse<Vec<Project>>> {
    let projects = controllers::project::list(&state).await?;
    Ok(ApiResponse::ok(projects))
}
