//! A `propose`-scoped actor must never change shared state. These tests pin
//! that invariant to the wire: the response, the table, and the change queue.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn token(pool: &PgPool, label: &str, scopes: &[&str]) -> String {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2) ON CONFLICT (email) DO NOTHING")
        .bind("anmol@airtribe.live").bind("Anmol").execute(pool).await.unwrap();
    let state = acp_server::db::AppState { db: pool.clone() };
    acp_server::controllers::token::mint(
        &state, label, "anmol@airtribe.live", scopes.iter().map(|s| s.to_string()).collect(), 30,
    ).await.unwrap().0
}

fn req(method: &str, uri: &str, token: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn json_of(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test]
async fn proposing_a_project_records_the_intent_and_creates_nothing(pool: PgPool) {
    let t = token(&pool, "hermes", &["read", "propose"]).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    let response = acp_server::app::app(state)
        .oneshot(req("POST", "/api/user/projects", &t,
            serde_json::json!({ "key": "acp", "name": "Control Plane" })))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = json_of(response).await;
    assert_eq!(json["data"]["status"], "proposed");
    assert!(json["data"]["changeId"].as_str().unwrap().parse::<Uuid>().is_ok());
    assert!(json["data"]["entity"].is_null(), "a proposal returns no entity");

    let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM project").fetch_one(&pool).await.unwrap();
    assert_eq!(projects, 0, "a propose-scoped token must not mutate shared state");

    let (count, state_col, patch): (i64, String, serde_json::Value) = sqlx::query_as(
        "SELECT count(*) OVER (), state, patch FROM change LIMIT 1",
    ).fetch_one(&pool).await.unwrap();

    assert_eq!(count, 1, "exactly one change row per proposal");
    assert_eq!(state_col, "pending");
    assert_eq!(patch["key"], "acp");
    assert_eq!(patch["name"], "Control Plane");
}

#[sqlx::test]
async fn proposing_a_task_status_leaves_the_task_untouched(pool: PgPool) {
    let t = token(&pool, "hermes", &["read", "propose"]).await;
    let project_id: Uuid = sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let phase_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 1) RETURNING id")
        .bind(project_id).fetch_one(&pool).await.unwrap();
    let task_id: Uuid = sqlx::query_scalar(
        "INSERT INTO task (phase_id, title) VALUES ($1, 'wire auth') RETURNING id")
        .bind(phase_id).fetch_one(&pool).await.unwrap();

    let response = acp_server::app::app(acp_server::db::AppState { db: pool.clone() })
        .oneshot(req("PATCH", &format!("/api/user/tasks/{task_id}"), &t,
            serde_json::json!({ "status": "done" })))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_of(response).await["data"]["status"], "proposed");

    let status: String = sqlx::query_scalar("SELECT status FROM task WHERE id = $1")
        .bind(task_id).fetch_one(&pool).await.unwrap();
    assert_ne!(status, "done", "a proposed status change must not be applied");
}

#[sqlx::test]
async fn proposing_against_a_missing_task_is_a_404_and_queues_nothing(pool: PgPool) {
    let t = token(&pool, "hermes", &["read", "propose"]).await;

    let response = acp_server::app::app(acp_server::db::AppState { db: pool.clone() })
        .oneshot(req("PATCH", &format!("/api/user/tasks/{}", Uuid::new_v4()), &t,
            serde_json::json!({ "status": "done" })))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let changes: i64 = sqlx::query_scalar("SELECT count(*) FROM change").fetch_one(&pool).await.unwrap();
    assert_eq!(changes, 0, "an unreplayable proposal must never be queued");
}

#[sqlx::test]
async fn a_write_token_still_applies_immediately(pool: PgPool) {
    let t = token(&pool, "rw", &["read", "write"]).await;

    let response = acp_server::app::app(acp_server::db::AppState { db: pool.clone() })
        .oneshot(req("POST", "/api/user/projects", &t,
            serde_json::json!({ "key": "acp", "name": "Control Plane" })))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = json_of(response).await;
    assert_eq!(json["data"]["status"], "applied");
    assert_eq!(json["data"]["entity"]["key"], "acp");

    let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM project").fetch_one(&pool).await.unwrap();
    assert_eq!(projects, 1);
}
