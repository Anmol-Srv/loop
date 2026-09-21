//! The home screen, in one request.
//!
//! Four small queries behind one endpoint rather than four endpoints the
//! client has to fan out to and then stitch. The screen is the unit of
//! fetching here, so the cost of a cold start is one round trip.

use serde::Serialize;
use uuid::Uuid;

use crate::controllers::approval::ChangeRow;
use crate::controllers::project::ProjectProgress;
use crate::controllers::{approval, project, task};
use crate::db::AppState;
use crate::errors::AppResult;
use crate::models::task::TaskRow;

/// How many of the available tasks the home screen previews. The count is the
/// whole list, so "3 of 27" renders without a second request.
const PREVIEW: usize = 5;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableSummary {
    pub count: usize,
    pub first: Vec<TaskRow>,
}

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
    pub disciplines: Vec<String>,
    pub open: i64,
    pub review: i64,
    pub blocking: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Home {
    pub my_tasks: Vec<TaskRow>,
    pub waiting_on_me: Vec<ChangeRow>,
    pub available: AvailableSummary,
    pub projects: Vec<ProjectProgress>,
    pub team: Vec<Capacity>,
}

/// Everyone's load, in one grouped query.
///
/// Added because the home screen was rendering a capacity card per person with
/// every count at zero: `/api/user/home` returned only the caller's tasks, and
/// `/api/user/people` returns identity with no counts. The view reported the
/// gap rather than inventing numbers, which is the right instinct and also a
/// signal that the endpoint was wrong.
pub async fn team_capacity(state: &AppState) -> AppResult<Vec<Capacity>> {
    let rows = sqlx::query_as::<_, Capacity>(
        "SELECT p.id  AS person_id,
                p.email,
                p.name,
                p.disciplines,
                count(t.id) FILTER (
                  WHERE t.status IN ('open', 'in_progress')
                ) AS open,
                count(t.id) FILTER (WHERE t.status = 'in_review') AS review,
                count(t.id) FILTER (
                  WHERE t.status NOT IN ('done', 'dropped')
                    AND EXISTS (
                      SELECT 1 FROM task blocked
                       WHERE t.id = ANY(blocked.blocked_by)
                         AND blocked.status NOT IN ('done', 'dropped')
                    )
                ) AS blocking
           FROM person p
           LEFT JOIN task t
             ON t.assignee_person_id = p.id
          WHERE p.deleted_at IS NULL
          GROUP BY p.id, p.email, p.name, p.disciplines
          ORDER BY open DESC, p.name",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(rows)
}

pub async fn home(state: &AppState, person_id: Uuid) -> AppResult<Home> {
    let my_tasks = task::mine(state, person_id).await?;
    let waiting_on_me = approval::pending_for(state, person_id).await?;
    let mut available = task::available(state, person_id).await?;
    let projects = project::progress(state, None).await?;
    let team = team_capacity(state).await?;

    let count = available.len();
    available.truncate(PREVIEW);

    Ok(Home {
        my_tasks,
        waiting_on_me,
        available: AvailableSummary { count, first: available },
        projects,
        team,
    })
}
