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
}

/// A task made on its own: standalone, or with `projectId` into that
/// project's first phase.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTaskBody {
    #[serde(flatten)]
    pub task: controllers::project::NewTask,
    #[serde(default)]
    pub project_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskBody {
    pub status: String,
    /// Why the work is finished with nothing to point at. Only read on a
    /// transition that would otherwise need evidence.
    #[serde(default)]
    pub manual_reason: Option<String>,
    /// The status the mover saw. When it is no longer true the move is
    /// refused with a 409 rather than undoing whoever got there first.
    #[serde(default)]
    pub expected_status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignBody {
    pub person_email: Option<String>,
    pub agent_label: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockersBody {
    pub blocked_by: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    /// Filters on the assignee's department, since that is what a task's
    /// discipline now is. The param keeps its old name: callers ask for a
    /// discipline and that is still what they get back.
    #[serde(alias = "discipline")]
    pub department: Option<String>,
    pub project_id: Option<Uuid>,
    pub phase_id: Option<Uuid>,
    pub status: Option<String>,
    pub assignee_email: Option<String>,
    pub assignee_kind: Option<String>,
    /// Only archived tasks. Without it, only live ones.
    #[serde(default)]
    pub archived: bool,
}

/// `?archived=true` on a list: only what is archived.
#[derive(Deserialize)]
pub struct ArchivedQuery {
    #[serde(default)]
    pub archived: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/phases/{phase_id}/tasks", post(create))
        .route("/api/user/tasks", get(search).post(create_direct))
        // The two literal paths are declared before `{id}` for a human reader;
        // the router prefers a static segment over a parameter regardless.
        .route("/api/user/tasks/mine", get(mine))
        .route("/api/user/tasks/{id}", get(one).patch(update).delete(remove))
        .route("/api/user/tasks/{id}/archive", post(archive))
        .route("/api/user/tasks/{id}/restore", post(restore))
        .route("/api/user/tasks/{id}/assign", post(assign))
        .route("/api/user/tasks/{id}/details", patch(details))
        .route("/api/user/tasks/{id}/claim", post(claim))
        .route("/api/user/tasks/{id}/release", post(release))
        .route("/api/user/tasks/{id}/blockers", patch(blockers))
        .route("/api/user/tasks/{id}/accept", post(accept))
        .route("/api/user/tasks/{id}/dismiss", post(dismiss))
        .route("/api/user/tracks", get(tracks))
}

/// The transition table, so a client offers only the moves the server takes.
async fn tracks(caller: Caller) -> AppResult<ApiResponse<serde_json::Value>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(crate::models::task::tracks_table()))
}

async fn create(
    State(state): State<AppState>,
    Path(phase_id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<CreateTaskBody>,
) -> AppResult<ApiResponse<Outcome<Task>>> {
    caller.can_mutate()?;
    let task = controllers::task::create(
        &state, &caller.actor, Some(phase_id), None, body.title, body.body, body.priority,
    )
    .await?;
    Ok(ApiResponse::ok(task))
}

async fn create_direct(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<NewTaskBody>,
) -> AppResult<ApiResponse<TaskRow>> {
    caller.can_mutate()?;
    let task = controllers::project::add_task(&state, &caller.actor, body.project_id, body.task).await?;
    Ok(ApiResponse::ok(controllers::task::get(&state, task.id, caller.actor.person_id).await?))
}

async fn search(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<TaskQuery>,
) -> AppResult<ApiResponse<Vec<TaskRow>>> {
    caller.require("read")?;
    let filter = TaskFilter {
        department: q.department,
        project_id: q.project_id,
        phase_id: q.phase_id,
        status: q.status,
        assignee_email: q.assignee_email,
        assignee_kind: q.assignee_kind,
        archived: q.archived,
    };
    Ok(ApiResponse::ok(controllers::task::all(&state, filter, caller.actor.person_id).await?))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<UpdateTaskBody>,
) -> AppResult<ApiResponse<Outcome<Task>>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(
        controllers::task::set_status(
            &state,
            &caller.actor,
            id,
            body.status,
            body.manual_reason,
            body.expected_status,
        )
        .await?,
    ))
}

/// Title, description, priority and assignee. Any writer may change these;
/// status stays with the assignee and has its own route.
async fn details(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<controllers::task::TaskDetails>,
) -> AppResult<ApiResponse<Outcome<Task>>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(
        controllers::task::update_details(&state, &caller.actor, id, body).await?,
    ))
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
    Ok(ApiResponse::ok(controllers::task::get(&state, id, caller.actor.person_id).await?))
}

async fn mine(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<ArchivedQuery>,
) -> AppResult<ApiResponse<Vec<TaskRow>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::task::mine(&state, caller.person_id()?, q.archived).await?))
}

async fn archive(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<TaskRow>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::task::set_archived(&state, &caller.actor, id, true).await?))
}

async fn restore(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<TaskRow>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::task::set_archived(&state, &caller.actor, id, false).await?))
}

async fn remove(State(state): State<AppState>, Path(id): Path<Uuid>, caller: Caller) -> AppResult<ApiResponse<serde_json::Value>> {
    caller.can_mutate()?;
    controllers::task::delete(&state, &caller.actor, id).await?;
    Ok(ApiResponse::ok(serde_json::json!({ "id": id, "deleted": true })))
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


#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AcceptBody {
    /// Move it into this project's first phase; absent keeps it where it is.
    #[serde(default)]
    pub project_id: Option<Uuid>,
}

#[derive(Deserialize, Default)]
pub struct DismissBody {
    #[serde(default)]
    pub reason: Option<String>,
}

/// A body that may be left off entirely: a plain "Accept" sends nothing.
fn optional_body<T: serde::de::DeserializeOwned + Default>(bytes: &[u8]) -> AppResult<T> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(bytes).map_err(|e| AppError::BadRequest(format!("the body is not valid JSON: {e}")))
}

/// Triage is the owner's call, made in the app: a signed-in person only.
fn session(caller: &Caller) -> AppResult<()> {
    if caller.kind != "session" {
        return Err(AppError::Forbidden("only a signed-in person can accept or dismiss triage".into()));
    }
    Ok(())
}

async fn accept(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    body: axum::body::Bytes,
) -> AppResult<ApiResponse<TaskRow>> {
    session(&caller)?;
    let project = optional_body::<AcceptBody>(&body)?.project_id;
    Ok(ApiResponse::ok(controllers::task::triage(&state, &caller.actor, id, true, project, None).await?))
}

async fn dismiss(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    body: axum::body::Bytes,
) -> AppResult<ApiResponse<TaskRow>> {
    session(&caller)?;
    let reason = optional_body::<DismissBody>(&body)?.reason;
    Ok(ApiResponse::ok(controllers::task::triage(&state, &caller.actor, id, false, None, reason).await?))
}
