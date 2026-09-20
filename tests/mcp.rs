use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// Mints a token with the given scopes.
async fn mint(pool: &PgPool, scopes: &[&str]) -> String {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind("anmol@airtribe.live").bind("Anmol").execute(pool).await.unwrap();
    let state = acp_server::db::AppState { db: pool.clone() };
    let (raw, _) = acp_server::controllers::token::mint(
        &state,
        &format!("mcp-{}", scopes.join("-")),
        "anmol@airtribe.live",
        scopes.iter().map(|s| s.to_string()).collect(),
        30,
    ).await.unwrap();
    raw
}

/// A token with the given scopes, plus a phase to work in.
async fn setup(pool: &PgPool, scopes: &[&str]) -> (String, Uuid) {
    let raw = mint(pool, scopes).await;
    let project_id: Uuid = sqlx::query_scalar(
        "INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
        .fetch_one(pool).await.unwrap();
    let phase_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 1) RETURNING id")
        .bind(project_id).fetch_one(pool).await.unwrap();
    (raw, phase_id)
}

fn rpc(token: Option<&str>, body: Value) -> Request<Body> {
    let b = Request::builder().method("POST").uri("/api/services/mcp")
        .header("content-type", "application/json");
    let b = match token {
        Some(t) => b.header("authorization", format!("Bearer {t}")),
        None => b,
    };
    b.body(Body::from(body.to_string())).unwrap()
}

async fn json_of(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn send(state: &acp_server::db::AppState, token: &str, method: &str, params: Value) -> Value {
    let response = acp_server::app::app(state.clone())
        .oneshot(rpc(Some(token), json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json_of(response).await
}

/// The text content of a successful tools/call, parsed back into JSON.
fn content(response: &Value) -> Value {
    let text = response["result"]["content"][0]["text"].as_str().expect("text content");
    serde_json::from_str(text).unwrap()
}

fn tool_names(response: &Value) -> Vec<String> {
    response["result"]["tools"].as_array().unwrap().iter()
        .map(|t| t["name"].as_str().unwrap().to_string()).collect()
}

#[sqlx::test]
async fn initialize_identifies_the_server(pool: PgPool) {
    let token = mint(&pool, &["read"]).await;
    let state = acp_server::db::AppState { db: pool };

    let body = send(&state, &token, "initialize", json!({})).await;
    assert_eq!(body["jsonrpc"], "2.0");
    assert!(body["result"]["protocolVersion"].is_string());
    assert_eq!(body["result"]["serverInfo"]["name"], "acp");
}

#[sqlx::test]
async fn tools_are_filtered_by_scope(pool: PgPool) {
    let read_only = mint(&pool, &["read"]).await;
    let proposer = mint(&pool, &["read", "propose"]).await;
    let state = acp_server::db::AppState { db: pool };

    let names = tool_names(&send(&state, &read_only, "tools/list", json!({})).await);
    assert!(names.contains(&"task_search".to_string()));
    assert!(!names.contains(&"task_create".to_string()), "read-only saw {names:?}");

    let names = tool_names(&send(&state, &proposer, "tools/list", json!({})).await);
    assert!(names.contains(&"task_create".to_string()));
}

#[sqlx::test]
async fn task_search_returns_tasks_as_text_content(pool: PgPool) {
    let (token, phase_id) = setup(&pool, &["read", "write"]).await;
    let state = acp_server::db::AppState { db: pool };

    send(&state, &token, "tools/call", json!({
        "name": "task_create", "arguments": { "phaseId": phase_id, "title": "wire mcp" }
    })).await;

    let tasks = content(&send(&state, &token, "tools/call", json!({
        "name": "task_search", "arguments": { "phaseId": phase_id }
    })).await);
    assert_eq!(tasks.as_array().unwrap().len(), 1);
    assert_eq!(tasks[0]["title"], "wire mcp");
}

#[sqlx::test]
async fn propose_scoped_task_create_creates_nothing(pool: PgPool) {
    let (token, phase_id) = setup(&pool, &["read", "propose"]).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    let result = content(&send(&state, &token, "tools/call", json!({
        "name": "task_create", "arguments": { "phaseId": phase_id, "title": "do not create me" }
    })).await);

    assert_eq!(result["status"], "proposed");
    assert!(Uuid::parse_str(result["changeId"].as_str().unwrap()).is_ok());

    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM task").fetch_one(&pool).await.unwrap();
    assert_eq!(tasks, 0, "a propose-scoped call must not create a task");

    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE state = 'pending'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(pending, 1);
}

#[sqlx::test]
async fn write_scoped_task_create_creates_the_task(pool: PgPool) {
    let (token, phase_id) = setup(&pool, &["read", "write"]).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    let result = content(&send(&state, &token, "tools/call", json!({
        "name": "task_create", "arguments": { "phaseId": phase_id, "title": "create me", "priority": 1 }
    })).await);

    assert_eq!(result["status"], "applied");
    assert_eq!(result["entity"]["title"], "create me");

    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM task").fetch_one(&pool).await.unwrap();
    assert_eq!(tasks, 1);
}

#[sqlx::test]
async fn unknown_tools_and_methods_are_rejected(pool: PgPool) {
    let token = mint(&pool, &["read"]).await;
    let state = acp_server::db::AppState { db: pool };

    let body = send(&state, &token, "tools/call", json!({ "name": "nonsense", "arguments": {} })).await;
    assert_eq!(body["error"]["code"], -32602);

    // A tool hidden by scope is unknown, not merely refused.
    let body = send(&state, &token, "tools/call", json!({
        "name": "task_create", "arguments": { "phaseId": Uuid::new_v4(), "title": "x" }
    })).await;
    assert_eq!(body["error"]["code"], -32602);

    let body = send(&state, &token, "tools/nope", json!({})).await;
    assert_eq!(body["error"]["code"], -32601);
}

#[sqlx::test]
async fn malformed_json_is_a_parse_error(pool: PgPool) {
    let token = mint(&pool, &["read"]).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state)
        .oneshot(Request::builder().method("POST").uri("/api/services/mcp")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from("{not json")).unwrap())
        .await.unwrap();
    assert_eq!(json_of(response).await["error"]["code"], -32700);
}

#[sqlx::test]
async fn docs_are_served_as_resources(pool: PgPool) {
    let token = mint(&pool, &["read"]).await;
    let state = acp_server::db::AppState { db: pool };

    let body = send(&state, &token, "resources/list", json!({})).await;
    let uris: Vec<String> = body["result"]["resources"].as_array().unwrap().iter()
        .map(|r| r["uri"].as_str().unwrap().to_string()).collect();
    assert!(uris.contains(&"acp://docs/task_search".to_string()));
    assert!(!uris.contains(&"acp://docs/task_create".to_string()));

    let body = send(&state, &token, "resources/read", json!({ "uri": "acp://docs/task_search" })).await;
    let text = body["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("# task_search"));

    let body = send(&state, &token, "resources/read", json!({ "uri": "acp://docs/nope" })).await;
    assert_eq!(body["error"]["code"], -32602);
}

#[sqlx::test]
async fn a_request_without_a_token_is_unauthorized(pool: PgPool) {
    let state = acp_server::db::AppState { db: pool };
    let response = acp_server::app::app(state)
        .oneshot(rpc(None, json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" })))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
