//! Personal agents: the rows people own, the tasks they hand them, and what
//! flows back and forth on those tasks.
//!
//! An agent acts only on tasks delegated to it, and every agent write runs in
//! a transaction marked with `acp.agent_id`, so the triggers that turn
//! dashboard changes into agent events do not echo an agent's own writes back
//! at it. Status moves go through `task::transition`, the one transition path,
//! acting as the task's assignee — the person who handed it off.

use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use sqlx::postgres::{PgListener, PgPoolOptions};
use sqlx::PgTransaction;
use uuid::Uuid;

use crate::controllers::note::{self, Author, Note};
use crate::controllers::{artifact, project, task, token};
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::Actor;
use crate::models::task::{next_statuses, TaskRow, HELD};

pub const RUNTIMES: [&str; 4] = ["hermes", "claude-code", "codex", "other"];

/// Compiled in, so the skill an agent downloads always matches the server it
/// talks to.
pub const SKILL: &str = include_str!("../../agent-kit/airtribe-agent/SKILL.md");

/// The skill's `version:` line. The inbox names it, so a changed skill changes
/// the inbox and wakes every connected agent to fetch the new one.
fn skill_version() -> &'static str {
    SKILL.lines().find_map(|l| l.strip_prefix("version:")).map(str::trim).unwrap_or("0")
}

fn onboarding_template(runtime: &str) -> &'static str {
    match runtime {
        "hermes" => include_str!("../../agent-kit/onboarding/hermes.md"),
        "claude-code" => include_str!("../../agent-kit/onboarding/claude-code.md"),
        "codex" => include_str!("../../agent-kit/onboarding/codex.md"),
        _ => include_str!("../../agent-kit/onboarding/other.md"),
    }
}

/// An agent as its owner sees it.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: Uuid,
    pub handle: String,
    pub name: String,
    pub runtime: String,
    /// waiting (never said hello), connected, or revoked.
    pub status: String,
    pub connected_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Delegated tasks that are not finished.
    pub active_tasks: i64,
    /// What the agent reported at hello: `{skill?, mcp?, watcher?}`.
    pub setup: Value,
    /// The held task it was handed most recently: `{id, title, state, now}`.
    pub current_task: Option<Value>,
    /// Notes it wrote per day over the last 14 days, oldest first.
    pub activity: Vec<i64>,
}

fn agent_select() -> String {
    format!(
        "SELECT a.id, a.handle, a.name, a.runtime,
            CASE WHEN a.revoked_at IS NOT NULL THEN 'revoked'
                 WHEN a.connected_at IS NOT NULL THEN 'connected'
                 ELSE 'waiting' END AS status,
            a.connected_at, a.last_seen_at, a.created_at,
            (SELECT count(*) FROM task t
              WHERE t.delegate_agent_id = a.id AND t.done_at IS NULL AND t.status <> 'dropped') AS active_tasks,
            a.setup,
            (SELECT json_build_object('id', t.id, 'title', t.title, 'state', t.agent_state, 'now', t.agent_now)
               FROM task t WHERE t.delegate_agent_id = a.id AND {HELD}
              ORDER BY t.delegated_at DESC NULLS LAST LIMIT 1) AS current_task,
            ARRAY(SELECT count(n.id) FROM generate_series(0, 13) i
                    LEFT JOIN note n ON n.agent_id = a.id AND n.created_at::date = current_date - 13 + i
                   GROUP BY i ORDER BY i) AS activity
       FROM agent a"
    )
}

/// A new or rotated agent: the token and the prompt that carries it are shown
/// once, here, and never stored anywhere they can be read again.
#[derive(Debug, Serialize)]
pub struct Minted {
    pub agent: Agent,
    pub token: String,
    pub prompt: String,
}

async fn one(db: impl sqlx::PgExecutor<'_>, id: Uuid) -> AppResult<Agent> {
    Ok(sqlx::query_as(&format!("{} WHERE a.id = $1", agent_select()))
        .bind(id)
        .fetch_one(db)
        .await?)
}

pub async fn list(state: &AppState, owner: Uuid) -> AppResult<Vec<Agent>> {
    Ok(sqlx::query_as(&format!(
        "{} WHERE a.owner_id = $1 ORDER BY a.revoked_at IS NOT NULL, a.created_at DESC",
        agent_select()
    ))
    .bind(owner)
    .fetch_all(&state.db)
    .await?)
}

pub async fn create(
    state: &AppState,
    owner: Uuid,
    handle: &str,
    name: &str,
    runtime: &str,
    server: &str,
) -> AppResult<Minted> {
    let handle = handle.trim().to_lowercase();
    if handle.is_empty()
        || !handle.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AppError::BadRequest(
            "a handle is one word: letters, digits, '-' or '_'".into(),
        ));
    }
    let name = match name.trim() {
        "" => handle.clone(),
        n => n.to_owned(),
    };
    if !RUNTIMES.contains(&runtime) {
        return Err(AppError::BadRequest(format!("runtime must be one of {}", RUNTIMES.join(", "))));
    }

    let mut tx = state.db.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO agent (owner_id, handle, name, runtime) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(owner)
    .bind(&handle)
    .bind(&name)
    .bind(runtime)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict(format!("you already have an agent called '{handle}'"))
        }
        _ => AppError::Database(e),
    })?;
    let (raw, _) = token::mint_agent(&mut tx, id, owner, &handle).await?;
    let agent = one(&mut *tx, id).await?;
    tx.commit().await?;

    let prompt = prompt(state, owner, &agent, &raw, server).await?;
    Ok(Minted { agent, token: raw, prompt })
}

