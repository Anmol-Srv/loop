use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::session::WebCaller;
use crate::models::project::Project;
use crate::models::task::{Task, TaskFilter, TASK_STATUSES};
use crate::routes::web::auth::page;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(board))
        .route("/projects/{id}", get(project))
        // Same handler: the HX-Request header picks the template.
        .route("/projects/{id}/tasks", get(project))
}

/// One phase and how far through it the work is.
#[derive(Clone, sqlx::FromRow)]
struct PhaseProgress {
    project_id: Uuid,
    phase_id: Uuid,
    name: String,
    status: String,
    done: i64,
    total: i64,
}

/// Progress for every phase, or for one project's phases, in a single grouped
/// query — never one query per phase.
async fn phase_progress(state: &AppState, project_id: Option<Uuid>) -> AppResult<Vec<PhaseProgress>> {
    let rows = sqlx::query_as(
        "SELECT p.project_id, p.id AS phase_id, p.name, p.status,
                COUNT(t.id) AS total,
                COUNT(t.id) FILTER (WHERE t.status = 'done') AS done
         FROM phase p
         LEFT JOIN task t ON t.phase_id = p.id
         WHERE ($1::uuid IS NULL OR p.project_id = $1)
         GROUP BY p.id
         ORDER BY p.project_id, p.position",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;

    Ok(rows)
}

struct ProjectRow {
    project: Project,
    phases: Vec<PhaseProgress>,
}

#[derive(Template)]
#[template(path = "board.html")]
struct BoardTemplate {
    projects: Vec<ProjectRow>,
}

async fn board(State(state): State<AppState>, WebCaller(caller): WebCaller) -> AppResult<Response> {
    caller.require("read")?;

    let projects = controllers::project::list(&state).await?;
    let progress = phase_progress(&state, None).await?;

    let projects = projects
        .into_iter()
        .map(|project| ProjectRow {
            phases: progress.iter().filter(|p| p.project_id == project.id).cloned().collect(),
            project,
        })
        .collect();

    Ok(page(&BoardTemplate { projects }))
}

/// A task as the board shows it. `agent` is precomputed so the row template
/// only has to ask "is this delegated?".
struct Row {
    id: Uuid,
    title: String,
    status: String,
    priority: i32,
    kind: Option<String>,
    agent: bool,
}

impl Row {
    fn of(task: &Task) -> Self {
        Row {
            id: task.id,
            title: task.title.clone(),
            status: task.status.clone(),
            priority: task.priority,
            agent: task.assignee_kind.as_deref() == Some("agent"),
            kind: task.assignee_kind.clone(),
        }
    }
}

struct PhaseTasks {
    phase: PhaseProgress,
    tasks: Vec<Row>,
}

/// One `<option>` with its selected state decided in Rust, so the template does
/// no string comparison.
struct Opt {
    value: &'static str,
    selected: bool,
}

fn options(values: &[&'static str], selected: &Option<String>) -> Vec<Opt> {
    values
        .iter()
        .map(|v| Opt { value: v, selected: selected.as_deref() == Some(*v) })
        .collect()
}

#[derive(Template)]
#[template(path = "project.html", blocks = ["task_list"])]
struct ProjectTemplate {
    project: Project,
    phases: Vec<PhaseTasks>,
    statuses: Vec<Opt>,
    kinds: Vec<Opt>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Filters {
    status: Option<String>,
    assignee_kind: Option<String>,
}

/// An empty select means "all", not "match the empty string".
fn blank(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

async fn project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    WebCaller(caller): WebCaller,
    headers: HeaderMap,
    Query(filters): Query<Filters>,
) -> AppResult<Response> {
    caller.require("read")?;

    // ponytail: no project-by-id controller exists yet and this handler may not
    // grow one; move the query there when a second caller needs it.
    let project: Project = sqlx::query_as(
        "SELECT id, key, name, status, lead_id, created_at, updated_at FROM project WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    let status = blank(filters.status);
    let assignee_kind = blank(filters.assignee_kind);

    let tasks = controllers::task::search(
        &state,
        TaskFilter {
            project_id: Some(id),
            status: status.clone(),
            assignee_kind: assignee_kind.clone(),
            ..Default::default()
        },
    )
    .await?;

    let phases = phase_progress(&state, Some(id))
        .await?
        .into_iter()
        .map(|phase| PhaseTasks {
            tasks: tasks.iter().filter(|t| t.phase_id == phase.phase_id).map(Row::of).collect(),
            phase,
        })
        .collect();

    let template = ProjectTemplate {
        project,
        phases,
        statuses: options(&TASK_STATUSES, &status),
        kinds: options(&["human", "agent"], &assignee_kind),
    };

    // HTMX asked for the list alone; a browser asked for the page around it.
    if headers.contains_key("hx-request") {
        Ok(page(&template.as_task_list()))
    } else {
        Ok(page(&template))
    }
}
