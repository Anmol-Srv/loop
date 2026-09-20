use axum::extract::{Path, State};
use axum::routing::{patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::change::Outcome;
use crate::models::phase::Phase;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePhaseBody {
    pub name: String,
    pub position: i32,
    #[serde(default)]
    pub gate: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePhaseBody {
    pub status: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/projects/{project_id}/phases", post(create).get(list))
        .route("/api/user/phases/{id}", patch(update))
}

async fn create(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<CreatePhaseBody>,
) -> AppResult<ApiResponse<Outcome<Phase>>> {
    caller.can_mutate()?;
    let phase = controllers::phase::create(&state, &caller.actor, project_id, body.name, body.position, body.gate).await?;
    Ok(ApiResponse::ok(phase))
}

async fn list(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Vec<Phase>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::phase::list(&state, project_id).await?))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<UpdatePhaseBody>,
) -> AppResult<ApiResponse<Outcome<Phase>>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::phase::set_status(&state, &caller.actor, id, body.status).await?))
}
