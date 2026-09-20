use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};
use crate::models::phase::{Phase, PHASE_STATUSES};

pub async fn create(
    state: &AppState,
    actor: &Actor,
    project_id: Uuid,
    name: String,
    position: i32,
    gate: bool,
) -> AppResult<Outcome<Phase>> {
    if name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }

    let id = Uuid::new_v4();
    let patch = json!({ "project_id": project_id, "name": name, "position": position, "gate": gate });

    if !actor.can_apply {
        let change_id = propose(&state.db, actor, TargetType::Phase, id, Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let phase: Phase = sqlx::query_as(
        "INSERT INTO phase (id, project_id, name, position, gate) VALUES ($1, $2, $3, $4, $5)
         RETURNING id, project_id, position, name, status, gate, created_at, updated_at",
    )
    .bind(id)
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

    record(&mut tx, actor, TargetType::Phase, phase.id, Op::Create, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: phase })
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

pub async fn set_status(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    status: String,
) -> AppResult<Outcome<Phase>> {
    if !PHASE_STATUSES.contains(&status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "status must be one of {}", PHASE_STATUSES.join(", ")
        )));
    }

    let patch = json!({ "status": status });

    if !actor.can_apply {
        // A proposal against a row that does not exist could never be replayed.
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Phase, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
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

    record(&mut tx, actor, TargetType::Phase, phase.id, Op::Update, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: phase })
}

async fn exists(state: &AppState, id: Uuid) -> AppResult<()> {
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM phase WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("phase not found".into()))?;
    Ok(())
}
