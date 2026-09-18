use axum::{routing::get, Router};
use serde::Serialize;

use crate::response::ApiResponse;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthData {
    pub status: &'static str,
}

pub fn routes() -> Router {
    Router::new().route("/health", get(health))
}

async fn health() -> ApiResponse<HealthData> {
    ApiResponse::ok(HealthData { status: "ok" })
}
