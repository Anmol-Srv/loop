use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;

/// Tokens are stored as a SHA-256 hex digest. The raw value is shown once at
/// mint time and never persisted.
pub fn hash_token(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TokenRow {
    pub id: Uuid,
    pub label: String,
    pub owner_id: Uuid,
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Resolve a raw bearer token. Unknown, revoked, and expired tokens all return
/// `None` so a caller cannot tell them apart.
pub async fn lookup(db: &PgPool, raw: &str) -> AppResult<Option<TokenRow>> {
    let row = sqlx::query_as::<_, TokenRow>(
        "SELECT id, label, owner_id, scopes, expires_at, revoked_at
         FROM agent_token
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(hash_token(raw))
    .fetch_optional(db)
    .await?;

    Ok(row)
}
