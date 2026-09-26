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

/// Where an agent's intake lands, on either track. The owner accepts it
/// (`open`) or dismisses it (`dropped`); nothing else leaves it and nothing
/// enters it — an intake is the only way in.
pub const TRIAGE: &str = "triage";

/// What an intake may call a task. The column's CHECK says the same.
pub const CATEGORIES: [&str; 5] = ["bug", "feature", "feedback", "question", "chore"];

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
pub const ALL_STATUSES: [&str; 8] =
    ["triage", "open", "in_progress", "handoff", "completed", "shipped", "blocked", "dropped"];

/// Every state a task on this track may hold.
pub fn statuses_for(department: Option<&str>) -> Vec<&'static str> {
    std::iter::once(TRIAGE).chain(flow_of(department).iter().chain(ASIDE.iter()).copied()).collect()
}

/// Where a task may go next. The one table: `set_status` refuses anything not
/// in it and `GET /api/user/tracks` serialises it, so the app can only offer
/// moves the server will take.
///
/// Membership in a track was never enough — any state was reachable from any
/// other, so an open task could be shipped past the PR check. The order is
/// the rule: `completed` only from `in_progress`, `shipped` only from
/// `completed`, `handoff` only from `in_progress`. Stepping back one is
/// always allowed, because work gets reopened.
pub fn next_statuses(department: Option<&str>, from: &str) -> &'static [&'static str] {
    let design = department == Some("design");
    match (design, from) {
        (_, "triage") => &["open", "dropped"],
        (_, "open") => &["in_progress", "blocked", "dropped"],
        (false, "in_progress") => &["completed", "open", "blocked", "dropped"],
        (false, "completed") => &["shipped", "in_progress", "blocked", "dropped"],
        (false, "shipped") => &["in_progress", "blocked", "dropped"],
        (true, "in_progress") => &["handoff", "open", "blocked", "dropped"],
        (true, "handoff") => &["completed", "in_progress", "blocked", "dropped"],
        (true, "completed") => &["in_progress", "blocked", "dropped"],
        (_, "blocked") => &["in_progress", "dropped"],
        (_, "dropped") => &["open"],
        // A state this track does not have: finished work left behind by a
        // department change. `open` is the way back onto the track.
        _ => &["open", "blocked", "dropped"],
    }
}

/// The artifact kinds that let a task make this move, if it needs any. A
/// `manual_reason` stands in for them; nothing else does.
pub fn evidence_for(department: Option<&str>, to: &str) -> Option<&'static [&'static str]> {
    match (department == Some("design"), to) {
        (true, "handoff") => Some(&["figma"]),
        (false, "completed") => Some(&["pr", "commit"]),
        _ => None,
    }
}

/// The moves anyone may make, not only the holder. Shipping records a fact
/// about production rather than a claim about ownership.
pub const ANYONE: [&str; 1] = ["shipped"];

/// `next_statuses` and `evidence_for` as the JSON `GET /api/user/tracks`
/// returns — generated, never written out, so it cannot drift from the rule.
pub fn tracks_table() -> serde_json::Value {
    let track = |dept: Option<&str>| -> serde_json::Map<String, serde_json::Value> {
        statuses_for(dept)
            .into_iter()
            .map(|from| (from.to_owned(), serde_json::json!(next_statuses(dept, from))))
            .collect()
    };
    let evidence = |dept: Option<&str>| -> serde_json::Map<String, serde_json::Value> {
        ALL_STATUSES
            .iter()
            .filter_map(|to| evidence_for(dept, to).map(|k| (to.to_string(), serde_json::json!(k))))
            .collect()
    };
    serde_json::json!({
        "eng": track(None),
        "design": track(Some("design")),
        "evidence": { "eng": evidence(None), "design": evidence(Some("design")) },
        "anyone": ANYONE,
    })
}

/// Where a task lands when its track changes under it — reassignment, or its
/// holder changing department. A status the new track lacks becomes `open`
/// (a design `handoff` given to an engineer is theirs to start), and the
/// second value says whether that is the new track's finish line, so
/// `done_at` is recomputed alongside it and the two never disagree.
pub fn settle(department: Option<&str>, status: &str) -> (String, bool) {
    let status = if statuses_for(department).contains(&status) { status } else { "open" };
    (status.to_owned(), status == terminal_of(department))
}

