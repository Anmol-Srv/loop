use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};
use crate::models::task::{
    task_row_select, Task, TaskFilter, TaskRow, DISCIPLINES, TASK_COLUMNS, TASK_STATUSES,
};

pub enum Assignee {
    Person(String),
    Agent(String),
    Nobody,
}

pub async fn create(
    state: &AppState,
    actor: &Actor,
    phase_id: Uuid,
    title: String,
    body: String,
    priority: i32,
    discipline: Option<String>,
) -> AppResult<Outcome<Task>> {
    if title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if !(0..=4).contains(&priority) {
        return Err(AppError::BadRequest("priority must be between 0 and 4".into()));
    }
    check_discipline(discipline.as_deref())?;

    let id = Uuid::new_v4();
    let patch = json!({
        "phase_id": phase_id, "title": title, "body": body,
        "priority": priority, "discipline": discipline
    });

    if !actor.can_apply {
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let task: Task = sqlx::query_as(&format!(
        "INSERT INTO task (id, phase_id, title, body, priority, discipline)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(phase_id)
    .bind(&title)
    .bind(&body)
    .bind(priority)
    .bind(&discipline)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::NotFound("phase not found".into())
        }
        _ => AppError::Database(e),
    })?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Create, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}

/// Filters are applied with a single query using NULL-tolerant predicates, so
/// there is no dynamic SQL string building to audit.
pub async fn search(state: &AppState, filter: TaskFilter) -> AppResult<Vec<Task>> {
    let tasks = sqlx::query_as(&format!(
        "SELECT t.{} FROM task t
         JOIN phase p ON p.id = t.phase_id
         LEFT JOIN person per ON per.id = t.assignee_person_id
         WHERE ($1::uuid IS NULL OR p.project_id = $1)
           AND ($2::uuid IS NULL OR t.phase_id = $2)
           AND ($3::text IS NULL OR t.status = $3)
           AND ($4::text IS NULL OR per.email = $4)
           AND ($5::text IS NULL OR t.assignee_kind = $5)
         ORDER BY t.priority, t.created_at",
        TASK_COLUMNS.replace(", ", ", t.")
    ))
    .bind(filter.project_id)
    .bind(filter.phase_id)
    .bind(filter.status)
    .bind(filter.assignee_email)
    .bind(filter.assignee_kind)
    .fetch_all(&state.db)
    .await?;

    Ok(tasks)
}

pub async fn set_status(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    status: String,
) -> AppResult<Outcome<Task>> {
    if !TASK_STATUSES.contains(&status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "status must be one of {}", TASK_STATUSES.join(", ")
        )));
    }

    let patch = json!({ "status": status });

    if !actor.can_apply {
        // A proposal against a row that does not exist could never be replayed.
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET status = $2, updated_at = now() WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(&status)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}

pub async fn assign(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    to: Assignee,
) -> AppResult<Outcome<Task>> {
    let patch = match &to {
        Assignee::Person(email) => json!({ "person_email": email }),
        Assignee::Agent(label) => json!({ "agent_label": label }),
        Assignee::Nobody => json!({ "person_email": null }),
    };

    if !actor.can_apply {
        // A proposal against a row that does not exist could never be replayed.
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let (kind, person_id, token_id) = match &to {
        Assignee::Person(email) => {
            let pid: Uuid = sqlx::query_scalar("SELECT id FROM person WHERE email = $1 AND deleted_at IS NULL")
                .bind(email)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("no person with email '{email}'")))?;
            (Some("human"), Some(pid), None)
        }
        Assignee::Agent(label) => {
            let tid: Uuid = sqlx::query_scalar(
                "SELECT id FROM credential WHERE label = $1 AND revoked_at IS NULL AND expires_at > now()
                 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(label)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("no active agent token labelled '{label}'")))?;
            (Some("agent"), None, Some(tid))
        }
        Assignee::Nobody => (None, None, None),
    };

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET assignee_kind = $2, assignee_person_id = $3, assignee_token_id = $4,
                         updated_at = now()
         WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(kind)
    .bind(person_id)
    .bind(token_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}

/// One task, with its project and phase names. The clients used to fetch the
/// whole board to render a single task; this is that request.
pub async fn get(state: &AppState, id: Uuid) -> AppResult<TaskRow> {
    sqlx::query_as(&format!("{} WHERE t.id = $1", task_row_select()))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("task not found".into()))
}

/// Everything assigned to one person. Unfiltered by status on purpose — the
/// caller decides whether finished work still belongs on their screen.
pub async fn mine(state: &AppState, person_id: Uuid) -> AppResult<Vec<TaskRow>> {
    Ok(sqlx::query_as(&format!(
        "{} WHERE t.assignee_person_id = $1 ORDER BY t.priority, t.created_at",
        task_row_select()
    ))
    .bind(person_id)
    .fetch_all(&state.db)
    .await?)
}

