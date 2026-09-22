use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub const TASK_STATUSES: [&str; 6] =
    ["open", "in_progress", "in_review", "blocked", "done", "dropped"];

pub const DISCIPLINES: [&str; 3] = ["design", "frontend", "backend"];

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    pub phase_id: Uuid,
    pub title: String,
    pub body: String,
    pub status: String,
    pub priority: i32,
    pub assignee_kind: Option<String>,
    pub assignee_person_id: Option<Uuid>,
    pub assignee_token_id: Option<Uuid>,
    pub claimed_by: Option<String>,
    pub claim_expires_at: Option<DateTime<Utc>>,
    pub discipline: Option<String>,
    pub blocked_by: Vec<Uuid>,
    pub done_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A task plus the names a list needs to render a row, and a count of how many
/// of its blockers are finished. Every personal view — by id, mine, available,
/// the home screen — returns this one shape so a client has a single row type.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub task: Task,
    pub project_id: Uuid,
    pub project_name: String,
    pub phase_name: String,
    /// The person holding it, by name and address, so a row never has to
    /// look an id up to say who.
    pub assignee_name: Option<String>,
    pub assignee_email: Option<String>,
    pub blockers_total: i64,
    pub blockers_done: i64,
}

#[derive(Debug, Default)]
pub struct TaskFilter {
    pub discipline: Option<String>,
    pub project_id: Option<Uuid>,
    pub phase_id: Option<Uuid>,
    pub status: Option<String>,
    pub assignee_email: Option<String>,
    pub assignee_kind: Option<String>,
}

pub const TASK_COLUMNS: &str = "id, phase_id, title, body, status, priority, \
    assignee_kind, assignee_person_id, assignee_token_id, claimed_by, \
    claim_expires_at, discipline, blocked_by, done_at, created_at, updated_at";

/// The `SELECT ... FROM` for a `TaskRow`, ending before any `WHERE`.
///
/// ponytail: the two blocker counts are correlated subqueries, one pair per
/// row. A board is hundreds of tasks, not millions, and `blocked_by` is empty
/// for nearly all of them. If a personal list ever gets slow, this becomes one
/// `LEFT JOIN LATERAL` over `unnest(blocked_by)`.
pub fn task_row_select() -> String {
    format!(
        "SELECT t.{cols},
                ph.project_id, pr.name AS project_name, ph.name AS phase_name,
                own.name AS assignee_name, own.email AS assignee_email,
                (SELECT count(*) FROM task b WHERE b.id = ANY(t.blocked_by)) AS blockers_total,
                (SELECT count(*) FROM task b
                  WHERE b.id = ANY(t.blocked_by) AND b.status = 'done') AS blockers_done
           FROM task t
           JOIN phase ph ON ph.id = t.phase_id
           JOIN project pr ON pr.id = ph.project_id
           LEFT JOIN person own ON own.id = t.assignee_person_id",
        cols = TASK_COLUMNS.replace(", ", ", t.")
    )
}
