use serde_json::json;
use sqlx::PgTransaction;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{propose, record, Actor, Op, Outcome, TargetType};
use crate::controllers::note;
use crate::controllers::project::{only, writer, HeldIn};
use crate::models::task::{
    can_manage, evidence_for, next_statuses, settle, task_row_select, terminal_of, Task,
    TaskFilter, TaskRow, task_columns_t, ANYONE, CATEGORIES, HELD, LIVE, TASK_COLUMNS, TRIAGE,
};

pub enum Assignee {
    Person(String),
    Agent(String),
    Nobody,
}

pub async fn create(
    state: &AppState,
    actor: &Actor,
    phase_id: Option<Uuid>,
    project_id: Option<Uuid>,
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
        "phase_id": phase_id, "project_id": project_id, "title": title, "body": body,
        "priority": priority
    });

    if !actor.can_apply {
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Create, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;
    // Neither: a standalone task. A project alone: its first phase.
    let phase_id = match (phase_id, project_id) {
        (None, Some(project)) => Some(destination(&mut tx, project, "").await?),
        (phase, _) => phase,
    };

    let task: Task = sqlx::query_as(&format!(
        "INSERT INTO task (id, phase_id, title, body, priority, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(phase_id)
    .bind(&title)
    .bind(&body)
    .bind(priority)
    .bind(actor.person_id)
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
        "SELECT {} FROM task t
         LEFT JOIN phase p ON p.id = t.phase_id
         LEFT JOIN person per ON per.id = t.assignee_person_id
         WHERE ($1::uuid IS NULL OR p.project_id = $1)
           AND ($2::uuid IS NULL OR t.phase_id = $2)
           AND ($3::text IS NULL OR t.status = $3)
           AND ($4::text IS NULL OR per.email = $4)
           AND ($5::text IS NULL OR t.assignee_kind = $5)
           AND {LIVE}
         ORDER BY t.priority, t.created_at",
        task_columns_t()
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

/// Move a task. `expected` is the status the caller last saw; when given and
/// no longer true, nothing moves and the caller hears who got there first —
/// otherwise a stale screen silently undoes a teammate's move.
pub async fn set_status(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    status: String,
    manual_reason: Option<String>,
    expected: Option<String>,
) -> AppResult<Outcome<Task>> {
    let patch = json!({ "status": status, "manual_reason": manual_reason });

    if !actor.can_apply {
        // A proposal against a row that does not exist could never be replayed.
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;
    let task = transition(&mut tx, actor, id, status, manual_reason, expected).await?;
    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}

/// `set_status`'s rules and write, inside the caller's transaction. An agent
/// moving its delegated task and an owner approving a submission come through
/// here too, so there is one transition path however a task moves.
pub async fn transition(
    tx: &mut PgTransaction<'_>,
    actor: &Actor,
    id: Uuid,
    status: String,
    manual_reason: Option<String>,
    expected: Option<String>,
) -> AppResult<Task> {
    let patch = json!({ "status": status, "manual_reason": manual_reason });

    // Everything the rules below need, in one read inside the transaction and
    // with the row locked: two people moving the same task are serialised
    // here, so the second one's `expected` is checked against the first one's
    // result rather than against what both of them read.
    let (current, assignee, department, admin, attached, last_mover): (
        String,
        Option<Uuid>,
        Option<String>,
        bool,
        Vec<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT t.status,
                t.assignee_person_id,
                own.department,
                EXISTS (SELECT 1 FROM person WHERE id = $2 AND role = 'admin'),
                ARRAY(SELECT a.kind FROM artifact a
                       WHERE a.parent_type = 'task' AND a.parent_id = t.id),
                (SELECT coalesce(p.name, c.actor) FROM change c
                   LEFT JOIN person p ON p.id = c.on_behalf_of
                  WHERE c.target_type = 'task' AND c.target_id = t.id
                    AND c.state IN ('applied', 'approved') AND c.patch ? 'status'
                  ORDER BY c.created_at DESC LIMIT 1)
           FROM task t
           LEFT JOIN person own ON own.id = t.assignee_person_id
          WHERE t.id = $1
            FOR UPDATE OF t",
    )
    .bind(id)
    .bind(actor.person_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    if let Some(expected) = expected {
        if expected != current {
            return Err(AppError::Conflict(format!(
                "{} moved this to {} a moment ago.",
                last_mover.as_deref().unwrap_or("Someone"),
                current.replace('_', " ")
            )));
        }
    }

    let department = department.as_deref();

    // A task is moved by the person it is assigned to; an admin can override,
    // which is what an admin is for. Shipping is the exception: it records a
    // fact about production rather than about ownership, so anyone who knows
    // it went out may say so — but only from `completed`, which the table
    // enforces. Checked here, not in the route, so the CLI and a replayed
    // proposal obey the same rule.
    let reason = check_move(department, &current, &status, &attached, manual_reason)?;
    let mine = actor.person_id.is_some() && assignee == actor.person_id;
    if !admin && !mine && !ANYONE.contains(&status.as_str()) {
        return Err(AppError::Forbidden(
            "only the person this task is assigned to can move it".into(),
        ));
    }

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
    .fetch_one(&mut **tx)
    .await?;

    record(tx, actor, TargetType::Task, task.id, Op::Update, patch).await?;
    Ok(task)
}

/// Whether a task on this track may go from `current` to `status` with this
/// evidence, returning the trimmed manual reason. The transition table and the
/// evidence gate, and nothing about who is asking — an agent's submission is
/// checked against this before anything moves.
pub fn check_move(
    department: Option<&str>,
    current: &str,
    status: &str,
    attached: &[String],
    manual_reason: Option<String>,
) -> AppResult<Option<String>> {
    let next = next_statuses(department, current);
    if !next.contains(&status) {
        return Err(AppError::BadRequest(format!(
            "a {} task in {current} goes to one of {}",
            department.unwrap_or("unassigned"),
            next.join(", ")
        )));
    }

    // Leaving the work behind means saying how it was finished. The manual
    // reason is the escape hatch for work that never had a PR — it is
    // recorded rather than waved through, so the board can still answer
    // "how did this get done".
    let reason = manual_reason.map(|r| r.trim().to_owned()).filter(|r| !r.is_empty());
    if let Some(kinds) = evidence_for(department, status) {
        if reason.is_none() && !attached.iter().any(|k| kinds.contains(&k.as_str())) {
            let what = match kinds {
                ["figma"] => "a Figma link",
                _ => "a PR or commit",
            };
            return Err(AppError::BadRequest(format!(
                "attach {what} first, or say why it was done without one"
            )));
        }
    }
    Ok(reason)
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
/// Archived or not; `viewer` is who `canArchive` is worked out for.
pub async fn get(state: &AppState, id: Uuid, viewer: Option<Uuid>) -> AppResult<TaskRow> {
    sqlx::query_as(&format!("{} WHERE t.id = $1", task_row_select("$2")))
        .bind(id)
        .bind(viewer)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("task not found".into()))
}

/// A file that came with a filed message: its type, name and bytes, for
/// whoever may read the message it came with (`file_visible`).
pub async fn file(state: &AppState, id: Uuid, viewer: Option<Uuid>) -> AppResult<(String, String, Vec<u8>)> {
    let (mime, name, bytes, visible, owner): (String, String, Vec<u8>, bool, Option<String>) =
        sqlx::query_as(&format!(
            "SELECT f.mime, f.name, f.bytes, {}, (SELECT name FROM person WHERE id = t.assignee_person_id)
               FROM task_file f JOIN task t ON t.id = f.task_id WHERE f.id = $1",
            crate::models::task::file_visible("$2")
        ))
        .bind(id)
        .bind(viewer)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("That file is no longer here.".into()))?;
    if !visible {
        let owner = owner.as_deref().and_then(|n| n.split_whitespace().next()).unwrap_or("its owner");
        return Err(AppError::Forbidden(format!(
            "This file came with a direct message; only {owner} and admins can open it."
        )));
    }
    Ok((mime, name, bytes))
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
///
/// Live tasks, or with `filter.archived` only archived ones. A task counts as
/// archived while its project is, except when the list is that project's:
/// asking for one project by id is asking for its tasks, and an archived
/// project's page still shows them.
pub async fn all(state: &AppState, filter: TaskFilter, viewer: Option<Uuid>) -> AppResult<Vec<TaskRow>> {
    Ok(sqlx::query_as(&format!(
        "{} WHERE ($1::uuid IS NULL OR pr.id = $1)
               AND ($2::uuid IS NULL OR t.phase_id = $2)
               AND ($3::text IS NULL OR t.status = $3)
               AND ($4::text IS NULL OR own.email = $4)
               AND ($5::text IS NULL OR t.assignee_kind = $5)
               AND ($6::text IS NULL OR own.department = $6)
               AND (t.archived_at IS NOT NULL OR ($1::uuid IS NULL AND pr.archived_at IS NOT NULL)) = $7
           ORDER BY t.priority, t.updated_at DESC",
        task_row_select("$8")
    ))
    .bind(filter.project_id)
    .bind(filter.phase_id)
    .bind(filter.status)
    .bind(filter.assignee_email)
    .bind(filter.assignee_kind)
    .bind(filter.department)
    .bind(filter.archived)
    .bind(viewer)
    .fetch_all(&state.db)
    .await?)
}

/// Live, or with `archived` only the archived.
pub async fn mine(state: &AppState, person_id: Uuid, archived: bool) -> AppResult<Vec<TaskRow>> {
    Ok(sqlx::query_as(&format!(
        "{} WHERE t.assignee_person_id = $1
               AND (t.archived_at IS NOT NULL OR pr.archived_at IS NOT NULL) = $2
             ORDER BY t.priority, t.created_at",
        task_row_select("$1")
    ))
    .bind(person_id)
    .bind(archived)
    .fetch_all(&state.db)
    .await?)
}


/// A person taking ownership of unclaimed work.
///
/// Permanent and editorial, so it writes a `change` row. It leaves
/// `claimed_by`/`claim_expires_at` alone — relics of the retired agent lease.
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
    /// Absent leaves it alone, `null` clears it, a value sets it.
    #[serde(default, deserialize_with = "crate::models::present", skip_serializing_if = "Option::is_none")]
    pub category: Option<Option<String>>,
    /// Absent leaves it where it is, `null` makes it standalone, an id moves
    /// it into that project's first phase. Only for `can_manage` holders.
    #[serde(default, deserialize_with = "crate::models::present", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Option<Uuid>>,
    /// Absent leaves the labels alone; a list replaces them, `[]` clears.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_ids: Option<Vec<Uuid>>,
    /// The `updatedAt` the editor last saw. A precondition, not an edit, so it
    /// stays out of the audit patch — and out of a replayed proposal, which is
    /// approved against the row as it is then.
    #[serde(default, skip_serializing)]
    pub expected_updated_at: Option<chrono::DateTime<chrono::Utc>>,
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
    if let Some(Some(c)) = &details.category {
        check_category(c)?;
    }

    let patch = json!({ "details": details });
    if !actor.can_apply {
        exists(state, id).await?;
        let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Update, patch).await?;
        return Ok(Outcome::Proposed { change_id });
    }

    let mut tx = state.db.begin().await?;

    let (status, assignee, updated_at): (String, Option<Uuid>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as(
            "SELECT status, assignee_person_id, updated_at FROM task WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("task not found".into()))?;
    stale_check(details.expected_updated_at, updated_at, "task")?;
    let moved = match details.project_id {
        None => None,
        Some(project) => {
            manage(&mut tx, actor, id, "move").await?;
            Some(match project {
                Some(p) => Some(destination(&mut tx, p, "").await?),
                None => None,
            })
        }
    };

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
    let (status, finished) = settle(department.as_deref(), &status);

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
            category           = CASE WHEN $8 THEN $9 ELSE category END,
            phase_id           = CASE WHEN $10 THEN $11 ELSE phase_id END,
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
    .bind(details.category.is_some())
    .bind(details.category.flatten())
    .bind(moved.is_some())
    .bind(moved.flatten())
    .fetch_one(&mut *tx)
    .await?;
    if let Some(labels) = &details.label_ids {
        crate::controllers::label::set_on_task(&mut tx, id, labels).await?;
    }

    record(&mut tx, actor, TargetType::Task, id, Op::Update, patch).await?;
    tx.commit().await?;
    Ok(Outcome::Applied { entity: task })
}

pub fn check_category(c: &str) -> AppResult<()> {
    if CATEGORIES.contains(&c) {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!("category must be one of {}", CATEGORIES.join(", "))))
    }
}

/// Accept a task out of triage (to `open`, optionally into another project's
/// first phase) or dismiss it (to `dropped`, the reason left as a note).
/// Only from triage, and only by whoever `transition` lets move it: its
/// assignee or an admin.
pub async fn triage(
    state: &AppState,
    actor: &Actor,
    id: Uuid,
    accept: bool,
    project_id: Option<Uuid>,
    reason: Option<String>,
) -> AppResult<TaskRow> {
    writer(actor)?;
    let mut tx = state.db.begin().await?;
    let current: String = sqlx::query_scalar("SELECT status FROM task WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("task not found".into()))?;
    if current != TRIAGE {
        return Err(AppError::Conflict(format!(
            "This task is not in triage \u{2014} it is {} now, so there is nothing to {}.",
            current.replace('_', " "),
            if accept { "accept" } else { "dismiss" }
        )));
    }
    let to = if accept { "open" } else { "dropped" };
    transition(&mut tx, actor, id, to.into(), None, None).await?;

    if let (true, Some(project)) = (accept, project_id) {
        let phase = destination(&mut tx, project, ", or accept it where it is").await?;
        sqlx::query("UPDATE task SET phase_id = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(phase)
            .execute(&mut *tx)
            .await?;
        record(&mut tx, actor, TargetType::Task, id, Op::Update, json!({ "phase_id": phase })).await?;
    }
    if let Some(reason) = reason.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        note::insert(&mut tx, id, note::Author::Person(actor.person_id), "note", &format!("Dismissed: {reason}"))
            .await?;
    }
    tx.commit().await?;
    get(state, id, actor.person_id).await
}

/// The phase a task moved or filed into `project` lands in, refusing a project
/// that is gone or archived. `hint` ends the archived message.
pub(crate) async fn destination(tx: &mut PgTransaction<'_>, project: Uuid, hint: &str) -> AppResult<Uuid> {
    let archived: Option<bool> = sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM project WHERE id = $1")
        .bind(project)
        .fetch_optional(&mut **tx)
        .await?;
    match archived {
        None => Err(AppError::NotFound("That project does not exist.".into())),
        Some(true) => Err(AppError::BadRequest(format!(
            "That project is archived \u{2014} pick a live project{hint}."
        ))),
        Some(false) => first_phase(tx, project).await,
    }
}

/// A project's first phase by position, made if it has none: the phase that
/// means "the work".
pub(crate) async fn first_phase(tx: &mut PgTransaction<'_>, project: Uuid) -> AppResult<Uuid> {
    let found: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM phase WHERE project_id = $1 ORDER BY position LIMIT 1")
            .bind(project)
            .fetch_optional(&mut **tx)
            .await?;
    match found {
        Some(id) => Ok(id),
        None => Ok(sqlx::query_scalar(
            "INSERT INTO phase (project_id, position, name, status) VALUES ($1, 0, $2, 'active') RETURNING id",
        )
        .bind(project)
        .bind(crate::controllers::project::DEFAULT_PHASE)
        .fetch_one(&mut **tx)
        .await?),
    }
}

/// Refuse an edit made against a version of the row that is no longer there.
/// Called with the row locked, so nothing can land between this and the write.
/// Compared as timestamps, not strings: the client echoes what it was sent,
/// and two spellings of one instant are the same version.
pub fn stale_check(
    expected: Option<chrono::DateTime<chrono::Utc>>,
    actual: chrono::DateTime<chrono::Utc>,
    what: &str,
) -> AppResult<()> {
    match expected {
        Some(e) if e != actual => Err(AppError::Conflict(format!(
            "Someone else edited this {what} since you opened it. Your changes are still here — check theirs and save again."
        ))),
        _ => Ok(()),
    }
}

/// Lock the task and refuse anyone `can_manage` does not name. Returns
/// whether its project is archived.
async fn manage(tx: &mut PgTransaction<'_>, actor: &Actor, id: Uuid, verb: &str) -> AppResult<bool> {
    #[allow(clippy::type_complexity)]
    let (allowed, task_by, task_by_name, project_by, project_by_name, project_archived): (
        bool,
        Option<Uuid>,
        Option<String>,
        Option<Uuid>,
        Option<String>,
        bool,
    ) = sqlx::query_as(&format!(
        "SELECT {}, t.created_by, tc.name, pr.created_by, pc.name, pr.archived_at IS NOT NULL
           FROM task t
           LEFT JOIN phase ph ON ph.id = t.phase_id
           LEFT JOIN project pr ON pr.id = ph.project_id
           LEFT JOIN person tc ON tc.id = t.created_by
           LEFT JOIN person pc ON pc.id = pr.created_by
          WHERE t.id = $1
            FOR UPDATE OF t",
        can_manage("$2")
    ))
    .bind(id)
    .bind(actor.person_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;
    if !allowed {
        let mut who: Vec<String> = task_by_name.map(|n| format!("{n}, who created this task")).into_iter().collect();
        if let Some(n) = project_by_name.filter(|_| project_by != task_by) {
            who.push(format!("{n}, who created its project"));
        }
        return Err(AppError::Forbidden(only(&who, verb, "task")));
    }
    Ok(project_archived)
}

/// Refuse while an agent still holds the task, or any task in the project:
/// archiving or deleting it out from under the agent would leave it working
/// on something nobody can see.
pub(crate) async fn refuse_held(tx: &mut PgTransaction<'_>, scope: HeldIn) -> AppResult<()> {
    let (task, project) = match scope {
        HeldIn::Task(id) => (Some(id), None),
        HeldIn::Project(id) => (None, Some(id)),
    };
    let held: Option<(String, String)> = sqlx::query_as(&format!(
        "SELECT t.title, a.name FROM task t
           LEFT JOIN phase ph ON ph.id = t.phase_id
           JOIN agent a ON a.id = t.delegate_agent_id
          WHERE (t.id = $1 OR ph.project_id = $2) AND {HELD}
          LIMIT 1"
    ))
    .bind(task)
    .bind(project)
    .fetch_optional(&mut **tx)
    .await?;
    match held {
        None => Ok(()),
        Some((_, agent)) if task.is_some() => {
            Err(AppError::Conflict(format!("Take it back from {agent} first.")))
        }
        Some((title, agent)) => Err(AppError::Conflict(format!(
            "{agent} still holds \u{201c}{title}\u{201d}. Take it back from {agent} first."
        ))),
    }
}

/// Take tasks that are going for good out of every `blocked_by` that names
/// them, so nothing waits on a task that no longer exists.
pub(crate) async fn unblock(tx: &mut PgTransaction<'_>, ids: &[Uuid]) -> AppResult<()> {
    sqlx::query(
        "UPDATE task SET blocked_by = ARRAY(SELECT b FROM unnest(blocked_by) b WHERE b <> ALL($1))
          WHERE blocked_by && $1",
    )
    .bind(ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Archive a task, or restore it. One inside an archived project comes back
/// with the project, not on its own.
pub async fn set_archived(state: &AppState, actor: &Actor, id: Uuid, archived: bool) -> AppResult<TaskRow> {
    writer(actor)?;
    let mut tx = state.db.begin().await?;
    let project_archived = manage(&mut tx, actor, id, if archived { "archive" } else { "restore" }).await?;
    if archived {
        refuse_held(&mut tx, HeldIn::Task(id)).await?;
    } else if project_archived {
        return Err(AppError::Conflict(
            "Its project is archived \u{2014} restore the project to bring this back.".into(),
        ));
    }
    sqlx::query(
        "UPDATE task SET archived_at = CASE WHEN $2 THEN coalesce(archived_at, now()) END WHERE id = $1",
    )
    .bind(id)
    .bind(archived)
    .execute(&mut *tx)
    .await?;
    record(&mut tx, actor, TargetType::Task, id, Op::Update, json!({ "archived": archived })).await?;
    tx.commit().await?;
    get(state, id, actor.person_id).await
}

/// Delete a task for good. Notes, run log and agent events go by their
/// foreign keys; its artifacts and other tasks' `blocked_by` entries for it
/// have none, so they go here.
pub async fn delete(state: &AppState, actor: &Actor, id: Uuid) -> AppResult<()> {
    writer(actor)?;
    let mut tx = state.db.begin().await?;
    manage(&mut tx, actor, id, "delete").await?;
    refuse_held(&mut tx, HeldIn::Task(id)).await?;
    sqlx::query("DELETE FROM artifact WHERE parent_type = 'task' AND parent_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    unblock(&mut tx, &[id]).await?;
    let title: String = sqlx::query_scalar("DELETE FROM task WHERE id = $1 RETURNING title")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    record(&mut tx, actor, TargetType::Task, id, Op::Delete, json!({ "title": title })).await?;
    tx.commit().await?;
    Ok(())
}
