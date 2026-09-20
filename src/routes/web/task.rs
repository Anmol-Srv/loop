//! Task detail and the run log that keeps itself current while work is running.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers::approval::ChangeRow;
use crate::controllers::{artifact, run_log};
use crate::db::AppState;
use crate::middleware::session::WebCaller;
use crate::models::artifact::Artifact;
use crate::models::run_log::RunLogLine;
use crate::routes::web::auth::page;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/tasks/{id}", get(detail))
        .route("/tasks/{id}/log", get(log_fragment))
}

#[derive(sqlx::FromRow)]
struct TaskDetail {
    id: Uuid,
    project_id: Uuid,
    project_name: String,
    phase_name: String,
    title: String,
    body: String,
    status: String,
    priority: i32,
    assignee_kind: Option<String>,
    assignee: Option<String>,
}

#[derive(Template)]
#[template(path = "task.html")]
struct TaskTemplate {
    task: TaskDetail,
    assignee_class: &'static str,
    artifacts: Vec<Artifact>,
    changes: Vec<ChangeRow>,
    // Fields below are read by the included `_run_log.html`.
    task_id: Uuid,
    lines: Vec<RunLogLine>,
    next_seq: i64,
    polling: bool,
    oob: bool,
}

#[derive(Template)]
#[template(path = "_run_log.html")]
struct RunLogTemplate {
    task_id: Uuid,
    lines: Vec<RunLogLine>,
    next_seq: i64,
    polling: bool,
    oob: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogQuery {
    #[serde(default)]
    after_seq: i64,
}

async fn detail(_caller: WebCaller, State(state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    let Some(task) = load(&state, id).await else {
        return (StatusCode::NOT_FOUND, "task not found").into_response();
    };

    let artifacts = artifact::list(&state, "task".into(), id).await.unwrap_or_default();
    let changes = history(&state, id).await;
    let lines = run_log::read(&state, id, 0).await.unwrap_or_default();

    page(&TaskTemplate {
        assignee_class: match task.assignee_kind.as_deref() {
            Some("agent") => "agent",
            _ => "human",
        },
        polling: task.status == "in_progress",
        next_seq: last_seq(&lines, 0),
        task_id: id,
        task,
        artifacts,
        changes,
        lines,
        oob: false,
    })
}

/// The fragment carries the cursor: it renders the lines after `afterSeq` and
/// an out-of-band poller pointing at the highest seq it just emitted, so the
/// next tick starts where this one stopped instead of replaying from zero.
async fn log_fragment(
    _caller: WebCaller,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<LogQuery>,
) -> Response {
    let Some(status) = status_of(&state, id).await else {
        return (StatusCode::NOT_FOUND, "task not found").into_response();
    };
    let lines = run_log::read(&state, id, q.after_seq).await.unwrap_or_default();

    page(&RunLogTemplate {
        task_id: id,
        next_seq: last_seq(&lines, q.after_seq),
        // A task that finished while we were watching emits a poller with no
        // trigger, so the page stops asking on its own.
        polling: status == "in_progress",
        lines,
        oob: true,
    })
}

fn last_seq(lines: &[RunLogLine], fallback: i64) -> i64 {
    lines.last().map_or(fallback, |l| l.seq)
}

async fn load(state: &AppState, id: Uuid) -> Option<TaskDetail> {
    sqlx::query_as(
        "SELECT t.id, ph.project_id, pr.name AS project_name, ph.name AS phase_name,
                t.title, t.body, t.status, t.priority, t.assignee_kind,
                COALESCE(per.email, tok.label) AS assignee
         FROM task t
         JOIN phase ph ON ph.id = t.phase_id
         JOIN project pr ON pr.id = ph.project_id
         LEFT JOIN person per ON per.id = t.assignee_person_id
         LEFT JOIN credential tok ON tok.id = t.assignee_token_id
         WHERE t.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
}

async fn status_of(state: &AppState, id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT status FROM task WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
}

/// Read directly rather than through a controller: the approval controller
/// lists what is pending across the plane, this is one task's whole history.
async fn history(state: &AppState, id: Uuid) -> Vec<ChangeRow> {
    sqlx::query_as(
        "SELECT id, actor, on_behalf_of, target_type, target_id, op, patch, state,
                applied_at, created_at
         FROM change
         WHERE target_type = 'task' AND target_id = $1
         ORDER BY created_at DESC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
}
