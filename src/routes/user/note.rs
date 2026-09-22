use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::controllers::note::Note;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

#[derive(Deserialize)]
pub struct NoteBody {
    pub body: String,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/user/tasks/{id}/notes", get(list).post(add))
}

async fn list(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Vec<Note>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::note::list(&state, id).await?))
}

/// `read` is the right scope: a note changes nothing on the board, and an
/// agent that can read a task should be able to say what it found.
async fn add(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<NoteBody>,
) -> AppResult<ApiResponse<Note>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(
        controllers::note::add(&state, caller.actor.person_id, id, body.body).await?,
    ))
}
