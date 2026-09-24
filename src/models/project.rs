use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub priority: i32,
    pub start_date: Option<chrono::NaiveDate>,
    pub target_date: Option<chrono::NaiveDate>,
    /// Filled by the controller after the row is read; not a column.
    #[sqlx(skip)]
    pub labels: Vec<crate::controllers::label::Label>,
    pub archived_at: Option<DateTime<Utc>>,
    /// Whether the viewer may archive, restore or delete it: `can_manage`.
    pub can_archive: bool,
    pub can_delete: bool,
    /// Every task in it, archived and dropped included: what a delete takes.
    pub task_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Who may archive, restore or delete a project: whoever created it, or an
/// admin. SQL over `pr`, for the person bound at `viewer`.
///
/// The one rule. Every project the app reads selects it as `can_archive` and
/// `can_delete`, and the controller refuses on it before acting, so what the
/// app offers and what the server allows cannot drift apart.
pub fn can_manage(viewer: &str) -> String {
    format!(
        "(coalesce(pr.created_by = {viewer}, false)
          OR EXISTS (SELECT 1 FROM person WHERE id = {viewer} AND role = 'admin'))"
    )
}

/// A project's columns as `Project` reads them, the table named `pr`.
pub fn columns(viewer: &str) -> String {
    let can = can_manage(viewer);
    format!(
        "pr.id, pr.key, pr.name, pr.description, pr.status, pr.priority, pr.start_date,
         pr.target_date, pr.archived_at, {can} AS can_archive, {can} AS can_delete,
         (SELECT count(*) FROM task t JOIN phase ph ON ph.id = t.phase_id
           WHERE ph.project_id = pr.id) AS task_count,
         pr.created_at, pr.updated_at"
    )
}
