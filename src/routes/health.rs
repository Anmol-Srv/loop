use axum::{routing::get, Router};
use serde::Serialize;

use crate::db::AppState;
use crate::response::ApiResponse;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthData {
    pub status: &'static str,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

async fn health() -> ApiResponse<HealthData> {
    ApiResponse::ok(HealthData { status: "ok" })
}
