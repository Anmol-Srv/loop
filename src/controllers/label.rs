//! Labels: a shared vocabulary, not free text.
//!
//! A `text[]` on each project would have been less schema and would have let
//! "Backend" and "backend" become two labels nobody could filter on together.
//! One row per label means a rename is one write and a colour means the same
//! thing everywhere it appears.

use std::collections::HashMap;

use serde::Serialize;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};

/// The palette a label may take. Names rather than hex so the app can map
/// them onto its own tokens and a label never carries a colour that does not
/// exist in the theme.
pub const COLOURS: [&str; 7] =
    ["slate", "blue", "green", "amber", "red", "purple", "pink"];

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: Uuid,
    pub name: String,
    pub colour: String,
}

pub async fn list(state: &AppState) -> AppResult<Vec<Label>> {
    Ok(sqlx::query_as("SELECT id, name, colour FROM label ORDER BY name")
        .fetch_all(&state.db)
        .await?)
}

/// Create a label, or return the one that already has this name.
///
/// Idempotent on purpose: the form lets you type a new label inline, and
/// typing one that exists should attach it rather than fail.
pub async fn ensure(state: &AppState, name: String, colour: Option<String>) -> AppResult<Label> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("a label needs a name".into()));
    }
    let colour = colour.unwrap_or_else(|| "slate".into());
    if !COLOURS.contains(&colour.as_str()) {
        return Err(AppError::BadRequest(format!(
            "unknown colour '{colour}'; expected one of {}",
            COLOURS.join(", ")
        )));
    }

    Ok(sqlx::query_as(
        "INSERT INTO label (name, colour) VALUES ($1, $2)
         ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
         RETURNING id, name, colour",
    )
    .bind(name)
    .bind(&colour)
    .fetch_one(&state.db)
    .await?)
}

/// Replace a project's labels with exactly this set.
pub async fn set_on_project(
    state: &AppState,
    project_id: Uuid,
    label_ids: &[Uuid],
) -> AppResult<()> {
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM project_label WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO project_label (project_id, label_id)
         SELECT $1, unnest($2::uuid[]) ON CONFLICT DO NOTHING",
    )
    .bind(project_id)
    .bind(label_ids)
    .execute(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::BadRequest("one of those labels does not exist".into())
        }
        _ => AppError::Database(e),
    })?;
    tx.commit().await?;
    Ok(())
}

/// Replace a task's labels with exactly this set, inside the caller's
/// transaction so the edit and its audit row land together.
pub async fn set_on_task(
    tx: &mut sqlx::PgTransaction<'_>,
    task_id: Uuid,
    label_ids: &[Uuid],
) -> AppResult<()> {
    sqlx::query("DELETE FROM task_label WHERE task_id = $1")
        .bind(task_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO task_label (task_id, label_id)
         SELECT $1, unnest($2::uuid[]) ON CONFLICT DO NOTHING",
    )
    .bind(task_id)
    .bind(label_ids)
    .execute(&mut **tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::BadRequest("one of those labels does not exist".into())
        }
        _ => AppError::Database(e),
    })?;
    Ok(())
}

/// Labels keyed by project, for folding into a response — every project's for
/// the list, one project's for the detail screen, which has no reason to read
/// the whole table to keep a handful of rows.
pub async fn by_project(
    state: &AppState,
    only: Option<Uuid>,
) -> AppResult<HashMap<Uuid, Vec<Label>>> {
    let rows: Vec<(Uuid, Uuid, String, String)> = sqlx::query_as(
        "SELECT pl.project_id, l.id, l.name, l.colour
           FROM project_label pl JOIN label l ON l.id = pl.label_id
          WHERE ($1::uuid IS NULL OR pl.project_id = $1)
          ORDER BY l.name",
    )
    .bind(only)
    .fetch_all(&state.db)
    .await?;
    let mut out: HashMap<Uuid, Vec<Label>> = HashMap::new();
    for (project_id, id, name, colour) in rows {
        out.entry(project_id).or_default().push(Label { id, name, colour });
    }
    Ok(out)
}
