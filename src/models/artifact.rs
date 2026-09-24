use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// What a piece of evidence is. `pr` and `commit` close engineering work,
/// `figma` closes design work, `doc` and `link` are context that closes
/// nothing.
pub const ARTIFACT_KINDS: [&str; 5] = ["pr", "commit", "figma", "doc", "link"];
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
    /// `{id, name}` of the person who added it (an agent's owner, when an
    /// agent attached it), or null for rows from before anyone was recorded.
    pub added_by: Option<Value>,
    /// The agent's name, when an agent attached it for `added_by`.
    pub added_by_agent: Option<String>,
    /// Whether the person asking may remove it; see `controllers::artifact`.
    pub can_remove: bool,
    pub created_at: DateTime<Utc>,
}
