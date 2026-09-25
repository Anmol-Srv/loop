//! A task filed from a thread reply carries the earlier messages in the
//! thread: stored with the source, masked with it for a DM, bounded, and
//! handed to the agent that later works the task in its context.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use acp_server::controllers::token;
use acp_server::db::AppState;

fn state(pool: &PgPool) -> AppState {
    AppState { db: pool.clone() }
}

async fn person(pool: &PgPool, email: &str, role: &str) -> String {
    sqlx::query(
        "INSERT INTO person (email, name, department, role)
         VALUES ($1, initcap(split_part($1, '@', 1)), 'backend', $2)",
    )
    .bind(email)
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    token::mint_session(&state(pool), email).await.unwrap().0
}

async fn call(
    pool: &PgPool,
    method: &str,
    uri: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("host", "acp.test")
        .header("authorization", format!("Bearer {token}"))
        .body(if body.is_null() {
            Body::empty()
        } else {
            Body::from(body.to_string())
        })
        .unwrap();
    let response = acp_server::app::app(state(pool))
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn ok(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> Value {
    let (status, v) = call(pool, method, uri, token, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {v}");
    if v.get("jsonrpc").is_some() {
        v
    } else {
        v["data"].clone()
    }
}

struct World {
    owner: String,
    other: String,
    admin: String,
    slack: String,
    worker: String,
    worker_id: Uuid,
}

async fn world(pool: &PgPool) -> World {
    let owner = person(pool, "anmol@airtribe.live", "member").await;
    let other = person(pool, "dhaval@airtribe.live", "member").await;
    let admin = person(pool, "root@airtribe.live", "admin").await;
    let slack = ok(pool, "POST", "/api/user/agents", &owner,
        json!({ "handle": "slacker", "name": "Slack Agent", "runtime": "hermes", "canIntake": true })).await;
    let worker = ok(
        pool,
        "POST",
        "/api/user/agents",
        &owner,
        json!({ "handle": "claude", "name": "Airtribe", "runtime": "claude-code" }),
    )
    .await;
    World {
        owner,
        other,
        admin,
        slack: slack["token"].as_str().unwrap().to_owned(),
        worker: worker["token"].as_str().unwrap().to_owned(),
        worker_id: worker["agent"]["id"].as_str().unwrap().parse().unwrap(),
    }
}

fn thread() -> Value {
    json!([
        { "author": "Rahul", "text": "Anyone else seeing *checkout* spin?", "ts": "1727258400.000100",
          "receivedAt": "2026-09-25T09:50:00Z" },
        { "author": "Priya", "text": "Yes, only with saved cards. cc @Anmol", "ts": "1727258700.000200",
          "receivedAt": "2026-09-25T09:55:00Z" },
    ])
}

fn filing(key: &str, channel: &str, thread: Value) -> Value {
    json!({
        "source": {
            "kind": "slack", "key": key, "url": format!("https://slack.test/{key}"), "channel": channel,
            "channelName": "#issues-and-feedback", "author": "Priya", "text": "It fails at confirm, 422",
            "receivedAt": "2026-09-25T10:00:00Z", "thread": thread,
        },
        "title": "Checkout fails for saved cards", "category": "bug",
        "reason": "looks like a bug: saved-card checkout fails", "confidence": 0.9,
    })
}

fn id(t: &Value) -> &str {
    t["id"].as_str().unwrap()
}

#[sqlx::test]
async fn the_thread_is_stored_and_returned_oldest_first(pool: PgPool) {
    let w = world(&pool).await;
    let t = ok(
        &pool,
        "POST",
        "/api/agent/intake",
        &w.slack,
        filing("m1", "C1", thread()),
    )
    .await;
    assert_eq!(t["source"]["thread"], thread());
    let seen = ok(
        &pool,
        "GET",
        &format!("/api/user/tasks/{}", id(&t)),
        &w.other,
        Value::Null,
    )
    .await;
    assert_eq!(
        seen["source"]["thread"],
        thread(),
        "a channel's thread is team-visible"
    );

    // No thread: an empty one, not a missing field.
    let mut bare = filing("m2", "C1", Value::Null);
    bare["source"].as_object_mut().unwrap().remove("thread");
    let t = ok(&pool, "POST", "/api/agent/intake", &w.slack, bare).await;
    assert_eq!(t["source"]["thread"], json!([]));

    // Appending carries the appended message's thread too.
    let (status, v) = call(
        &pool,
        "POST",
        &format!("/api/agent/intake/{}/append", id(&t)),
        &w.slack,
        json!({ "source": filing("m3", "C1", thread())["source"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let stored: Value =
        sqlx::query_scalar("SELECT thread FROM task_source WHERE source_key = 'm3'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, thread());
}

#[sqlx::test]
async fn the_thread_is_bounded(pool: PgPool) {
    let w = world(&pool).await;
    let one = thread()[0].clone();
    let (status, v) = call(
        &pool,
        "POST",
        "/api/agent/intake",
        &w.slack,
        filing("m1", "C1", Value::Array(vec![one.clone(); 31])),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("at most the 30"),
        "{v}"
    );

    let mut long = one.clone();
    long["text"] = json!("x".repeat(4_001));
    let (status, v) = call(
        &pool,
        "POST",
        "/api/agent/intake",
        &w.slack,
        filing("m1", "C1", json!([one.clone(), long])),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("source.thread[1] (from Rahul) is 4001 characters"),
        "{v}"
    );
    let filed: i64 = sqlx::query_scalar("SELECT count(*) FROM task")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(filed, 0, "nothing was filed");

    let mut edge = one.clone();
    edge["text"] = json!("é".repeat(4_000));
    ok(
        &pool,
        "POST",
        "/api/agent/intake",
        &w.slack,
        filing("m1", "C1", Value::Array(vec![edge; 30])),
    )
    .await;
}

#[sqlx::test]
async fn a_direct_messages_thread_is_the_owners(pool: PgPool) {
    let w = world(&pool).await;
    let t = ok(
        &pool,
        "POST",
        "/api/agent/intake",
        &w.slack,
        filing("m1", "D42", thread()),
    )
    .await;
    let uri = format!("/api/user/tasks/{}", id(&t));
    let teammate = ok(&pool, "GET", &uri, &w.other, Value::Null).await;
    assert_eq!(teammate["source"]["thread"], json!([]));
    assert_eq!(teammate["source"]["text"], "From a direct message");
    for who in [&w.owner, &w.admin] {
        assert_eq!(
            ok(&pool, "GET", &uri, who, Value::Null).await["source"]["thread"],
            thread()
        );
    }
    let all = ok(&pool, "GET", "/api/user/tasks", &w.other, Value::Null).await;
    assert!(
        all.as_array()
            .unwrap()
            .iter()
            .all(|t| t["source"]["thread"] == json!([])),
        "lists mask it the same way"
    );
}

#[sqlx::test]
async fn the_working_agent_reads_the_source_and_its_thread(pool: PgPool) {
    let w = world(&pool).await;
    // A DM: the worker acts for the owner, so it reads what the owner can.
    let t = ok(
        &pool,
        "POST",
        "/api/agent/intake",
        &w.slack,
        filing("m1", "D42", thread()),
    )
    .await;
    let task = id(&t).to_owned();
    ok(
        &pool,
        "POST",
        &format!("/api/user/tasks/{task}/accept"),
        &w.owner,
        Value::Null,
    )
    .await;
    ok(
        &pool,
        "POST",
        &format!("/api/user/tasks/{task}/handoff"),
        &w.owner,
        json!({ "agentId": w.worker_id }),
    )
    .await;

    let ctx = ok(
        &pool,
        "GET",
        &format!("/api/agent/tasks/{task}"),
        &w.worker,
        Value::Null,
    )
    .await;
    let src = &ctx["source"];
    assert_eq!(src["kind"], "slack");
    assert_eq!(src["url"], "https://slack.test/m1");
    assert_eq!(src["channelName"], "#issues-and-feedback");
    assert_eq!(src["author"], "Priya");
    assert_eq!(src["text"], "It fails at confirm, 422");
    assert_eq!(src["thread"], thread());
    assert_eq!(src["files"], json!([]));
    assert!(src["receivedAt"].is_string());

    // The same through MCP.
    let r = ok(
        &pool,
        "POST",
        "/api/services/mcp",
        &w.worker,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "task_context", "arguments": { "taskId": task } } }),
    )
    .await;
    let text: Value =
        serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["source"]["thread"], thread(), "{r}");
}

#[sqlx::test]
async fn a_task_with_no_source_has_a_null_one(pool: PgPool) {
    let w = world(&pool).await;
    let me: Uuid = sqlx::query_scalar("SELECT id FROM person WHERE email = 'anmol@airtribe.live'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let t = ok(
        &pool,
        "POST",
        "/api/user/tasks",
        &w.owner,
        json!({ "title": "Tidy the README", "assigneeId": me }),
    )
    .await;
    ok(
        &pool,
        "POST",
        &format!("/api/user/tasks/{}/handoff", id(&t)),
        &w.owner,
        json!({ "agentId": w.worker_id }),
    )
    .await;
    let ctx = ok(
        &pool,
        "GET",
        &format!("/api/agent/tasks/{}", id(&t)),
        &w.worker,
        Value::Null,
    )
    .await;
    assert!(ctx["source"].is_null(), "{ctx}");
}

#[sqlx::test]
async fn mcp_intake_create_takes_a_thread(pool: PgPool) {
    let w = world(&pool).await;
    let r = ok(
        &pool,
        "POST",
        "/api/services/mcp",
        &w.slack,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
    )
    .await;
    let create = r["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "intake_create")
        .unwrap()
        .clone();
    assert_eq!(
        create["inputSchema"]["properties"]["source"]["properties"]["thread"]["maxItems"],
        30
    );

    let r = ok(
        &pool,
        "POST",
        "/api/services/mcp",
        &w.slack,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "intake_create", "arguments": filing("m1", "C1", thread()) } }),
    )
    .await;
    let t: Value =
        serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(t["source"]["thread"], thread(), "{r}");
}
