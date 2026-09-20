//! Sign-in, first-time setup, and sign-out.
//!
//! These three routes and `/health` are the **only** unauthenticated endpoints
//! in the system; everything else resolves a credential through
//! `middleware::auth::resolve`.
//!
//! Login must not enumerate the team (design §3). A wrong password, an address
//! nobody holds, and an address that exists but has never set a password all
//! return the same `AppError::Unauthorized(BAD_LOGIN)`, and all pay the same
//! argon2 cost — `password::verify_dummy` burns the work when no row matched.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::controllers::{people, token};
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::auth::Caller;
use crate::models::password;
use crate::models::token::hash_token;
use crate::response::ApiResponse;

/// The one thing a failed login is ever allowed to say.
const BAD_LOGIN: &str = "email or password is incorrect";

/// Consecutive failures before the person is locked, and for how long.
const MAX_ATTEMPTS: i32 = 5;
const LOCK_MINUTES: i64 = 15;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/setup", post(setup))
        .route("/api/auth/logout", post(logout))
}

#[derive(Deserialize)]
pub struct LoginBody {
    email: String,
    password: String,
}

#[derive(Deserialize)]
pub struct SetupBody {
    email: String,
    code: String,
    password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub email: String,
    pub name: String,
    pub role: String,
    pub scopes: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedIn {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub me: Identity,
}

#[derive(sqlx::FromRow)]
struct Account {
    id: Uuid,
    email: String,
    name: String,
    role: String,
    password_hash: String,
    failed_attempts: i32,
    locked_until: Option<DateTime<Utc>>,
}

/// Mint the session for a person who has just proved who they are. Shared by
/// login and setup so both return the identical shape.
async fn sign_in(state: &AppState, email: &str, name: &str, role: &str) -> AppResult<ApiResponse<SignedIn>> {
    let (raw, row) = token::mint_session(state, email).await?;

    Ok(ApiResponse::ok(SignedIn {
        token: raw,
        expires_at: row.expires_at,
        me: Identity {
            email: email.to_string(),
            name: name.to_string(),
            role: role.to_string(),
            scopes: row.scopes,
        },
    }))
}

/// Lowercase and trim only. Deliberately *not* `people`'s domain check: an
/// off-domain address must fail exactly like a wrong password, not with a
/// different sentence that says the address was even looked at.
fn normalise(email: &str) -> String {
    email.trim().to_lowercase()
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> AppResult<ApiResponse<SignedIn>> {
    let email = normalise(&body.email);

    let account = sqlx::query_as::<_, Account>(
        "SELECT id, email, name, role, password_hash, failed_attempts, locked_until
           FROM person
          WHERE email = $1 AND deleted_at IS NULL AND password_hash IS NOT NULL",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;

    let Some(account) = account else {
        // No row: still pay the argon2 cost, then say the same thing.
        password::verify_dummy(&body.password);
        return Err(AppError::Unauthorized(BAD_LOGIN.into()));
    };

    let correct = password::verify(&body.password, &account.password_hash);
    let locked = account.locked_until.is_some_and(|t| t > Utc::now());

    if locked {
        // Neither branch touches failed_attempts, so hammering a locked
        // account cannot push the unlock time further out.
        //
        // The lockout message is disclosed *only* to someone who supplied the
        // correct password. They have already proved the account exists, so
        // telling them why they are being refused leaks nothing; a guesser
        // gets the same sentence as for an address that does not exist.
        return Err(AppError::Unauthorized(if correct {
            format!("account temporarily locked; try again in up to {LOCK_MINUTES} minutes")
        } else {
            BAD_LOGIN.into()
        }));
    }

    if !correct {
        // Single statement: the fifth failure locks and resets the counter.
        sqlx::query(
            "UPDATE person
                SET failed_attempts = CASE WHEN failed_attempts + 1 >= $2 THEN 0 ELSE failed_attempts + 1 END,
                    locked_until = CASE WHEN failed_attempts + 1 >= $2
                                        THEN now() + ($3 || ' minutes')::interval END,
                    updated_at = now()
              WHERE id = $1",
        )
        .bind(account.id)
        .bind(MAX_ATTEMPTS)
        .bind(LOCK_MINUTES.to_string())
        .execute(&state.db)
        .await?;

        return Err(AppError::Unauthorized(BAD_LOGIN.into()));
    }

    if account.failed_attempts != 0 || account.locked_until.is_some() {
        sqlx::query(
            "UPDATE person SET failed_attempts = 0, locked_until = NULL, updated_at = now()
              WHERE id = $1",
        )
        .bind(account.id)
        .execute(&state.db)
        .await?;
    }

    sign_in(&state, &account.email, &account.name, &account.role).await
}

/// First sign-in: spend the setup code, set the password, and hand back a
/// session. There is no separate login step afterwards.
async fn setup(
    State(state): State<AppState>,
    Json(body): Json<SetupBody>,
) -> AppResult<ApiResponse<SignedIn>> {
    let person = people::set_password(&state, &body.email, &body.code, &body.password).await?;
    sign_in(&state, &person.email, &person.name, &person.role).await
}

/// Revoke the credential that made this request, and only that one. Other
/// sessions the person holds keep working; ending all of them is
/// `acp-admin revoke-person`.
async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    _caller: Caller,
) -> AppResult<ApiResponse<serde_json::Value>> {
    // `_caller` proves the token resolved; this re-reads it to know *which*
    // row to revoke, since `Caller` deliberately carries no credential id.
    let raw = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("missing bearer token".into()))?;

    sqlx::query("UPDATE credential SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL")
        .bind(hash_token(raw))
        .execute(&state.db)
        .await?;

    Ok(ApiResponse::ok(json!({ "revoked": true })))
}
