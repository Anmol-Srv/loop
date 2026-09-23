//! Self-serve agent credentials. Minting one needs only `read` (§5): a
//! read-only reviewer may run a read-only agent, and the schema guarantees an
//! agent can never do more than propose.

use axum::extract::{Path, State};
use axum::routing::{delete, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::controllers;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

fn default_valid_days() -> i64 {
    30
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MintBody {
    pub label: String,
    pub scopes: Vec<String>,
    #[serde(default = "default_valid_days")]
    pub valid_days: i64,
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: Uuid,
    pub label: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Minted {
    /// Shown once. Only the hash is stored.
    pub token: String,
    pub agent: Agent,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/agents", post(mint).get(list))
        .route("/api/user/agents/{id}", delete(revoke))
}

/// The person behind the credential. An agent carries its owner's person id,
/// so that alone never refused an agent — the credential kind does. Without
/// it, a read/propose token could mint a sibling valid for a century.
async fn owner(state: &AppState, caller: &Caller) -> AppResult<(Uuid, String)> {
    if caller.kind != "session" {
        return Err(AppError::Forbidden("only a signed-in person can own an agent".into()));
    }
    let id = caller
        .actor
        .person_id
        .ok_or_else(|| AppError::Forbidden("only a signed-in person can own an agent".into()))?;

    let email: String = sqlx::query_scalar("SELECT email FROM person WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;

    Ok((id, email))
}

async fn mint(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<MintBody>,
) -> AppResult<ApiResponse<Minted>> {
    caller.require("read")?;
    let (_, email) = owner(&state, &caller).await?;

    // An agent can do no more than the person who made it. `write` covers the
    // two agent-only scopes: whoever may apply a change may propose one, and
    // may hand out a lease on work.
    for scope in &body.scopes {
        let held = caller.has(scope)
            || (caller.has("write") && matches!(scope.as_str(), "propose" | "claim"));
        if !held {
            return Err(AppError::Forbidden(format!(
                "you cannot give an agent '{scope}'; your own credential lacks it"
            )));
        }
    }

    // mint_agent turns `write`/`admin` into a sentence rather than a
    // constraint violation; let that message reach the caller unaltered.
    let (token, row) =
        controllers::token::mint_agent(&state, &body.label, &email, body.scopes, body.valid_days)
            .await?;

    let agent = sqlx::query_as::<_, Agent>(
        "SELECT id, label, scopes, created_at, expires_at, last_used_at
         FROM credential WHERE id = $1",
    )
    .bind(row.id)
    .fetch_one(&state.db)
    .await?;

    Ok(ApiResponse::ok(Minted { token, agent }))
}

async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<Agent>>> {
    caller.require("read")?;
    let (person_id, _) = owner(&state, &caller).await?;

    let agents = sqlx::query_as::<_, Agent>(
        "SELECT id, label, scopes, created_at, expires_at, last_used_at
           FROM credential
          WHERE owner_id = $1 AND kind = 'agent'
            AND revoked_at IS NULL AND expires_at > now()
          ORDER BY created_at DESC",
    )
    .bind(person_id)
    .fetch_all(&state.db)
    .await?;

    Ok(ApiResponse::ok(agents))
}

async fn revoke(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Agent>> {
    caller.require("read")?;
    let (person_id, _) = owner(&state, &caller).await?;

    // Ownership is part of the WHERE clause, so someone else's agent is
    // indistinguishable from one that does not exist. A 403 here would confirm
    // the id is real.
    sqlx::query_as::<_, Agent>(
        "UPDATE credential SET revoked_at = now()
          WHERE id = $1 AND owner_id = $2 AND kind = 'agent' AND revoked_at IS NULL
      RETURNING id, label, scopes, created_at, expires_at, last_used_at",
    )
    .bind(id)
    .bind(person_id)
    .fetch_optional(&state.db)
    .await?
    .map(ApiResponse::ok)
    .ok_or_else(|| AppError::NotFound(format!("no agent credential '{id}'")))
}
