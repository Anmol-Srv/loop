use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};
use crate::models::project::Project;
use crate::models::task::{Task, DISCIPLINES, TASK_COLUMNS};

/// A task as the create form describes it: enough to hand someone work.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub assignee_id: Option<Uuid>,
    #[serde(default)]
    pub discipline: Option<String>,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_priority() -> i32 {
    2
}

/// The phase every project is born with. Phases are how the schema groups
/// tasks and how the MCP surface reasons about a project's shape, but the
/// owner's flow is project → tasks with no phase in between, so one is made
/// on the project's behalf and the app never has to mention it.
pub const DEFAULT_PHASE: &str = "Work";

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
    tasks: Vec<NewTask>,
) -> AppResult<Outcome<Project>> {
    if name.trim().is_empty() {
        return Err(AppError::BadRequest("a name is required".into()));
    }
    for t in &tasks {
        check_task(t)?;
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
        "tasks": tasks,
    });

    if !actor.can_apply {
        let change_id = propose(&state.db, actor, TargetType::Project, id, Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let project: Project = sqlx::query_as(
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

    record(&mut tx, actor, TargetType::Project, project.id, Op::Create, patch).await?;

    // The default phase and the form's tasks, in the same transaction: a
    // project that exists with half its tasks is not what anyone submitted.
    let phase_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, position, name, status) VALUES ($1, 0, $2, 'active')
         RETURNING id",
    )
    .bind(id)
    .bind(DEFAULT_PHASE)
    .fetch_one(&mut *tx)
    .await?;
    for t in &tasks {
        insert_task(&mut tx, actor, phase_id, t).await?;
    }

    tx.commit().await?;

    Ok(Outcome::Applied { entity: project })
}

/// Add one task to a project after the fact, into its default phase.
///
/// Projects made before phases were hidden may have several; the first by
/// position is the one that means "the work", and a project with none gets
/// one here rather than failing.
pub async fn add_task(
    state: &AppState,
    actor: &Actor,
    project_id: Uuid,
    task: NewTask,
) -> AppResult<Task> {
    check_task(&task)?;
    let mut tx = state.db.begin().await?;

    let phase_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM phase WHERE project_id = $1 ORDER BY position LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(&mut *tx)
    .await?;
    let phase_id = match phase_id {
        Some(id) => id,
        None => sqlx::query_scalar(
            "INSERT INTO phase (project_id, position, name, status)
             VALUES ($1, 0, $2, 'active') RETURNING id",
        )
        .bind(project_id)
        .bind(DEFAULT_PHASE)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
                AppError::NotFound("project not found".into())
            }
            _ => AppError::Database(e),
        })?,
    };

    let created = insert_task(&mut tx, actor, phase_id, &task).await?;
    tx.commit().await?;
    Ok(created)
}

fn check_task(t: &NewTask) -> AppResult<()> {
    if t.title.trim().is_empty() {
        return Err(AppError::BadRequest("every task needs a title".into()));
    }
    if !(0..=4).contains(&t.priority) {
        return Err(AppError::BadRequest("priority must be between 0 and 4".into()));
    }
    if let Some(d) = t.discipline.as_deref() {
        if !DISCIPLINES.contains(&d) {
            return Err(AppError::BadRequest(format!(
                "unknown discipline '{d}'; expected one of {}",
                DISCIPLINES.join(", ")
            )));
        }
    }
    Ok(())
}

/// One task row plus its change record, inside the caller's transaction.
/// Assignment happens here too — the form assigns as it creates, and going
/// through `task::assign` afterwards would mean a second transaction and a
/// second audit row for what the user did once.
async fn insert_task(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &Actor,
    phase_id: Uuid,
    t: &NewTask,
) -> AppResult<Task> {
    let task: Task = sqlx::query_as(&format!(
        "INSERT INTO task (phase_id, title, body, priority, discipline,
                           assignee_kind, assignee_person_id)
         VALUES ($1, $2, $3, $4, $5,
                 CASE WHEN $6::uuid IS NULL THEN NULL ELSE 'human' END, $6)
         RETURNING {TASK_COLUMNS}"
    ))
    .bind(phase_id)
    .bind(t.title.trim())
    .bind(t.body.trim())
    .bind(t.priority)
    .bind(&t.discipline)
    .bind(t.assignee_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::BadRequest("the assignee is not a person here".into())
        }
        _ => AppError::Database(e),
    })?;
    record(tx, actor, TargetType::Task, task.id, Op::Create, json!(t)).await?;
    Ok(task)
}

pub async fn list(state: &AppState) -> AppResult<Vec<Project>> {
    let projects: Vec<Project> = sqlx::query_as(
        "SELECT id, key, name, description, status, created_at, updated_at
         FROM project ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(projects)
}

/// One project with its roster, for the detail screen.
pub async fn get(state: &AppState, id: Uuid) -> AppResult<Project> {
    sqlx::query_as(
        "SELECT id, key, name, description, status, created_at, updated_at
         FROM project WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("project not found".into()))
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
