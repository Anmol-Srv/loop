use axum::extract::{Path, Query, State};
use axum::routing::{delete, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::artifact::Artifact;
use crate::models::change::Outcome;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddArtifactBody {
    pub parent_type: String,
    pub parent_id: Uuid,
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactQuery {
    pub parent_type: String,
    pub parent_id: Uuid,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/artifacts", post(add).get(list))
        .route("/api/user/artifacts/{id}", delete(remove))
}

async fn add(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<AddArtifactBody>,
) -> AppResult<ApiResponse<Outcome<Artifact>>> {
    caller.can_mutate()?;
    let artifact = controllers::artifact::add(
        &state, &caller.actor, body.parent_type, body.parent_id, body.kind, body.url, body.title,
    ).await?;
    Ok(ApiResponse::ok(artifact))
}

async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<ArtifactQuery>,
) -> AppResult<ApiResponse<Vec<Artifact>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::artifact::list(&state, q.parent_type, q.parent_id).await?))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Outcome<Artifact>>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::artifact::remove(&state, &caller.actor, id).await?))
}
