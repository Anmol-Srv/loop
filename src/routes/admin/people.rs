//! The `admin`-scoped endpoints (§4): everything an administrator does
//! day-to-day, so only the first-admin bootstrap needs database access.

use axum::extract::{Path, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::controllers;
use crate::controllers::people::PersonRow;
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonPatch {
    #[serde(default)]
    pub department: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
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
        .route("/api/admin/people/{id}", patch(update))
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

    let email = body.email.trim().to_lowercase();
    let id = controllers::people::id_of(&state, &email).await?;
    let (person, sessions_ended) = controllers::people::set_role(&state, id, &body.role).await?;

    Ok(ApiResponse::ok(PersonRole {
        email,
        role: person.role,
        sessions_ended,
    }))
}

/// A person's department and role, by id — the admin's edit of someone else.
/// Both optional; one request can change either or both. The role's name is
/// checked before anything is written, so a typo cannot half-apply a request.
/// The last-admin refusal can still come after the department has moved;
/// that half stands on its own and is not worth a shared transaction.
async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<PersonPatch>,
) -> AppResult<ApiResponse<PersonRow>> {
    caller.require("admin")?;
    if body.department.is_none() && body.role.is_none() {
        return Err(AppError::BadRequest("give a department, a role, or both".into()));
    }
    if let Some(role) = &body.role {
        if !controllers::people::ROLES.contains(&role.as_str()) {
            return Err(AppError::BadRequest(format!(
                "unknown role '{role}'; expected one of {}",
                controllers::people::ROLES.join(", ")
            )));
        }
    }

    let mut person = None;
    if let Some(department) = &body.department {
        person = Some(controllers::people::set_department(&state, &caller.actor, id, department).await?);
    }
    if let Some(role) = &body.role {
        person = Some(controllers::people::set_role(&state, id, role).await?.0);
    }
    Ok(ApiResponse::ok(person.expect("one of the two was given")))
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
