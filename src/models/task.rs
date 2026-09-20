use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub const TASK_STATUSES: [&str; 6] =
    ["open", "in_progress", "in_review", "blocked", "done", "dropped"];

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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct TaskFilter {
    pub project_id: Option<Uuid>,
    pub phase_id: Option<Uuid>,
    pub status: Option<String>,
    pub assignee_email: Option<String>,
    pub assignee_kind: Option<String>,
}

pub const TASK_COLUMNS: &str = "id, phase_id, title, body, status, priority, \
    assignee_kind, assignee_person_id, assignee_token_id, claimed_by, \
    claim_expires_at, created_at, updated_at";
