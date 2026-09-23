use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// The two tracks a task can run on, chosen by its assignee's department.
///
/// Engineering ends when the work is in production; design ends when it has
/// been handed over. They share the states that mean the same thing, which is
/// why `completed` appears in both — mid-flow for engineering, terminal for
/// design.
pub const ENG_FLOW: [&str; 4] = ["open", "in_progress", "completed", "shipped"];
pub const DESIGN_FLOW: [&str; 4] = ["open", "in_progress", "handoff", "completed"];

/// Reachable from anywhere on either track, and not part of either's order.
pub const ASIDE: [&str; 2] = ["blocked", "dropped"];

/// The flow for a department. An unassigned task has no department and so no
/// track; engineering is the default because it is the larger half of the
/// team and because `open` is all an unassigned task can be anyway.
pub fn flow_of(department: Option<&str>) -> &'static [&'static str] {
    match department {
        Some("design") => &DESIGN_FLOW,
        _ => &ENG_FLOW,
    }
}

/// The state that means "finished" on this track. `done_at` is stamped here
/// and nowhere else, so the dashboard counts one thing.
pub fn terminal_of(department: Option<&str>) -> &'static str {
    flow_of(department).last().expect("a flow is never empty")
}

/// When a task stops holding up the tasks that wait on it, as a SQL tuple.
///
/// Not `done_at`: that marks the end of a task's own track, which is later
/// than the moment the work it blocks can start. Design unblocks engineering
/// at `handoff` — that is what a handoff is — and engineering unblocks its
/// dependents at `completed`, not when the change finally ships. A dropped
/// blocker is resolved too; nothing is going to arrive.
pub const BLOCKER_RESOLVED: &str = "('handoff', 'completed', 'shipped', 'dropped')";

/// Every state either track can produce. For a schema or a filter menu that
/// has no one task in hand; `statuses_for` is what a real task is checked
/// against.
pub const ALL_STATUSES: [&str; 7] =
    ["open", "in_progress", "handoff", "completed", "shipped", "blocked", "dropped"];

/// Every state a task on this track may hold.
pub fn statuses_for(department: Option<&str>) -> Vec<&'static str> {
    flow_of(department).iter().chain(ASIDE.iter()).copied().collect()
}

/// The departments a person can belong to. A task's discipline is whoever
/// holds it, so this is the only place the vocabulary is written down.
pub const DEPARTMENTS: [&str; 3] = ["design", "frontend", "backend"];

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
    pub blocked_by: Vec<Uuid>,
    /// Why this was completed with nothing to point at. Only ever set by the
    /// transition that had no evidence.
    pub manual_reason: Option<String>,
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
    /// The assignee's department. `None` when nobody holds the task — an
    /// unassigned task belongs to no discipline yet, which is the truth.
    pub discipline: Option<String>,
    pub blockers_total: i64,
    pub blockers_done: i64,
}

#[derive(Debug, Default)]
pub struct TaskFilter {
    pub department: Option<String>,
    pub project_id: Option<Uuid>,
    pub phase_id: Option<Uuid>,
    pub status: Option<String>,
    pub assignee_email: Option<String>,
    pub assignee_kind: Option<String>,
}

pub const TASK_COLUMNS: &str = "id, phase_id, title, body, status, priority, \
    assignee_kind, assignee_person_id, assignee_token_id, claimed_by, \
    claim_expires_at, blocked_by, manual_reason, done_at, created_at, updated_at";

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
                own.department AS discipline,
                (SELECT count(*) FROM task b WHERE b.id = ANY(t.blocked_by)) AS blockers_total,
                (SELECT count(*) FROM task b
                  WHERE b.id = ANY(t.blocked_by) AND b.status IN {RESOLVED}) AS blockers_done
           FROM task t
           JOIN phase ph ON ph.id = t.phase_id
           JOIN project pr ON pr.id = ph.project_id
           LEFT JOIN person own ON own.id = t.assignee_person_id",
        cols = TASK_COLUMNS.replace(", ", ", t."),
        RESOLVED = BLOCKER_RESOLVED,
    )
}
