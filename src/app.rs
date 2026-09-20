use axum::Router;

use crate::db::AppState;
use crate::routes;

pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::routes())
        .merge(routes::user::project::routes())
        .merge(routes::user::phase::routes())
        .merge(routes::user::task::routes())
        .merge(routes::user::artifact::routes())
        .merge(routes::user::change::routes())
        .merge(routes::services::mcp::routes())
        .with_state(state)
}
