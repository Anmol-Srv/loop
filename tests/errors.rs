use acp_server::errors::AppError;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use http_body_util::BodyExt;

#[tokio::test]
async fn not_found_maps_to_404_envelope() {
    let response = AppError::NotFound("project not found".into()).into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["success"], false);
    assert_eq!(json["error"]["code"], "NOT_FOUND");
    assert_eq!(json["error"]["message"], "project not found");
}

#[tokio::test]
async fn database_errors_do_not_leak_details() {
    let response = AppError::Database(sqlx::Error::RowNotFound).into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["error"]["code"], "INTERNAL_ERROR");
    assert_eq!(json["error"]["message"], "internal server error");
}
