//! The `admin`-scoped endpoints (§4): everything an administrator does
//! day-to-day, so only the first-admin bootstrap needs database access.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::controllers;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailBody {
    pub email: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleBody {
    pub email: String,
    pub role: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub email: String,
    /// The admin passes this on out of band; only its hash is stored.
    pub code: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonRole {
    pub email: String,
    pub role: String,
    /// Sessions ended by the change. A role change only bites once the
    /// credentials minted under the old role are gone.
    pub sessions_ended: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Revoked {
    pub email: String,
    pub credentials_revoked: u64,
}

/// Explicitly listed columns: `token_hash` lives in this table and must never
/// leave it.
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub email: String,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/invite", post(invite))
        .route("/api/admin/role", post(role))
        .route("/api/admin/revoke", post(revoke))
        .route("/api/admin/sessions", get(sessions))
}

/// Refuse anything that would leave the system with no administrator. Checked
/// for the target rather than only for the caller: the last admin is the last
/// admin whoever asks.
async fn refuse_if_last_admin(state: &AppState, email: &str, action: &str) -> AppResult<()> {
    let admins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM person WHERE role = 'admin' AND deleted_at IS NULL",
    )
    .fetch_one(&state.db)
    .await?;

    let is_admin: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM person
          WHERE email = $1 AND role = 'admin' AND deleted_at IS NULL)",
    )
    .bind(email)
    .fetch_one(&state.db)
    .await?;

    if is_admin && admins <= 1 {
        return Err(AppError::Conflict(format!(
            "'{email}' is the only admin; promote someone else before {action}"
        )));
    }

    Ok(())
}

async fn invite(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<EmailBody>,
) -> AppResult<ApiResponse<Invite>> {
    caller.require("admin")?;
    let code = controllers::people::invite(&state, &body.email).await?;
    Ok(ApiResponse::ok(Invite {
        email: body.email.trim().to_lowercase(),
        code,
    }))
}

async fn role(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<RoleBody>,
) -> AppResult<ApiResponse<PersonRole>> {
    caller.require("admin")?;

    if !matches!(body.role.as_str(), "member" | "admin") {
        return Err(AppError::BadRequest(format!(
            "unknown role '{}'; expected 'member' or 'admin'",
            body.role
        )));
    }

    let email = body.email.trim().to_lowercase();
    if body.role != "admin" {
        refuse_if_last_admin(&state, &email, "demoting them").await?;
    }

    let updated = sqlx::query_scalar::<_, String>(
        "UPDATE person SET role = $1, updated_at = now()
          WHERE email = $2 AND deleted_at IS NULL RETURNING role",
    )
    .bind(&body.role)
    .bind(&email)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("no person with email '{email}'")))?;

    // Scopes are baked into a credential when it is minted, so a demotion has
    // no effect on a session already in someone's Keychain. Ending their
    // sessions is what makes the demotion real; they sign in again and get the
    // scopes their new role grants. Agent credentials are left alone — they
    // never carry `admin` in the first place.
    let ended = sqlx::query(
        "UPDATE credential SET revoked_at = now()
          WHERE kind = 'session'
            AND revoked_at IS NULL
            AND owner_id = (SELECT id FROM person WHERE email = $1)",
    )
    .bind(&email)
    .execute(&state.db)
    .await?
    .rows_affected();

    Ok(ApiResponse::ok(PersonRole {
        email,
        role: updated,
        sessions_ended: ended,
    }))
}

async fn revoke(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<EmailBody>,
) -> AppResult<ApiResponse<Revoked>> {
    caller.require("admin")?;

    let email = body.email.trim().to_lowercase();
    refuse_if_last_admin(&state, &email, "revoking them").await?;

    let credentials_revoked = controllers::people::revoke_person(&state, &email).await?;
    Ok(ApiResponse::ok(Revoked {
        email,
        credentials_revoked,
    }))
}

async fn sessions(
    State(state): State<AppState>,
    caller: Caller,
) -> AppResult<ApiResponse<Vec<Session>>> {
    caller.require("admin")?;

    let sessions = sqlx::query_as::<_, Session>(
        "SELECT p.email, c.label, c.created_at, c.last_used_at, c.expires_at
           FROM credential c JOIN person p ON p.id = c.owner_id
          WHERE c.kind = 'session' AND c.revoked_at IS NULL AND c.expires_at > now()
          ORDER BY c.last_used_at DESC NULLS LAST, c.created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(ApiResponse::ok(sessions))
}
