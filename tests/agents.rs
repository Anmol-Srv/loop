//! Personal agents, end to end over HTTP: connecting one, handing it a task,
//! everything it can do on that task, what reaches it from the dashboard, and
//! the MCP handshake real clients perform. Each permission edge is here
//! because a gap in one lets an agent act on work that is not its own.

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

async fn person(pool: &PgPool, email: &str, department: &str) -> (String, Uuid) {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO person (email, name, department) VALUES ($1, initcap(split_part($1, '@', 1)), $2)
         RETURNING id",
    )
    .bind(email)
    .bind(department)
    .fetch_one(pool)
    .await
    .unwrap();
    let (raw, _) = token::mint_session(&state(pool), email).await.unwrap();
    (raw, id)
}

async fn task(pool: &PgPool, status: &str, assignee: Uuid) -> Uuid {
    let project: Uuid = sqlx::query_scalar(
        "INSERT INTO project (key, name, description) VALUES (gen_random_uuid()::text, 'Payments', 'Take money')
         RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let phase: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 0) RETURNING id",
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, status, assignee_kind, assignee_person_id)
         VALUES ($1, 'Wire checkout', $2, 'human', $3) RETURNING id",
    )
    .bind(phase)
    .bind(status)
    .bind(assignee)
    .fetch_one(pool)
    .await
    .unwrap()
}

struct Reply {
    status: StatusCode,
    content_type: String,
    text: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or_default()
    }
    fn data(&self) -> Value {
        self.json()["data"].clone()
    }
    fn message(&self) -> String {
        self.json()["error"]["message"].as_str().unwrap_or_default().to_owned()
    }
}

