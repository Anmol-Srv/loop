use acp_server::controllers::token;
use acp_server::db::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

async fn seed(pool: &PgPool) {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .execute(pool)
        .await
        .unwrap();
}

fn create_req(token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/api/user/projects")
        .header("content-type", "application/json");
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    b.body(Body::from(r#"{"key":"acp","name":"Control Plane"}"#)).unwrap()
}

#[sqlx::test]
async fn requests_without_a_token_are_rejected(pool: PgPool) {
    let app = acp_server::app::app(AppState { db: pool });
    let response = app.oneshot(create_req(None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn a_garbage_token_is_rejected(pool: PgPool) {
    let app = acp_server::app::app(AppState { db: pool });
    let response = app.oneshot(create_req(Some("not-a-real-token"))).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn read_only_token_cannot_mutate(pool: PgPool) {
    seed(&pool).await;
    let state = AppState { db: pool };
    let (raw, _) = token::mint(&state, "ro", "anmol@airtribe.live", vec!["read".into()], 30).await.unwrap();

    let response = acp_server::app::app(state).oneshot(create_req(Some(&raw))).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn write_token_applies_immediately(pool: PgPool) {
    seed(&pool).await;
    let state = AppState { db: pool.clone() };
    let (raw, _) = token::mint(&state, "rw", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30).await.unwrap();

    let response = acp_server::app::app(state).oneshot(create_req(Some(&raw))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let state_col: String = sqlx::query_scalar("SELECT state FROM change LIMIT 1").fetch_one(&pool).await.unwrap();
    assert_eq!(state_col, "applied");
}

#[sqlx::test]
async fn propose_token_queues_a_pending_change(pool: PgPool) {
    seed(&pool).await;
    let state = AppState { db: pool.clone() };
    let (raw, _) = token::mint(&state, "hermes", "anmol@airtribe.live", vec!["read".into(), "propose".into()], 30).await.unwrap();

    let response = acp_server::app::app(state).oneshot(create_req(Some(&raw))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let _json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let state_col: String = sqlx::query_scalar("SELECT state FROM change LIMIT 1").fetch_one(&pool).await.unwrap();
    assert_eq!(state_col, "pending", "a propose-scoped actor must never write an applied change");
}
