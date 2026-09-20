use axum::extract::State;
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub label: String,
    pub person_id: Option<Uuid>,
    pub email: Option<String>,
    pub role: Option<String>,
    /// What this person works on. The app uses it to say which disciplines an
    /// "available to claim" list was matched against.
    pub disciplines: Vec<String>,
    pub scopes: Vec<String>,
    pub can_apply: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/user/me", get(me))
}

/// Who the caller is and what they may do. A client renders its controls from
/// this rather than guessing, so the UI shows the same truth the API enforces.
async fn me(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Me>> {
    let who: Option<(String, String, Vec<String>)> = match caller.actor.person_id {
        Some(id) => {
            sqlx::query_as("SELECT email, role, disciplines FROM person WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.db)
                .await?
        }
        None => None,
    };
    let (email, role, disciplines) = match who {
        Some((e, r, d)) => (Some(e), Some(r), d),
        None => (None, None, Vec::new()),
    };

    Ok(ApiResponse::ok(Me {
        label: caller.actor.label.clone(),
        person_id: caller.actor.person_id,
        email,
        role,
        disciplines,
        can_apply: caller.actor.can_apply,
        scopes: caller.scopes,
    }))
}
