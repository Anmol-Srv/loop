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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
