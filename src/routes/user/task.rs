use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::controllers::task::Assignee;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::auth::Caller;
use crate::models::task::{Task, TaskFilter};
use crate::response::ApiResponse;

fn default_priority() -> i32 {
    2
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskBody {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskBody {
    pub status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignBody {
    pub person_email: Option<String>,
    pub agent_label: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    pub project_id: Option<Uuid>,
    pub phase_id: Option<Uuid>,
    pub status: Option<String>,
    pub assignee_email: Option<String>,
    pub assignee_kind: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/phases/{phase_id}/tasks", post(create))
        .route("/api/user/tasks", get(search))
        .route("/api/user/tasks/{id}", patch(update))
        .route("/api/user/tasks/{id}/assign", post(assign))
}

async fn create(
    State(state): State<AppState>,
    Path(phase_id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<CreateTaskBody>,
) -> AppResult<ApiResponse<Task>> {
    caller.can_mutate()?;
    let task = controllers::task::create(&state, &caller.actor, phase_id, body.title, body.body, body.priority).await?;
    Ok(ApiResponse::ok(task))
}

async fn search(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<TaskQuery>,
) -> AppResult<ApiResponse<Vec<Task>>> {
    caller.require("read")?;
    let filter = TaskFilter {
        project_id: q.project_id,
        phase_id: q.phase_id,
        status: q.status,
        assignee_email: q.assignee_email,
        assignee_kind: q.assignee_kind,
    };
    Ok(ApiResponse::ok(controllers::task::search(&state, filter).await?))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<UpdateTaskBody>,
) -> AppResult<ApiResponse<Task>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::task::set_status(&state, &caller.actor, id, body.status).await?))
}

async fn assign(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<AssignBody>,
) -> AppResult<ApiResponse<Task>> {
    caller.can_mutate()?;

    let to = match (body.person_email, body.agent_label) {
        (Some(_), Some(_)) => {
            return Err(AppError::BadRequest("give either personEmail or agentLabel, not both".into()))
        }
        (Some(email), None) => Assignee::Person(email),
        (None, Some(label)) => Assignee::Agent(label),
        (None, None) => Assignee::Nobody,
    };

    Ok(ApiResponse::ok(controllers::task::assign(&state, &caller.actor, id, to).await?))
}
