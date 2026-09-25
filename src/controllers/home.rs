//! The home screen, in one request.
//!
//! Four small queries behind one endpoint rather than four endpoints the
//! client has to fan out to and then stitch. The screen is the unit of
//! fetching here, so the cost of a cold start is one round trip.

use serde::Serialize;
use uuid::Uuid;

use crate::controllers::approval::ChangeRow;
use crate::controllers::project::ProjectProgress;
use crate::controllers::{agent, approval, project, task};
use crate::db::AppState;
use crate::errors::AppResult;
use crate::models::task::TaskRow;

/// One person's live load.
///
/// `blocking` is the interesting number: how many of their unfinished tasks
/// some other task is waiting on. A person with one task that is holding up
/// three others is more of a bottleneck than a person with five independent
/// ones, and a plain open count hides that entirely.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Capacity {
    pub person_id: Uuid,
    pub email: String,
    pub name: String,
    pub department: String,
    pub open: i64,
    pub review: i64,
    pub blocking: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Home {
    pub my_tasks: Vec<TaskRow>,
    pub waiting_on_me: Vec<ChangeRow>,
    pub projects: Vec<ProjectProgress>,
    pub team: Vec<Capacity>,
    /// Questions and submissions from my agents, on tasks assigned to me,
    /// and (`kind: "triage"`) tasks an intake agent filed for me.
    pub needs_attention: Vec<agent::Attention>,
}

/// Everyone's load, in one grouped query.
///
/// Added because the home screen was rendering a capacity card per person with
/// every count at zero: `/api/user/home` returned only the caller's tasks, and
/// `/api/user/people` returns identity with no counts. The view reported the
/// gap rather than inventing numbers, which is the right instinct and also a
/// signal that the endpoint was wrong.
pub async fn team_capacity(state: &AppState) -> AppResult<Vec<Capacity>> {
    // `open` is what someone is carrying: work they can act on, including
    // work that is stuck. `review` is theirs but waiting on someone else — a
    // handoff, or engineering work completed and not yet shipped.
    //
    // The waiting set is computed once. Asking `EXISTS … = ANY(blocked_by)`
    // per task scanned every task for every task — a second at 7,500 — while
    // the set of ids anything live waits on is one pass and a hash lookup.
    let rows = sqlx::query_as::<_, Capacity>(&format!(
        "WITH waiting AS (
           SELECT DISTINCT unnest(t.blocked_by) AS id
             FROM task t
            WHERE t.done_at IS NULL AND t.status <> 'dropped' AND {live}
         )
         SELECT p.id  AS person_id,
                p.email,
                p.name,
                p.department,
                count(t.id) FILTER (
                  WHERE t.status IN ('open', 'in_progress', 'blocked')
                ) AS open,
                count(t.id) FILTER (WHERE t.status IN ('handoff', 'completed')
                                      AND t.done_at IS NULL) AS review,
                count(t.id) FILTER (
                  WHERE t.status NOT IN {resolved}
                    AND t.id IN (SELECT id FROM waiting)
                ) AS blocking
           FROM person p
           LEFT JOIN task t
             ON t.assignee_person_id = p.id AND {live}
          WHERE p.deleted_at IS NULL
          GROUP BY p.id, p.email, p.name, p.department
          ORDER BY open DESC, p.name",
        resolved = crate::models::task::BLOCKER_RESOLVED,
        live = crate::models::task::LIVE,
    ))
    .fetch_all(&state.db)
    .await?;

    Ok(rows)
}

/// The sidebar's two numbers.
///
/// Every page draws the sidebar, and it used to get these by fetching all of
/// `/home` — the team rollup included — just to count two lists. Two counts
/// are cheap; the whole home screen is not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    /// Unfinished and accepted: triage is counted on its own, not here.
    pub my_open: i64,
    pub active_projects: i64,
    /// My tasks an intake agent filed that I have not accepted or dismissed.
    pub triage: i64,
}

pub async fn counts(state: &AppState, person_id: Uuid) -> AppResult<Counts> {
    let (my_open, triage): (i64, i64) = sqlx::query_as(&format!(
        "SELECT count(*) FILTER (WHERE t.done_at IS NULL AND t.status NOT IN ('dropped', 'triage')),
                count(*) FILTER (WHERE t.status = 'triage')
           FROM task t
          WHERE t.assignee_person_id = $1 AND {}",
        crate::models::task::LIVE
    ))
    .bind(person_id)
    .fetch_one(&state.db)
    .await?;
    let active_projects =
        sqlx::query_scalar("SELECT count(*) FROM project WHERE status = 'active' AND archived_at IS NULL")
        .fetch_one(&state.db)
        .await?;
    Ok(Counts { my_open, active_projects, triage })
}

pub async fn home(state: &AppState, person_id: Uuid) -> AppResult<Home> {
    let my_tasks = task::mine(state, person_id, false).await?;
    let waiting_on_me = approval::pending_for(state, person_id).await?;
    let projects = project::progress(state, None).await?;
    let team = team_capacity(state).await?;
    let needs_attention = agent::needs_attention(state, person_id).await?;

    Ok(Home {
        my_tasks,
        waiting_on_me,
        projects,
        team,
        needs_attention,
    })
}
