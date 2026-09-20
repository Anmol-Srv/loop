use axum::Router;

use crate::db::AppState;
use crate::routes;

pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::routes())
        .merge(routes::user::project::routes())
        .merge(routes::user::phase::routes())
        .with_state(state)
}
