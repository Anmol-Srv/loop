use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::artifact::{Artifact, ARTIFACT_KINDS, PARENT_TYPES};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};

/// An artifact as the person bound to `$1` sees it. `can_remove` is the whole
/// rule: only whoever added it may remove it (an agent's attachment is its
/// owner's), and a row with nobody recorded is an admin's to clear. There is
/// no admin override on anyone's own resources.
const COLUMNS: &str = "a.id, a.parent_type, a.parent_id, a.kind, a.url, a.title, a.metadata,
    CASE WHEN p.id IS NULL THEN NULL ELSE jsonb_build_object('id', p.id, 'name', p.name) END AS added_by,
    ag.name AS added_by_agent,
    coalesce(a.added_by = $1, EXISTS (SELECT 1 FROM person WHERE id = $1 AND role = 'admin')) AS can_remove,
    a.created_at";
const JOINS: &str = "LEFT JOIN person p ON p.id = a.added_by LEFT JOIN agent ag ON ag.id = a.added_by_agent_id";

pub async fn add(
    state: &AppState,
    actor: &Actor,
    parent_type: String,
    parent_id: Uuid,
    kind: String,
    url: String,
    title: String,
) -> AppResult<Outcome<Artifact>> {
    validate(&parent_type, &kind, &url)?;

    if !actor.can_apply {
        let patch = json!({
            "parent_type": parent_type, "parent_id": parent_id,
            "kind": kind, "url": url, "title": title
        });
        let change_id =
            propose(&state.db, actor, TargetType::Artifact, Uuid::new_v4(), Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;
    let artifact = insert(&mut tx, actor, &parent_type, parent_id, &kind, &url, &title).await?;
    tx.commit().await?;
    Ok(Outcome::Applied { entity: artifact })
}

/// What makes an artifact acceptable, whoever is adding it.
pub fn validate(parent_type: &str, kind: &str, url: &str) -> AppResult<()> {
    if !PARENT_TYPES.contains(&parent_type) {
        return Err(AppError::BadRequest(format!(
            "parentType must be one of {}", PARENT_TYPES.join(", ")
        )));
    }
    if !ARTIFACT_KINDS.contains(&kind) {
        return Err(AppError::BadRequest(format!(
            "kind must be one of {}", ARTIFACT_KINDS.join(", ")
        )));
    }
    if url.trim().is_empty() {
        return Err(AppError::BadRequest("url is required".into()));
    }
    // The app hands a link to `open`, which will as happily run a `file://`
    // or an app's custom scheme as load a page. Only the web gets through. A
    // commit is exempt: its `url` holds a hash, and nothing opens it.
    let lower = url.trim().to_ascii_lowercase();
    if kind != "commit" && !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(AppError::BadRequest("a link must start with http:// or https://".into()));
    }
    Ok(())
}

/// Write an artifact inside the caller's transaction. Validation is the
/// caller's: `add` above, or an agent route that has already checked the task
/// is delegated to it.
pub async fn insert(
    tx: &mut sqlx::PgTransaction<'_>,
    actor: &Actor,
    parent_type: &str,
    parent_id: Uuid,
    kind: &str,
    url: &str,
    title: &str,
) -> AppResult<Artifact> {
    let id = Uuid::new_v4();
    let patch = json!({
        "parent_type": parent_type, "parent_id": parent_id,
        "kind": kind, "url": url, "title": title
    });

    // An agent's transaction carries `acp.agent_id`, so its attachment
    // records which agent made it.
    let artifact: Artifact = sqlx::query_as(&format!(
        "WITH a AS (
           INSERT INTO artifact (id, parent_type, parent_id, kind, url, title, added_by, added_by_agent_id)
           VALUES ($2, $3, $4, $5, $6, $7, $1, acting_agent()) RETURNING *)
         SELECT {COLUMNS} FROM a {JOINS}"
    ))
    .bind(actor.person_id)
    .bind(id)
    .bind(parent_type)
    .bind(parent_id)
    .bind(kind)
    .bind(url)
    .bind(title)
    .fetch_one(&mut **tx)
    .await?;

    record(tx, actor, TargetType::Artifact, artifact.id, Op::Create, patch).await?;
    Ok(artifact)
}

/// `viewer` is who `canRemove` is worked out for.
pub async fn list(
    state: &AppState,
    viewer: Option<Uuid>,
    parent_type: String,
    parent_id: Uuid,
) -> AppResult<Vec<Artifact>> {
    let artifacts = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM artifact a {JOINS} WHERE a.parent_type = $2 AND a.parent_id = $3
         ORDER BY a.created_at DESC"
    ))
    .bind(viewer)
    .bind(parent_type)
    .bind(parent_id)
    .fetch_all(&state.db)
    .await?;

    Ok(artifacts)
}

/// Remove a piece of evidence that was attached by mistake.
///
/// Recorded as a `Delete` change so the audit trail still shows it existed —
/// a PR link that vanishes with no trace is exactly the kind of thing this
/// table is here to prevent. It does not move the task's status back: the
/// evidence gate guards the transition, not the state afterwards.
///
/// Every removal comes through here — a person, a proposal, its approval — so
/// this is where who-may-remove is enforced, before anything is queued. There
/// is no path that edits an artifact; one would check the same `can_remove`.
pub async fn remove(state: &AppState, actor: &Actor, id: Uuid) -> AppResult<Outcome<Artifact>> {
    let mut tx = state.db.begin().await?;
    let artifact: Artifact = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM artifact a {JOINS} WHERE a.id = $2 FOR UPDATE OF a"
    ))
    .bind(actor.person_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("that link is already gone".into()))?;
    if !artifact.can_remove {
        return Err(AppError::Forbidden(refusal(&artifact)));
    }

    if actor.can_apply {
        sqlx::query("DELETE FROM artifact WHERE id = $1").bind(id).execute(&mut *tx).await?;
    }
    // `record` files it as pending when the actor cannot apply.
    let change_id = record(&mut tx, actor, TargetType::Artifact, id, Op::Delete, json!({ "id": id })).await?;
    tx.commit().await?;
    Ok(if actor.can_apply { Outcome::Applied { entity: artifact } } else { Outcome::Proposed { change_id } })
}

/// "Only Dhaval can remove this link — they added it."
fn refusal(a: &Artifact) -> String {
    let what = match a.kind.as_str() {
        "pr" => "PR",
        "figma" => "Figma file",
        k => k,
    };
    match (a.added_by.as_ref().and_then(|p| p["name"].as_str()), &a.added_by_agent) {
        (Some(name), Some(agent)) => {
            format!("Only {name} can remove this {what} \u{2014} their agent {agent} attached it.")
        }
        (Some(name), None) => format!("Only {name} can remove this {what} \u{2014} they added it."),
        (None, _) => format!("Only an admin can remove this {what} \u{2014} nobody is recorded as adding it."),
    }
}
