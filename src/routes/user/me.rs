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
    /// What to call them. The sidebar used to show the part of the email
    /// before the dot, which for most of the team is a truncated surname.
    pub name: Option<String>,
    pub role: Option<String>,
    /// The department this person belongs to. A task they are given takes
    /// its discipline from here.
    pub department: String,
    pub scopes: Vec<String>,
    pub can_apply: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/user/me", get(me))
}

/// Who the caller is and what they may do. A client renders its controls from
/// this rather than guessing, so the UI shows the same truth the API enforces.
async fn me(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Me>> {
    let who: Option<(String, String, String, String)> = match caller.actor.person_id {
        Some(id) => {
            sqlx::query_as("SELECT email, name, role, department FROM person WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.db)
                .await?
        }
        None => None,
    };
    let (email, name, role, department) = match who {
        Some((e, n, r, d)) => (Some(e), Some(n), Some(r), d),
        // An agent credential belongs to no person and so to no department.
        None => (None, None, None, String::new()),
    };

    Ok(ApiResponse::ok(Me {
        label: caller.actor.label.clone(),
        person_id: caller.actor.person_id,
        email,
        name,
        role,
        department,
        can_apply: caller.actor.can_apply,
        scopes: caller.scopes,
    }))
}
