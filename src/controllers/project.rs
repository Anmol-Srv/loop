use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};
use crate::models::project::Project;

pub async fn create(
    state: &AppState,
    actor: &Actor,
    key: String,
    name: String,
) -> AppResult<Outcome<Project>> {
    if key.trim().is_empty() || name.trim().is_empty() {
        return Err(AppError::BadRequest("key and name are required".into()));
    }

    // The id is decided up front so a proposal can name the row it will create.
    let id = Uuid::new_v4();
    let patch = json!({ "key": key, "name": name });

    if !actor.can_apply {
        let change_id = propose(&state.db, actor, TargetType::Project, id, Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let project: Project = sqlx::query_as(
        "INSERT INTO project (id, key, name) VALUES ($1, $2, $3)
         RETURNING id, key, name, status, lead_id, created_at, updated_at",
    )
    .bind(id)
    .bind(&key)
    .bind(&name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict(format!("a project with key '{key}' already exists"))
        }
        _ => AppError::Database(e),
    })?;

    record(&mut tx, actor, TargetType::Project, project.id, Op::Create, patch).await?;

    tx.commit().await?;

    Ok(Outcome::Applied { entity: project })
}

pub async fn list(state: &AppState) -> AppResult<Vec<Project>> {
    let projects = sqlx::query_as(
        "SELECT id, key, name, status, lead_id, created_at, updated_at
         FROM project ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(projects)
}