/// Work this person could pick up right now: a discipline they hold, nobody
/// assigned, and every blocker done. Derived, never stored — a cached flag
/// would go stale the moment a blocker closed.
///
/// Finished and dropped work is excluded on top of those three rules. A `done`
/// task with no assignee satisfies all three and is still not work anyone can
/// pick up; leaving it in made the list read as a lie.
pub async fn available(state: &AppState, person_id: Uuid) -> AppResult<Vec<TaskRow>> {
    Ok(sqlx::query_as(&format!(
        "{} WHERE t.status NOT IN ('done', 'dropped')
             AND t.assignee_kind IS NULL
             AND t.discipline IN (SELECT unnest(p.disciplines) FROM person p WHERE p.id = $1)
             AND NOT EXISTS (
                   SELECT 1 FROM task b
                    WHERE b.id = ANY(t.blocked_by) AND b.status <> 'done')
           ORDER BY t.priority, t.created_at",
        task_row_select()
    ))
    .bind(person_id)
    .fetch_all(&state.db)
    .await?)
}

/// A person taking ownership of unclaimed work.
///
/// This is not the agent lease in `controllers::work`: it is permanent, it is
/// editorial, and so it writes a `change` row. It deliberately leaves
/// `claimed_by`/`claim_expires_at` alone — those belong to the lease, and a
/// human holding a task forever is not a lease.
pub async fn claim(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    person_id: Uuid,
) -> AppResult<Outcome<Task>> {
    let patch = json!({ "claim_person_id": person_id });

    if !actor.can_apply {
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let task: Option<Task> = sqlx::query_as(&format!(
        "UPDATE task SET assignee_kind = 'human', assignee_person_id = $2, updated_at = now()
          WHERE id = $1 AND assignee_kind IS NULL RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(person_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(task) = task else {
        // The update matched nothing: either there is no such task, or someone
        // already holds it. Those are different answers to the caller.
        exists(state, id).await?;
        return Err(AppError::Conflict("task is already assigned".into()));
    };

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}

/// Hand your own task back to the pool. Only the holder may do this.
pub async fn release(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    person_id: Uuid,
) -> AppResult<Outcome<Task>> {
    let patch = json!({ "release_person_id": person_id });

    if !actor.can_apply {
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let task: Option<Task> = sqlx::query_as(&format!(
        "UPDATE task SET assignee_kind = NULL, assignee_person_id = NULL, updated_at = now()
          WHERE id = $1 AND assignee_person_id = $2 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(person_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(task) = task else {
        exists(state, id).await?;
        return Err(AppError::Forbidden("that task is not yours to release".into()));
    };

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}

/// Replace a task's blocker list.
///
/// A cycle here is unrecoverable through the UI — every task in the ring waits
/// on the ring — so it is refused at the door rather than detected later. The
/// check walks the existing dependency graph up from each proposed blocker: if
/// the walk reaches this task, adding the edge would close a loop.
pub async fn set_blockers(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    blocked_by: Vec<Uuid>,
) -> AppResult<Outcome<Task>> {
    if blocked_by.contains(&id) {
        return Err(AppError::BadRequest("a task cannot block itself".into()));
    }

    let patch = json!({ "blocked_by": blocked_by });

    if !actor.can_apply {
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let known: i64 = sqlx::query_scalar("SELECT count(*) FROM task WHERE id = ANY($1)")
        .bind(&blocked_by)
        .fetch_one(&mut *tx)
        .await?;
    if known != blocked_by.len() as i64 {
        return Err(AppError::BadRequest("one of those blockers does not exist".into()));
    }

    let cycles: bool = sqlx::query_scalar(
        "WITH RECURSIVE deps(id) AS (
             SELECT unnest($2::uuid[])
             UNION
             SELECT unnest(t.blocked_by) FROM task t JOIN deps d ON t.id = d.id
         )
         SELECT EXISTS (SELECT 1 FROM deps WHERE id = $1)",
    )
    .bind(id)
    .bind(&blocked_by)
    .fetch_one(&mut *tx)
    .await?;
    if cycles {
        return Err(AppError::BadRequest(
            "those blockers would create a dependency cycle".into(),
        ));
    }

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET blocked_by = $2, updated_at = now()
          WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(&blocked_by)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}

fn check_discipline(discipline: Option<&str>) -> AppResult<()> {
    match discipline {
        Some(d) if !DISCIPLINES.contains(&d) => Err(AppError::BadRequest(format!(
            "unknown discipline '{d}'; expected one of {}",
            DISCIPLINES.join(", ")
        ))),
        _ => Ok(()),
    }
}

/// Label a task with a discipline, or `None` to unlabel it. Unlabelled is a
/// real answer — a chore that belongs to nobody's craft should not show up in
/// anybody's available list.
pub async fn set_discipline(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    discipline: Option<String>,
) -> AppResult<Outcome<Task>> {
    check_discipline(discipline.as_deref())?;

    let patch = json!({ "discipline": discipline });

    if !actor.can_apply {
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET discipline = $2, updated_at = now()
          WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(&discipline)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, patch).await?;

    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}

async fn exists(state: &AppState, id: Uuid) -> AppResult<()> {
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM task WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("task not found".into()))?;
    Ok(())
}
