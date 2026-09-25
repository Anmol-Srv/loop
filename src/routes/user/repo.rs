use axum::extract::{Path, State};
use axum::routing::{get, patch, put};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::controllers::repo::Repo;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

#[derive(Deserialize)]
pub struct AddBody {
    pub name: String,
    pub url: String,
}

#[derive(Deserialize)]
pub struct EditBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// `null` or `""` clears the folder.
#[derive(Deserialize)]
pub struct PathBody {
    #[serde(default)]
    pub path: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/projects/{id}/repos", get(list).post(add))
        .route("/api/user/repos/{id}", patch(edit).delete(remove))
        .route("/api/user/repos/{id}/path", put(set_path))
}

async fn list(State(state): State<AppState>, Path(id): Path<Uuid>, caller: Caller) -> AppResult<ApiResponse<Vec<Repo>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::repo::list(&state, caller.actor.person_id, id).await?))
}

async fn add(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<AddBody>,
) -> AppResult<ApiResponse<Repo>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::repo::add(&state, &caller.actor, id, &body.name, &body.url).await?))
}

async fn edit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<EditBody>,
) -> AppResult<ApiResponse<Repo>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(
        controllers::repo::update(&state, &caller.actor, id, body.name.as_deref(), body.url.as_deref()).await?,
    ))
}

async fn remove(State(state): State<AppState>, Path(id): Path<Uuid>, caller: Caller) -> AppResult<ApiResponse<serde_json::Value>> {
    caller.can_mutate()?;
    controllers::repo::remove(&state, &caller.actor, id).await?;
    Ok(ApiResponse::ok(serde_json::json!({ "id": id, "deleted": true })))
}

async fn set_path(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<PathBody>,
) -> AppResult<ApiResponse<Repo>> {
    caller.require("read")?;
    let me = caller.person_id()?;
    Ok(ApiResponse::ok(controllers::repo::set_path(&state, me, id, body.path.as_deref()).await?))
}
