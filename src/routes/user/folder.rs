use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::controllers::folder::Folder;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

#[derive(Deserialize)]
pub struct AddBody {
    pub name: String,
    pub path: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/folders", get(list).post(add))
        .route("/api/user/folders/{id}", delete(remove))
        .route("/api/user/folders/{id}/default", post(set_default))
}

async fn list(
    State(state): State<AppState>,
    caller: Caller,
) -> AppResult<ApiResponse<Vec<Folder>>> {
    caller.require("read")?;
    let me = caller.person_id()?;
    Ok(ApiResponse::ok(
        controllers::folder::list(&state, me).await?,
    ))
}

async fn add(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<AddBody>,
) -> AppResult<ApiResponse<Folder>> {
    caller.require("read")?;
    let me = caller.person_id()?;
    Ok(ApiResponse::ok(
        controllers::folder::add(&state, me, &body.name, &body.path).await?,
    ))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<serde_json::Value>> {
    caller.require("read")?;
    let me = caller.person_id()?;
    controllers::folder::remove(&state, me, id).await?;
    Ok(ApiResponse::ok(
        serde_json::json!({ "id": id, "deleted": true }),
    ))
}

async fn set_default(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Folder>> {
    caller.require("read")?;
    let me = caller.person_id()?;
    Ok(ApiResponse::ok(
        controllers::folder::set_default(&state, me, id).await?,
    ))
}
