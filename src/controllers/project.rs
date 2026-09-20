use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};
use crate::models::project::Project;

pub async fn create(
    state: &AppState,
    actor: &Actor,
    key: String,
    name: String,
) -> AppResult<Outcome<Project>> {
    if key.trim().is_empty() || name.trim().is_empty() {
        return Err(AppError::BadRequest("key and name are required".into()));
    }

    // The id is decided up front so a proposal can name the row it will create.
    let id = Uuid::new_v4();
    let patch = json!({ "key": key, "name": name });

    if !actor.can_apply {
        let change_id = propose(&state.db, actor, TargetType::Project, id, Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let project: Project = sqlx::query_as(
        "INSERT INTO project (id, key, name) VALUES ($1, $2, $3)
         RETURNING id, key, name, status, lead_id, created_at, updated_at",
    )
    .bind(id)
    .bind(&key)
    .bind(&name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict(format!("a project with key '{key}' already exists"))
        }
        _ => AppError::Database(e),
    })?;

    record(&mut tx, actor, TargetType::Project, project.id, Op::Create, patch).await?;

    tx.commit().await?;

    Ok(Outcome::Applied { entity: project })
}

pub async fn list(state: &AppState) -> AppResult<Vec<Project>> {
    let projects = sqlx::query_as(
        "SELECT id, key, name, status, lead_id, created_at, updated_at
         FROM project ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(projects)
}

/// Done/total for a project, and the same split per discipline.
///
/// The flow strip and the home screen both want this, so it is one function
/// with an optional project filter rather than two queries that could drift.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisciplineProgress {
    pub discipline: String,
    pub done: i64,
    pub total: i64,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProgress {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub status: String,
    pub done: i64,
    pub total: i64,
    /// Only disciplines that actually have tasks. Unlabelled tasks count
    /// towards the project total but belong to no discipline.
    pub disciplines: Vec<DisciplineProgress>,
    /// The phase currently being worked, for a one-line "where is this".
    /// None when no phase is active.
    pub active_phase: Option<String>,
    /// Distinct people holding unfinished work here. The honest measure of
    /// "who is on this" — an assignee with nothing left to do is not active.
    pub active_people: i64,
}

pub async fn progress(state: &AppState, only: Option<Uuid>) -> AppResult<Vec<ProjectProgress>> {
    // Active phase and headcount come from one extra grouped query rather than
    // being folded into the discipline rollup: mixing them would need a second
    // level of DISTINCT and the join would double-count.
    let extra: Vec<(Uuid, Option<String>, i64)> = sqlx::query_as(
        "SELECT pr.id,
                min(ph.name) FILTER (WHERE ph.status = 'active') AS active_phase,
                count(DISTINCT t.assignee_person_id)
                  FILTER (WHERE t.status NOT IN ('done', 'dropped')) AS active_people
           FROM project pr
           LEFT JOIN phase ph ON ph.project_id = pr.id
           LEFT JOIN task t ON t.phase_id = ph.id
          WHERE ($1::uuid IS NULL OR pr.id = $1)
          GROUP BY pr.id",
    )
    .bind(only)
    .fetch_all(&state.db)
    .await?;

    let rows: Vec<(Uuid, String, String, String, Option<String>, i64, i64)> = sqlx::query_as(
        "SELECT pr.id, pr.key, pr.name, pr.status, t.discipline,
                count(t.id) AS total,
                count(*) FILTER (WHERE t.status = 'done') AS done
           FROM project pr
           LEFT JOIN phase ph ON ph.project_id = pr.id
           LEFT JOIN task t ON t.phase_id = ph.id
          WHERE ($1::uuid IS NULL OR pr.id = $1)
          GROUP BY pr.id, pr.key, pr.name, pr.status, t.discipline
          ORDER BY pr.name, pr.id",
    )
    .bind(only)
    .fetch_all(&state.db)
    .await?;

    let mut out: Vec<ProjectProgress> = Vec::new();
    for (id, key, name, status, discipline, total, done) in rows {
        if out.last().map(|p| p.id) != Some(id) {
            let (active_phase, active_people) = extra
                .iter()
                .find(|(pid, _, _)| *pid == id)
                .map(|(_, phase, people)| (phase.clone(), *people))
                .unwrap_or((None, 0));
            out.push(ProjectProgress {
                id, key, name, status, done: 0, total: 0,
                disciplines: Vec::new(), active_phase, active_people,
            });
        }
        let project = out.last_mut().expect("just pushed");
        project.done += done;
        project.total += total;
        if let Some(discipline) = discipline {
            project.disciplines.push(DisciplineProgress { discipline, done, total });
        }
    }

    Ok(out)
}
