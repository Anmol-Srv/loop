//! The agent page's data: runs an agent reports (validation, retention, who
//! reads them) and the overview's numbers on a seeded scenario, owner-only.

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

async fn person(pool: &PgPool, email: &str, role: &str) -> (String, Uuid) {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO person (email, name, department, role)
         VALUES ($1, initcap(split_part($1, '@', 1)), 'backend', $2) RETURNING id",
    )
    .bind(email)
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    let (raw, _) = token::mint_session(&state(pool), email).await.unwrap();
    (raw, id)
}

async fn send(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("host", "acp.test")
        .header("authorization", format!("Bearer {token}"))
        .body(if body.is_null() { Body::empty() } else { Body::from(body.to_string()) })
        .unwrap();
    let response = acp_server::app::app(state(pool)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn ok(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> Value {
    let (status, v) = send(pool, method, uri, token, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {v}");
    v["data"].clone()
}

struct World {
    owner: String,
    owner_id: Uuid,
    other: String,
    admin: String,
    agent: String,
    agent_id: Uuid,
}

async fn world(pool: &PgPool, can_intake: bool) -> World {
    let (owner, owner_id) = person(pool, "anmol@airtribe.live", "member").await;
    let (other, _) = person(pool, "dhaval@airtribe.live", "member").await;
    let (admin, _) = person(pool, "root@airtribe.live", "admin").await;
    let minted = ok(pool, "POST", "/api/user/agents", &owner,
        json!({ "handle": "hermes", "name": "Hermes", "runtime": "hermes", "canIntake": can_intake })).await;
    World {
        owner,
        owner_id,
        other,
        admin,
        agent: minted["token"].as_str().unwrap().to_owned(),
        agent_id: minted["agent"]["id"].as_str().unwrap().parse().unwrap(),
    }
}

async fn task(pool: &PgPool, owner: Uuid, title: &str) -> Uuid {
    sqlx::query_scalar(
        "WITH pr AS (INSERT INTO project (key, name) VALUES (upper(left(md5($2), 4)), 'Payments') RETURNING id),
              ph AS (INSERT INTO phase (project_id, name, position) SELECT id, 'Build', 0 FROM pr RETURNING id)
         INSERT INTO task (phase_id, title, status, assignee_kind, assignee_person_id)
         SELECT id, $2, 'in_progress', 'human', $1 FROM ph RETURNING id",
    )
    .bind(owner)
    .bind(title)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn runs_are_validated_kept_to_500_and_read_by_the_owner(pool: PgPool) {
    let w = world(&pool, true).await;

    let (status, v) = send(&pool, "POST", "/api/agent/runs", &w.agent, json!({ "status": "great" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(v["error"]["message"].as_str().unwrap().contains("ok (the pass finished)"), "{v}");
    let (status, _) = send(&pool, "POST", "/api/agent/runs", &w.agent, json!({ "status": "failed" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a failed run says what failed");
    let (status, _) = send(&pool, "POST", "/api/agent/runs", &w.agent,
        json!({ "status": "ok", "counts": { "filed": -1 } })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(&pool, "POST", "/api/agent/runs", &w.owner, json!({ "status": "ok" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a person does not report runs");

    let run = ok(&pool, "POST", "/api/agent/runs", &w.agent, json!({
        "startedAt": (chrono::Utc::now() - chrono::Duration::minutes(2)).to_rfc3339(), "status": "partial", "summary": "Read #issues-and-feedback",
        "counts": { "filed": 2, "appended": 1, "alreadyFiled": 3, "skipped": 5 }, "error": "  one thread 404'd  ",
    })).await;
    assert_eq!(run["status"], "partial");
    assert_eq!(run["counts"], json!({ "filed": 2, "appended": 1, "alreadyFiled": 3, "skipped": 5 }));
    assert_eq!(run["error"], "one thread 404'd");
    assert!(run["startedAt"].as_str().unwrap() < run["finishedAt"].as_str().unwrap());

    // The same over MCP.
    let r = send(&pool, "POST", "/api/services/mcp", &w.agent, json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "run_report", "arguments": { "status": "ok", "summary": "quiet hour" } }
    })).await.1;
    assert_ne!(r["result"]["isError"], true, "{r}");

    // Retention: the newest 500 per agent.
    sqlx::query("INSERT INTO agent_run (agent_id, status, summary) SELECT $1, 'ok', 'old ' || i FROM generate_series(1, 500) i")
        .bind(w.agent_id)
        .execute(&pool)
        .await
        .unwrap();
    ok(&pool, "POST", "/api/agent/runs", &w.agent, json!({ "status": "ok", "summary": "newest" })).await;
    let (kept, partial_kept): (i64, bool) = sqlx::query_as(
        "SELECT count(*), bool_or(status = 'partial') FROM agent_run WHERE agent_id = $1",
    )
    .bind(w.agent_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kept, 500);
    assert!(!partial_kept, "the oldest went first");

    // Owner and admin read them; nobody else.
    let o = ok(&pool, "GET", &format!("/api/user/agents/{}/overview", w.agent_id), &w.owner, Value::Null).await;
    let runs = o["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 50);
    assert_eq!(runs[0]["summary"], "newest");
    ok(&pool, "GET", &format!("/api/user/agents/{}/overview", w.agent_id), &w.admin, Value::Null).await;
    let (status, _) = send(&pool, "GET", &format!("/api/user/agents/{}/overview", w.agent_id), &w.other, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(&pool, "GET", &format!("/api/user/agents/{}/overview", w.agent_id), &w.agent, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an agent does not read its own page");
    let (status, _) = send(&pool, "GET", &format!("/api/user/agents/{}/overview", Uuid::new_v4()), &w.owner, Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn overview_counts_a_seeded_scenario(pool: PgPool) {
    let w = world(&pool, true).await;
    let a = &w.agent;
    let done = task(&pool, w.owner_id, "Wire checkout").await;
    let review = task(&pool, w.owner_id, "Refund webhook").await;
    let back = task(&pool, w.owner_id, "Retry payouts").await;

    // One finished and approved, one in review, one taken back.
    for t in [done, review, back] {
        ok(&pool, "POST", &format!("/api/user/tasks/{t}/handoff"), &w.owner, json!({ "agentId": w.agent_id })).await;
        ok(&pool, "POST", &format!("/api/agent/tasks/{t}/ack"), a, Value::Null).await;
    }
    ok(&pool, "POST", &format!("/api/agent/tasks/{done}/update"), a, json!({ "body": "Picked it up" })).await;
    ok(&pool, "POST", &format!("/api/agent/tasks/{done}/ask"), a, json!({ "body": "Which currency?" })).await;
    ok(&pool, "POST", &format!("/api/user/tasks/{done}/answer"), &w.owner, json!({ "body": "INR" })).await;
    ok(&pool, "POST", &format!("/api/agent/tasks/{done}/log"), a, json!({ "lines": ["$ cargo test", "ok"] })).await;
    for t in [done, review] {
        ok(&pool, "POST", &format!("/api/agent/tasks/{t}/attach"), a, json!({ "kind": "pr", "url": "https://github.com/x/y/pull/1" })).await;
        ok(&pool, "POST", &format!("/api/agent/tasks/{t}/submit"), a, json!({ "target": "completed", "summary": "Done, tests pass" })).await;
    }
    ok(&pool, "POST", &format!("/api/user/tasks/{done}/review"), &w.owner, json!({ "decision": "approve" })).await;
    ok(&pool, "POST", &format!("/api/user/tasks/{back}/takeback"), &w.owner, Value::Null).await;

    // Three filed: accepted, dismissed, still in triage.
    let mut filed = Vec::new();
    for key in ["m1", "m2", "m3"] {
        let row = ok(&pool, "POST", "/api/agent/intake", a, json!({
            "source": { "kind": "slack", "key": key, "url": format!("https://slack.test/{key}"), "channel": "C1",
                        "channelName": "#issues-and-feedback", "author": "Priya", "text": "Checkout fails",
                        "receivedAt": "2026-09-25T10:00:00Z" },
            "title": format!("Filed {key}"), "category": "bug", "reason": "a bug", "confidence": 0.9,
        })).await;
        filed.push(row["id"].as_str().unwrap().parse::<Uuid>().unwrap());
    }
    sqlx::query("UPDATE task SET status = 'open' WHERE id = $1").bind(filed[0]).execute(&pool).await.unwrap();
    sqlx::query("UPDATE task SET status = 'dropped' WHERE id = $1").bind(filed[1]).execute(&pool).await.unwrap();

    let o = ok(&pool, "GET", &format!("/api/user/agents/{}/overview", w.agent_id), &w.owner, Value::Null).await;
    assert_eq!(o["agent"]["id"], w.agent_id.to_string());
    assert_eq!(o["agent"]["handle"], "hermes", "the agent in the list's shape");
    let s = &o["stats"];
    assert_eq!(s["tasksHandled"], 3, "{s}");
    assert_eq!(s["tasksDone"], 1);
    assert_eq!(s["inReviewNow"], 1);
    assert_eq!(s["questionsAsked"], 1);
    assert!(s["medianAckMinutes"].as_f64().is_some_and(|m| (0.0..1.0).contains(&m)), "{s}");
    assert_eq!(s["filed"], 3);
    assert_eq!(s["accepted"], 1);
    assert_eq!(s["dismissed"], 1);
    assert_eq!(s["acceptRate"], 0.5);

    let days = o["activity"].as_array().unwrap();
    assert_eq!(days.len(), 30);
    let today = &days[29];
    assert_eq!(today["filed"], 3);
    assert!(today["notes"].as_i64().unwrap() >= 4, "progress, question, two submissions: {today}");
    assert!(today["events"].as_i64().unwrap() >= 3, "three hand-offs at least: {today}");

    let kinds: Vec<&str> = o["recent"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap()).collect();
    for k in ["handed_off", "progress", "question", "answer", "submission", "approved", "taken_back", "filed"] {
        assert!(kinds.contains(&k), "{k} in {kinds:?}");
    }
    let first = &o["recent"][0];
    assert!(first["taskTitle"].is_string() && first["at"].is_string());

    let title = |v: &Value| v["title"].as_str().unwrap().to_owned();
    let active: Vec<String> = o["tasks"]["active"].as_array().unwrap().iter().map(title).collect();
    let recent: Vec<String> = o["tasks"]["recent"].as_array().unwrap().iter().map(title).collect();
    assert!(active.contains(&"Refund webhook".to_owned()) && active.contains(&"Filed m3".to_owned()), "{active:?}");
    for t in ["Wire checkout", "Retry payouts", "Filed m1", "Filed m2"] {
        assert!(recent.contains(&t.to_owned()), "{t} in {recent:?}; active {active:?}");
    }
    let review_row = o["tasks"]["active"].as_array().unwrap().iter().find(|r| r["title"] == "Refund webhook").unwrap();
    assert_eq!(review_row["agentState"], "in_review");
    assert_eq!(review_row["projectName"], "Payments");

    let logs = o["logs"].as_array().unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0]["text"], "$ cargo test");
    assert_eq!(logs[0]["taskTitle"], "Wire checkout");
    assert_eq!(logs[1]["seq"], 2);

    // A teammate sees none of it.
    let (status, _) = send(&pool, "GET", &format!("/api/user/agents/{}/overview", w.agent_id), &w.other, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn a_new_agent_has_an_empty_overview(pool: PgPool) {
    let w = world(&pool, false).await;
    let o = ok(&pool, "GET", &format!("/api/user/agents/{}/overview", w.agent_id), &w.owner, Value::Null).await;
    assert_eq!(o["stats"]["tasksHandled"], 0);
    assert!(o["stats"]["medianAckMinutes"].is_null());
    assert!(o["stats"]["acceptRate"].is_null());
    assert_eq!(o["activity"].as_array().unwrap().len(), 30);
    assert_eq!(o["recent"], json!([]));
    assert_eq!(o["tasks"], json!({ "active": [], "recent": [] }));
    assert_eq!(o["runs"], json!([]));
    assert_eq!(o["logs"], json!([]));
}
