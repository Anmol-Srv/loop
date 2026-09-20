//! Approving a proposal replays it; rejecting drops it. Only a `write` token
//! may do either — a `propose` token that could approve its own work would
//! make the whole guardrail decorative.

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

async fn call(pool: &PgPool, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = acp_server::app::app(acp_server::db::AppState { db: pool.clone() })
        .oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

/// Propose a project and return the change id.
async fn propose_project(pool: &PgPool, t: &str, key: &str) -> Uuid {
    let (status, json) = call(pool, req("POST", "/api/user/projects", t,
        serde_json::json!({ "key": key, "name": "Control Plane" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["status"], "proposed");
    json["data"]["changeId"].as_str().unwrap().parse().unwrap()
}

async fn change_state(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM change WHERE id = $1")
        .bind(id).fetch_one(pool).await.unwrap()
}

#[sqlx::test]
async fn approving_a_proposal_replays_it(pool: PgPool) {
    let proposer = token(&pool, "hermes", &["read", "propose"]).await;
    let approver = token(&pool, "anmol", &["read", "write"]).await;

    let change_id = propose_project(&pool, &proposer, "acp").await;

    let (status, json) = call(&pool, req("POST", &format!("/api/user/changes/{change_id}/approve"), &approver, serde_json::json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["state"], "approved");
    assert!(!json["data"]["appliedAt"].is_null(), "an approved change records when it landed");

    let key: String = sqlx::query_scalar("SELECT key FROM project").fetch_one(&pool).await.unwrap();
    assert_eq!(key, "acp");
}

#[sqlx::test]
async fn approving_a_task_assignment_replays_it(pool: PgPool) {
    // The task/update branch is the only ambiguous one in the dispatcher.
    let proposer = token(&pool, "hermes", &["read", "propose"]).await;
    let approver = token(&pool, "anmol", &["read", "write"]).await;

    let project_id: Uuid = sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let phase_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 1) RETURNING id")
        .bind(project_id).fetch_one(&pool).await.unwrap();
    let task_id: Uuid = sqlx::query_scalar(
        "INSERT INTO task (phase_id, title) VALUES ($1, 'wire auth') RETURNING id")
        .bind(phase_id).fetch_one(&pool).await.unwrap();

    let (_, json) = call(&pool, req("POST", &format!("/api/user/tasks/{task_id}/assign"), &proposer,
        serde_json::json!({ "personEmail": "anmol@airtribe.live" }))).await;
    let change_id: Uuid = json["data"]["changeId"].as_str().unwrap().parse().unwrap();

    let (status, _) = call(&pool, req("POST", &format!("/api/user/changes/{change_id}/approve"), &approver, serde_json::json!({}))).await;
    assert_eq!(status, StatusCode::OK);

    let kind: Option<String> = sqlx::query_scalar("SELECT assignee_kind FROM task WHERE id = $1")
        .bind(task_id).fetch_one(&pool).await.unwrap();
    assert_eq!(kind.as_deref(), Some("human"));
}

#[sqlx::test]
async fn rejecting_a_proposal_drops_it(pool: PgPool) {
    let proposer = token(&pool, "hermes", &["read", "propose"]).await;
    let approver = token(&pool, "anmol", &["read", "write"]).await;

    let change_id = propose_project(&pool, &proposer, "acp").await;

    let (status, json) = call(&pool, req("POST", &format!("/api/user/changes/{change_id}/reject"), &approver, serde_json::json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["state"], "rejected");

    let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM project").fetch_one(&pool).await.unwrap();
    assert_eq!(projects, 0);
}

#[sqlx::test]
async fn a_propose_token_cannot_approve_its_own_proposal(pool: PgPool) {
    let proposer = token(&pool, "hermes", &["read", "propose"]).await;
    let change_id = propose_project(&pool, &proposer, "acp").await;

    let (status, _) = call(&pool, req("POST", &format!("/api/user/changes/{change_id}/approve"), &proposer, serde_json::json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    assert_eq!(change_state(&pool, change_id).await, "pending");
    let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM project").fetch_one(&pool).await.unwrap();
    assert_eq!(projects, 0, "a propose token must not be able to let its own work through");
}

#[sqlx::test]
async fn approving_twice_is_a_conflict(pool: PgPool) {
    let proposer = token(&pool, "hermes", &["read", "propose"]).await;
    let approver = token(&pool, "anmol", &["read", "write"]).await;

    let change_id = propose_project(&pool, &proposer, "acp").await;
    let uri = format!("/api/user/changes/{change_id}/approve");

    let (first, _) = call(&pool, req("POST", &uri, &approver, serde_json::json!({}))).await;
    assert_eq!(first, StatusCode::OK);

    let (second, _) = call(&pool, req("POST", &uri, &approver, serde_json::json!({}))).await;
    assert_eq!(second, StatusCode::CONFLICT);

    let (rejected, _) = call(&pool, req("POST", &format!("/api/user/changes/{change_id}/reject"), &approver, serde_json::json!({}))).await;
    assert_eq!(rejected, StatusCode::CONFLICT, "an approved change cannot then be rejected");
}

#[sqlx::test]
async fn the_pending_queue_lists_only_pending_changes(pool: PgPool) {
    let proposer = token(&pool, "hermes", &["read", "propose"]).await;
    let approver = token(&pool, "anmol", &["read", "write"]).await;

    let approved = propose_project(&pool, &proposer, "one").await;
    let still_pending = propose_project(&pool, &proposer, "two").await;
    call(&pool, req("POST", &format!("/api/user/changes/{approved}/approve"), &approver, serde_json::json!({}))).await;

    let (status, json) = call(&pool, req("GET", "/api/user/changes/pending", &approver, serde_json::Value::Null)).await;
    assert_eq!(status, StatusCode::OK);

    let ids: Vec<&str> = json["data"].as_array().unwrap().iter()
        .map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec![still_pending.to_string()]);
}

#[sqlx::test]
async fn a_failed_replay_leaves_the_proposal_pending(pool: PgPool) {
    let proposer = token(&pool, "hermes", &["read", "propose"]).await;
    let approver = token(&pool, "anmol", &["read", "write"]).await;

    let change_id = propose_project(&pool, &proposer, "acp").await;

    // Someone takes the key while the proposal waits.
    let (status, _) = call(&pool, req("POST", "/api/user/projects", &approver,
        serde_json::json!({ "key": "acp", "name": "Squatter" }))).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = call(&pool, req("POST", &format!("/api/user/changes/{change_id}/approve"), &approver, serde_json::json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(change_state(&pool, change_id).await, "pending");
}
