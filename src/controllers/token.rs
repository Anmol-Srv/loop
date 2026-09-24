use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::token::{hash_token, TokenRow};

const COLUMNS: &str =
    "id, kind, label, owner_id, scopes, expires_at, revoked_at, last_used_at, created_at, agent_id";

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
    db: impl sqlx::PgExecutor<'_>,
    kind: &str,
    label: &str,
    owner: uuid::Uuid,
    scopes: &[String],
    valid_days: i64,
    agent_id: Option<uuid::Uuid>,
) -> AppResult<(String, TokenRow)> {
    let raw = generate_raw_token();

    let row = sqlx::query_as::<_, TokenRow>(&format!(
        "INSERT INTO credential (kind, label, token_hash, owner_id, scopes, expires_at, agent_id)
         VALUES ($1, $2, $3, $4, $5, now() + ($6 || ' days')::interval, $7)
         RETURNING {COLUMNS}"
    ))
    .bind(kind)
    .bind(label)
    .bind(hash_token(&raw))
    .bind(owner)
    .bind(scopes)
    .bind(valid_days.to_string())
    .bind(agent_id)
    .fetch_one(db)
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
    insert(&state.db, "session", label, owner, &scopes, valid_days, None).await
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

    insert(&state.db, "session", owner_email, owner, &session_scopes(&role), 30, None).await
}

/// Mint the credential an agent speaks with. It carries no scopes: agent
/// routes authorise by the agent and what is delegated to it, never by scope,
/// and the `/api/user` routes it has no business on refuse it for want of
/// `read`. The database still forbids an agent `write` or `admin`.
pub async fn mint_agent(
    tx: &mut sqlx::PgTransaction<'_>,
    agent_id: uuid::Uuid,
    owner: uuid::Uuid,
    handle: &str,
) -> AppResult<(String, TokenRow)> {
    insert(&mut **tx, "agent", handle, owner, &[], MAX_AGENT_DAYS, Some(agent_id)).await
}

/// The longest an agent credential lives. Long enough for a quarter's
/// automation, short enough that a forgotten one ends by itself.
pub const MAX_AGENT_DAYS: i64 = 90;

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
