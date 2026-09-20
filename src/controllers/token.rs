use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::token::{hash_token, TokenRow};

const COLUMNS: &str =
    "id, kind, label, owner_id, scopes, expires_at, revoked_at, last_used_at";

/// 256 bits of randomness from uuid's CSPRNG. Using uuid here rather than
/// pulling in `rand` keeps the dependency list shorter.
fn generate_raw_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// A person's scopes come from their role, never from the caller. One mapping,
/// used by every path that creates a session.
pub fn session_scopes(role: &str) -> Vec<String> {
    let mut scopes = vec!["read".to_string(), "write".to_string()];
    if role == "admin" {
        scopes.push("admin".to_string());
    }
    scopes
}

async fn owner_id(state: &AppState, email: &str) -> AppResult<uuid::Uuid> {
    sqlx::query_scalar("SELECT id FROM person WHERE email = $1 AND deleted_at IS NULL")
        .bind(email)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("no person with email '{email}'")))
}

async fn insert(
    state: &AppState,
    kind: &str,
    label: &str,
    owner: uuid::Uuid,
    scopes: &[String],
    valid_days: i64,
) -> AppResult<(String, TokenRow)> {
    let raw = generate_raw_token();

    let row = sqlx::query_as::<_, TokenRow>(&format!(
        "INSERT INTO credential (kind, label, token_hash, owner_id, scopes, expires_at)
         VALUES ($1, $2, $3, $4, $5, now() + ($6 || ' days')::interval)
         RETURNING {COLUMNS}"
    ))
    .bind(kind)
    .bind(label)
    .bind(hash_token(&raw))
    .bind(owner)
    .bind(scopes)
    .bind(valid_days.to_string())
    .fetch_one(&state.db)
    .await?;

    Ok((raw, row))
}

/// Mint a session credential for a named person with explicit scopes.
///
/// ponytail: kept for the direct-database escape hatch in the design's §7 and
/// for tests that need a credential with chosen scopes. Ordinary sign-in goes
/// through `mint_session`.
pub async fn mint(
    state: &AppState,
    label: &str,
    owner_email: &str,
    scopes: Vec<String>,
    valid_days: i64,
) -> AppResult<(String, TokenRow)> {
    for scope in &scopes {
        if !matches!(scope.as_str(), "read" | "claim" | "propose" | "write" | "admin") {
            return Err(AppError::BadRequest(format!("unknown scope '{scope}'")));
        }
    }

    let owner = owner_id(state, owner_email).await?;
    insert(state, "session", label, owner, &scopes, valid_days).await
}

/// Sign a person in: scopes derived from their role, labelled with their email.
pub async fn mint_session(state: &AppState, owner_email: &str) -> AppResult<(String, TokenRow)> {
    let (owner, role): (uuid::Uuid, String) = sqlx::query_as(
        "SELECT id, role FROM person WHERE email = $1 AND deleted_at IS NULL",
    )
    .bind(owner_email)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("no person with email '{owner_email}'")))?;

    insert(state, "session", owner_email, owner, &session_scopes(&role), 30).await
}

/// Mint an agent credential. An agent may propose, never apply — the database
/// constraint is the backstop, this check is so the caller gets a sentence
/// rather than a constraint violation.
pub async fn mint_agent(
    state: &AppState,
    label: &str,
    owner_email: &str,
    scopes: Vec<String>,
    valid_days: i64,
) -> AppResult<(String, TokenRow)> {
    for scope in &scopes {
        match scope.as_str() {
            "read" | "claim" | "propose" => {}
            "write" | "admin" => {
                return Err(AppError::BadRequest(format!(
                    "an agent credential cannot hold '{scope}'; agents propose, they never apply"
                )))
            }
            other => return Err(AppError::BadRequest(format!("unknown scope '{other}'"))),
        }
    }

    let owner = owner_id(state, owner_email).await?;
    insert(state, "agent", label, owner, &scopes, valid_days).await
}

/// Revoking a person revokes every credential they own — sessions and agents.
pub async fn revoke_for_person(state: &AppState, owner_email: &str) -> AppResult<u64> {
    let result = sqlx::query(
        "UPDATE credential SET revoked_at = now()
         WHERE revoked_at IS NULL
           AND owner_id = (SELECT id FROM person WHERE email = $1)",
    )
    .bind(owner_email)
    .execute(&state.db)
    .await?;

    Ok(result.rows_affected())
}
