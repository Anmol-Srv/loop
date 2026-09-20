use axum::extract::State;
use axum::{routing::get, Router};
use serde::Serialize;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::response::ApiResponse;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthData {
    pub status: &'static str,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(live))
        .route("/health/ready", get(ready))
}

/// Liveness: the process is up. Deliberately touches nothing else, so a sick
/// database cannot get the process killed and restarted pointlessly.
async fn live() -> ApiResponse<HealthData> {
    ApiResponse::ok(HealthData { status: "ok" })
}

/// Readiness: the process can actually serve. Checked by the deployment before
/// sending traffic.
async fn ready(State(state): State<AppState>) -> AppResult<ApiResponse<HealthData>> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(format!("database unreachable: {e}")))?;

    Ok(ApiResponse::ok(HealthData { status: "ready" }))
}
