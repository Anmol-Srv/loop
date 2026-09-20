use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

fn post(uri: &str, token: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn write_token(pool: &PgPool) -> String {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .execute(pool)
        .await
        .unwrap();
    let state = acp_server::db::AppState { db: pool.clone() };
    acp_server::controllers::token::mint(
        &state, "test", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30,
    )
    .await
    .unwrap()
    .0
}

#[sqlx::test]
async fn creating_a_project_returns_it_and_records_one_change(pool: PgPool) {
    let t = write_token(&pool).await;
    let app = acp_server::app::app(acp_server::db::AppState { db: pool.clone() });

    let response = app
        .oneshot(post("/api/user/projects", &t, serde_json::json!({ "key": "acp", "name": "Control Plane" })))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["success"], true);
    assert_eq!(json["data"]["key"], "acp");
    assert_eq!(json["data"]["name"], "Control Plane");
    assert_eq!(json["data"]["status"], "active");

    let (count, target_type, op): (i64, String, String) = sqlx::query_as(
        "SELECT count(*) OVER (), target_type, op FROM change LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 1, "exactly one change row per mutation");
    assert_eq!(target_type, "project");
    assert_eq!(op, "create");
}

#[sqlx::test]
async fn duplicate_project_keys_are_rejected(pool: PgPool) {
    let t = write_token(&pool).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    let first = acp_server::app::app(state.clone())
        .oneshot(post("/api/user/projects", &t, serde_json::json!({ "key": "acp", "name": "One" })))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = acp_server::app::app(state)
        .oneshot(post("/api/user/projects", &t, serde_json::json!({ "key": "acp", "name": "Two" })))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::CONFLICT);

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM change")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "a rejected mutation must leave no change row");
}

#[sqlx::test]
async fn listing_returns_created_projects(pool: PgPool) {
    let t = write_token(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    acp_server::app::app(state.clone())
        .oneshot(post("/api/user/projects", &t, serde_json::json!({ "key": "acp", "name": "Control Plane" })))
        .await
        .unwrap();

    let response = acp_server::app::app(state)
        .oneshot(
            Request::builder()
                .uri("/api/user/projects")
                .header("authorization", format!("Bearer {t}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["key"], "acp");
}
