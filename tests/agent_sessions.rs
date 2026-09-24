//! Agent sessions: the now line, the step log, private instructions, setup
//! reported at hello, and who sees what. The private side of a session is
//! filtered by the server, so each edge is checked as another member sees it.

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

struct Session {
    owner: String,
    other: String,
    admin: String,
    agent: String,
    agent_id: Uuid,
    task: Uuid,
}

/// Anmol's agent holding an acknowledged task of his; Dhaval is another
/// member, Root an admin.
async fn session(pool: &PgPool) -> Session {
    let (owner, me) = person(pool, "anmol@airtribe.live", "member").await;
    let (other, _) = person(pool, "dhaval@airtribe.live", "member").await;
    let (admin, _) = person(pool, "root@airtribe.live", "admin").await;
    let minted = ok(pool, "POST", "/api/user/agents", &owner, json!({ "handle": "hermes", "runtime": "hermes" })).await;
    let agent_id: Uuid = minted["agent"]["id"].as_str().unwrap().parse().unwrap();
    let agent = minted["token"].as_str().unwrap().to_owned();
    let task: Uuid = sqlx::query_scalar(
        "WITH pr AS (INSERT INTO project (key, name) VALUES ('PAY', 'Payments') RETURNING id),
              ph AS (INSERT INTO phase (project_id, name, position) SELECT id, 'Build', 0 FROM pr RETURNING id)
         INSERT INTO task (phase_id, title, status, assignee_kind, assignee_person_id)
         SELECT id, 'Wire checkout', 'in_progress', 'human', $1 FROM ph RETURNING id",
    )
    .bind(me)
    .fetch_one(pool)
    .await
    .unwrap();
    ok(pool, "POST", &format!("/api/user/tasks/{task}/handoff"), &owner, json!({ "agentId": agent_id })).await;
    ok(pool, "POST", &format!("/api/agent/tasks/{task}/ack"), &agent, Value::Null).await;
    Session { owner, other, admin, agent, agent_id, task }
}

