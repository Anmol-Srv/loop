use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::token::{hash_token, TokenRow};

/// 256 bits of randomness from uuid's CSPRNG. Using uuid here rather than
/// pulling in `rand` keeps the dependency list shorter.
fn generate_raw_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub async fn mint(
    state: &AppState,
    label: &str,
    owner_email: &str,
    scopes: Vec<String>,
    valid_days: i64,
) -> AppResult<(String, TokenRow)> {
    for scope in &scopes {
        if !matches!(scope.as_str(), "read" | "claim" | "propose" | "write") {
            return Err(AppError::BadRequest(format!("unknown scope '{scope}'")));
        }
    }

    let owner_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM person WHERE email = $1 AND deleted_at IS NULL",
    )
    .bind(owner_email)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("no person with email '{owner_email}'")))?;

    let raw = generate_raw_token();

    let row = sqlx::query_as::<_, TokenRow>(
        "INSERT INTO agent_token (label, token_hash, owner_id, scopes, expires_at)
         VALUES ($1, $2, $3, $4, now() + ($5 || ' days')::interval)
         RETURNING id, label, owner_id, scopes, expires_at, revoked_at",
    )
    .bind(label)
    .bind(hash_token(&raw))
    .bind(owner_id)
    .bind(&scopes)
    .bind(valid_days.to_string())
    .fetch_one(&state.db)
    .await?;

    Ok((raw, row))
}

/// Revoking a person revokes every token they own.
pub async fn revoke_for_person(state: &AppState, owner_email: &str) -> AppResult<u64> {
    let result = sqlx::query(
        "UPDATE agent_token SET revoked_at = now()
         WHERE revoked_at IS NULL
           AND owner_id = (SELECT id FROM person WHERE email = $1)",
    )
    .bind(owner_email)
    .execute(&state.db)
    .await?;

    Ok(result.rows_affected())
}
