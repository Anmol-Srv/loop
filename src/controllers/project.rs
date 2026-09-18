use serde_json::json;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{record, Actor, Op, TargetType};
use crate::models::project::Project;

pub async fn create(state: &AppState, actor: &Actor, key: String, name: String) -> AppResult<Project> {
    if key.trim().is_empty() || name.trim().is_empty() {
        return Err(AppError::BadRequest("key and name are required".into()));
    }

    let mut tx = state.db.begin().await?;

    let project: Project = sqlx::query_as(
        "INSERT INTO project (key, name) VALUES ($1, $2)
         RETURNING id, key, name, status, lead_id, created_at, updated_at",
    )
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

    record(
        &mut tx,
        actor,
        TargetType::Project,
        project.id,
        Op::Create,
        json!({ "key": project.key, "name": project.name }),
    )
    .await?;

    tx.commit().await?;

    Ok(project)
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
