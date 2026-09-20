use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub const ARTIFACT_KINDS: [&str; 3] = ["pr", "doc", "link"];
pub const PARENT_TYPES: [&str; 3] = ["project", "phase", "task"];

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: Uuid,
    pub parent_type: String,
    pub parent_id: Uuid,
    pub kind: String,
    pub url: String,
    pub title: String,
    pub metadata: Value,
    pub added_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}
