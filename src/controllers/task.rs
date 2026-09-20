use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{record, Actor, Op, TargetType};
use crate::models::task::{Task, TaskFilter, TASK_COLUMNS, TASK_STATUSES};

pub enum Assignee {
    Person(String),
    Agent(String),
    Nobody,
}

pub async fn create(
    state: &AppState,
    actor: &Actor,
    phase_id: Uuid,
    title: String,
    body: String,
    priority: i32,
) -> AppResult<Task> {
    if title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if !(0..=4).contains(&priority) {
        return Err(AppError::BadRequest("priority must be between 0 and 4".into()));
    }

    let mut tx = state.db.begin().await?;

    let task: Task = sqlx::query_as(&format!(
        "INSERT INTO task (phase_id, title, body, priority) VALUES ($1, $2, $3, $4)
         RETURNING {TASK_COLUMNS}"
    ))
    .bind(phase_id)
    .bind(&title)
    .bind(&body)
    .bind(priority)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::NotFound("phase not found".into())
        }
        _ => AppError::Database(e),
    })?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Create, json!({ "title": task.title })).await?;

    tx.commit().await?;
    Ok(task)
}

/// Filters are applied with a single query using NULL-tolerant predicates, so
/// there is no dynamic SQL string building to audit.
pub async fn search(state: &AppState, filter: TaskFilter) -> AppResult<Vec<Task>> {
    let tasks = sqlx::query_as(&format!(
        "SELECT t.{} FROM task t
         JOIN phase p ON p.id = t.phase_id
         LEFT JOIN person per ON per.id = t.assignee_person_id
         WHERE ($1::uuid IS NULL OR p.project_id = $1)
           AND ($2::uuid IS NULL OR t.phase_id = $2)
           AND ($3::text IS NULL OR t.status = $3)
           AND ($4::text IS NULL OR per.email = $4)
           AND ($5::text IS NULL OR t.assignee_kind = $5)
         ORDER BY t.priority, t.created_at",
        TASK_COLUMNS.replace(", ", ", t.")
    ))
    .bind(filter.project_id)
    .bind(filter.phase_id)
    .bind(filter.status)
    .bind(filter.assignee_email)
    .bind(filter.assignee_kind)
    .fetch_all(&state.db)
    .await?;

    Ok(tasks)
}

pub async fn set_status(state: &AppState, actor: &Actor, id: Uuid, status: String) -> AppResult<Task> {
    if !TASK_STATUSES.contains(&status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "status must be one of {}", TASK_STATUSES.join(", ")
        )));
    }

    let mut tx = state.db.begin().await?;

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET status = $2, updated_at = now() WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(&status)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, json!({ "status": status })).await?;

    tx.commit().await?;
    Ok(task)
}

pub async fn assign(state: &AppState, actor: &Actor, id: Uuid, to: Assignee) -> AppResult<Task> {
    let mut tx = state.db.begin().await?;

    let (kind, person_id, token_id, patch) = match &to {
        Assignee::Person(email) => {
            let pid: Uuid = sqlx::query_scalar("SELECT id FROM person WHERE email = $1 AND deleted_at IS NULL")
                .bind(email)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("no person with email '{email}'")))?;
            (Some("human"), Some(pid), None, json!({ "assignee": email }))
        }
        Assignee::Agent(label) => {
            let tid: Uuid = sqlx::query_scalar(
                "SELECT id FROM agent_token WHERE label = $1 AND revoked_at IS NULL AND expires_at > now()
                 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(label)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("no active agent token labelled '{label}'")))?;
            (Some("agent"), None, Some(tid), json!({ "assignee": label }))
        }
        Assignee::Nobody => (None, None, None, json!({ "assignee": null })),
    };

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET assignee_kind = $2, assignee_person_id = $3, assignee_token_id = $4,
                         updated_at = now()
         WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(kind)
    .bind(person_id)
    .bind(token_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, patch).await?;

    tx.commit().await?;
    Ok(task)
}
