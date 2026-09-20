use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub const PHASE_STATUSES: [&str; 4] = ["planned", "active", "blocked", "done"];

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    pub id: Uuid,
    pub project_id: Uuid,
    pub position: i32,
    pub name: String,
    pub status: String,
    pub gate: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
