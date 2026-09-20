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
        .merge(routes::user::me::routes())
        .merge(routes::user::agent::routes())
        .merge(routes::admin::people::routes())
        .merge(routes::user::work::routes())
        .merge(routes::user::run_log::routes())
        .merge(routes::services::mcp::routes())
        .merge(routes::web::auth::routes())
        .merge(routes::web::board::routes())
        .merge(routes::web::inbox::routes())
        .merge(routes::web::task::routes())
        .with_state(state)
}
