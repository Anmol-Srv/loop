use axum::Router;

use crate::db::AppState;

// Filled in by auth step 4.
pub fn routes() -> Router<AppState> {
    Router::new()
}
