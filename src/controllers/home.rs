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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Home {
    pub my_tasks: Vec<TaskRow>,
    pub waiting_on_me: Vec<ChangeRow>,
    pub available: AvailableSummary,
    pub projects: Vec<ProjectProgress>,
}

pub async fn home(state: &AppState, person_id: Uuid) -> AppResult<Home> {
    let my_tasks = task::mine(state, person_id).await?;
    let waiting_on_me = approval::pending_for(state, person_id).await?;
    let mut available = task::available(state, person_id).await?;
    let projects = project::progress(state, None).await?;

    let count = available.len();
    available.truncate(PREVIEW);

    Ok(Home {
        my_tasks,
        waiting_on_me,
        available: AvailableSummary { count, first: available },
        projects,
    })
}
