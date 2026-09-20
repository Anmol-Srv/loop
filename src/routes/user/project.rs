use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::change::Outcome;
use crate::controllers::project::ProjectProgress;
use crate::models::project::Project;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBody {
    pub key: String,
    pub name: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/projects", post(create).get(list))
        .route("/api/user/projects/{id}/flow", get(flow))
}

async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<CreateProjectBody>,
) -> AppResult<ApiResponse<Outcome<Project>>> {
    caller.can_mutate()?;
    let project = controllers::project::create(&state, &caller.actor, body.key, body.name).await?;
    Ok(ApiResponse::ok(project))
}

async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<Project>>> {
    caller.require("read")?;
    let projects = controllers::project::list(&state).await?;
    Ok(ApiResponse::ok(projects))
}

/// The flow strip: done/total for the project and for each discipline in it.
///
/// A separate endpoint rather than a wider `/phases`, because the strip is
/// about disciplines and `/phases` is about phases — two shapes in one
/// response would have made both clients unpack something they did not want.
async fn flow(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<ProjectProgress>> {
    caller.require("read")?;
    controllers::project::progress(&state, Some(id))
        .await?
        .pop()
        .map(ApiResponse::ok)
        .ok_or_else(|| crate::errors::AppError::NotFound("project not found".into()))
}
