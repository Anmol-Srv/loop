use serde_json::Value;
use sqlx::PgTransaction;
use uuid::Uuid;

use crate::errors::AppResult;

/// Who is making a change. `can_apply` is false for agent tokens without the
/// `write` scope, which forces their changes into the pending queue.
#[derive(Debug, Clone)]
pub struct Actor {
    pub label: String,
    pub person_id: Option<Uuid>,
    pub can_apply: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum TargetType {
    Project,
    Phase,
    Task,
    Artifact,
}

impl TargetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetType::Project => "project",
            TargetType::Phase => "phase",
            TargetType::Task => "task",
            TargetType::Artifact => "artifact",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Op {
    Create,
    Update,
    Delete,
}

impl Op {
    pub fn as_str(&self) -> &'static str {
        match self {
            Op::Create => "create",
            Op::Update => "update",
            Op::Delete => "delete",
        }
    }
}

/// Record exactly one `change` row inside the caller's transaction.
///
/// This is the only place in the codebase that inserts into `change`. Every
/// mutation routes through it so that audit, history, and the agent approval
/// gate all derive from one mechanism.
pub async fn record(
    tx: &mut PgTransaction<'_>,
    actor: &Actor,
    target_type: TargetType,
    target_id: Uuid,
    op: Op,
    patch: Value,
) -> AppResult<Uuid> {
    let state = if actor.can_apply { "applied" } else { "pending" };

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO change (actor, on_behalf_of, target_type, target_id, op, patch, state, applied_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, CASE WHEN $7 = 'applied' THEN now() ELSE NULL END)
         RETURNING id",
    )
    .bind(&actor.label)
    .bind(actor.person_id)
    .bind(target_type.as_str())
    .bind(target_id)
    .bind(op.as_str())
    .bind(patch)
    .bind(state)
    .fetch_one(&mut **tx)
    .await?;

    Ok(id)
}

/// The result of a mutation: either it happened, or it is waiting for a human.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "status")]
pub enum Outcome<T> {
    #[serde(rename = "applied")]
    Applied { entity: T },
    #[serde(rename = "proposed")]
    Proposed {
        #[serde(rename = "changeId")]
        change_id: Uuid,
    },
}

/// Record an intended mutation without performing it. Used when the actor
/// lacks the `write` scope, so the effect waits for human approval.
///
/// The `patch` must carry every argument needed to replay the operation.
pub async fn propose(
    db: &sqlx::PgPool,
    actor: &Actor,
    target_type: TargetType,
    target_id: Uuid,
    op: Op,
    patch: Value,
) -> AppResult<Uuid> {
    let mut tx = db.begin().await?;
    let id = record(&mut tx, actor, target_type, target_id, op, patch).await?;
    tx.commit().await?;
    Ok(id)
}