#[sqlx::test]
async fn the_now_line_is_set_and_cleared_when_work_stops(pool: PgPool) {
    let s = session(&pool).await;
    let t = s.task;

    let (status, v) = send(&pool, "POST", &format!("/api/agent/tasks/{t}/now"), &s.agent, json!({ "text": "  " })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(v["error"]["message"].as_str().unwrap().contains("1 to 120"), "{v}");
    let long = "x".repeat(121);
    let (status, _) = send(&pool, "POST", &format!("/api/agent/tasks/{t}/now"), &s.agent, json!({ "text": long })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let row = ok(&pool, "POST", &format!("/api/agent/tasks/{t}/now"), &s.agent, json!({ "text": " running the tests " })).await;
    assert_eq!(row["delegate"]["now"], "running the tests");
    assert_eq!(row["delegate"]["state"], "working", "a now line means working");
    assert!(row["delegate"]["nowAt"].is_string());

    // Anyone on the team sees it on the task.
    let seen = ok(&pool, "GET", &format!("/api/user/tasks/{t}"), &s.other, Value::Null).await;
    assert_eq!(seen["delegate"]["now"], "running the tests");
    assert_eq!(seen["delegate"]["runtime"], "hermes");
    assert_eq!(seen["delegate"]["ownerName"], "Anmol");

    let row = ok(&pool, "POST", &format!("/api/agent/tasks/{t}/update"), &s.agent,
        json!({ "body": "Half done", "now": "writing the webhook" })).await;
    assert_eq!(row["delegate"]["now"], "writing the webhook");

    // Asking leaves `working`, so the line goes, and cannot be set while waiting.
    let row = ok(&pool, "POST", &format!("/api/agent/tasks/{t}/ask"), &s.agent, json!({ "body": "Which currency?" })).await;
    assert!(row["delegate"]["now"].is_null(), "{row}");
    assert!(row["delegate"]["nowAt"].is_null());
    let (status, v) = send(&pool, "POST", &format!("/api/agent/tasks/{t}/now"), &s.agent, json!({ "text": "waiting" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(v["error"]["message"].as_str().unwrap().contains("answer"), "{v}");

    // Taking it back clears it too, whatever path moved the state.
    ok(&pool, "POST", &format!("/api/user/tasks/{t}/answer"), &s.owner, json!({ "body": "INR" })).await;
    ok(&pool, "POST", &format!("/api/agent/tasks/{t}/now"), &s.agent, json!({ "text": "back on it" })).await;
    ok(&pool, "POST", &format!("/api/user/tasks/{t}/takeback"), &s.owner, Value::Null).await;
    let now: Option<String> = sqlx::query_scalar("SELECT agent_now FROM task WHERE id = $1")
        .bind(t)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(now, None);
}

#[sqlx::test]
async fn the_step_log_is_the_owners_to_read(pool: PgPool) {
    let s = session(&pool).await;
    let t = s.task;
    let log = format!("/api/agent/tasks/{t}/log");

    let (status, _) = send(&pool, "POST", &log, &s.agent, json!({ "lines": [] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(&pool, "POST", &log, &s.agent, json!({ "lines": vec!["x"; 201] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, v) = send(&pool, "POST", &log, &s.agent, json!({ "lines": ["ok", "y".repeat(2001)] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(v["error"]["message"].as_str().unwrap().contains("line 2"), "{v}");

    assert_eq!(ok(&pool, "POST", &log, &s.agent, json!({ "lines": ["$ cargo test", "ok"] })).await["lastSeq"], 2);
    assert_eq!(ok(&pool, "POST", &log, &s.agent, json!({ "lines": ["done"] })).await["lastSeq"], 3);

    let read = format!("/api/user/tasks/{t}/logs");
    let lines = ok(&pool, "GET", &read, &s.owner, Value::Null).await;
    let texts: Vec<&str> = lines.as_array().unwrap().iter().map(|l| l["text"].as_str().unwrap()).collect();
    assert_eq!(texts, ["$ cargo test", "ok", "done"]);
    assert_eq!(ok(&pool, "GET", &format!("{read}?afterSeq=2"), &s.owner, Value::Null).await.as_array().unwrap().len(), 1);
    assert_eq!(ok(&pool, "GET", &read, &s.admin, Value::Null).await.as_array().unwrap().len(), 3);

    let (status, v) = send(&pool, "GET", &read, &s.other, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(v["error"]["message"].as_str().unwrap().contains("step log"), "{v}");

    // Once taken back, the agent can no longer write to it.
    ok(&pool, "POST", &format!("/api/user/tasks/{t}/takeback"), &s.owner, Value::Null).await;
    let (status, _) = send(&pool, "POST", &log, &s.agent, json!({ "lines": ["late"] })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn instructions_are_private_and_reach_the_agent(pool: PgPool) {
    let s = session(&pool).await;
    let t = s.task;
    let instruct = format!("/api/user/tasks/{t}/instruct");

    let (status, v) = send(&pool, "POST", &instruct, &s.other, json!({ "body": "Ship it" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    let (status, _) = send(&pool, "POST", &instruct, &s.agent, json!({ "body": "Ship it" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an agent cannot instruct itself");

    let note = ok(&pool, "POST", &instruct, &s.owner, json!({ "body": "Use the sandbox keys" })).await;
    assert_eq!(note["kind"], "instruction");

    let (_, events) = send(&pool, "GET", "/api/agent/events", &s.agent, Value::Null).await;
    let e = events["data"].as_array().unwrap().iter().find(|e| e["kind"] == "instruction").expect("an instruction event");
    assert_eq!(e["payload"]["body"], "Use the sandbox keys");
    assert_eq!(e["payload"]["author"], "Anmol");
    let request = Request::builder()
        .uri("/api/agent/inbox")
        .header("authorization", format!("Bearer {}", s.agent))
        .body(Body::empty())
        .unwrap();
    let inbox = acp_server::app::app(state(&pool)).oneshot(request).await.unwrap();
    let inbox = String::from_utf8(inbox.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(inbox.contains("instruction on \"Wire checkout\"") && inbox.contains("Anmol: Use the sandbox keys"), "{inbox}");

    let context = ok(&pool, "GET", &format!("/api/agent/tasks/{t}"), &s.agent, Value::Null).await;
    assert!(context["notes"].as_array().unwrap().iter().any(|n| n["kind"] == "instruction"), "{context}");

    ok(&pool, "POST", &format!("/api/user/tasks/{t}/takeback"), &s.owner, Value::Null).await;
    let (status, v) = send(&pool, "POST", &instruct, &s.owner, json!({ "body": "Too late" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
}

#[sqlx::test]
async fn private_notes_are_filtered_by_viewer(pool: PgPool) {
    let s = session(&pool).await;
    let t = s.task;
    ok(&pool, "POST", &format!("/api/agent/tasks/{t}/update"), &s.agent, json!({ "body": "Started" })).await;
    ok(&pool, "POST", &format!("/api/agent/tasks/{t}/ask"), &s.agent, json!({ "body": "Which currency?" })).await;
    ok(&pool, "POST", &format!("/api/user/tasks/{t}/answer"), &s.owner, json!({ "body": "INR" })).await;
    ok(&pool, "POST", &format!("/api/user/tasks/{t}/instruct"), &s.owner, json!({ "body": "Sandbox keys" })).await;

    let kinds = |notes: Value| -> Vec<String> {
        notes.as_array().unwrap().iter().map(|n| n["kind"].as_str().unwrap().to_owned()).collect()
    };
    let notes = format!("/api/user/tasks/{t}/notes");
    assert_eq!(kinds(ok(&pool, "GET", &notes, &s.other, Value::Null).await), ["progress"]);
    let everything = ["progress", "question", "answer", "instruction"];
    assert_eq!(kinds(ok(&pool, "GET", &notes, &s.owner, Value::Null).await), everything);
    assert_eq!(kinds(ok(&pool, "GET", &notes, &s.admin, Value::Null).await), everything);

    let task = format!("/api/user/tasks/{t}");
    assert_eq!(ok(&pool, "GET", &task, &s.owner, Value::Null).await["canSeeAgentPrivate"], true);
    assert_eq!(ok(&pool, "GET", &task, &s.admin, Value::Null).await["canSeeAgentPrivate"], true);
    assert_eq!(ok(&pool, "GET", &task, &s.other, Value::Null).await["canSeeAgentPrivate"], false);
}

#[sqlx::test]
async fn hello_reports_setup_and_the_owner_sees_the_session(pool: PgPool) {
    let s = session(&pool).await;
    ok(&pool, "POST", "/api/agent/hello", &s.agent,
        json!({ "runtime": "hermes", "setup": { "skill": "3", "mcp": true, "watcher": false } })).await;
    // A later hello without setup keeps what was reported.
    ok(&pool, "POST", "/api/agent/hello", &s.agent, json!({ "runtime": "hermes" })).await;
    ok(&pool, "POST", &format!("/api/agent/tasks/{}/now", s.task), &s.agent, json!({ "text": "reading the code" })).await;

    let agents = ok(&pool, "GET", "/api/user/agents", &s.owner, Value::Null).await;
    let a = &agents[0];
    assert_eq!(a["setup"], json!({ "skill": "3", "mcp": true, "watcher": false }));
    assert_eq!(a["currentTask"]["id"], s.task.to_string());
    assert_eq!(a["currentTask"]["title"], "Wire checkout");
    assert_eq!(a["currentTask"]["state"], "working");
    assert_eq!(a["currentTask"]["now"], "reading the code");
    let activity = a["activity"].as_array().unwrap();
    assert_eq!(activity.len(), 14);
    assert_eq!(activity[13], 0, "a now line is not a note");
    ok(&pool, "POST", &format!("/api/agent/tasks/{}/note", s.task), &s.agent, json!({ "body": "Found it" })).await;
    let agents = ok(&pool, "GET", "/api/user/agents", &s.owner, Value::Null).await;
    assert_eq!(agents[0]["activity"][13], 1, "today is last");

    assert_eq!(ok(&pool, "GET", "/api/user/agents", &s.other, Value::Null).await, json!([]), "agents stay the owner's");
}

#[sqlx::test]
async fn delegation_time_is_recorded(pool: PgPool) {
    let s = session(&pool).await;
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{}", s.task), &s.other, Value::Null).await;
    assert!(row["delegate"]["delegatedAt"].is_string(), "{row}");
}

#[sqlx::test]
async fn agents_at_work_are_visible_to_the_team(pool: PgPool) {
    let s = session(&pool).await;
    ok(&pool, "POST", &format!("/api/agent/tasks/{}/now", s.task), &s.agent, json!({ "text": "running the tests" })).await;

    let active = ok(&pool, "GET", "/api/user/agents/active", &s.other, Value::Null).await;
    let rows = active.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{active}");
    let r = &rows[0];
    assert_eq!(r["agent"], json!({ "id": s.agent_id, "name": "hermes", "handle": "hermes", "runtime": "hermes" }));
    assert_eq!(r["owner"]["name"], "Anmol");
    assert_eq!(r["task"]["title"], "Wire checkout");
    assert_eq!(r["task"]["projectName"], "Payments");
    assert_eq!(r["state"], "working");
    assert_eq!(r["now"], "running the tests");
    assert!(r["nowAt"].is_string() && r["delegatedAt"].is_string() && r["lastSeenAt"].is_string());

    let (status, _) = send(&pool, "GET", "/api/user/agents/active", &s.agent, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a signed-in person only");

    // Archived work is not at work.
    sqlx::query("UPDATE task SET archived_at = now() WHERE id = $1").bind(s.task).execute(&pool).await.unwrap();
    assert_eq!(ok(&pool, "GET", "/api/user/agents/active", &s.other, Value::Null).await, json!([]));
    sqlx::query("UPDATE task SET archived_at = NULL WHERE id = $1").bind(s.task).execute(&pool).await.unwrap();

    // Nor is finished work.
    ok(&pool, "POST", &format!("/api/agent/tasks/{}/attach", s.task), &s.agent,
        json!({ "kind": "pr", "url": "https://github.com/airtribe/api/pull/1" })).await;
    ok(&pool, "POST", &format!("/api/agent/tasks/{}/submit", s.task), &s.agent,
        json!({ "target": "completed", "summary": "Done, tests pass" })).await;
    assert_eq!(ok(&pool, "GET", "/api/user/agents/active", &s.other, Value::Null).await[0]["state"], "in_review");
    ok(&pool, "POST", &format!("/api/user/tasks/{}/review", s.task), &s.owner, json!({ "decision": "approve" })).await;
    assert_eq!(ok(&pool, "GET", "/api/user/agents/active", &s.other, Value::Null).await, json!([]));
}

#[sqlx::test]
async fn the_mcp_tools_set_now_and_append_to_the_log(pool: PgPool) {
    let s = session(&pool).await;
    let call = |name: &str, arguments: Value| {
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": arguments } })
    };
    let (_, v) = send(&pool, "POST", "/api/services/mcp", &s.agent,
        call("task_now", json!({ "taskId": s.task, "text": "profiling" }))).await;
    assert!(v["result"]["isError"].is_null(), "{v}");
    let (_, v) = send(&pool, "POST", "/api/services/mcp", &s.agent,
        call("task_log", json!({ "taskId": s.task, "lines": ["a", "b"] }))).await;
    assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("\"lastSeq\":2"), "{v}");
    let (_, v) = send(&pool, "POST", "/api/services/mcp", &s.agent,
        call("task_log", json!({ "taskId": s.task }))).await;
    assert_eq!(v["result"]["isError"], true, "{v}");

    let row = ok(&pool, "GET", &format!("/api/user/tasks/{}", s.task), &s.owner, Value::Null).await;
    assert_eq!(row["delegate"]["now"], "profiling");
}
