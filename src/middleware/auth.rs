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
}

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
/// `middleware::session` both go through here, so they cannot diverge.
pub async fn resolve(db: &PgPool, raw: &str) -> AppResult<Caller> {
    let row = token::lookup(db, raw)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid or expired token".into()))?;

    let can_apply = row.scopes.iter().any(|s| s == "write");

    // Every resolve touches `last_used_at` so stale sessions are visible, but
    // the expiry only slides once under 29 days — one write per person per
    // day rather than one per poll from the Mac app. Agent credentials are
    // minted with a deliberate lifetime and never slide.
    sqlx::query(
        "UPDATE credential
            SET last_used_at = now(),
                expires_at = CASE WHEN $2 AND expires_at < now() + interval '29 days'
                                  THEN now() + interval '30 days'
                                  ELSE expires_at END
          WHERE id = $1",
    )
    .bind(row.id)
    .bind(row.kind == "session")
    .execute(db)
    .await?;

    Ok(Caller {
        actor: Actor {
            label: row.label,
            person_id: Some(row.owner_id),
            can_apply,
        },
        scopes: row.scopes,
    })
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