/// The departments a person can belong to. A task's discipline is whoever
/// holds it, so this is the only place the vocabulary is written down.
pub const DEPARTMENTS: [&str; 3] = ["design", "frontend", "backend"];

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    /// None for a standalone task: one in no project.
    pub phase_id: Option<Uuid>,
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
    /// The finishing status the delegate agent submitted for review.
    pub review_target: Option<String>,
    /// Set while the task itself is archived. Its project can archive it too:
    /// see `TaskRow::project_archived_at`.
    pub archived_at: Option<DateTime<Utc>>,
    /// The agent this task is handed off to, if any: `{id, handle, name,
    /// runtime, ownerName, state, now, nowAt, delegatedAt, lastSeenAt}`. Built in SQL so every query
    /// that returns a task carries it without a second round trip.
    pub delegate: Option<serde_json::Value>,
    /// bug, feature, feedback, question or chore; set at intake or by any
    /// writer later.
    pub category: Option<String>,
    /// A folder from the assignee's own list, by name, to work in when this
    /// task has no project (or its project has no folder for them). `None`
    /// leaves it to their default. The name, not an id — folders are per
    /// person, and `controllers::agent::context` resolves it against
    /// whoever the task is delegated to, the only place that person is known.
    pub folder_name: Option<String>,
    /// When the owner approved the current hand-off's plan, or `None` before
    /// that (or once a fresh plan clears it). No content, so — unlike `brief`
    /// on `TaskRow` — every viewer reads it: it is what a teammate's neutral
    /// "Plan approved" line is built from.
    pub plan_approved_at: Option<DateTime<Utc>>,
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
    /// All three None for a standalone task.
    pub project_id: Option<Uuid>,
    pub project_name: Option<String>,
    pub phase_name: Option<String>,
    /// The person holding it, by name and address, so a row never has to
    /// look an id up to say who.
    pub assignee_name: Option<String>,
    pub assignee_email: Option<String>,
    /// The assignee's department. `None` when nobody holds the task — an
    /// unassigned task belongs to no discipline yet, which is the truth.
    pub discipline: Option<String>,
    pub blockers_total: i64,
    pub blockers_done: i64,
    /// Set while the project is archived, which archives every task in it.
    pub project_archived_at: Option<DateTime<Utc>>,
    /// Whether the viewer may archive, restore or delete it: `can_manage`.
    pub can_archive: bool,
    pub can_delete: bool,
    /// Whether the viewer may read the agent's private side of this task —
    /// questions, answers, instructions, the step log: `sees_agent_private`.
    pub can_see_agent_private: bool,
    /// Where an intake agent found it: `{kind, url, channel, channelName,
    /// author, text, thread, receivedAt, reason, confidence, private, files}`,
    /// or null. `thread` is the earlier messages, oldest first:
    /// `[{author, text, ts, receivedAt}]`. A direct message's author, text and thread are only for
    /// `sees_agent_private`; everyone else reads "From a direct message".
    /// `files` is `[{id, name, mime, size, width, height}]`, each one
    /// withheld on the same rule as the message it came with.
    pub source: Option<serde_json::Value>,
    /// `[{id, name, colour}]`, by name; the shared label vocabulary.
    pub labels: serde_json::Value,
    /// The owner's optional note at hand-off, set on the current delegation
    /// only. Masked the same as the agent's private side —
    /// `sees_agent_private` — since it is the owner's alone to write and read.
    pub brief: Option<String>,
}

#[derive(Debug, Default)]
pub struct TaskFilter {
    pub department: Option<String>,
    pub project_id: Option<Uuid>,
    pub phase_id: Option<Uuid>,
    pub status: Option<String>,
    pub assignee_email: Option<String>,
    pub assignee_kind: Option<String>,
    /// Only archived tasks (its own flag or its project's) instead of only
    /// live ones.
    pub archived: bool,
}

/// Who may archive, restore or delete a task: whoever created it, whoever
/// created its project (a standalone task has none), or an admin. SQL over `t`
/// and `pr`, for the person bound at `viewer` — selected into every `TaskRow`
/// and checked by the controller before it acts, so the app and the server
/// ask one question.
pub fn can_manage(viewer: &str) -> String {
    format!(
        "(coalesce(t.created_by = {viewer} OR pr.created_by = {viewer}, false)
          OR EXISTS (SELECT 1 FROM person WHERE id = {viewer} AND role = 'admin'))"
    )
}

/// Who may read the private side of a task's agent session: the task's
/// owner (its assignee, the person who hands it to their agent) or an admin.
/// SQL over `t`, for the person bound at `viewer`; a NULL viewer sees nothing.
pub fn sees_agent_private(viewer: &str) -> String {
    format!(
        "(coalesce(t.assignee_person_id = {viewer}, false)
          OR EXISTS (SELECT 1 FROM person WHERE id = {viewer} AND role = 'admin'))"
    )
}

/// A task that is neither archived nor in an archived project, as SQL over
/// `t` alone, for the queries that do not join the project.
/// A standalone task (no phase) is archived only by its own flag.
pub const LIVE: &str = "(t.archived_at IS NULL AND NOT EXISTS
    (SELECT 1 FROM phase ph JOIN project pr ON pr.id = ph.project_id
      WHERE ph.id = t.phase_id AND pr.archived_at IS NOT NULL))";

/// An agent still holds the task: it has to be taken back before the task can
/// be archived or deleted. The delegate stays on a finished task as history.
pub const HELD: &str = "(t.delegate_agent_id IS NOT NULL
    AND coalesce(t.agent_state, '') NOT IN ('done', 'stopped')
    AND t.done_at IS NULL AND t.status <> 'dropped')";

