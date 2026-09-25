//! A person's own folders to work in when a task has no project (or no folder
//! there for them): named, so a task can pin one across whichever machine
//! that person's agent runs on. Private, like a repo's local path — every
//! query below is scoped to the caller's own `person_id`, so nobody's list
//! can return anyone else's.

use serde::Serialize;
use uuid::Uuid;

use crate::controllers::repo::check_path;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub is_default: bool,
}

const COLUMNS: &str = "id, name, path, is_default";

fn check_name(name: &str) -> AppResult<()> {
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::BadRequest(
            "a folder needs a name of 1 to 80 characters".into(),
        ));
    }
    Ok(())
}

pub async fn list(state: &AppState, me: Uuid) -> AppResult<Vec<Folder>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM user_folder WHERE person_id = $1 ORDER BY created_at, id"
    ))
    .bind(me)
    .fetch_all(&state.db)
    .await?)
}

async fn one(state: &AppState, me: Uuid, id: Uuid) -> AppResult<Folder> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM user_folder WHERE person_id = $1 AND id = $2"
    ))
    .bind(me)
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("that folder is gone".into()))
}

/// Add a folder. A person's first is their default whether they ask for that
/// or not: an agent needs somewhere to work the moment there is anywhere at
/// all.
pub async fn add(state: &AppState, me: Uuid, name: &str, path: &str) -> AppResult<Folder> {
    let name = name.trim();
    check_name(name)?;
    let path = path.trim();
    check_path(path)?;

    let mut tx = state.db.begin().await?;
    let first: bool =
        sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM user_folder WHERE person_id = $1)")
            .bind(me)
            .fetch_one(&mut *tx)
            .await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO user_folder (person_id, name, path, is_default) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(me)
    .bind(name)
    .bind(path)
    .bind(first)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::BadRequest(format!("you already have a folder named \"{name}\""))
        }
        _ => AppError::Database(e),
    })?;
    tx.commit().await?;
    one(state, me, id).await
}

pub async fn remove(state: &AppState, me: Uuid, id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM user_folder WHERE person_id = $1 AND id = $2")
        .bind(me)
        .bind(id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("that folder is gone".into()));
    }
    Ok(())
}

/// Make this the one used when a task pins none: unset whichever folder was
/// default and set this one, in the same transaction so the partial unique
/// index never sees two at once.
pub async fn set_default(state: &AppState, me: Uuid, id: Uuid) -> AppResult<Folder> {
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE user_folder SET is_default = false WHERE person_id = $1 AND is_default")
        .bind(me)
        .execute(&mut *tx)
        .await?;
    let result =
        sqlx::query("UPDATE user_folder SET is_default = true WHERE person_id = $1 AND id = $2")
            .bind(me)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("that folder is gone".into()));
    }
    tx.commit().await?;
    one(state, me, id).await
}
