use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RunLogLine {
    pub id: Uuid,
    pub task_id: Uuid,
    pub seq: i64,
    pub text: String,
    pub created_at: DateTime<Utc>,
}
