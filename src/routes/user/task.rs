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
use crate::models::change::Outcome;
use crate::models::task::{Task, TaskFilter, TaskRow};
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
    #[serde(default)]
    pub discipline: Option<String>,
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
pub struct DisciplineBody {
    /// `null` clears the label.
    pub discipline: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockersBody {
    pub blocked_by: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    pub discipline: Option<String>,
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
        // The two literal paths are declared before `{id}` for a human reader;
        // the router prefers a static segment over a parameter regardless.
        .route("/api/user/tasks/mine", get(mine))
        .route("/api/user/tasks/available", get(available))
        .route("/api/user/tasks/{id}", get(one).patch(update))
        .route("/api/user/tasks/{id}/assign", post(assign))
        .route("/api/user/tasks/{id}/claim", post(claim))
        .route("/api/user/tasks/{id}/release", post(release))
        .route("/api/user/tasks/{id}/blockers", patch(blockers))
        .route("/api/user/tasks/{id}/discipline", patch(discipline))
}

async fn create(
    State(state): State<AppState>,
    Path(phase_id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<CreateTaskBody>,
) -> AppResult<ApiResponse<Outcome<Task>>> {
    caller.can_mutate()?;
    let task = controllers::task::create(
        &state, &caller.actor, phase_id, body.title, body.body, body.priority, body.discipline,
    )
    .await?;
    Ok(ApiResponse::ok(task))
}

async fn search(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<TaskQuery>,
) -> AppResult<ApiResponse<Vec<TaskRow>>> {
    caller.require("read")?;
    let filter = TaskFilter {
        discipline: q.discipline,
        project_id: q.project_id,
        phase_id: q.phase_id,
        status: q.status,
        assignee_email: q.assignee_email,
        assignee_kind: q.assignee_kind,
    };
    Ok(ApiResponse::ok(controllers::task::all(&state, filter).await?))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<UpdateTaskBody>,
) -> AppResult<ApiResponse<Outcome<Task>>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::task::set_status(&state, &caller.actor, id, body.status).await?))
}

async fn assign(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<AssignBody>,
) -> AppResult<ApiResponse<Outcome<Task>>> {
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

async fn one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<TaskRow>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::task::get(&state, id).await?))
}

async fn mine(
    State(state): State<AppState>,
    caller: Caller,
) -> AppResult<ApiResponse<Vec<TaskRow>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::task::mine(&state, caller.person_id()?).await?))
}

async fn available(
    State(state): State<AppState>,
    caller: Caller,
) -> AppResult<ApiResponse<Vec<TaskRow>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::task::available(&state, caller.person_id()?).await?))
}

async fn claim(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Outcome<Task>>> {
    caller.can_mutate()?;
    let person_id = caller.person_id()?;
    Ok(ApiResponse::ok(controllers::task::claim(&state, &caller.actor, id, person_id).await?))
}

async fn release(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Outcome<Task>>> {
    caller.can_mutate()?;
    let person_id = caller.person_id()?;
    Ok(ApiResponse::ok(controllers::task::release(&state, &caller.actor, id, person_id).await?))
}

async fn blockers(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<BlockersBody>,
) -> AppResult<ApiResponse<Outcome<Task>>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(
        controllers::task::set_blockers(&state, &caller.actor, id, body.blocked_by).await?,
    ))
}

async fn discipline(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<DisciplineBody>,
) -> AppResult<ApiResponse<Outcome<Task>>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(
        controllers::task::set_discipline(&state, &caller.actor, id, body.discipline).await?,
    ))
}
