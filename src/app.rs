use axum::Router;

use crate::db::AppState;
use crate::routes;

pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::routes())
        .merge(routes::auth::routes())
        .merge(routes::user::project::routes())
        .merge(routes::user::phase::routes())
        .merge(routes::user::task::routes())
        .merge(routes::user::artifact::routes())
        .merge(routes::user::change::routes())
        .merge(routes::user::me::routes())
        .merge(routes::user::agent::routes())
        .merge(routes::user::people::routes())
        .merge(routes::user::home::routes())
        .merge(routes::admin::people::routes())
        .merge(routes::user::work::routes())
        .merge(routes::user::run_log::routes())
        .merge(routes::services::mcp::routes())
        .with_state(state)
}
