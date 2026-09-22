use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};
use crate::models::project::Project;

/// Turn a project name into a key: lowercase, words joined by hyphens.
///
/// The create form asks for a title and nothing else, because a key is a
/// detail of the URL and not a decision worth making twice. `resolve_key`
/// below is what makes the derived key unique.
pub fn slug(name: &str) -> String {
    let s: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    s.chars().take(48).collect()
}

/// The first free key of the form `slug`, `slug-2`, `slug-3`.
///
/// Two people creating "Checkout redesign" at the same instant can still
/// collide; the insert's unique violation catches that and reports a
/// conflict. This is about the ordinary case, where the second project of the
/// same name should just work.
async fn resolve_key(state: &AppState, slug: &str) -> AppResult<String> {
    let taken: Vec<String> = sqlx::query_scalar(
        "SELECT key FROM project WHERE key = $1 OR key LIKE $1 || '-%'",
    )
    .bind(slug)
    .fetch_all(&state.db)
    .await?;

    if !taken.iter().any(|k| k == slug) {
        return Ok(slug.to_owned());
    }
    for n in 2.. {
        let candidate = format!("{slug}-{n}");
        if !taken.iter().any(|k| *k == candidate) {
            return Ok(candidate);
        }
    }
    unreachable!("the loop returns")
}

pub async fn create(
    state: &AppState,
    actor: &Actor,
    key: Option<String>,
    name: String,
    description: String,
    member_ids: Vec<Uuid>,
) -> AppResult<Outcome<Project>> {
    if name.trim().is_empty() {
        return Err(AppError::BadRequest("a name is required".into()));
    }

    // A caller that names its own key means it (the CLI, a replayed proposal);
    // everything else derives one from the title.
    let key = match key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()) {
        Some(explicit) => explicit,
        None => {
            let slug = slug(&name);
            if slug.is_empty() {
                return Err(AppError::BadRequest("a name needs at least one letter or digit".into()));
            }
            resolve_key(state, &slug).await?
        }
    };

    // The id is decided up front so a proposal can name the row it will create.
    let id = Uuid::new_v4();
    let patch = json!({
        "key": key,
        "name": name,
        "description": description,
        "member_ids": member_ids,
    });

    if !actor.can_apply {
        let change_id = propose(&state.db, actor, TargetType::Project, id, Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let mut project: Project = sqlx::query_as(
        "INSERT INTO project (id, key, name, description) VALUES ($1, $2, $3, $4)
         RETURNING id, key, name, description, status, created_at, updated_at",
    )
    .bind(id)
    .bind(&key)
    .bind(&name)
    .bind(&description)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict(format!("a project with key '{key}' already exists"))
        }
        _ => AppError::Database(e),
    })?;

    // One statement for the whole roster. A person id that does not exist
    // fails the foreign key and rolls the project back with it — a project
    // with half its members is not a project anyone asked for.
    project.member_ids = sqlx::query_scalar(
        "INSERT INTO project_member (project_id, person_id)
         SELECT $1, unnest($2::uuid[]) ON CONFLICT DO NOTHING
         RETURNING person_id",
    )
    .bind(id)
    .bind(&member_ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::BadRequest("one of the members is not a person here".into())
        }
        _ => AppError::Database(e),
    })?;

    record(&mut tx, actor, TargetType::Project, project.id, Op::Create, patch).await?;

    tx.commit().await?;

    Ok(Outcome::Applied { entity: project })
}

pub async fn list(state: &AppState) -> AppResult<Vec<Project>> {
    let mut projects: Vec<Project> = sqlx::query_as(
        "SELECT id, key, name, description, status, created_at, updated_at
         FROM project ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    // Rosters in one query, then folded in — a join would repeat the project
    // row per member and make the caller de-duplicate.
    let members: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT project_id, person_id FROM project_member ORDER BY added_at",
    )
    .fetch_all(&state.db)
    .await?;
    for p in &mut projects {
        p.member_ids = members
            .iter()
            .filter(|(pid, _)| *pid == p.id)
            .map(|(_, person)| *person)
            .collect();
    }

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
