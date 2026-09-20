//! The human side of the proposal guardrail: list what is waiting, then
//! approve it (replay the recorded intent) or reject it (drop it).

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::controllers::{artifact, phase, project, task};
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::Actor;

const CHANGE_COLUMNS: &str = "id, actor, on_behalf_of, target_type, target_id, op, patch, state, applied_at, created_at";

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRow {
    pub id: Uuid,
    pub actor: String,
    pub on_behalf_of: Option<Uuid>,
    pub target_type: String,
    pub target_id: Uuid,
    pub op: String,
    pub patch: Value,
    pub state: String,
    pub applied_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_pending(state: &AppState) -> AppResult<Vec<ChangeRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {CHANGE_COLUMNS} FROM change WHERE state = 'pending' ORDER BY created_at"
    ))
    .fetch_all(&state.db)
    .await?)
}

/// Replay the proposal, then flip it to `approved`.
///
/// The `change` row is locked for the whole operation and its transition
/// commits only if the replay succeeded, so a failed replay leaves the
/// proposal `pending` and surfaces the underlying error. The replay's own
/// writes commit in the controllers' transactions; the lock is what stops two
/// approvals racing the same proposal.
pub async fn approve(state: &AppState, approver: &Actor, change_id: Uuid) -> AppResult<ChangeRow> {
    let mut tx = state.db.begin().await?;

    let change: ChangeRow = sqlx::query_as(&format!(
        "SELECT {CHANGE_COLUMNS} FROM change WHERE id = $1 FOR UPDATE"
    ))
    .bind(change_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("change not found".into()))?;

    require_pending(&change)?;

    let replay_actor = Actor {
        label: format!("{} (approved by {})", change.actor, approver.label),
        person_id: approver.person_id,
        can_apply: true,
    };
    replay(state, &replay_actor, &change).await?;

    let approved: ChangeRow = sqlx::query_as(&format!(
        "UPDATE change SET state = 'approved', applied_at = now() WHERE id = $1
         RETURNING {CHANGE_COLUMNS}"
    ))
    .bind(change_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(approved)
}

pub async fn reject(state: &AppState, _approver: &Actor, change_id: Uuid) -> AppResult<ChangeRow> {
    let mut tx = state.db.begin().await?;

    let change: ChangeRow = sqlx::query_as(&format!(
        "SELECT {CHANGE_COLUMNS} FROM change WHERE id = $1 FOR UPDATE"
    ))
    .bind(change_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("change not found".into()))?;

    require_pending(&change)?;

    let rejected: ChangeRow = sqlx::query_as(&format!(
        "UPDATE change SET state = 'rejected' WHERE id = $1 RETURNING {CHANGE_COLUMNS}"
    ))
    .bind(change_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(rejected)
}

fn require_pending(change: &ChangeRow) -> AppResult<()> {
    if change.state == "pending" {
        Ok(())
    } else {
        Err(AppError::Conflict(format!("change is already {}", change.state)))
    }
}

/// Dispatch a recorded proposal back through the controller that made it.
///
/// Exhaustive over the seven mutating operations; anything else is a bug in
/// whoever queued the row, so it errors rather than silently doing nothing.
async fn replay(state: &AppState, actor: &Actor, change: &ChangeRow) -> AppResult<()> {
    let p = &change.patch;

    let uuid_at = |k: &str| -> AppResult<Uuid> {
        p.get(k)
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| AppError::Internal(format!("proposal is missing '{k}'")))
    };
    let str_at = |k: &str| -> AppResult<String> {
        p.get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| AppError::Internal(format!("proposal is missing '{k}'")))
    };
    let i32_at = |k: &str| -> AppResult<i32> {
        p.get(k)
            .and_then(Value::as_i64)
            .map(|n| n as i32)
            .ok_or_else(|| AppError::Internal(format!("proposal is missing '{k}'")))
    };

    match (change.target_type.as_str(), change.op.as_str()) {
        ("project", "create") => {
            project::create(state, actor, str_at("key")?, str_at("name")?).await?;
        }
        ("phase", "create") => {
            phase::create(
                state,
                actor,
                uuid_at("project_id")?,
                str_at("name")?,
                i32_at("position")?,
                p.get("gate").and_then(Value::as_bool).unwrap_or(false),
            )
            .await?;
        }
        ("phase", "update") => {
            phase::set_status(state, actor, change.target_id, str_at("status")?).await?;
        }
        ("task", "create") => {
            task::create(
                state,
                actor,
                uuid_at("phase_id")?,
                str_at("title")?,
                str_at("body")?,
                i32_at("priority")?,
            )
            .await?;
        }
        ("task", "update") => {
            // Which key the patch carries says which controller made it.
            if p.get("status").is_some() {
                task::set_status(state, actor, change.target_id, str_at("status")?).await?;
            } else if let Some(label) = p.get("agent_label").and_then(Value::as_str) {
                task::assign(state, actor, change.target_id, task::Assignee::Agent(label.into())).await?;
            } else if let Some(email) = p.get("person_email").and_then(Value::as_str) {
                task::assign(state, actor, change.target_id, task::Assignee::Person(email.into())).await?;
            } else if p.get("person_email").is_some() {
                // present but null: an unassign
                task::assign(state, actor, change.target_id, task::Assignee::Nobody).await?;
            } else {
                return Err(AppError::Internal("task update proposal has no recognised key".into()));
            }
        }
        ("artifact", "create") => {
            artifact::add(
                state,
                actor,
                str_at("parent_type")?,
                uuid_at("parent_id")?,
                str_at("kind")?,
                str_at("url")?,
                str_at("title")?,
            )
            .await?;
        }
        (t, o) => return Err(AppError::Internal(format!("cannot replay {t}/{o}"))),
    }

    Ok(())
}
