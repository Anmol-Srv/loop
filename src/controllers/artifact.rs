use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::artifact::{Artifact, ARTIFACT_KINDS, PARENT_TYPES};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};

const COLUMNS: &str = "id, parent_type, parent_id, kind, url, title, metadata, added_by, created_at";

pub async fn add(
    state: &AppState,
    actor: &Actor,
    parent_type: String,
    parent_id: Uuid,
    kind: String,
    url: String,
    title: String,
) -> AppResult<Outcome<Artifact>> {
    if !PARENT_TYPES.contains(&parent_type.as_str()) {
        return Err(AppError::BadRequest(format!(
            "parentType must be one of {}", PARENT_TYPES.join(", ")
        )));
    }
    if !ARTIFACT_KINDS.contains(&kind.as_str()) {
        return Err(AppError::BadRequest(format!(
            "kind must be one of {}", ARTIFACT_KINDS.join(", ")
        )));
    }
    if url.trim().is_empty() {
        return Err(AppError::BadRequest("url is required".into()));
    }

    let id = Uuid::new_v4();
    let patch = json!({
        "parent_type": parent_type, "parent_id": parent_id,
        "kind": kind, "url": url, "title": title
    });

    if !actor.can_apply {
        let change_id = propose(&state.db, actor, TargetType::Artifact, id, Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let artifact: Artifact = sqlx::query_as(&format!(
        "INSERT INTO artifact (id, parent_type, parent_id, kind, url, title, added_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(&parent_type)
    .bind(parent_id)
    .bind(&kind)
    .bind(&url)
    .bind(&title)
    .bind(actor.person_id)
    .fetch_one(&mut *tx)
    .await?;

    record(&mut tx, actor, TargetType::Artifact, artifact.id, Op::Create, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: artifact })
}

pub async fn list(state: &AppState, parent_type: String, parent_id: Uuid) -> AppResult<Vec<Artifact>> {
    let artifacts = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM artifact WHERE parent_type = $1 AND parent_id = $2
         ORDER BY created_at DESC"
    ))
    .bind(parent_type)
    .bind(parent_id)
    .fetch_all(&state.db)
    .await?;

    Ok(artifacts)
}
