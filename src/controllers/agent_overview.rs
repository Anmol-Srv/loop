//! One agent, whole: what it has done, how quickly it answers, what it is
//! holding, and the runs and step log behind it. The agent page reads this.
//!
//! All of it is the owner's (and admins'): the step log, questions and
//! answers are private, so the overview is refused to anyone else rather
//! than served half-empty.
//!
//! Runs are how an agent that works on a schedule (an intake agent reading
//! Slack) says each pass happened, and what it found — its logs, in the sense
//! the owner means.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::controllers::agent;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::task::LIVE;

/// Runs kept per agent; older ones are dropped as new ones arrive.
pub const RUNS_KEPT: i64 = 500;
const SUMMARY_MAX: usize = 500;
const ERROR_MAX: usize = 4000;

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    #[serde(default)]
    pub filed: i64,
    #[serde(default)]
    pub appended: i64,
    #[serde(default)]
    pub already_filed: i64,
    #[serde(default)]
    pub skipped: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    /// When the pass began; now if not given.
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    /// ok, partial or failed.
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub counts: Counts,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: i64,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub status: String,
    pub summary: String,
    pub counts: Value,
    pub error: Option<String>,
}

/// Record one pass. Errors are read by models, so they say what to send.
pub async fn report(state: &AppState, agent: Uuid, r: RunReport) -> AppResult<Run> {
    if !matches!(r.status.as_str(), "ok" | "partial" | "failed") {
        return Err(AppError::BadRequest(format!(
            "status is ok (the pass finished), partial (some items failed) or failed (the pass did not run); you sent '{}'",
            r.status
        )));
    }
    let c = &r.counts;
    if [c.filed, c.appended, c.already_filed, c.skipped].iter().any(|n| *n < 0) {
        return Err(AppError::BadRequest("counts are how many of each; none can be negative".into()));
    }
    let summary = r.summary.trim();
    if summary.chars().count() > SUMMARY_MAX {
        return Err(AppError::BadRequest(format!("keep the summary to {SUMMARY_MAX} characters: one line on what the pass did")));
    }
    let error = r.error.as_deref().map(str::trim).filter(|e| !e.is_empty());
    if error.is_some_and(|e| e.chars().count() > ERROR_MAX) {
        return Err(AppError::BadRequest(format!("keep the error to {ERROR_MAX} characters; the step that failed and its message")));
    }
    if r.status == "failed" && error.is_none() {
        return Err(AppError::BadRequest("a failed run needs an error: what failed, so your owner can fix it".into()));
    }
    if r.started_at.is_some_and(|t| t > Utc::now() + chrono::Duration::minutes(5)) {
        return Err(AppError::BadRequest("startedAt is in the future; send when the pass began, or leave it out".into()));
    }

    let mut tx = state.db.begin().await?;
    let run: Run = sqlx::query_as(
        "INSERT INTO agent_run (agent_id, started_at, status, summary, counts, error)
         VALUES ($1, least(coalesce($2, now()), now()), $3, $4, $5, $6)
         RETURNING id, started_at, finished_at, status, summary, counts, error",
    )
    .bind(agent)
    .bind(r.started_at)
    .bind(&r.status)
    .bind(summary)
    .bind(serde_json::to_value(&r.counts).unwrap_or_else(|_| json!({})))
    .bind(error)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM agent_run WHERE agent_id = $1 AND id <= coalesce(
           (SELECT id FROM agent_run WHERE agent_id = $1 ORDER BY id DESC OFFSET $2 LIMIT 1), 0)",
    )
    .bind(agent)
    .bind(RUNS_KEPT)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(run)
}

