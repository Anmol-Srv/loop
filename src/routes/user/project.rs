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
use crate::controllers::project::{NewTask, ProjectProgress};
use crate::models::task::Task;
use crate::models::project::Project;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBody {
    /// Optional: the form does not ask for one, and the controller derives it
    /// from the name. The CLI still passes an explicit key.
    #[serde(default)]
    pub key: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Linear's project properties. Every one is optional; a project with a
    /// name is a project.
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub target_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub label_ids: Vec<Uuid>,
    /// Work handed out in the same breath as the project is made.
    #[serde(default)]
    pub tasks: Vec<NewTask>,
}

fn default_priority() -> i32 {
    2
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/projects", post(create).get(list))
        .route("/api/user/projects/{id}", get(show).patch(update))
        .route("/api/user/projects/{id}/flow", get(flow))
        .route("/api/user/projects/{id}/tasks", post(add_task))
}

async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<CreateProjectBody>,
) -> AppResult<ApiResponse<Outcome<Project>>> {
    caller.can_mutate()?;
    let project = controllers::project::create(
        &state,
        &caller.actor,
        body.key,
        body.name,
        body.description,
        body.priority,
        body.start_date,
        body.target_date,
        body.label_ids,
        body.tasks,
    )
    .await?;
    Ok(ApiResponse::ok(project))
}

async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<Project>>> {
    caller.require("read")?;
    let projects = controllers::project::list(&state).await?;
    Ok(ApiResponse::ok(projects))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<controllers::project::ProjectPatch>,
) -> AppResult<ApiResponse<Outcome<Project>>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::project::update(&state, &caller.actor, id, body).await?))
}

async fn show(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Project>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::project::get(&state, id).await?))
}

async fn add_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<NewTask>,
) -> AppResult<ApiResponse<Task>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::project::add_task(&state, &caller.actor, id, body).await?))
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
