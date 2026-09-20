use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{record, Actor, Op, TargetType};
use crate::models::phase::{Phase, PHASE_STATUSES};

pub async fn create(
    state: &AppState,
    actor: &Actor,
    project_id: Uuid,
    name: String,
    position: i32,
    gate: bool,
) -> AppResult<Phase> {
    if name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }

    let mut tx = state.db.begin().await?;

    let phase: Phase = sqlx::query_as(
        "INSERT INTO phase (project_id, name, position, gate) VALUES ($1, $2, $3, $4)
         RETURNING id, project_id, position, name, status, gate, created_at, updated_at",
    )
    .bind(project_id)
    .bind(&name)
    .bind(position)
    .bind(gate)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict(format!("position {position} is already taken in this project"))
        }
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::NotFound("project not found".into())
        }
        _ => AppError::Database(e),
    })?;

    record(&mut tx, actor, TargetType::Phase, phase.id, Op::Create,
        json!({ "name": phase.name, "position": phase.position })).await?;

    tx.commit().await?;
    Ok(phase)
}

pub async fn list(state: &AppState, project_id: Uuid) -> AppResult<Vec<Phase>> {
    let phases = sqlx::query_as(
        "SELECT id, project_id, position, name, status, gate, created_at, updated_at
         FROM phase WHERE project_id = $1 ORDER BY position",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;

    Ok(phases)
}

pub async fn set_status(state: &AppState, actor: &Actor, id: Uuid, status: String) -> AppResult<Phase> {
    if !PHASE_STATUSES.contains(&status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "status must be one of {}", PHASE_STATUSES.join(", ")
        )));
    }

    let mut tx = state.db.begin().await?;

    let phase: Phase = sqlx::query_as(
        "UPDATE phase SET status = $2, updated_at = now() WHERE id = $1
         RETURNING id, project_id, position, name, status, gate, created_at, updated_at",
    )
    .bind(id)
    .bind(&status)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("phase not found".into()))?;

    record(&mut tx, actor, TargetType::Phase, phase.id, Op::Update, json!({ "status": status })).await?;

    tx.commit().await?;
    Ok(phase)
}