async fn send(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> Reply {
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
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply { status, content_type, text: String::from_utf8_lossy(&bytes).into_owned() }
}

/// An agent for this owner: (id, token).
async fn agent(pool: &PgPool, owner: &str, handle: &str) -> (Uuid, String) {
    let r = send(pool, "POST", "/api/user/agents", owner, json!({ "handle": handle, "runtime": "hermes" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let d = r.data();
    (d["agent"]["id"].as_str().unwrap().parse().unwrap(), d["token"].as_str().unwrap().to_owned())
}

async fn hand_off(pool: &PgPool, owner: &str, task: Uuid, agent: Uuid) -> Reply {
    send(pool, "POST", &format!("/api/user/tasks/{task}/handoff"), owner, json!({ "agentId": agent })).await
}

/// Events the agent has not acknowledged, as (kind, payload).
async fn events(pool: &PgPool, token: &str) -> Vec<(String, Value)> {
    let r = send(pool, "GET", "/api/agent/events", token, Value::Null).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    r.data()
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["kind"].as_str().unwrap().to_owned(), e["payload"].clone()))
        .collect()
}

async fn ack_all(pool: &PgPool, token: &str) {
    send(pool, "POST", "/api/agent/events/ack", token, json!({ "through": i64::MAX })).await;
}

/// A connected agent holding a handed-off, acknowledged task, events cleared.
async fn working(pool: &PgPool, department: &str, status: &str) -> (String, Uuid, String, Uuid) {
    let (owner, me) = person(pool, "anmol@airtribe.live", department).await;
    let (agent_id, agent_token) = agent(pool, &owner, "hermes").await;
    let t = task(pool, status, me).await;
    assert_eq!(hand_off(pool, &owner, t, agent_id).await.status, StatusCode::OK);
    send(pool, "POST", &format!("/api/agent/tasks/{t}/ack"), &agent_token, Value::Null).await;
    ack_all(pool, &agent_token).await;
    (owner, t, agent_token, agent_id)
}

// ---- Connecting -----------------------------------------------------------

#[sqlx::test]
async fn connecting_an_agent_waits_for_hello_and_serves_its_setup(pool: PgPool) {
    let (owner, _) = person(&pool, "anmol@airtribe.live", "backend").await;
    let r = send(&pool, "POST", "/api/user/agents", &owner,
        json!({ "handle": "Hermes", "name": "Hermes (Mac)", "runtime": "hermes" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let d = r.data();
    let token = d["token"].as_str().unwrap().to_owned();
    assert_eq!(d["agent"]["handle"], "hermes", "handles are lower-case");
    assert_eq!(d["agent"]["status"], "waiting");
    let prompt = d["prompt"].as_str().unwrap();
    assert!(prompt.contains(&token) && prompt.contains("Anmol's agent"), "{prompt}");
    assert!(prompt.contains("http://acp.test/api/agent/onboarding?runtime=hermes"), "{prompt}");

    let dup = send(&pool, "POST", "/api/user/agents", &owner, json!({ "handle": "hermes" })).await;
    assert_eq!(dup.status, StatusCode::CONFLICT);

    let hello = send(&pool, "POST", "/api/agent/hello", &token, json!({ "runtime": "hermes", "version": "1" })).await;
    assert_eq!(hello.status, StatusCode::OK, "{}", hello.text);
    assert_eq!(hello.data()["owner"]["name"], "Anmol");
    assert_eq!(hello.data()["server"], "http://acp.test");
    let listed = send(&pool, "GET", "/api/user/agents", &owner, Value::Null).await.data();
    assert_eq!(listed[0]["status"], "connected");
    assert!(listed[0]["lastSeenAt"].is_string());

    let skill = send(&pool, "GET", "/api/agent/skill", &token, Value::Null).await;
    assert!(skill.content_type.starts_with("text/markdown"));
    assert!(!skill.text.contains("{{"), "every placeholder rendered");
    assert!(skill.text.contains("Anmol"));

    for runtime in ["hermes", "claude-code", "codex", "other"] {
        let doc = send(&pool, "GET", &format!("/api/agent/onboarding?runtime={runtime}"), &token, Value::Null).await;
        assert_eq!(doc.status, StatusCode::OK);
        assert!(!doc.text.contains("{{"), "{runtime} left a placeholder");
        assert!(!doc.text.contains(&token), "the onboarding doc never echoes the token");
    }
}

#[sqlx::test]
async fn a_session_is_refused_on_agent_routes(pool: PgPool) {
    let (owner, _) = person(&pool, "anmol@airtribe.live", "backend").await;
    for (method, uri) in [("GET", "/api/agent/inbox"), ("GET", "/api/agent/tasks"), ("POST", "/api/agent/hello")] {
        let r = send(&pool, method, uri, &owner, json!({})).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{uri}");
        assert!(r.message().contains("agent"), "a sentence: {}", r.message());
    }
}

#[sqlx::test]
async fn an_agent_cannot_use_the_board_routes(pool: PgPool) {
    let (owner, _) = person(&pool, "anmol@airtribe.live", "backend").await;
    let (_, token) = agent(&pool, &owner, "hermes").await;
    assert_eq!(send(&pool, "GET", "/api/user/tasks", &token, Value::Null).await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&pool, "GET", "/api/user/agents", &token, Value::Null).await.status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn rotating_ends_the_old_token_and_keeps_the_tasks(pool: PgPool) {
    let (owner, t, old, agent_id) = working(&pool, "backend", "open").await;
    let r = send(&pool, "POST", &format!("/api/user/agents/{agent_id}/rotate"), &owner, Value::Null).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let new = r.data()["token"].as_str().unwrap().to_owned();

    assert_eq!(send(&pool, "GET", "/api/agent/me", &old, Value::Null).await.status, StatusCode::UNAUTHORIZED);
    let tasks = send(&pool, "GET", "/api/agent/tasks", &new, Value::Null).await.data();
    assert_eq!(tasks[0]["id"], t.to_string());
}

#[sqlx::test]
async fn revoking_an_agent_cuts_it_off_and_takes_its_tasks_back(pool: PgPool) {
    let (owner, t, token, agent_id) = working(&pool, "backend", "open").await;
    let r = send(&pool, "DELETE", &format!("/api/user/agents/{agent_id}"), &owner, Value::Null).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.data()["status"], "revoked");

    assert_eq!(send(&pool, "GET", "/api/agent/inbox", &token, Value::Null).await.status, StatusCode::UNAUTHORIZED);
    let row = send(&pool, "GET", &format!("/api/user/tasks/{t}"), &owner, Value::Null).await.data();
    assert!(row["delegate"].is_null());
    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM agent_event WHERE agent_id = $1 ORDER BY id")
        .bind(agent_id)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(kinds.last().map(String::as_str), Some("taken_back"));

    let again = hand_off(&pool, &owner, t, agent_id).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "a revoked agent takes no work");

    // Revoking frees the handle: the same tool reconnects under its old name.
    let (fresh, _) = agent(&pool, &owner, "hermes").await;
    assert_ne!(fresh, agent_id);
}

// ---- Hand-off --------------------------------------------------------------

#[sqlx::test]
async fn only_the_assignee_hands_off_and_only_to_their_own_agent(pool: PgPool) {
    let (owner, me) = person(&pool, "anmol@airtribe.live", "backend").await;
    let (other, _) = person(&pool, "dhaval@airtribe.live", "backend").await;
    let (mine, _) = agent(&pool, &owner, "hermes").await;
    let (theirs, _) = agent(&pool, &other, "claude").await;
    let t = task(&pool, "open", me).await;

    let r = hand_off(&pool, &other, t, theirs).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "not their task: {}", r.text);
    let r = hand_off(&pool, &owner, t, theirs).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "not my agent: {}", r.text);

    let r = hand_off(&pool, &owner, t, mine).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let delegate = &r.data()["delegate"];
    assert_eq!(delegate["handle"], "hermes");
    assert_eq!(delegate["state"], "handed_off");
    assert!(r.data()["reviewTarget"].is_null());

    let done = task(&pool, "shipped", me).await;
    sqlx::query("UPDATE task SET done_at = now() WHERE id = $1").bind(done).execute(&pool).await.unwrap();
    assert_eq!(hand_off(&pool, &owner, done, mine).await.status, StatusCode::CONFLICT, "finished work");
    let dropped = task(&pool, "dropped", me).await;
    assert_eq!(hand_off(&pool, &owner, dropped, mine).await.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn another_owners_agent_cannot_touch_the_task(pool: PgPool) {
    let (_, t, _, _) = working(&pool, "backend", "open").await;
    let (other, _) = person(&pool, "dhaval@airtribe.live", "backend").await;
    let (_, stranger) = agent(&pool, &other, "claude").await;

    for (method, path, body) in [
        ("GET", "", Value::Null),
        ("POST", "/ack", Value::Null),
        ("POST", "/update", json!({ "body": "x", "status": "in_progress" })),
        ("POST", "/note", json!({ "body": "x" })),
        ("POST", "/attach", json!({ "kind": "pr", "url": "https://github.com/x/y/pull/1" })),
    ] {
        let r = send(&pool, method, &format!("/api/agent/tasks/{t}{path}"), &stranger, body).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{path}: {}", r.text);
        assert!(r.message().contains("not handed off to you"), "{}", r.message());
    }
}

// ---- What the agent does ---------------------------------------------------

#[sqlx::test]
async fn ack_update_ask_note_attach_act_as_the_assignee(pool: PgPool) {
    let (owner, me) = person(&pool, "anmol@airtribe.live", "backend").await;
    let (agent_id, token) = agent(&pool, &owner, "hermes").await;
    let t = task(&pool, "open", me).await;
    hand_off(&pool, &owner, t, agent_id).await;

    let r = send(&pool, "POST", &format!("/api/agent/tasks/{t}/ack"), &token, Value::Null).await;
    assert_eq!(r.data()["delegate"]["state"], "acknowledged");

    let r = send(&pool, "POST", &format!("/api/agent/tasks/{t}/update"), &token,
        json!({ "body": "Plan: wire the endpoint", "status": "in_progress", "expectedStatus": "open" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.data()["status"], "in_progress");
    assert_eq!(r.data()["delegate"]["state"], "working");
    let actor: String = sqlx::query_scalar(
        "SELECT actor FROM change WHERE target_id = $1 AND patch ? 'status' AND state = 'applied'",
    )
    .bind(t)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(actor, "hermes (agent)", "the move is on the audit trail as the agent");

    // A stale expectation is a conflict, as it is for a person.
    let r = send(&pool, "POST", &format!("/api/agent/tasks/{t}/update"), &token,
        json!({ "body": "blocked", "status": "blocked", "expectedStatus": "open" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text);

    // Finishing moves are submissions.
    let r = send(&pool, "POST", &format!("/api/agent/tasks/{t}/update"), &token,
        json!({ "body": "done", "status": "completed" })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.message().contains("submit"), "{}", r.message());

    let r = send(&pool, "POST", &format!("/api/agent/tasks/{t}/ask"), &token,
        json!({ "body": "Stripe or Razorpay?" })).await;
    assert_eq!(r.data()["delegate"]["state"], "needs_input");
    let home = send(&pool, "GET", "/api/user/home", &owner, Value::Null).await.data();
    let item = &home["needsAttention"][0];
    assert_eq!((item["kind"].as_str(), item["body"].as_str()), (Some("question"), Some("Stripe or Razorpay?")));
    assert_eq!(item["agentName"], "hermes");

    send(&pool, "POST", &format!("/api/agent/tasks/{t}/note"), &token, json!({ "body": "FYI: flaky test" })).await;
    let r = send(&pool, "POST", &format!("/api/agent/tasks/{t}/attach"), &token,
        json!({ "kind": "pr", "url": "https://github.com/a/b/pull/7", "title": "Checkout" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);

    let notes = send(&pool, "GET", &format!("/api/user/tasks/{t}/notes"), &owner, Value::Null).await.data();
    let kinds: Vec<&str> = notes.as_array().unwrap().iter().map(|n| n["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["progress", "question", "note"]);
    assert_eq!(notes[0]["agent"]["name"], "hermes");
    assert!(notes[0]["authorId"].is_null());

    // None of that came back to the agent as an event.
    assert_eq!(events(&pool, &token).await.iter().filter(|(k, _)| k != "handed_off").count(), 0);

    // The owner answers: the agent is working again and hears the answer.
    let r = send(&pool, "POST", &format!("/api/user/tasks/{t}/answer"), &owner, json!({ "body": "Razorpay" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.data()["kind"], "answer");
    let evs = events(&pool, &token).await;
    let (kind, payload) = evs.last().unwrap();
    assert_eq!((kind.as_str(), payload["body"].as_str()), ("answer", Some("Razorpay")));
    let again = send(&pool, "POST", &format!("/api/user/tasks/{t}/answer"), &owner, json!({ "body": "x" })).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "no open question now");
}

#[sqlx::test]
async fn submit_checks_the_move_and_its_evidence_now(pool: PgPool) {
    let (owner, t, token, _) = working(&pool, "backend", "in_progress").await;
    let submit = |target: &'static str, reason: Option<&'static str>| {
        let (pool, token) = (pool.clone(), token.clone());
        async move {
            send(&pool, "POST", &format!("/api/agent/tasks/{t}/submit"), &token,
                json!({ "target": target, "summary": "Endpoint live, tests pass", "manualReason": reason })).await
        }
    };

    let r = submit("completed", None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.message().contains("PR or commit"), "{}", r.message());
    let r = submit("shipped", None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "shipped only from completed");
    let r = submit("handoff", None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "not a finishing move on this track");

    send(&pool, "POST", &format!("/api/agent/tasks/{t}/attach"), &token,
        json!({ "kind": "commit", "url": "3f9a2c1" })).await;
    let r = submit("completed", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.data()["reviewTarget"], "completed");
    assert_eq!(r.data()["delegate"]["state"], "in_review");
    assert_eq!(r.data()["status"], "in_progress", "nothing moves until the owner approves");
    let home = send(&pool, "GET", "/api/user/home", &owner, Value::Null).await.data();
    assert_eq!(home["needsAttention"][0]["kind"], "review");

    // Changes requested: back to work, and the agent hears why.
    let r = send(&pool, "POST", &format!("/api/user/tasks/{t}/review"), &owner, json!({ "decision": "changes" })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "say what to change");
    let r = send(&pool, "POST", &format!("/api/user/tasks/{t}/review"), &owner,
        json!({ "decision": "changes", "body": "Add the refund path" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.data()["delegate"]["state"], "working");
    assert!(r.data()["reviewTarget"].is_null());
    let (kind, payload) = events(&pool, &token).await.pop().unwrap();
    assert_eq!((kind.as_str(), payload["body"].as_str()), ("changes_requested", Some("Add the refund path")));

    // Approved: the move applies through the normal path.
    submit("completed", None).await;
    let r = send(&pool, "POST", &format!("/api/user/tasks/{t}/review"), &owner, json!({ "decision": "approve" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.data()["status"], "completed");
    assert_eq!(r.data()["delegate"]["state"], "done");
    assert!(events(&pool, &token).await.iter().any(|(k, _)| k == "approved"));

    // Shipping finishes an engineering task, and stamps it.
    submit("shipped", None).await;
    let r = send(&pool, "POST", &format!("/api/user/tasks/{t}/review"), &owner, json!({ "decision": "approve" })).await;
    assert_eq!(r.data()["status"], "shipped");
    assert!(r.data()["doneAt"].is_string());
}

#[sqlx::test]
async fn a_design_handoff_needs_figma_unless_there_is_a_reason(pool: PgPool) {
    let (owner, t, token, _) = working(&pool, "design", "in_progress").await;
    let submit = |reason: Option<&'static str>| {
        let (pool, token) = (pool.clone(), token.clone());
        async move {
            send(&pool, "POST", &format!("/api/agent/tasks/{t}/submit"), &token,
                json!({ "target": "handoff", "summary": "Frames ready", "manualReason": reason })).await
        }
    };
    let r = submit(None).await;
    assert!(r.message().contains("Figma"), "{}", r.message());
    assert_eq!(submit(Some("Specced in the doc")).await.status, StatusCode::OK);
    let r = send(&pool, "POST", &format!("/api/user/tasks/{t}/review"), &owner, json!({ "decision": "approve" })).await;
    assert_eq!(r.data()["status"], "handoff");
    assert_eq!(r.data()["manualReason"], "Specced in the doc", "the reason travels with the approval");
}

// ---- What reaches the agent ------------------------------------------------

#[sqlx::test]
async fn dashboard_changes_reach_the_agent_and_its_own_do_not(pool: PgPool) {
    let (owner, t, token, _) = working(&pool, "backend", "open").await;

    send(&pool, "PATCH", &format!("/api/user/tasks/{t}/details"), &owner,
        json!({ "priority": 0, "body": "Now with refunds" })).await;
    send(&pool, "POST", &format!("/api/user/tasks/{t}/notes"), &owner, json!({ "body": "See the spec" })).await;
    send(&pool, "POST", "/api/user/artifacts", &owner,
        json!({ "parentType": "task", "parentId": t, "kind": "figma", "url": "https://figma.com/f/1" })).await;
    // The agent's own write is not echoed back.
    send(&pool, "POST", &format!("/api/agent/tasks/{t}/update"), &token,
        json!({ "body": "Started", "status": "in_progress" })).await;

    let evs = events(&pool, &token).await;
    let kinds: Vec<&str> = evs.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds, ["changed", "note", "artifact"], "{evs:?}");
    assert_eq!(evs[0].1["fields"], json!(["body", "priority"]));
    assert_eq!(evs[0].1["values"]["priority"], 0);
    assert_eq!(evs[1].1["author"], "Anmol");
    assert_eq!(evs[2].1["kind"], "figma");

    // Dropping is a stop.
    ack_all(&pool, &token).await;
    send(&pool, "PATCH", &format!("/api/user/tasks/{t}"), &owner, json!({ "status": "dropped" })).await;
    let evs = events(&pool, &token).await;
    assert_eq!(evs.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["dropped"]);
    let state: String = sqlx::query_scalar("SELECT agent_state FROM task WHERE id = $1")
        .bind(t).fetch_one(&pool).await.unwrap();
    assert_eq!(state, "stopped");

    // A question sent after the drop, before the agent read its inbox, is
    // refused rather than reopening the task as waiting on its owner.
    let late = send(&pool, "POST", &format!("/api/agent/tasks/{t}/ask"), &token, json!({ "body": "Push access?" })).await;
    assert_eq!(late.status, StatusCode::FORBIDDEN, "{}", late.text);
    assert!(late.message().contains("dropped"), "{}", late.text);
    let state: String = sqlx::query_scalar("SELECT agent_state FROM task WHERE id = $1")
        .bind(t).fetch_one(&pool).await.unwrap();
    assert_eq!(state, "stopped");
}

#[sqlx::test]
async fn taking_back_ends_the_agents_access(pool: PgPool) {
    let (owner, t, token, _) = working(&pool, "backend", "open").await;
    let r = send(&pool, "POST", &format!("/api/user/tasks/{t}/takeback"), &owner, Value::Null).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.data()["delegate"].is_null());
    assert_eq!(events(&pool, &token).await.last().unwrap().0, "taken_back");
    let r = send(&pool, "GET", &format!("/api/agent/tasks/{t}"), &token, Value::Null).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let again = send(&pool, "POST", &format!("/api/user/tasks/{t}/takeback"), &owner, Value::Null).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn reassigning_the_task_takes_it_back_from_the_agent(pool: PgPool) {
    let (owner, t, token, _) = working(&pool, "backend", "open").await;
    let (_, dhaval) = person(&pool, "dhaval@airtribe.live", "backend").await;
    let r = send(&pool, "PATCH", &format!("/api/user/tasks/{t}/details"), &owner,
        json!({ "assigneeId": dhaval })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.data()["entity"]["delegate"].is_null(), "the agent worked for the old assignee");
    assert_eq!(events(&pool, &token).await.last().unwrap().0, "taken_back");
}

#[sqlx::test]
async fn task_context_carries_the_related_work(pool: PgPool) {
    let (owner, t, token, _) = working(&pool, "frontend", "open").await;
    let (_, backend) = person(&pool, "dhaval@airtribe.live", "backend").await;
    let api = task(&pool, "completed", backend).await;
    sqlx::query("INSERT INTO artifact (parent_type, parent_id, kind, url) VALUES ('task', $1, 'pr', 'https://github.com/a/b/pull/9')")
        .bind(api).execute(&pool).await.unwrap();
    send(&pool, "PATCH", &format!("/api/user/tasks/{t}/blockers"), &owner, json!({ "blockedBy": [api] })).await;

    let r = send(&pool, "GET", &format!("/api/agent/tasks/{t}"), &token, Value::Null).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let c = r.data();
    assert_eq!(c["track"], "eng");
    assert_eq!(c["allowedNext"], json!(["in_progress", "blocked", "dropped"]));
    assert_eq!(c["project"]["name"], "Payments");
    assert_eq!(c["owner"]["name"], "Anmol");
    let dep = &c["related"]["blockedBy"][0];
    assert_eq!(dep["department"], "backend");
    assert_eq!(dep["evidence"][0]["url"], "https://github.com/a/b/pull/9");
    assert_eq!(c["related"]["blocks"], json!([]));
}

#[sqlx::test]
async fn the_feed_long_polls_and_the_cursor_only_moves_forward(pool: PgPool) {
    let (owner, t, token, agent_id) = working(&pool, "backend", "open").await;

    let started = std::time::Instant::now();
    let r = send(&pool, "GET", "/api/agent/events?wait=1", &token, Value::Null).await;
    assert_eq!(r.data(), json!([]));
    assert!(started.elapsed() >= std::time::Duration::from_millis(900), "held for the wait");

    let waiting = {
        let (pool, token) = (pool.clone(), token.clone());
        tokio::spawn(async move { send(&pool, "GET", "/api/agent/events?wait=20", &token, Value::Null).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let started = std::time::Instant::now();
    send(&pool, "POST", &format!("/api/user/tasks/{t}/notes"), &owner, json!({ "body": "ping" })).await;
    let r = waiting.await.unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "woken by the notification");
    let evs = r.data();
    assert_eq!(evs[0]["kind"], "note");
    let id = evs[0]["id"].as_i64().unwrap();

    let r = send(&pool, "POST", "/api/agent/events/ack", &token, json!({ "through": id })).await;
    assert_eq!(r.data()["eventCursor"], id);
    let r = send(&pool, "POST", "/api/agent/events/ack", &token, json!({ "through": 0 })).await;
    assert_eq!(r.data()["eventCursor"], id, "monotonic");
    let r = send(&pool, "POST", "/api/agent/events/ack", &token, json!({ "through": id + 1000 })).await;
    assert_eq!(r.data()["eventCursor"], id, "never past the last event");
    let cursor: i64 = sqlx::query_scalar("SELECT event_cursor FROM agent WHERE id = $1")
        .bind(agent_id).fetch_one(&pool).await.unwrap();
    assert_eq!(cursor, id);
}

#[sqlx::test]
async fn the_inbox_is_stable_bytes_until_something_changes(pool: PgPool) {
    let (owner, me) = person(&pool, "anmol@airtribe.live", "backend").await;
    let (agent_id, token) = agent(&pool, &owner, "hermes").await;

    let empty = send(&pool, "GET", "/api/agent/inbox", &token, Value::Null).await;
    assert!(empty.content_type.starts_with("text/plain"), "{}", empty.content_type);
    assert!(empty.text.contains("Nothing needs you."), "{}", empty.text);

    let t = task(&pool, "open", me).await;
    hand_off(&pool, &owner, t, agent_id).await;
    let first = send(&pool, "GET", "/api/agent/inbox", &token, Value::Null).await.text;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let second = send(&pool, "GET", "/api/agent/inbox", &token, Value::Null).await.text;
    assert_eq!(first, second, "no clock in the inbox");
    assert!(first.contains("not yet acknowledged") && first.contains("handed_off on \"Wire checkout\""), "{first}");

    send(&pool, "POST", &format!("/api/user/tasks/{t}/notes"), &owner, json!({ "body": "Use the v2 API" })).await;
    let third = send(&pool, "GET", "/api/agent/inbox", &token, Value::Null).await.text;
    assert_ne!(second, third);
    assert!(third.contains("Anmol: Use the v2 API"), "{third}");

    send(&pool, "POST", &format!("/api/agent/tasks/{t}/ack"), &token, Value::Null).await;
    ack_all(&pool, &token).await;
    let done = send(&pool, "GET", "/api/agent/inbox", &token, Value::Null).await.text;
    assert_eq!(done, empty.text);
}

// ---- MCP -------------------------------------------------------------------

async fn rpc(pool: &PgPool, token: &str, method: &str, body: Value) -> Reply {
    let request = Request::builder()
        .method(method)
        .uri("/api/services/mcp")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {token}"))
        .body(if body.is_null() { Body::empty() } else { Body::from(body.to_string()) })
        .unwrap();
    let response = acp_server::app::app(state(pool)).oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply { status, content_type, text: String::from_utf8_lossy(&bytes).into_owned() }
}

fn call(id: i64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

#[sqlx::test]
async fn mcp_handshake_as_real_clients_do_it(pool: PgPool) {
    let (_, t, token, _) = working(&pool, "backend", "open").await;

    let r = rpc(&pool, &token, "POST", call(0, "initialize", json!({
        "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "hermes", "version": "1" }
    }))).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.content_type.starts_with("application/json"));
    let init = r.json();
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26", "a supported version is echoed");
    assert!(init["result"]["instructions"].as_str().unwrap().contains("acp://skill"));
    let r = rpc(&pool, &token, "POST", call(0, "initialize", json!({ "protocolVersion": "2099-01-01" }))).await;
    assert_eq!(r.json()["result"]["protocolVersion"], "2025-06-18");

    let r = rpc(&pool, &token, "POST", json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert!(r.text.is_empty());
    assert_eq!(rpc(&pool, &token, "GET", Value::Null).await.status, StatusCode::METHOD_NOT_ALLOWED);

    let tools = rpc(&pool, &token, "POST", call(1, "tools/list", json!({}))).await.json();
    let mut names: Vec<&str> =
        tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["agent_inbox", "agent_tasks", "events_ack", "task_ack", "task_ask", "task_attach",
        "task_context", "task_log", "task_note", "task_now", "task_submit", "task_update"]);

    let r = rpc(&pool, &token, "POST", call(2, "tools/call", json!({ "name": "agent_inbox", "arguments": {} }))).await.json();
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().starts_with("Airtribe inbox for hermes"));

    let r = rpc(&pool, &token, "POST", call(3, "tools/call", json!({
        "name": "task_update", "arguments": { "taskId": t, "body": "go", "status": "in_progress" }
    }))).await.json();
    let row: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(row["status"], "in_progress");

    // A refused call is a tool result the model can read, not a protocol error.
    let r = rpc(&pool, &token, "POST", call(4, "tools/call", json!({
        "name": "task_submit", "arguments": { "taskId": t, "target": "completed", "summary": "done" }
    }))).await.json();
    assert_eq!(r["result"]["isError"], true);
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("PR or commit"));
    let r = rpc(&pool, &token, "POST", call(5, "tools/call", json!({ "name": "task_create", "arguments": {} }))).await.json();
    assert_eq!(r["error"]["code"], -32602, "board tools are not an agent's");

    let r = rpc(&pool, &token, "POST", call(6, "resources/list", json!({}))).await.json();
    assert_eq!(r["result"]["resources"][0]["uri"], "acp://skill");
    let r = rpc(&pool, &token, "POST", call(7, "resources/read", json!({ "uri": "acp://skill" }))).await.json();
    assert!(r["result"]["contents"][0]["text"].as_str().unwrap().contains("# Working tasks from Airtribe"));
}

#[sqlx::test]
async fn a_persons_mcp_tools_no_longer_include_leases(pool: PgPool) {
    let (owner, _) = person(&pool, "anmol@airtribe.live", "backend").await;
    let tools = rpc(&pool, &owner, "POST", call(1, "tools/list", json!({}))).await.json();
    let names: Vec<&str> =
        tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"task_search") && names.contains(&"task_update"), "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("work_") || *n == "run_log_append" || *n == "agent_inbox"), "{names:?}");
}

// ---- Migration -------------------------------------------------------------

/// Existing agent credentials become agents, one each, and two with one label
/// under one owner do not collide.
#[sqlx::test(migrations = false)]
async fn the_backfill_gives_every_agent_credential_an_agent(pool: PgPool) {
    let all = sqlx::migrate!();
    let before = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.migrations.iter().filter(|m| m.version <= 20260922000010).cloned().collect(),
        ),
        ..sqlx::migrate!()
    };
    before.run(&pool).await.unwrap();
    let owner: Uuid = sqlx::query_scalar("INSERT INTO person (email, name) VALUES ('a@airtribe.live', 'A') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    for hash in ["h1", "h2"] {
        sqlx::query(
            "INSERT INTO credential (kind, label, token_hash, owner_id, scopes, expires_at)
             VALUES ('agent', 'hermes', $1, $2, ARRAY['read','claim'], now() + interval '1 day')",
        )
        .bind(hash).bind(owner).execute(&pool).await.unwrap();
    }
    all.run(&pool).await.unwrap();

    let handles: Vec<String> = sqlx::query_scalar(
        "SELECT a.handle FROM credential c JOIN agent a ON a.id = c.agent_id ORDER BY a.handle",
    )
    .fetch_all(&pool).await.unwrap();
    assert_eq!(handles, ["hermes", "hermes-2"]);
}