macro_rules! plain_task_columns {
    () => {
        "id, phase_id, title, body, status, priority, \
         assignee_kind, assignee_person_id, assignee_token_id, claimed_by, \
         claim_expires_at, blocked_by, manual_reason, done_at, created_at, updated_at, \
         review_target, archived_at, category, folder_name, plan_approved_at"
    };
}

/// The delegate as one JSON value. Unqualified column names, so it reads the
/// same in a `RETURNING` and in a `FROM task t` select.
macro_rules! delegate_column {
    () => {
        "(SELECT json_build_object('id', a.id, 'handle', a.handle, 'name', a.name, \
                 'runtime', a.runtime, 'ownerName', (SELECT name FROM person WHERE id = a.owner_id), \
                 'ownerEmail', (SELECT email FROM person WHERE id = a.owner_id), \
                 'state', agent_state, 'now', agent_now, 'nowAt', agent_now_at, \
                 'delegatedAt', delegated_at, 'lastSeenAt', a.last_seen_at) \
            FROM agent a WHERE a.id = delegate_agent_id) AS delegate"
    };
}

pub const TASK_COLUMNS: &str = concat!(plain_task_columns!(), ", ", delegate_column!());

/// `TASK_COLUMNS` for a query that names the task `t`.
pub fn task_columns_t() -> String {
    format!("t.{}, {}", plain_task_columns!().replace(", ", ", t."), delegate_column!())
}

/// The `SELECT ... FROM` for a `TaskRow`, ending before any `WHERE`.
/// `viewer` is the placeholder (or `NULL::uuid`) `canArchive` is worked out
/// for.
///
/// ponytail: the two blocker counts are correlated subqueries, one pair per
/// row. A board is hundreds of tasks, not millions, and `blocked_by` is empty
/// for nearly all of them. If a personal list ever gets slow, this becomes one
/// `LEFT JOIN LATERAL` over `unnest(blocked_by)`.
pub fn task_row_select(viewer: &str) -> String {
    format!(
        "SELECT {cols},
                ph.project_id, pr.name AS project_name, ph.name AS phase_name,
                own.name AS assignee_name, own.email AS assignee_email,
                own.department AS discipline,
                (SELECT count(*) FROM task b WHERE b.id = ANY(t.blocked_by)) AS blockers_total,
                (SELECT count(*) FROM task b
                  WHERE b.id = ANY(t.blocked_by) AND b.status IN {RESOLVED}) AS blockers_done,
                pr.archived_at AS project_archived_at,
                {can} AS can_archive, {can} AS can_delete,
                {private} AS can_see_agent_private,
                (SELECT json_build_object('kind', s.kind, 'url', s.url, 'channel', s.channel,
                        'channelName', s.channel_name, 'receivedAt', s.received_at,
                        'reason', CASE WHEN NOT s.private OR {private} THEN s.reason END,
                        'confidence', s.confidence, 'private', s.private,
                        'agentName', (SELECT a.name FROM agent a WHERE a.id = s.agent_id),
                        'author', CASE WHEN NOT s.private OR {private} THEN s.author END,
                        'text', CASE WHEN NOT s.private OR {private} THEN s.text
                                     ELSE 'From a direct message' END,
                        'thread', CASE WHEN NOT s.private OR {private} THEN s.thread ELSE '[]' END,
                        'files', {files})
                   FROM task_source s WHERE s.task_id = t.id AND NOT s.appended) AS source,
                coalesce((SELECT json_agg(json_build_object('id', l.id, 'name', l.name, 'colour', l.colour)
                                          ORDER BY l.name)
                            FROM task_label tl JOIN label l ON l.id = tl.label_id
                           WHERE tl.task_id = t.id), '[]') AS labels,
                CASE WHEN {private} THEN t.brief END AS brief
           FROM task t
           LEFT JOIN phase ph ON ph.id = t.phase_id
           LEFT JOIN project pr ON pr.id = ph.project_id
           LEFT JOIN person own ON own.id = t.assignee_person_id",
        cols = task_columns_t(),
        can = can_manage(viewer),
        private = sees_agent_private(viewer),
        files = format!(
            "coalesce((SELECT json_agg(json_build_object('id', f.id, 'name', f.name, 'mime', f.mime,
                                'size', f.size, 'width', f.width, 'height', f.height) ORDER BY f.created_at, f.name)
                         FROM task_file f WHERE f.task_id = t.id AND {}), '[]')",
            file_visible(viewer)
        ),
        RESOLVED = BLOCKER_RESOLVED,
    )
}

/// Whether the viewer at `viewer` may read file `f` of task `t`: it is
/// withheld exactly when the message it came with is — a direct message's
/// files are the owner's and admins'. One rule for the list and the bytes.
pub fn file_visible(viewer: &str) -> String {
    format!(
        "NOT EXISTS (SELECT 1 FROM task_source fs
                      WHERE fs.task_id = f.task_id AND fs.source_key = f.source_key AND fs.private
                        AND NOT {})",
        sees_agent_private(viewer)
    )
}