/// The agent page's payload. See the module doc for who may read it.
pub async fn overview(state: &AppState, viewer: Uuid, id: Uuid) -> AppResult<Value> {
    let allowed: Option<bool> = sqlx::query_scalar(
        "SELECT a.owner_id = $2 OR EXISTS (SELECT 1 FROM person WHERE id = $2 AND role = 'admin')
           FROM agent a WHERE a.id = $1",
    )
    .bind(id)
    .bind(viewer)
    .fetch_optional(&state.db)
    .await?;
    match allowed {
        None => return Err(AppError::NotFound("agent not found".into())),
        Some(false) => return Err(AppError::Forbidden("only the agent's owner, or an admin, can see its page".into())),
        Some(true) => {}
    }
    let agent = agent::one(&state.db, id).await?;

    // Hand-off to first sign of life (a note or a step-log line) stands in
    // for acknowledgement, which leaves no timestamp of its own.
    // ponytail: a log line on a task re-handed to another agent could count;
    // record acknowledged_at on the task if that ever skews the number.
    let stats: Value = sqlx::query_scalar(
        "WITH handed AS (SELECT task_id, created_at FROM agent_event WHERE agent_id = $1 AND kind = 'handed_off'),
              filed AS (SELECT t.status FROM task_source s JOIN task t ON t.id = s.task_id
                         WHERE s.agent_id = $1 AND NOT s.appended),
              acks AS (SELECT extract(epoch FROM least(
                  (SELECT min(n.created_at) FROM note n WHERE n.task_id = h.task_id AND n.agent_id = $1 AND n.created_at >= h.created_at),
                  (SELECT min(l.created_at) FROM run_log_line l WHERE l.task_id = h.task_id AND l.created_at >= h.created_at)
                ) - h.created_at) / 60.0 AS mins FROM handed h)
         SELECT json_build_object(
           'tasksHandled', (SELECT count(DISTINCT task_id) FROM handed),
           'tasksDone', (SELECT count(DISTINCT task_id) FROM agent_event WHERE agent_id = $1 AND kind = 'approved'),
           'inReviewNow', (SELECT count(*) FROM task t WHERE t.delegate_agent_id = $1 AND t.agent_state = 'in_review'),
           'questionsAsked', (SELECT count(*) FROM note WHERE agent_id = $1 AND kind = 'question'),
           'medianAckMinutes', (SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY mins) FROM acks WHERE mins IS NOT NULL),
           'filed', (SELECT count(*) FROM filed),
           'accepted', (SELECT count(*) FROM filed WHERE status NOT IN ('triage', 'dropped')),
           'dismissed', (SELECT count(*) FROM filed WHERE status = 'dropped'))",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let mut stats = stats;
    let (accepted, dismissed) = (stats["accepted"].as_i64().unwrap_or(0), stats["dismissed"].as_i64().unwrap_or(0));
    stats["acceptRate"] = if accepted + dismissed > 0 { json!(accepted as f64 / (accepted + dismissed) as f64) } else { Value::Null };

    let activity: Value = sqlx::query_scalar(
        "SELECT json_agg(json_build_object(
                  'day', d::date,
                  'events', (SELECT count(*) FROM agent_event e WHERE e.agent_id = $1 AND e.created_at::date = d::date),
                  'notes', (SELECT count(*) FROM note n WHERE n.agent_id = $1 AND n.created_at::date = d::date),
                  'filed', (SELECT count(*) FROM task_source s WHERE s.agent_id = $1 AND NOT s.appended AND s.created_at::date = d::date))
                ORDER BY d)
           FROM generate_series(current_date - 29, current_date, interval '1 day') d",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    // What went back and forth, newest first. Everything the owner sees on
    // the task pages, in one stream: the agent's own notes, the owner's side
    // as the events it was sent, and what it filed.
    let recent: Value = sqlx::query_scalar(
        "SELECT coalesce(json_agg(r ORDER BY r.at DESC), '[]') FROM (
           SELECT * FROM (
             SELECT n.created_at AS at, n.kind, n.task_id AS \"taskId\", t.title AS \"taskTitle\", n.body AS text
               FROM note n JOIN task t ON t.id = n.task_id WHERE n.agent_id = $1
             UNION ALL
             SELECT e.created_at, e.kind, e.task_id, t.title, coalesce(e.payload->>'body', '')
               FROM agent_event e JOIN task t ON t.id = e.task_id
              WHERE e.agent_id = $1
                AND e.kind IN ('handed_off', 'taken_back', 'answer', 'instruction', 'approved', 'changes_requested')
             UNION ALL
             SELECT s.created_at, 'filed', s.task_id, t.title, coalesce(s.channel_name, s.kind)
               FROM task_source s JOIN task t ON t.id = s.task_id WHERE s.agent_id = $1 AND NOT s.appended
           ) u ORDER BY at DESC LIMIT 50) r",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    let task_lite = "json_build_object('id', t.id, 'title', t.title, 'status', t.status, 'priority', t.priority,
           'agentState', CASE WHEN t.delegate_agent_id = $1 THEN t.agent_state END,
           'projectName', pr.name, 'updatedAt', t.updated_at, 'delegatedAt', t.delegated_at,
           'filed', EXISTS (SELECT 1 FROM task_source s WHERE s.task_id = t.id AND s.agent_id = $1 AND NOT s.appended))";
    let mine = "(t.delegate_agent_id = $1
                 OR t.id IN (SELECT task_id FROM agent_event WHERE agent_id = $1 AND kind = 'handed_off')
                 OR t.id IN (SELECT task_id FROM task_source WHERE agent_id = $1 AND NOT appended))";
    // Active: held now, or filed and still waiting in triage.
    let active = "(t.delegate_agent_id IS NOT DISTINCT FROM $1 AND coalesce(t.agent_state, '') NOT IN ('done', 'stopped')
          AND t.done_at IS NULL AND t.status <> 'dropped') OR t.status = 'triage'";
    let tasks: Value = sqlx::query_scalar(&format!(
        "WITH mine AS (
           SELECT {task_lite} AS row, ({active}) AS active, t.updated_at
             FROM task t
             LEFT JOIN phase ph ON ph.id = t.phase_id
             LEFT JOIN project pr ON pr.id = ph.project_id
            WHERE {mine} AND {LIVE})
         SELECT json_build_object(
           'active', coalesce((SELECT json_agg(row ORDER BY updated_at DESC) FROM mine WHERE active), '[]'),
           'recent', coalesce((SELECT json_agg(row ORDER BY updated_at DESC)
                                 FROM (SELECT * FROM mine WHERE NOT active ORDER BY updated_at DESC LIMIT 20) x), '[]'))"
    ))
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    let runs: Vec<Run> = sqlx::query_as(
        "SELECT id, started_at, finished_at, status, summary, counts, error
           FROM agent_run WHERE agent_id = $1 ORDER BY id DESC LIMIT 50",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    // The newest hundred step-log lines across the tasks it held, returned
    // oldest first so each task's lines read in order.
    let logs: Value = sqlx::query_scalar(
        "SELECT coalesce(json_agg(l ORDER BY l.at, l.seq), '[]') FROM (
           SELECT l.task_id AS \"taskId\", t.title AS \"taskTitle\", l.seq, l.text, l.created_at AS at
             FROM run_log_line l JOIN task t ON t.id = l.task_id
            WHERE t.delegate_agent_id = $1
               OR t.id IN (SELECT task_id FROM agent_event WHERE agent_id = $1 AND kind = 'handed_off')
            ORDER BY l.created_at DESC, l.seq DESC LIMIT 100) l",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(json!({
        "agent": agent,
        "stats": stats,
        "activity": activity,
        "recent": recent,
        "tasks": tasks,
        "runs": runs,
        "logs": logs,
    }))
}
