use axum::Router;

use crate::db::AppState;
use crate::routes;

pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::routes())
        .with_state(state)
}
