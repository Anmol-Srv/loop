use axum::extract::State;
use axum::routing::get;
use axum::Router;

use crate::controllers::home::{self, Home};
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::response::ApiResponse;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/user/home", get(get_home))
}

async fn get_home(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Home>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(home::home(&state, caller.person_id()?).await?))
}
