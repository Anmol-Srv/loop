use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use sqlx::PgPool;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::Actor;
use crate::models::token;

#[derive(Debug, Clone)]
pub struct Caller {
    pub actor: Actor,
    pub scopes: Vec<String>,
    /// 'session' or 'agent'. An agent's `person_id` is its owner's, so the
    /// person id alone cannot tell a person from their agent — this can.
    pub kind: String,
    /// The agent an agent credential speaks for; `None` for a session.
    pub agent_id: Option<uuid::Uuid>,
}

/// However recently a session was used, it ends this long after sign-in, so a
/// session copied off a laptop cannot be kept alive forever by using it.
pub const SESSION_MAX_DAYS: i64 = 90;

impl Caller {
    pub fn has(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    pub fn require(&self, scope: &str) -> AppResult<()> {
        if self.has(scope) {
            Ok(())
        } else {
            Err(AppError::Forbidden(format!("this token lacks the '{scope}' scope")))
        }
    }

    /// The person behind the credential. Every credential is owned by
    /// somebody — an agent token included — so the personal endpoints work
    /// for an agent acting on its owner's behalf.
    pub fn person_id(&self) -> AppResult<uuid::Uuid> {
        self.actor
            .person_id
            .ok_or_else(|| AppError::Forbidden("this credential is not tied to a person".into()))
    }

    /// The agent behind an agent credential. The `/api/agent` routes and the
    /// agent MCP tools start here, so a person's session is turned away with
    /// a sentence rather than acting as an agent.
    pub fn agent(&self) -> AppResult<uuid::Uuid> {
        self.agent_id.ok_or_else(|| {
            AppError::Forbidden(
                "this is for agent credentials; connect an agent from the Agents page and use its token".into(),
            )
        })
    }

    /// A mutation needs either `propose` (queues as pending) or `write`
    /// (applies immediately).
    pub fn can_mutate(&self) -> AppResult<()> {
        if self.has("propose") || self.has("write") {
            Ok(())
        } else {
            Err(AppError::Forbidden("this token lacks the 'propose' or 'write' scope".into()))
        }
    }
}

/// Turn a raw token string into a `Caller`. The only place a token becomes
/// authority: the header extractor below and the cookie extractor in
/// Every client — the Mac app, the CLI, and MCP — goes through here, so no
/// two of them can diverge on what a credential means.
pub async fn resolve(db: &PgPool, raw: &str) -> AppResult<Caller> {
    let row = token::lookup(db, raw)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid or expired token".into()))?;

    if row.kind == "session"
        && row.created_at < chrono::Utc::now() - chrono::Duration::days(SESSION_MAX_DAYS)
    {
        return Err(AppError::Unauthorized("invalid or expired token".into()));
    }

    let can_apply = row.scopes.iter().any(|s| s == "write");

    // Every resolve touches `last_used_at` so stale sessions are visible, but
    // the expiry only slides once under 29 days — one write per person per
    // day rather than one per poll from the Mac app. Agent credentials are
    // minted with a deliberate lifetime and never slide. The slide stops at
    // the 90-day cap, so the expiry an admin sees is the real one.
    //
    // Even the touch is a write per request, and the app makes several per
    // screen, so it waits five minutes between them: "last used" to the
    // minute is all an admin reads it for, and the slide lagging five minutes
    // behind a 30-day window changes nothing. The cap is on `created_at`, so
    // skipping a touch cannot extend anything.
    let fresh = row
        .last_used_at
        .is_some_and(|at| at > chrono::Utc::now() - chrono::Duration::minutes(5));
    if !fresh {
        touch(db, &row).await?;
    }

    // An agent's "last seen" is what its owner reads to know it is alive, so
    // it is finer than the credential's: to the minute. The WHERE makes the
    // write a no-op inside the minute.
    // ponytail: one UPDATE per agent call; batch in memory if agents ever
    // call often enough for that to show.
    if let Some(agent) = row.agent_id {
        sqlx::query(
            "UPDATE agent SET last_seen_at = now()
              WHERE id = $1 AND (last_seen_at IS NULL OR last_seen_at < now() - interval '1 minute')",
        )
        .bind(agent)
        .execute(db)
        .await?;
    }

    Ok(Caller {
        actor: Actor {
            label: row.label,
            person_id: Some(row.owner_id),
            can_apply,
        },
        scopes: row.scopes,
        kind: row.kind,
        agent_id: row.agent_id,
    })
}

async fn touch(db: &PgPool, row: &token::TokenRow) -> AppResult<()> {
    sqlx::query(
        "UPDATE credential
            SET last_used_at = now(),
                expires_at = CASE WHEN $2 AND expires_at < now() + interval '29 days'
                                  THEN least(now() + interval '30 days',
                                             created_at + ($3 || ' days')::interval)
                                  ELSE expires_at END
          WHERE id = $1",
    )
    .bind(row.id)
    .bind(row.kind == "session")
    .bind(SESSION_MAX_DAYS.to_string())
    .execute(db)
    .await?;
    Ok(())
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| AppError::Unauthorized("missing bearer token".into()))?;

        resolve(&state.db, raw).await
    }
}
