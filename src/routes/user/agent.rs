//! A person's own agents, and handing their tasks to one.
//!
//! Signed-in people only: an agent cannot mint a sibling, and only the person
//! a task is assigned to hands it off, answers its agent or reviews its work.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{delete, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers::agent::{self, Agent, Minted};
use crate::controllers::note::Note;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::auth::Caller;
use crate::models::task::TaskRow;
use crate::response::ApiResponse;
use crate::routes::agent::server_url;

#[derive(Deserialize)]
pub struct CreateBody {
    pub handle: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_runtime")]
    pub runtime: String,
}

fn default_runtime() -> String {
    "other".into()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/agents", post(create).get(list))
        .route("/api/user/agents/{id}", delete(revoke))
        .route("/api/user/agents/{id}/rotate", post(rotate))
        .route("/api/user/tasks/{id}/handoff", post(hand_off))
        .route("/api/user/tasks/{id}/takeback", post(take_back))
        .route("/api/user/tasks/{id}/answer", post(answer))
        .route("/api/user/tasks/{id}/review", post(review))
}

/// The person behind a session. An agent carries its owner's person id, so
/// that alone never refused an agent — the credential kind does. Without it,
/// an agent could mint itself a sibling.
fn person(caller: &Caller) -> AppResult<Uuid> {
    if caller.kind != "session" {
        return Err(AppError::Forbidden("only a signed-in person can do this".into()));
    }
    caller.person_id()
}

async fn create(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> AppResult<ApiResponse<Minted>> {
    let owner = person(&caller)?;
    Ok(ApiResponse::ok(
        agent::create(&state, owner, &body.handle, &body.name, &body.runtime, &server_url(&headers)).await?,
    ))
}

async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<Agent>>> {
    Ok(ApiResponse::ok(agent::list(&state, person(&caller)?).await?))
}

async fn rotate(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    headers: HeaderMap,
) -> AppResult<ApiResponse<Minted>> {
    Ok(ApiResponse::ok(agent::rotate(&state, person(&caller)?, id, &server_url(&headers)).await?))
}

async fn revoke(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Agent>> {
    Ok(ApiResponse::ok(agent::revoke(&state, person(&caller)?, id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandOffBody {
    pub agent_id: Uuid,
}

async fn hand_off(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<HandOffBody>,
) -> AppResult<ApiResponse<TaskRow>> {
    Ok(ApiResponse::ok(agent::hand_off(&state, person(&caller)?, id, body.agent_id).await?))
}

async fn take_back(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<TaskRow>> {
    Ok(ApiResponse::ok(agent::take_back(&state, person(&caller)?, id).await?))
}

#[derive(Deserialize)]
pub struct AnswerBody {
    pub body: String,
}

async fn answer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<AnswerBody>,
) -> AppResult<ApiResponse<Note>> {
    Ok(ApiResponse::ok(agent::answer(&state, person(&caller)?, id, &body.body).await?))
}

#[derive(Deserialize)]
pub struct ReviewBody {
    /// approve or changes.
    pub decision: String,
    #[serde(default)]
    pub body: Option<String>,
}

async fn review(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<ReviewBody>,
) -> AppResult<ApiResponse<TaskRow>> {
    let me = person(&caller)?;
    let approve = match body.decision.as_str() {
        "approve" => true,
        "changes" => false,
        _ => return Err(AppError::BadRequest("decision is 'approve' or 'changes'".into())),
    };
    Ok(ApiResponse::ok(
        agent::review(&state, &caller.actor, me, id, approve, body.body.as_deref()).await?,
    ))
}
