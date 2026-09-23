use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};
use crate::models::task::{
    statuses_for, task_row_select, terminal_of, Task, TaskFilter, TaskRow, TASK_COLUMNS,
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
) -> AppResult<Outcome<Task>> {
    if title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if !(0..=4).contains(&priority) {
        return Err(AppError::BadRequest("priority must be between 0 and 4".into()));
    }

    let id = Uuid::new_v4();
    let patch = json!({
        "phase_id": phase_id, "title": title, "body": body,
        "priority": priority
    });

    if !actor.can_apply {
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let task: Task = sqlx::query_as(&format!(
        "INSERT INTO task (id, phase_id, title, body, priority)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(phase_id)
    .bind(&title)
    .bind(&body)
    .bind(priority)
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
    manual_reason: Option<String>,
) -> AppResult<Outcome<Task>> {
    let patch = json!({ "status": status, "manual_reason": manual_reason });

    if !actor.can_apply {
        // A proposal against a row that does not exist could never be replayed.
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    // Everything the rules below need, in one read: who holds it, what track
    // that puts it on, and what evidence is already attached.
    let (assignee, department, admin, prs, figmas): (
        Option<Uuid>,
        Option<String>,
        bool,
        i64,
        i64,
    ) = sqlx::query_as(
        "SELECT t.assignee_person_id,
                own.department,
                EXISTS (SELECT 1 FROM person WHERE id = $2 AND role = 'admin'),
                (SELECT count(*) FROM artifact a
                  WHERE a.parent_type = 'task' AND a.parent_id = t.id
                    AND a.kind IN ('pr', 'commit')),
                (SELECT count(*) FROM artifact a
                  WHERE a.parent_type = 'task' AND a.parent_id = t.id
                    AND a.kind = 'figma')
           FROM task t
           LEFT JOIN person own ON own.id = t.assignee_person_id
          WHERE t.id = $1",
    )
    .bind(id)
    .bind(actor.person_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    let department = department.as_deref();
    let allowed = statuses_for(department);
    if !allowed.contains(&status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "a {} task goes to one of {}",
            department.unwrap_or("unassigned"),
            allowed.join(", ")
        )));
    }

    // A task is moved by the person it is assigned to; an admin can override,
    // which is what an admin is for. Shipping is the exception: it records a
    // fact about production rather than about ownership, so anyone who knows
    // it went out may say so. Checked here, not in the route, so the CLI and
    // a replayed proposal obey the same rule.
    let mine = actor.person_id.is_some() && assignee == actor.person_id;
    if !admin && !mine && status != "shipped" {
        return Err(AppError::Forbidden(
            "only the person this task is assigned to can move it".into(),
        ));
    }

    // Leaving the work behind means saying how it was finished. The manual
    // reason is the escape hatch for work that never had a PR — it is
    // recorded rather than waved through, so the board can still answer
    // "how did this get done".
    let reason = manual_reason.map(|r| r.trim().to_owned()).filter(|r| !r.is_empty());
    let needs = match (department, status.as_str()) {
        (Some("design"), "handoff") => Some(("a Figma link", figmas > 0)),
        (Some("design"), _) => None,
        (_, "completed") => Some(("a PR or commit", prs > 0)),
        _ => None,
    };
    if let Some((what, have)) = needs {
        if !have && reason.is_none() {
            return Err(AppError::BadRequest(format!(
                "attach {what} first, or say why it was done without one"
            )));
        }
    }

    let mut tx = state.db.begin().await?;

    // `done_at` is stamped at the track's terminal state and cleared on the
    // way out, so a reopened task does not keep claiming a finish date and
    // the dashboard counts one thing across both tracks.
    let finished = status == terminal_of(department);
    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET status = $2, updated_at = now(),
                manual_reason = $4,
                done_at = CASE WHEN $3 THEN coalesce(done_at, now()) ELSE NULL END
          WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(&status)
    .bind(finished)
    .bind(&reason)
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
/// Every task in the workspace, filtered. The dashboard's table reads this.
///
/// Added because the table was built from `myTasks ∪ available.first` — a
/// personal list wearing a dashboard's clothes. `available.first` is truncated
/// to five server-side, so unclaimed work silently vanished past the fifth
/// row, and nobody else's in-flight work was ever in it, while the filters
/// advertised "All projects". A filter that cannot see all projects is a lie.
///
/// Returns the enriched row (project and phase names, blocker counts) rather
/// than the bare task, because every caller needed the join anyway.
pub async fn all(state: &AppState, filter: TaskFilter) -> AppResult<Vec<TaskRow>> {
    Ok(sqlx::query_as(&format!(
        "{} WHERE ($1::uuid IS NULL OR pr.id = $1)
               AND ($2::uuid IS NULL OR t.phase_id = $2)
               AND ($3::text IS NULL OR t.status = $3)
               AND ($4::text IS NULL OR own.email = $4)
               AND ($5::text IS NULL OR t.assignee_kind = $5)
               AND ($6::text IS NULL OR own.department = $6)
           ORDER BY t.priority, t.updated_at DESC",
        task_row_select()
    ))
    .bind(filter.project_id)
    .bind(filter.phase_id)
    .bind(filter.status)
    .bind(filter.assignee_email)
    .bind(filter.assignee_kind)
    .bind(filter.department)
    .fetch_all(&state.db)
    .await?)
}

pub async fn mine(state: &AppState, person_id: Uuid) -> AppResult<Vec<TaskRow>> {
    Ok(sqlx::query_as(&format!(
        "{} WHERE t.assignee_person_id = $1 ORDER BY t.priority, t.created_at",
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


async fn exists(state: &AppState, id: Uuid) -> AppResult<()> {
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM task WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("task not found".into()))?;
    Ok(())
}

/// The fields of a task anyone with write access may change: its words, its
/// priority, and who holds it. Status is not here — it has its own rules and
/// its own route, and only the assignee moves it.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetails {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    /// Absent leaves the assignee alone, `null` unassigns, an id reassigns.
    #[serde(default, deserialize_with = "crate::models::present")]
    pub assignee_id: Option<Option<Uuid>>,
}

/// Edit a task's details, reassigning it if asked.
///
/// Reassignment can move a task to another track, because the track is the
/// assignee's department. A status the new track does not have — a design
/// `handoff` given to an engineer — becomes `open` for the new holder, which
/// is what a handoff means: it is theirs to start. `done_at` is recomputed
/// against the new track's finish line in the same statement, so the two can
/// never disagree.
pub async fn update_details(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    details: TaskDetails,
) -> AppResult<Outcome<Task>> {
    if let Some(title) = &details.title {
        if title.trim().is_empty() {
            return Err(AppError::BadRequest("a task needs a title".into()));
        }
    }
    if let Some(p) = details.priority {
        if !(0..=4).contains(&p) {
            return Err(AppError::BadRequest("priority must be between 0 and 4".into()));
        }
    }

    let patch = json!({ "details": details });
    if !actor.can_apply {
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let (status, assignee): (String, Option<Uuid>) =
        sqlx::query_as("SELECT status, assignee_person_id FROM task WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    let assignee = details.assignee_id.unwrap_or(assignee);
    let department: Option<String> = match assignee {
        Some(person) => Some(
            sqlx::query_scalar("SELECT department FROM person WHERE id = $1 AND deleted_at IS NULL")
                .bind(person)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| AppError::BadRequest("the assignee is not a person here".into()))?,
        ),
        None => None,
    };
    let department = department.as_deref();
    let status = if statuses_for(department).contains(&status.as_str()) {
        status
    } else {
        "open".to_owned()
    };
    let finished = status == terminal_of(department);

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET
            title              = coalesce($2, title),
            body               = coalesce($3, body),
            priority           = coalesce($4, priority),
            assignee_kind      = CASE WHEN $5::uuid IS NULL THEN NULL ELSE 'human' END,
            assignee_person_id = $5,
            assignee_token_id  = NULL,
            status             = $6,
            done_at            = CASE WHEN $7 THEN coalesce(done_at, now()) ELSE NULL END,
            updated_at         = now()
          WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(details.title.as_deref().map(str::trim))
    .bind(details.body.as_deref().map(str::trim))
    .bind(details.priority)
    .bind(assignee)
    .bind(&status)
    .bind(finished)
    .fetch_one(&mut *tx)
    .await?;

    record(&mut tx, actor, TargetType::Task, id, Op::Update, patch).await?;
    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}