/// A fresh token under the same agent. Every live one it had stops working,
/// so a leaked token is ended by rotating, without losing the agent's tasks.
pub async fn rotate(state: &AppState, owner: Uuid, id: Uuid, server: &str) -> AppResult<Minted> {
    let mut tx = state.db.begin().await?;
    let handle: String = sqlx::query_scalar(
        "SELECT handle FROM agent WHERE id = $1 AND owner_id = $2 AND revoked_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("no such agent of yours".into()))?;
    sqlx::query("UPDATE credential SET revoked_at = now() WHERE agent_id = $1 AND revoked_at IS NULL")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let (raw, _) = token::mint_agent(&mut tx, id, owner, &handle).await?;
    let agent = one(&mut *tx, id).await?;
    tx.commit().await?;

    let prompt = prompt(state, owner, &agent, &raw, server).await?;
    Ok(Minted { agent, token: raw, prompt })
}

/// Revoke an agent: its tokens stop working and every unfinished task it held
/// goes back to its owner, which tells the agent `taken_back` in case it is
/// still listening. Someone else's agent reads as one that does not exist.
pub async fn revoke(state: &AppState, owner: Uuid, id: Uuid) -> AppResult<Agent> {
    let mut tx = state.db.begin().await?;
    let found = sqlx::query(
        "UPDATE agent SET revoked_at = now() WHERE id = $1 AND owner_id = $2 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(owner)
    .execute(&mut *tx)
    .await?;
    if found.rows_affected() == 0 {
        return Err(AppError::NotFound("no such agent of yours".into()));
    }
    sqlx::query("UPDATE credential SET revoked_at = now() WHERE agent_id = $1 AND revoked_at IS NULL")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE task SET delegate_agent_id = NULL, agent_state = 'stopped', review_target = NULL
          WHERE delegate_agent_id = $1 AND done_at IS NULL AND status <> 'dropped'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let agent = one(&mut *tx, id).await?;
    tx.commit().await?;
    Ok(agent)
}

/// Fill the `{{…}}` placeholders the agent-kit templates use.
pub fn render(template: &str, server: &str, handle: &str, name: &str, owner: &str) -> String {
    template
        .replace("{{server}}", server)
        .replace("{{handle}}", handle)
        .replace("{{name}}", name)
        .replace("{{owner}}", owner)
}

async fn owner_name(db: impl sqlx::PgExecutor<'_>, owner: Uuid) -> AppResult<String> {
    Ok(sqlx::query_scalar("SELECT name FROM person WHERE id = $1").bind(owner).fetch_one(db).await?)
}

/// The paste-once text that connects an agent. It carries the token, and
/// points at setup steps that only ever refer to it.
async fn prompt(state: &AppState, owner: Uuid, agent: &Agent, token: &str, server: &str) -> AppResult<String> {
    let owner = owner_name(&state.db, owner).await?;
    Ok(format!(
        "Set yourself up as {owner}'s agent in Airtribe Control Plane. This is a one-time setup \
         of your own environment — not a coding task, so no repository, branch or triage is \
         involved.\n\
         \n\
         Agent: \"{handle}\"\n\
         Server: {server}\n\
         Token: {token} (secret — store it as the steps say, never print it again)\n\
         \n\
         Your setup steps are one fetch away — run this with the token above, read the whole \
         output, and follow it step by step:\n\
         \n\
         curl -fsS --oauth2-bearer \"<token>\" \"{server}/api/agent/onboarding?runtime={runtime}\"\n\
         \n\
         Then tell {owner} what you set up and anything that failed.\n",
        handle = agent.handle,
        runtime = agent.runtime,
    ))
}

// ---- The agent's own view --------------------------------------------------

/// Who the agent is: its row plus the handle, name and owner the templates
/// are rendered with.
pub struct Identity {
    pub agent: Agent,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub owner_email: String,
}

pub async fn identity(state: &AppState, id: Uuid) -> AppResult<Identity> {
    let agent = one(&state.db, id).await?;
    let (owner_id, owner_name, owner_email): (Uuid, String, String) = sqlx::query_as(
        "SELECT p.id, p.name, p.email FROM agent a JOIN person p ON p.id = a.owner_id WHERE a.id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(Identity { agent, owner_id, owner_name, owner_email })
}

pub async fn me(state: &AppState, id: Uuid, server: &str) -> AppResult<Value> {
    let who = identity(state, id).await?;
    Ok(json!({
        "agent": who.agent,
        "owner": { "id": who.owner_id, "name": who.owner_name, "email": who.owner_email },
        "server": server,
    }))
}

/// What an agent's setup reported at hello, shown to its owner as a checklist.
#[derive(Debug, Default, Serialize, serde::Deserialize)]
pub struct Setup {
    /// The skill `version:` it installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watcher: Option<bool>,
}

/// First contact flips the dashboard from "waiting" to "connected". Saying
/// hello again is harmless: it keeps the first contact time, and a hello
/// without `setup` keeps the last one reported.
pub async fn hello(
    state: &AppState,
    id: Uuid,
    runtime: Option<&str>,
    setup: Option<Setup>,
    server: &str,
) -> AppResult<Value> {
    if let Some(r) = runtime {
        if !RUNTIMES.contains(&r) {
            return Err(AppError::BadRequest(format!("runtime must be one of {}", RUNTIMES.join(", "))));
        }
    }
    sqlx::query(
        "UPDATE agent SET connected_at = coalesce(connected_at, now()), last_seen_at = now(),
                          runtime = coalesce($2, runtime), setup = coalesce($3, setup)
          WHERE id = $1",
    )
    .bind(id)
    .bind(runtime)
    .bind(setup.map(|s| json!(s)))
    .execute(&state.db)
    .await?;
    me(state, id, server).await
}

pub async fn skill(state: &AppState, id: Uuid, server: &str) -> AppResult<String> {
    let who = identity(state, id).await?;
    Ok(render(SKILL, server, &who.agent.handle, &who.agent.name, &who.owner_name))
}

pub async fn onboarding(state: &AppState, id: Uuid, runtime: Option<&str>, server: &str) -> AppResult<String> {
    let who = identity(state, id).await?;
    let runtime = runtime.unwrap_or(&who.agent.runtime);
    if !RUNTIMES.contains(&runtime) {
        return Err(AppError::BadRequest(format!("runtime must be one of {}", RUNTIMES.join(", "))));
    }
    Ok(render(onboarding_template(runtime), server, &who.agent.handle, &who.agent.name, &who.owner_name))
}

pub async fn tasks(state: &AppState, id: Uuid) -> AppResult<Vec<TaskRow>> {
    Ok(sqlx::query_as(&format!(
        "{} WHERE t.delegate_agent_id = $1 AND t.archived_at IS NULL AND pr.archived_at IS NULL
          ORDER BY t.priority, t.updated_at DESC",
        crate::models::task::task_row_select("NULL::uuid")
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?)
}

/// A task this task waits on, or one waiting on it, with the evidence a
/// worker needs from it: the backend PR a frontend task wires to, the Figma a
/// UI task builds from.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Related {
    #[serde(skip)]
    relation: String,
    id: Uuid,
    title: String,
    status: String,
    assignee_name: Option<String>,
    department: Option<String>,
    evidence: Value,
}

/// Everything a worker needs to start without searching.
pub async fn context(state: &AppState, id: Uuid, task_id: Uuid) -> AppResult<Value> {
    let row = task::get(state, task_id, None).await?;
    if row.task.delegate.as_ref().and_then(|d| d["id"].as_str()) != Some(id.to_string().as_str()) {
        return Err(not_delegated(task_id));
    }
    let department = row.discipline.as_deref();
    let track = if department == Some("design") { "design" } else { "eng" };
    let allowed = next_statuses(department, &row.task.status);

    let project = project::get(state, row.project_id, None).await?;
    let resources = artifact::list(state, None, "project".into(), row.project_id).await?;
    let owner: Option<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT id, name, email, department FROM person WHERE id = $1",
    )
    .bind(row.task.assignee_person_id)
    .fetch_optional(&state.db)
    .await?;
    let notes = note::list(state, task_id, row.task.assignee_person_id).await?;

    let mut artifacts = serde_json::Map::new();
    for a in artifact::list(state, None, "task".into(), task_id).await?.into_iter().rev() {
        let kind = a.kind.clone();
        artifacts.entry(kind).or_insert_with(|| json!([])).as_array_mut().unwrap().push(json!(a));
    }

    let related: Vec<Related> = sqlx::query_as(
        "SELECT CASE WHEN t.id = ANY($2) THEN 'blockedBy' ELSE 'blocks' END AS relation,
                t.id, t.title, t.status, own.name AS assignee_name, own.department,
                coalesce((SELECT json_agg(json_build_object('kind', a.kind, 'url', a.url, 'title', a.title)
                                          ORDER BY a.created_at)
                            FROM artifact a
                           WHERE a.parent_type = 'task' AND a.parent_id = t.id
                             AND a.kind IN ('pr', 'commit', 'figma')), '[]') AS evidence
           FROM task t
           LEFT JOIN person own ON own.id = t.assignee_person_id
          WHERE t.id = ANY($2) OR $1 = ANY(t.blocked_by)
          ORDER BY t.created_at",
    )
    .bind(task_id)
    .bind(&row.task.blocked_by)
    .fetch_all(&state.db)
    .await?;
    let (blocked_by, blocks): (Vec<Related>, Vec<Related>) =
        related.into_iter().partition(|r| r.relation == "blockedBy");

    Ok(json!({
        "task": row,
        "track": track,
        "allowedNext": allowed,
        "project": {
            "id": project.id,
            "name": project.name,
            "description": project.description,
            "labels": project.labels,
            "resources": resources,
        },
        "owner": owner.map(|(id, name, email, department)| json!({
            "id": id, "name": name, "email": email, "department": department,
        })),
        "notes": notes,
        "artifacts": artifacts,
        "related": { "blockedBy": blocked_by, "blocks": blocks },
    }))
}

fn not_delegated(task_id: Uuid) -> AppError {
    AppError::Forbidden(format!(
        "task {task_id} is not handed off to you (it may have been taken back); check your inbox"
    ))
}

/// What an agent action needs about the task, read with the row locked in a
/// transaction marked as this agent's.
struct Delegated {
    tx: PgTransaction<'static>,
    actor: Actor,
    status: String,
    agent_state: Option<String>,
    department: Option<String>,
    attached: Vec<String>,
}

async fn delegated(state: &AppState, agent: Uuid, task_id: Uuid) -> AppResult<Delegated> {
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT set_config('acp.agent_id', $1, true)")
        .bind(agent.to_string())
        .execute(&mut *tx)
        .await?;
    let (status, agent_state, finished, department, attached, name, owner): (
        String,
        Option<String>,
        bool,
        Option<String>,
        Vec<String>,
        String,
        Uuid,
    ) = sqlx::query_as(
        "SELECT t.status, t.agent_state, t.done_at IS NOT NULL, own.department,
                ARRAY(SELECT a.kind FROM artifact a WHERE a.parent_type = 'task' AND a.parent_id = t.id),
                ag.name, ag.owner_id
           FROM task t
           JOIN agent ag ON ag.id = t.delegate_agent_id
           LEFT JOIN person own ON own.id = t.assignee_person_id
          WHERE t.id = $1 AND t.delegate_agent_id = $2
            FOR UPDATE OF t",
    )
    .bind(task_id)
    .bind(agent)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| not_delegated(task_id))?;
    // A dropped, stopped or finished task keeps its delegate as history, but
    // the agent no longer holds it: a question posted after a drop must not
    // reopen it. An approved engineering task at `completed` is not finished —
    // the agent may still ask to ship it.
    if status == "dropped" || finished || agent_state.as_deref() == Some("stopped") {
        return Err(AppError::Forbidden(format!(
            "task {task_id} is {} — you no longer hold it; stop work on it and check your inbox",
            if status == "dropped" { "dropped" } else { "finished" }
        )));
    }

    // The agent moves the task as the person who handed it off, so the track
    // rules and the assignee check in `task::transition` apply unchanged.
    let actor = Actor { label: format!("{name} (agent)"), person_id: Some(owner), can_apply: true };
    Ok(Delegated { tx, actor, status, agent_state, department, attached })
}

async fn set_state(tx: &mut PgTransaction<'_>, task_id: Uuid, state: &str, review_target: Option<&str>) -> AppResult<()> {
    sqlx::query("UPDATE task SET agent_state = $2, review_target = $3 WHERE id = $1")
        .bind(task_id)
        .bind(state)
        .bind(review_target)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn ack(state: &AppState, agent: Uuid, task_id: Uuid) -> AppResult<TaskRow> {
    let mut d = delegated(state, agent, task_id).await?;
    if d.agent_state.as_deref() == Some("handed_off") {
        set_state(&mut d.tx, task_id, "acknowledged", None).await?;
    }
    d.tx.commit().await?;
    task::get(state, task_id, None).await
}

/// The moves an update may make. Finishing a task is a submission, because
/// only the owner decides it is finished.
const UPDATE_TARGETS: [&str; 3] = ["open", "in_progress", "blocked"];

pub async fn update(
    state: &AppState,
    agent: Uuid,
    task_id: Uuid,
    body: &str,
    status: Option<String>,
    expected_status: Option<String>,
    now: Option<&str>,
) -> AppResult<TaskRow> {
    let now = now.map(now_line).transpose()?;
    let mut d = delegated(state, agent, task_id).await?;
    if let Some(status) = status {
        if !UPDATE_TARGETS.contains(&status.as_str()) {
            return Err(AppError::BadRequest(format!(
                "an update can move a task to {}; to finish it, attach the evidence and submit it for review",
                UPDATE_TARGETS.join(", ")
            )));
        }
        if status != d.status {
            task::transition(&mut d.tx, &d.actor, task_id, status, None, expected_status).await?;
        }
    }
    note::insert(&mut d.tx, task_id, Author::Agent(agent), "progress", body).await?;
    set_state(&mut d.tx, task_id, "working", None).await?;
    if let Some(now) = now {
        set_now(&mut d.tx, task_id, &now).await?;
    }
    d.tx.commit().await?;
    task::get(state, task_id, None).await
}

/// A now line: one trimmed sentence, 1–120 characters.
fn now_line(text: &str) -> AppResult<String> {
    let text = text.trim();
    let n = text.chars().count();
    if n == 0 || n > 120 {
        return Err(AppError::BadRequest(format!(
            "a now line is 1 to 120 characters saying what you are doing right now \
             (e.g. \"running the checkout tests\"); this one is {n}"
        )));
    }
    Ok(text.to_owned())
}

/// Cleared by the `task_agent_session` trigger once the agent stops working.
async fn set_now(tx: &mut PgTransaction<'_>, task_id: Uuid, text: &str) -> AppResult<()> {
    sqlx::query("UPDATE task SET agent_now = $2, agent_now_at = now() WHERE id = $1")
        .bind(task_id)
        .bind(text)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Say what you are doing right now. It marks you working; while the task
/// waits on your owner there is nothing to be doing on it.
pub async fn now(state: &AppState, agent: Uuid, task_id: Uuid, text: &str) -> AppResult<TaskRow> {
    let text = now_line(text)?;
    let mut d = delegated(state, agent, task_id).await?;
    match d.agent_state.as_deref() {
        Some("needs_input") => {
            return Err(AppError::Conflict(
                "this task is waiting on your owner's answer; post a now line once they answer and you are working again".into(),
            ))
        }
        Some("in_review") => {
            return Err(AppError::Conflict(
                "this task is waiting on your owner's review; post a now line if they send it back".into(),
            ))
        }
        Some("working") => {}
        _ => set_state(&mut d.tx, task_id, "working", None).await?,
    }
    set_now(&mut d.tx, task_id, &text).await?;
    d.tx.commit().await?;
    task::get(state, task_id, None).await
}

const LOG_MAX_LINES: usize = 200;
const LOG_MAX_CHARS: usize = 2000;

/// Append lines to the task's step log, numbered on from the last one. The
/// task row is locked by `delegated`, so two posts cannot take the same seq.
pub async fn log(state: &AppState, agent: Uuid, task_id: Uuid, lines: &[String]) -> AppResult<Value> {
    if lines.is_empty() || lines.len() > LOG_MAX_LINES {
        return Err(AppError::BadRequest(format!(
            "send 1 to {LOG_MAX_LINES} lines per request; you sent {}",
            lines.len()
        )));
    }
    if let Some(i) = lines.iter().position(|l| l.chars().count() > LOG_MAX_CHARS) {
        return Err(AppError::BadRequest(format!(
            "line {} is over {LOG_MAX_CHARS} characters; split or trim it",
            i + 1
        )));
    }
    let mut d = delegated(state, agent, task_id).await?;
    let last: i64 = sqlx::query_scalar(
        "WITH added AS (
           INSERT INTO run_log_line (task_id, seq, text)
           SELECT $1, coalesce((SELECT max(seq) FROM run_log_line WHERE task_id = $1), 0) + u.n, u.line
             FROM unnest($2::text[]) WITH ORDINALITY AS u(line, n)
           RETURNING seq)
         SELECT max(seq) FROM added",
    )
    .bind(task_id)
    .bind(lines)
    .fetch_one(&mut *d.tx)
    .await?;
    d.tx.commit().await?;
    Ok(json!({ "appended": lines.len(), "lastSeq": last }))
}

pub async fn ask(state: &AppState, agent: Uuid, task_id: Uuid, body: &str) -> AppResult<TaskRow> {
    let mut d = delegated(state, agent, task_id).await?;
    note::insert(&mut d.tx, task_id, Author::Agent(agent), "question", body).await?;
    set_state(&mut d.tx, task_id, "needs_input", None).await?;
    d.tx.commit().await?;
    task::get(state, task_id, None).await
}

pub async fn note(state: &AppState, agent: Uuid, task_id: Uuid, body: &str) -> AppResult<Note> {
    let mut d = delegated(state, agent, task_id).await?;
    let note = note::insert(&mut d.tx, task_id, Author::Agent(agent), "note", body).await?;
    d.tx.commit().await?;
    Ok(note)
}

pub async fn attach(
    state: &AppState,
    agent: Uuid,
    task_id: Uuid,
    kind: &str,
    url: &str,
    title: &str,
) -> AppResult<crate::models::artifact::Artifact> {
    artifact::validate("task", kind, url)?;
    let mut d = delegated(state, agent, task_id).await?;
    let a = artifact::insert(&mut d.tx, &d.actor, "task", task_id, kind, url, title).await?;
    d.tx.commit().await?;
    Ok(a)
}

/// Ask the owner to finish the task. The move and its evidence are checked
/// now, against the same table and gate a person's move is, so a submission
/// the owner approves is one that will apply.
pub async fn submit(
    state: &AppState,
    agent: Uuid,
    task_id: Uuid,
    target: &str,
    summary: &str,
    manual_reason: Option<String>,
) -> AppResult<TaskRow> {
    let mut d = delegated(state, agent, task_id).await?;
    let finishing: &[&str] = match d.department.as_deref() {
        Some("design") => &["handoff", "completed"],
        _ => &["completed", "shipped"],
    };
    if !finishing.contains(&target) {
        return Err(AppError::BadRequest(format!(
            "submit is for finishing this task: target is one of {}; use update for other moves",
            finishing.join(", ")
        )));
    }
    if summary.trim().is_empty() {
        return Err(AppError::BadRequest(
            "a submission needs a summary your owner can check: what you did and how you verified it".into(),
        ));
    }
    let reason = task::check_move(d.department.as_deref(), &d.status, target, &d.attached, manual_reason)?;

    note::insert(&mut d.tx, task_id, Author::Agent(agent), "submission", summary).await?;
    sqlx::query(
        "UPDATE task SET agent_state = 'in_review', review_target = $2, manual_reason = $3 WHERE id = $1",
    )
    .bind(task_id)
    .bind(target)
    .bind(&reason)
    .execute(&mut *d.tx)
    .await?;
    d.tx.commit().await?;
    task::get(state, task_id, None).await
}

// ---- Events and the inbox --------------------------------------------------

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: i64,
    pub task_id: Uuid,
    pub task_title: String,
    pub kind: String,
    pub payload: Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

async fn events_after(state: &AppState, agent: Uuid, after: i64, limit: i64) -> AppResult<Vec<Event>> {
    Ok(sqlx::query_as(
        "SELECT e.id, e.task_id, t.title AS task_title, e.kind, e.payload, e.created_at
           FROM agent_event e JOIN task t ON t.id = e.task_id
          WHERE e.agent_id = $1 AND e.id > $2
          ORDER BY e.id LIMIT $3",
    )
    .bind(agent)
    .bind(after)
    .bind(limit)
    .fetch_all(&state.db)
    .await?)
}

/// The longest a feed request is held open. Under the idle timeouts of the
/// proxies in front of the server.
pub const MAX_WAIT: u64 = 25;

/// Events after `after` (default: what the agent last acknowledged). With
/// `wait`, an empty answer is held until an event arrives or `wait` seconds
/// pass.
///
/// The wait listens on its own connection rather than one from the pool:
/// held connections would starve the app of the ten it has.
/// ponytail: one Postgres connection per waiting agent. A team's worth is
/// nothing; share one listener across requests if there are ever hundreds.
pub async fn events(state: &AppState, agent: Uuid, after: Option<i64>, wait: u64) -> AppResult<Vec<Event>> {
    let after = match after {
        Some(a) => a,
        None => sqlx::query_scalar("SELECT event_cursor FROM agent WHERE id = $1")
            .bind(agent)
            .fetch_one(&state.db)
            .await?,
    };
    let found = events_after(state, agent, after, 100).await?;
    if !found.is_empty() || wait == 0 {
        return Ok(found);
    }

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*state.db.connect_options()).clone())
        .await?;
    let mut listener = PgListener::connect_with(&pool).await?;
    listener.listen("agent_event").await?;
    // Read again now that we are listening: an event committed between the
    // first read and the LISTEN would otherwise wait out the whole timeout.
    let mut found = events_after(state, agent, after, 100).await?;
    if found.is_empty() {
        let me = agent.to_string();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(wait.min(MAX_WAIT));
        loop {
            match tokio::time::timeout_at(deadline, listener.recv()).await {
                Err(_) => break,
                Ok(Ok(n)) if n.payload() == me => {
                    found = events_after(state, agent, after, 100).await?;
                    break;
                }
                Ok(Ok(_)) => continue,
                Ok(Err(e)) => return Err(e.into()),
            }
        }
    }
    drop(listener);
    pool.close().await;
    Ok(found)
}

/// Mark events up to `through` handled. Never moves backwards, and never past
/// the last event that exists, so an overshoot cannot hide future ones.
pub async fn ack_events(state: &AppState, agent: Uuid, through: i64) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "UPDATE agent SET event_cursor = greatest(event_cursor, least($2,
                  (SELECT coalesce(max(id), 0) FROM agent_event WHERE agent_id = $1)))
          WHERE id = $1 RETURNING event_cursor",
    )
    .bind(agent)
    .bind(through)
    .fetch_one(&state.db)
    .await?)
}

fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

fn summarise(e: &Event) -> String {
    let p = &e.payload;
    let text = |k: &str| p[k].as_str().unwrap_or_default();
    match e.kind.as_str() {
        "changed" => p["values"]
            .as_object()
            .map(|v| {
                v.iter()
                    .map(|(k, v)| match v {
                        Value::String(s) => format!("{k} = {}", one_line(s, 120)),
                        other => format!("{k} = {other}"),
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default(),
        "note" | "answer" | "instruction" | "approved" | "changes_requested" => {
            format!("{}: {}", p["author"].as_str().unwrap_or("someone"), one_line(text("body"), 200))
        }
        "artifact" => format!("{} {}", text("kind"), text("url")),
        "taken_back" | "dropped" => "stop work on it now".into(),
        _ => String::new(),
    }
}

/// A plain-text summary that stays byte-identical until something changes:
/// no clocks, no counters that tick. A monitor that diffs it wakes the model
/// only when there is something new.
pub async fn inbox(state: &AppState, agent: Uuid) -> AppResult<String> {
    let who = identity(state, agent).await?;
    let cursor: i64 = sqlx::query_scalar("SELECT event_cursor FROM agent WHERE id = $1")
        .bind(agent)
        .fetch_one(&state.db)
        .await?;
    let waiting: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, title FROM task
          WHERE delegate_agent_id = $1 AND agent_state = 'handed_off'
          ORDER BY priority, created_at, id",
    )
    .bind(agent)
    .fetch_all(&state.db)
    .await?;
    const SHOWN: i64 = 50;
    let events = events_after(state, agent, cursor, SHOWN + 1).await?;

    let mut out = format!(
        "Airtribe inbox for {} ({}), working for {}.\n\
         Skill: airtribe-agent {}. If your installed copy has a different `version:`, \
         download it again from /api/agent/skill before acting.\n",
        who.agent.name, who.agent.handle, who.owner_name, skill_version()
    );
    if waiting.is_empty() && events.is_empty() {
        out.push_str("\nNothing needs you.\n");
        return Ok(out);
    }
    if !waiting.is_empty() {
        out.push_str("\nHanded off to you, not yet acknowledged:\n");
        for (id, title) in &waiting {
            out.push_str(&format!("- {} [{id}]\n", one_line(title, 120)));
        }
    }
    if !events.is_empty() {
        out.push_str("\nEvents to handle, oldest first:\n");
        for e in events.iter().take(SHOWN as usize) {
            let summary = summarise(e);
            let sep = if summary.is_empty() { "" } else { ": " };
            out.push_str(&format!(
                "- #{} {} on \"{}\" [{}]{sep}{summary}\n",
                e.id,
                e.kind,
                one_line(&e.task_title, 120),
                e.task_id
            ));
        }
        if events.len() as i64 > SHOWN {
            out.push_str("- … more after these; acknowledge these to see them\n");
        }
        out.push_str("\nAcknowledge events once handled (events_ack with the highest id).\n");
    }
    Ok(out)
}

// ---- The owner's side ------------------------------------------------------

/// The task, if `person` is its assignee, locked for the owner action.
async fn owned(
    tx: &mut PgTransaction<'_>,
    person: Uuid,
    task_id: Uuid,
    what: &str,
) -> AppResult<(Option<Uuid>, Option<String>, Option<String>, Option<String>, bool)> {
    let row: (Option<Uuid>, Option<Uuid>, Option<String>, Option<String>, Option<String>, bool) =
        sqlx::query_as(
            "SELECT assignee_person_id, delegate_agent_id, agent_state, review_target, manual_reason,
                    done_at IS NOT NULL OR status = 'dropped'
               FROM task WHERE id = $1 FOR UPDATE",
        )
        .bind(task_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::NotFound("task not found".into()))?;
    if row.0 != Some(person) {
        return Err(AppError::Forbidden(format!(
            "only the person this task is assigned to can {what}"
        )));
    }
    Ok((row.1, row.2, row.3, row.4, row.5))
}

pub async fn hand_off(state: &AppState, person: Uuid, task_id: Uuid, agent: Uuid) -> AppResult<TaskRow> {
    let mut tx = state.db.begin().await?;
    let (delegate, agent_state, _, _, finished) = owned(&mut tx, person, task_id, "hand it off").await?;
    if finished {
        return Err(AppError::Conflict("this task is finished; there is nothing to hand off".into()));
    }
    let live: Option<bool> = sqlx::query_scalar(
        "SELECT revoked_at IS NULL FROM agent WHERE id = $1 AND owner_id = $2",
    )
    .bind(agent)
    .bind(person)
    .fetch_optional(&mut *tx)
    .await?;
    match live {
        None => return Err(AppError::NotFound("no such agent of yours".into())),
        Some(false) => return Err(AppError::Conflict("that agent has been revoked".into())),
        Some(true) => {}
    }
    // Handing it to the agent that already holds it is a no-op, not a
    // second `handed_off` for it to act on.
    if delegate != Some(agent) || agent_state.as_deref() != Some("handed_off") {
        sqlx::query(
            "UPDATE task SET delegate_agent_id = $2, agent_state = 'handed_off', review_target = NULL,
                             delegated_at = now()
              WHERE id = $1",
        )
        .bind(task_id)
        .bind(agent)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    task::get(state, task_id, Some(person)).await
}

pub async fn take_back(state: &AppState, person: Uuid, task_id: Uuid) -> AppResult<TaskRow> {
    let mut tx = state.db.begin().await?;
    let (delegate, ..) = owned(&mut tx, person, task_id, "take it back").await?;
    if delegate.is_none() {
        return Err(AppError::Conflict("this task is not handed off".into()));
    }
    sqlx::query(
        "UPDATE task SET delegate_agent_id = NULL, agent_state = 'stopped', review_target = NULL
          WHERE id = $1",
    )
    .bind(task_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    task::get(state, task_id, Some(person)).await
}

pub async fn answer(state: &AppState, person: Uuid, task_id: Uuid, body: &str) -> AppResult<Note> {
    let mut tx = state.db.begin().await?;
    let (delegate, agent_state, ..) = owned(&mut tx, person, task_id, "answer its agent").await?;
    if delegate.is_none() || agent_state.as_deref() != Some("needs_input") {
        return Err(AppError::Conflict("there is no open question on this task".into()));
    }
    set_state(&mut tx, task_id, "working", None).await?;
    let note = note::insert(&mut tx, task_id, Author::Person(Some(person)), "answer", body).await?;
    tx.commit().await?;
    Ok(note)
}

/// A private instruction to the agent holding the task. Only its owner and
/// admins see it on the task; the agent hears it as an `instruction` event.
pub async fn instruct(state: &AppState, person: Uuid, task_id: Uuid, body: &str) -> AppResult<Note> {
    let mut tx = state.db.begin().await?;
    let (delegate, agent_state, _, _, finished) = owned(&mut tx, person, task_id, "instruct its agent").await?;
    if delegate.is_none() || finished || matches!(agent_state.as_deref(), Some("done" | "stopped")) {
        return Err(AppError::Conflict(
            "no agent is working on this task right now; hand it off first".into(),
        ));
    }
    let note = note::insert(&mut tx, task_id, Author::Person(Some(person)), "instruction", body).await?;
    tx.commit().await?;
    Ok(note)
}

/// Approve a submission — apply the move it asked for, through the normal
/// transition path — or send it back with what to change.
pub async fn review(
    state: &AppState,
    actor: &Actor,
    person: Uuid,
    task_id: Uuid,
    approve: bool,
    body: Option<&str>,
) -> AppResult<TaskRow> {
    let mut tx = state.db.begin().await?;
    let (delegate, agent_state, target, reason, _) = owned(&mut tx, person, task_id, "review its agent's work").await?;
    let (Some(_), Some("in_review"), Some(target)) = (delegate, agent_state.as_deref(), target) else {
        return Err(AppError::Conflict("there is no submission waiting for review on this task".into()));
    };
    let body = body.map(str::trim).filter(|b| !b.is_empty());
    if approve {
        task::transition(&mut tx, actor, task_id, target, reason, None).await?;
        set_state(&mut tx, task_id, "done", None).await?;
        note::insert(&mut tx, task_id, Author::Person(Some(person)), "review", body.unwrap_or("Approved.")).await?;
    } else {
        let body = body.ok_or_else(|| AppError::BadRequest("say what needs to change".into()))?;
        set_state(&mut tx, task_id, "working", None).await?;
        note::insert(&mut tx, task_id, Author::Person(Some(person)), "review", body).await?;
    }
    tx.commit().await?;
    task::get(state, task_id, Some(person)).await
}

/// Questions and submissions waiting on this person, for the home screen.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    /// question or review.
    pub kind: String,
    pub task_id: Uuid,
    pub title: String,
    pub agent_name: String,
    pub body: String,
}

pub async fn needs_attention(state: &AppState, person: Uuid) -> AppResult<Vec<Attention>> {
    Ok(sqlx::query_as(
        "SELECT CASE t.agent_state WHEN 'needs_input' THEN 'question' ELSE 'review' END AS kind,
                t.id AS task_id, t.title, a.name AS agent_name,
                coalesce((SELECT n.body FROM note n
                           WHERE n.task_id = t.id AND n.agent_id = a.id
                             AND n.kind = CASE t.agent_state WHEN 'needs_input' THEN 'question'
                                                             ELSE 'submission' END
                           ORDER BY n.created_at DESC LIMIT 1), '') AS body
           FROM task t JOIN agent a ON a.id = t.delegate_agent_id
          WHERE t.assignee_person_id = $1 AND t.agent_state IN ('needs_input', 'in_review')
          ORDER BY t.updated_at DESC",
    )
    .bind(person)
    .fetch_all(&state.db)
    .await?)
}

/// A task an agent is on right now, for everyone: the team sees who is
/// working on what, never the private side.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Active {
    /// `{id, name, handle, runtime}`.
    pub agent: Value,
    /// `{id, name}`.
    pub owner: Value,
    /// `{id, title, projectName}`.
    pub task: Value,
    pub state: String,
    pub now: Option<String>,
    pub now_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub delegated_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn active(state: &AppState) -> AppResult<Vec<Active>> {
    Ok(sqlx::query_as(
        "SELECT json_build_object('id', a.id, 'name', a.name, 'handle', a.handle, 'runtime', a.runtime) AS agent,
                json_build_object('id', p.id, 'name', p.name, 'email', p.email) AS owner,
                json_build_object('id', t.id, 'title', t.title, 'projectName', pr.name) AS task,
                t.agent_state AS state, t.agent_now AS now, t.agent_now_at AS now_at,
                a.last_seen_at, t.delegated_at
           FROM task t
           JOIN agent a ON a.id = t.delegate_agent_id
           JOIN person p ON p.id = a.owner_id
           JOIN phase ph ON ph.id = t.phase_id
           JOIN project pr ON pr.id = ph.project_id
          WHERE t.agent_state IN ('acknowledged', 'working', 'needs_input', 'in_review')
            AND t.done_at IS NULL AND t.status <> 'dropped' AND a.revoked_at IS NULL
            AND t.archived_at IS NULL AND pr.archived_at IS NULL
          ORDER BY t.agent_state = 'working' DESC,
                   coalesce(t.agent_now_at, a.last_seen_at) DESC NULLS LAST, t.id",
    )
    .fetch_all(&state.db)
    .await?)
}
