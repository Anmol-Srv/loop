//! A project's repositories, and each person's own folder for each.
//!
//! The repo (name, URL) is the team's. The folder it is checked out to is the
//! viewer's alone: every read below joins `repo_path` on the person asking, so
//! nobody's query can return anyone else's path.

use serde::Serialize;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::Actor;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    /// The viewer's own checkout, never anyone else's.
    pub my_path: Option<String>,
    /// Whether the viewer may rename or remove it: `can_edit`.
    pub can_edit: bool,
}

/// Who may edit or remove a repo: whoever added it, or an admin. SQL over
/// `r`, for the person bound at `viewer`. Read as `can_edit` on every row and
/// checked before every edit, so what the app offers and what the server
/// allows are the same expression.
fn can_edit(viewer: &str) -> String {
    format!(
        "(coalesce(r.created_by = {viewer}, false)
          OR EXISTS (SELECT 1 FROM person WHERE id = {viewer} AND role = 'admin'))"
    )
}

/// A repo row as the person bound at `$1` sees it. The path join is on `$1`:
/// this is the privacy rule.
fn select() -> String {
    format!(
        "SELECT r.id, r.name, r.url, rp.path AS my_path, {} AS can_edit
           FROM project_repo r
           LEFT JOIN repo_path rp ON rp.repo_id = r.id AND rp.person_id = $1",
        can_edit("$1")
    )
}

/// `https://…`, `http://…`, or the `git@host:org/repo` form a clone uses.
pub fn check_url(url: &str) -> AppResult<()> {
    let lower = url.to_ascii_lowercase();
    let web = lower.starts_with("https://") || lower.starts_with("http://");
    let ssh = url
        .strip_prefix("git@")
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(host, path)| !host.is_empty() && !path.is_empty());
    if url.is_empty() || url.len() > 500 || url.chars().any(char::is_whitespace) || !(web || ssh) {
        return Err(AppError::BadRequest(
            "a repository URL must start with https:// or look like git@github.com:org/repo".into(),
        ));
    }
    Ok(())
}

fn check_name(name: &str) -> AppResult<()> {
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::BadRequest("a repository needs a name of 1 to 80 characters".into()));
    }
    Ok(())
}

/// A local folder: absolute, one line, at most 500 characters.
pub fn check_path(path: &str) -> AppResult<()> {
    if !path.starts_with('/') {
        return Err(AppError::BadRequest("the folder must be an absolute path, starting with /".into()));
    }
    if path.contains(['\n', '\r']) || path.len() > 500 {
        return Err(AppError::BadRequest("the folder must be one line of at most 500 characters".into()));
    }
    Ok(())
}

/// Repos are added and edited directly, never proposed: one is a pointer to
/// code, not a change to the plan.
fn writer(actor: &Actor) -> AppResult<Uuid> {
    match actor.person_id {
        Some(id) if actor.can_apply => Ok(id),
        _ => Err(AppError::Forbidden("changing a project's repositories needs write access".into())),
    }
}

pub async fn list(state: &AppState, viewer: Option<Uuid>, project_id: Uuid) -> AppResult<Vec<Repo>> {
    Ok(sqlx::query_as(&format!("{} WHERE r.project_id = $2 ORDER BY r.created_at, r.id", select()))
        .bind(viewer)
        .bind(project_id)
        .fetch_all(&state.db)
        .await?)
}

async fn one(db: impl sqlx::PgExecutor<'_>, viewer: Uuid, id: Uuid) -> AppResult<Repo> {
    sqlx::query_as(&format!("{} WHERE r.id = $2", select()))
        .bind(viewer)
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::NotFound("that repository is gone".into()))
}

/// Insert inside the caller's transaction; the create form uses it too.
pub async fn insert(
    tx: &mut sqlx::PgTransaction<'_>,
    by: Option<Uuid>,
    project_id: Uuid,
    name: &str,
    url: &str,
) -> AppResult<Uuid> {
    let (name, url) = (name.trim(), url.trim());
    check_name(name)?;
    check_url(url)?;
    sqlx::query_scalar(
        "INSERT INTO project_repo (project_id, name, url, created_by) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(project_id)
    .bind(name)
    .bind(url)
    .bind(by)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::NotFound("project not found".into())
        }
        _ => AppError::Database(e),
    })
}

pub async fn add(state: &AppState, actor: &Actor, project_id: Uuid, name: &str, url: &str) -> AppResult<Repo> {
    let me = writer(actor)?;
    let mut tx = state.db.begin().await?;
    let id = insert(&mut tx, Some(me), project_id, name, url).await?;
    tx.commit().await?;
    one(&state.db, me, id).await
}

/// Lock the repo and refuse anyone `can_edit` does not name.
async fn editable(tx: &mut sqlx::PgTransaction<'_>, me: Uuid, id: Uuid, verb: &str) -> AppResult<()> {
    let (allowed, creator): (bool, Option<String>) = sqlx::query_as(&format!(
        "SELECT {}, c.name FROM project_repo r LEFT JOIN person c ON c.id = r.created_by
          WHERE r.id = $1 FOR UPDATE OF r",
        can_edit("$2")
    ))
    .bind(id)
    .bind(me)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("that repository is gone".into()))?;
    if !allowed {
        let who: Vec<String> = creator.map(|n| format!("{n}, who added this repository")).into_iter().collect();
        return Err(AppError::Forbidden(super::project::only(&who, verb, "repository")));
    }
    Ok(())
}

pub async fn update(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    name: Option<&str>,
    url: Option<&str>,
) -> AppResult<Repo> {
    let me = writer(actor)?;
    let (name, url) = (name.map(str::trim), url.map(str::trim));
    if let Some(n) = name {
        check_name(n)?;
    }
    if let Some(u) = url {
        check_url(u)?;
    }
    let mut tx = state.db.begin().await?;
    editable(&mut tx, me, id, "edit").await?;
    sqlx::query("UPDATE project_repo SET name = coalesce($2, name), url = coalesce($3, url) WHERE id = $1")
        .bind(id)
        .bind(name)
        .bind(url)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    one(&state.db, me, id).await
}

pub async fn remove(state: &AppState, actor: &Actor, id: Uuid) -> AppResult<()> {
    let me = writer(actor)?;
    let mut tx = state.db.begin().await?;
    editable(&mut tx, me, id, "remove").await?;
    sqlx::query("DELETE FROM project_repo WHERE id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Set or clear the caller's own folder for a repo. Anyone who can read the
/// project may keep one: it is their machine, and it changes nothing shared.
pub async fn set_path(state: &AppState, me: Uuid, id: Uuid, path: Option<&str>) -> AppResult<Repo> {
    match path.map(str::trim).filter(|p| !p.is_empty()) {
        None => {
            sqlx::query("DELETE FROM repo_path WHERE repo_id = $1 AND person_id = $2")
                .bind(id)
                .bind(me)
                .execute(&state.db)
                .await?;
        }
        Some(path) => {
            check_path(path)?;
            sqlx::query(
                "INSERT INTO repo_path (repo_id, person_id, path) VALUES ($1, $2, $3)
                 ON CONFLICT (repo_id, person_id) DO UPDATE SET path = EXCLUDED.path, updated_at = now()",
            )
            .bind(id)
            .bind(me)
            .bind(path)
            .execute(&state.db)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
                    AppError::NotFound("that repository is gone".into())
                }
                _ => AppError::Database(e),
            })?;
        }
    }
    one(&state.db, me, id).await
}
