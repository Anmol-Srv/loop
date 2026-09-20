use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[sqlx::test]
async fn health_returns_ok_envelope(pool: sqlx::PgPool) {
    let app = acp_server::app::app(acp_server::db::AppState { db: pool });

    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["success"], true);
    assert_eq!(json["data"]["status"], "ok");
}
