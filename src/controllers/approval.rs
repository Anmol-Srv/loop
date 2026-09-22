//! The human side of the proposal guardrail: list what is waiting, then
//! approve it (replay the recorded intent) or reject it (drop it).

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::controllers::{artifact, phase, project, task};
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{Actor, Outcome};

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

/// Proposals an agent queued on one person's behalf: their inbox, and the
/// only thing standing between an agent's intent and the board.
pub async fn pending_for(state: &AppState, person_id: Uuid) -> AppResult<Vec<ChangeRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {CHANGE_COLUMNS} FROM change
          WHERE state = 'pending' AND on_behalf_of = $1 ORDER BY created_at"
    ))
    .bind(person_id)
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
    let created = replay(state, &replay_actor, &change).await?;

    // A `create` proposal's target_id was a placeholder; repoint it at the row
    // that now exists so the audit trail leads somewhere.
    let approved: ChangeRow = sqlx::query_as(&format!(
        "UPDATE change SET state = 'approved', applied_at = now(),
                           target_id = COALESCE($2, target_id)
         WHERE id = $1
         RETURNING {CHANGE_COLUMNS}"
    ))
    .bind(change_id)
    .bind(created)
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
/// Replay a proposal. Returns the id of the row a `create` produced, so the
/// caller can repoint `change.target_id` at the row that actually exists — the
/// id recorded at proposal time was only a placeholder.
async fn replay(state: &AppState, actor: &Actor, change: &ChangeRow) -> AppResult<Option<Uuid>> {
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

    let created = match (change.target_type.as_str(), change.op.as_str()) {
        ("project", "create") => match project::create(
            state,
            actor,
            Some(str_at("key")?),
            str_at("name")?,
            p.get("description").and_then(Value::as_str).unwrap_or_default().to_owned(),
            p.get("member_ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).filter_map(|s| s.parse().ok()).collect())
                .unwrap_or_default(),
            Vec::new(),
        )
        .await?
        {
            Outcome::Applied { entity } => Some(entity.id),
            Outcome::Proposed { .. } => None,
        },
        ("phase", "create") => match phase::create(
            state,
            actor,
            uuid_at("project_id")?,
            str_at("name")?,
            i32_at("position")?,
            p.get("gate").and_then(Value::as_bool).unwrap_or(false),
        )
        .await?
        {
            Outcome::Applied { entity } => Some(entity.id),
            Outcome::Proposed { .. } => None,
        },
        ("phase", "update") => {
            phase::set_status(state, actor, change.target_id, str_at("status")?).await?;
            None
        }
        ("task", "create") => match task::create(
            state,
            actor,
            uuid_at("phase_id")?,
            str_at("title")?,
            str_at("body")?,
            i32_at("priority")?,
            p.get("discipline").and_then(Value::as_str).map(str::to_string),
        )
        .await?
        {
            Outcome::Applied { entity } => Some(entity.id),
            Outcome::Proposed { .. } => None,
        },
        ("task", "update") => {
            // Which key the patch carries says which controller made it.
            if p.get("status").is_some() {
                task::set_status(state, actor, change.target_id, str_at("status")?).await?;
            } else if p.get("discipline").is_some() {
                let d = p.get("discipline").and_then(Value::as_str).map(str::to_string);
                task::set_discipline(state, actor, change.target_id, d).await?;
            } else if let Some(blockers) = p.get("blocked_by").and_then(Value::as_array) {
                let blockers = blockers
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .and_then(|s| s.parse().ok())
                            .ok_or_else(|| AppError::Internal("blocked_by holds a non-uuid".into()))
                    })
                    .collect::<AppResult<Vec<Uuid>>>()?;
                task::set_blockers(state, actor, change.target_id, blockers).await?;
            } else if p.get("claim_person_id").is_some() {
                task::claim(state, actor, change.target_id, uuid_at("claim_person_id")?).await?;
            } else if p.get("release_person_id").is_some() {
                task::release(state, actor, change.target_id, uuid_at("release_person_id")?).await?;
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
            None
        }
        ("artifact", "create") => match artifact::add(
            state,
            actor,
            str_at("parent_type")?,
            uuid_at("parent_id")?,
            str_at("kind")?,
            str_at("url")?,
            str_at("title")?,
        )
        .await?
        {
            Outcome::Applied { entity } => Some(entity.id),
            Outcome::Proposed { .. } => None,
        },
        (t, o) => return Err(AppError::Internal(format!("cannot replay {t}/{o}"))),
    };

    Ok(created)
}
