//! Intake: an agent filing tasks for its owner. Capability, the intake
//! project, dedupe (a constraint, not a check), triage moves, DM privacy,
//! the recent listing and the MCP tools — each as the server enforces it.

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

fn message(v: &Value) -> String {
    v["error"]["message"].as_str().unwrap_or_default().to_owned()
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
        json!({ "handle": "slacker", "name": "Slack Agent", "runtime": "hermes", "canIntake": can_intake })).await;
    assert_eq!(minted["agent"]["canIntake"], can_intake);
    World {
        owner,
        owner_id,
        other,
        admin,
        agent: minted["token"].as_str().unwrap().to_owned(),
        agent_id: minted["agent"]["id"].as_str().unwrap().parse().unwrap(),
    }
}

fn source(key: &str, channel: &str) -> Value {
    json!({
        "kind": "slack", "key": key, "url": format!("https://slack.test/{key}"),
        "channel": channel, "channelName": "#issues-and-feedback", "author": "Priya",
        "text": "Checkout fails for saved cards", "receivedAt": "2026-09-25T10:00:00Z",
    })
}

fn filing(key: &str, channel: &str) -> Value {
    json!({
        "source": source(key, channel), "title": "Checkout fails for saved cards",
        "body": "Priya reports saved cards error at pay.", "category": "bug",
        "reason": "looks like a bug: checkout fails for saved cards", "confidence": 0.86,
    })
}

async fn file(pool: &PgPool, w: &World, key: &str) -> Value {
    ok(pool, "POST", "/api/agent/intake", &w.agent, filing(key, "C123")).await
}

#[sqlx::test]
async fn intake_needs_the_capability_and_the_owner_toggles_it(pool: PgPool) {
    let w = world(&pool, false).await;
    let (status, v) = send(&pool, "POST", "/api/agent/intake", &w.agent, filing("m1", "C1")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(message(&v).contains("Can create tasks for me"), "{v}");
    let (status, _) = send(&pool, "GET", "/api/agent/intake/recent", &w.agent, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Only the owner toggles it; someone else's agent does not exist for them.
    let uri = format!("/api/user/agents/{}", w.agent_id);
    let (status, _) = send(&pool, "PATCH", &uri, &w.other, json!({ "canIntake": true })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&pool, "PATCH", &uri, &w.agent, json!({ "canIntake": true })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an agent cannot grant itself intake");
    let agent = ok(&pool, "PATCH", &uri, &w.owner, json!({ "canIntake": true })).await;
    assert_eq!(agent["canIntake"], true);
    assert_eq!(agent["intakeStats"], json!({ "triage": 0, "accepted": 0, "dismissed": 0 }));

    let task = file(&pool, &w, "m1").await;
    assert_eq!(task["status"], "triage");
    assert_eq!(task["category"], "bug");
    assert_eq!(task["assigneePersonId"], json!(w.owner_id));
    assert!(task["delegate"].is_null());
    assert_eq!(task["source"]["channelName"], "#issues-and-feedback");
    assert_eq!(task["source"]["confidence"], json!(0.86));

    // A task is filed only with a real category.
    let mut bad = filing("m2", "C1");
    bad["category"] = json!("rant");
    let (status, v) = send(&pool, "POST", "/api/agent/intake", &w.agent, bad).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message(&v).contains("bug, feature"), "{v}");
}

#[sqlx::test]
async fn the_first_intake_makes_the_project_once_and_archiving_makes_a_new_one(pool: PgPool) {
    let w = world(&pool, true).await;
    let a = file(&pool, &w, "m1").await;
    let b = file(&pool, &w, "m2").await;
    assert_eq!(a["projectId"], b["projectId"], "one intake project");
    assert_eq!(a["projectName"], "Slack \u{2014} Anmol");
    let project = ok(&pool, "GET", &format!("/api/user/projects/{}", a["projectId"].as_str().unwrap()), &w.other, Value::Null).await;
    assert_eq!(project["key"], "intake-anmol-slack");
    assert_eq!(project["description"], "Tasks Slack Agent filed from Slack for Anmol.");
    let agents = ok(&pool, "GET", "/api/user/agents", &w.owner, Value::Null).await;
    assert_eq!(agents[0]["intakeProjectId"], a["projectId"]);
    assert_eq!(agents[0]["intakeStats"]["triage"], 2);

    // The owner created it, so they may archive it; the next intake starts afresh.
    ok(&pool, "POST", &format!("/api/user/projects/{}/archive", a["projectId"].as_str().unwrap()), &w.owner, Value::Null).await;
    let agents = ok(&pool, "GET", "/api/user/agents", &w.owner, Value::Null).await;
    assert!(agents[0]["intakeProjectId"].is_null(), "an archived intake project is not linked");
    let c = file(&pool, &w, "m3").await;
    assert_ne!(c["projectId"], a["projectId"]);
    let fresh = ok(&pool, "GET", &format!("/api/user/projects/{}", c["projectId"].as_str().unwrap()), &w.owner, Value::Null).await;
    assert_eq!(fresh["key"], "intake-anmol-slack-2");
}

#[sqlx::test]
async fn the_same_message_is_filed_once(pool: PgPool) {
    let w = world(&pool, true).await;
    let first = file(&pool, &w, "m1").await;
    let id = first["id"].as_str().unwrap();
    let (status, v) = send(&pool, "POST", "/api/agent/intake", &w.agent, filing("m1", "C123")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(message(&v).contains(id), "names the task: {v}");
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM task").fetch_one(&pool).await.unwrap();
    assert_eq!(tasks, 1, "nothing created");

    // The guarantee is the constraint, not the pre-check.
    let raced = sqlx::query(
        "INSERT INTO task_source (task_id, agent_id, kind, source_key, appended) VALUES ($1, $2, 'slack', 'm1', true)",
    )
    .bind(first["id"].as_str().unwrap().parse::<Uuid>().unwrap())
    .bind(w.agent_id)
    .execute(&pool)
    .await;
    assert!(raced.unwrap_err().as_database_error().unwrap().is_unique_violation());
}

#[sqlx::test]
async fn appending_is_for_your_own_filings_and_dedupes_too(pool: PgPool) {
    let w = world(&pool, true).await;
    let task = file(&pool, &w, "m1").await;
    let id = task["id"].as_str().unwrap();
    let uri = format!("/api/agent/intake/{id}/append");

    let note = ok(&pool, "POST", &uri, &w.agent, json!({ "source": source("m2", "C123"), "text": "Also on web" })).await;
    assert_eq!(note["kind"], "note");
    assert_eq!(note["agent"]["name"], "Slack Agent");
    assert!(note["body"].as_str().unwrap().contains("https://slack.test/m2"), "{note}");

    for key in ["m2", "m1"] {
        let (status, v) = send(&pool, "POST", &uri, &w.agent, json!({ "source": source(key, "C123") })).await;
        assert_eq!(status, StatusCode::CONFLICT, "{key}: {v}");
    }

    // A task this agent did not file.
    let other_id: Uuid = sqlx::query_scalar(
        "WITH pr AS (INSERT INTO project (key, name) VALUES ('X', 'X') RETURNING id),
              ph AS (INSERT INTO phase (project_id, name, position) SELECT id, 'W', 0 FROM pr RETURNING id)
         INSERT INTO task (phase_id, title) SELECT id, 'Other' FROM ph RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let (status, v) = send(&pool, "POST", &format!("/api/agent/intake/{other_id}/append"), &w.agent,
        json!({ "source": source("m9", "C123") })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(message(&v).contains("not filed by you"), "{v}");

    // A direct message's words stay out of the team-visible note.
    let note = ok(&pool, "POST", &uri, &w.agent, json!({ "source": source("m3", "D42") })).await;
    assert!(!note["body"].as_str().unwrap().contains("Checkout fails"), "{note}");
}

#[sqlx::test]
async fn triage_is_accepted_or_dismissed_and_nothing_else(pool: PgPool) {
    let w = world(&pool, true).await;
    let a = file(&pool, &w, "m1").await;
    let b = file(&pool, &w, "m2").await;
    let (ida, idb) = (a["id"].as_str().unwrap(), b["id"].as_str().unwrap());

    // Only accept and dismiss leave triage.
    let (status, v) = send(&pool, "PATCH", &format!("/api/user/tasks/{ida}"), &w.owner, json!({ "status": "in_progress" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let tracks = ok(&pool, "GET", "/api/user/tracks", &w.owner, Value::Null).await;
    assert_eq!(tracks["eng"]["triage"], json!(["open", "dropped"]));
    assert_eq!(tracks["design"]["triage"], json!(["open", "dropped"]));
    assert!(tracks["eng"].as_object().unwrap().values().all(|to| !to.as_array().unwrap().contains(&json!("triage"))),
        "nothing enters triage");

    // Only the owner (or an admin) decides; a teammate is refused.
    let (status, v) = send(&pool, "POST", &format!("/api/user/tasks/{ida}/accept"), &w.other, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    let (status, _) = send(&pool, "POST", &format!("/api/user/tasks/{ida}/accept"), &w.agent, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an agent cannot accept its own filing");

    // Accept into another project: its first phase.
    let target: Uuid = sqlx::query_scalar(
        "WITH pr AS (INSERT INTO project (key, name) VALUES ('PAY', 'Payments') RETURNING id),
              ph AS (INSERT INTO phase (project_id, name, position) SELECT id, 'Build', 0 FROM pr RETURNING id)
         SELECT id FROM pr",
    ).fetch_one(&pool).await.unwrap();
    let (status, v) = send(&pool, "POST", &format!("/api/user/tasks/{ida}/accept"), &w.owner,
        json!({ "projectId": Uuid::new_v4() })).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
    let row = ok(&pool, "POST", &format!("/api/user/tasks/{ida}/accept"), &w.owner, json!({ "projectId": target })).await;
    assert_eq!(row["status"], "open");
    assert_eq!(row["projectName"], "Payments");
    assert_eq!(row["phaseName"], "Build");
    assert!(row["doneAt"].is_null());

    let (status, v) = send(&pool, "POST", &format!("/api/user/tasks/{ida}/dismiss"), &w.owner, Value::Null).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(message(&v).contains("not in triage"), "{v}");

    // An admin may dismiss, with a reason left as a note.
    let row = ok(&pool, "POST", &format!("/api/user/tasks/{idb}/dismiss"), &w.admin, json!({ "reason": "duplicate of PAY-12" })).await;
    assert_eq!(row["status"], "dropped");
    let reason: String = sqlx::query_scalar("SELECT body FROM note WHERE task_id = $1")
        .bind(idb.parse::<Uuid>().unwrap()).fetch_one(&pool).await.unwrap();
    assert!(reason.contains("duplicate of PAY-12"));

    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM change WHERE target_id = ANY($1) AND patch ? 'status' AND patch->>'status' IN ('open', 'dropped')",
    )
    .bind(vec![ida.parse::<Uuid>().unwrap(), idb.parse::<Uuid>().unwrap()])
    .fetch_one(&pool).await.unwrap();
    assert_eq!(audited, 2, "both moves are in the audit trail");

    let agents = ok(&pool, "GET", "/api/user/agents", &w.owner, Value::Null).await;
    assert_eq!(agents[0]["intakeStats"], json!({ "triage": 0, "accepted": 1, "dismissed": 1 }));
}

#[sqlx::test]
async fn triage_shows_in_counts_and_home_and_not_as_open_work(pool: PgPool) {
    let w = world(&pool, true).await;
    let a = file(&pool, &w, "m1").await;
    let counts = ok(&pool, "GET", "/api/user/counts", &w.owner, Value::Null).await;
    assert_eq!(counts["triage"], 1);
    assert_eq!(counts["myOpen"], 0, "triage is not accepted work");

    let home = ok(&pool, "GET", "/api/user/home", &w.owner, Value::Null).await;
    let item = &home["needsAttention"][0];
    assert_eq!(item["kind"], "triage");
    assert_eq!(item["taskId"], a["id"]);
    assert_eq!(item["agentName"], "Slack Agent");
    assert_eq!(item["body"], "bug");
    let me = home["team"].as_array().unwrap().iter().find(|p| p["name"] == "Anmol").unwrap();
    assert_eq!(me["open"], 0);

    // Category is editable by any writer later.
    let id = a["id"].as_str().unwrap();
    ok(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &w.other, json!({ "category": "feature" })).await;
    let (status, _) = send(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &w.other, json!({ "category": "rant" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &w.other, Value::Null).await;
    assert_eq!(row["category"], "feature");
    ok(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &w.other, json!({ "category": null })).await;
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &w.other, Value::Null).await;
    assert!(row["category"].is_null());
    assert_eq!(row["status"], "triage", "an edit does not move it");
}

#[sqlx::test]
async fn a_direct_messages_words_are_the_owners(pool: PgPool) {
    let w = world(&pool, true).await;
    let public = file(&pool, &w, "m1").await;
    let dm = ok(&pool, "POST", "/api/agent/intake", &w.agent, filing("m2", "D42")).await;
    let mut flagged = filing("m3", "C9");
    flagged["source"]["private"] = json!(true);
    let flagged = ok(&pool, "POST", "/api/agent/intake", &w.agent, flagged).await;

    let get = |id: &Value, who: &str| {
        let (pool, who, uri) = (pool.clone(), who.to_owned(), format!("/api/user/tasks/{}", id.as_str().unwrap()));
        async move { ok(&pool, "GET", &uri, &who, Value::Null).await }
    };
    let seen = get(&public["id"], &w.other).await;
    assert_eq!(seen["source"]["text"], "Checkout fails for saved cards", "a channel message is team-visible");
    assert_eq!(seen["source"]["author"], "Priya");

    for task in [&dm, &flagged] {
        let teammate = get(&task["id"], &w.other).await;
        assert_eq!(teammate["source"]["text"], "From a direct message");
        assert!(teammate["source"]["author"].is_null());
        assert_eq!(teammate["source"]["private"], true);
        assert_eq!(teammate["source"]["url"], task["source"]["url"], "the rest is team-visible");
        for who in [&w.owner, &w.admin] {
            let full = get(&task["id"], who).await;
            assert_eq!(full["source"]["text"], "Checkout fails for saved cards");
            assert_eq!(full["source"]["author"], "Priya");
        }
    }
    // Lists filter the same way.
    let all = ok(&pool, "GET", "/api/user/tasks", &w.other, Value::Null).await;
    let private: Vec<&Value> = all.as_array().unwrap().iter().filter(|t| t["source"]["private"] == true).collect();
    assert_eq!(private.len(), 2);
    assert!(private.iter().all(|t| t["source"]["author"].is_null() && t["source"]["text"] == "From a direct message"));
}

#[sqlx::test]
async fn recent_lists_what_this_agent_filed(pool: PgPool) {
    let w = world(&pool, true).await;
    file(&pool, &w, "m1").await;
    let b = file(&pool, &w, "m2").await;
    ok(&pool, "POST", &format!("/api/agent/intake/{}/append", b["id"].as_str().unwrap()), &w.agent,
        json!({ "source": source("m3", "C123") })).await;
    sqlx::query("UPDATE task_source SET created_at = now() - interval '20 days' WHERE source_key = 'm1'")
        .execute(&pool).await.unwrap();

    let recent = ok(&pool, "GET", "/api/agent/intake/recent", &w.agent, Value::Null).await;
    assert_eq!(recent.as_array().unwrap().len(), 1, "14 days by default, appends are not filings: {recent}");
    assert_eq!(recent[0]["id"], b["id"]);
    assert_eq!(recent[0]["status"], "triage");
    assert_eq!(recent[0]["category"], "bug");
    assert_eq!(recent[0]["source"], json!({ "key": "m2", "url": "https://slack.test/m2", "channel": "C123" }));
    let month = ok(&pool, "GET", "/api/agent/intake/recent?days=30", &w.agent, Value::Null).await;
    assert_eq!(month.as_array().unwrap().len(), 2);
}

// ---- MCP -------------------------------------------------------------------

async fn rpc(pool: &PgPool, token: &str, id: i64, method: &str, params: Value) -> Value {
    let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let (_, v) = {
        let request = Request::builder()
            .method("POST")
            .uri("/api/services/mcp")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = acp_server::app::app(state(pool)).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice::<Value>(&bytes).unwrap())
    };
    v
}

fn tool_text(v: &Value) -> Value {
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
}

#[sqlx::test]
async fn the_intake_tools_are_there_only_with_the_capability(pool: PgPool) {
    let w = world(&pool, false).await;
    let names = |v: Value| -> Vec<String> {
        v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect()
    };
    let before = names(rpc(&pool, &w.agent, 1, "tools/list", json!({})).await);
    assert!(!before.iter().any(|n| n.starts_with("intake_")), "{before:?}");
    let r = rpc(&pool, &w.agent, 2, "tools/call", json!({ "name": "intake_recent", "arguments": {} })).await;
    assert_eq!(r["error"]["code"], -32602, "a hidden tool cannot be called");

    ok(&pool, "PATCH", &format!("/api/user/agents/{}", w.agent_id), &w.owner, json!({ "canIntake": true })).await;
    let after = names(rpc(&pool, &w.agent, 3, "tools/list", json!({})).await);
    for t in ["intake_create", "intake_append", "intake_recent"] {
        assert!(after.iter().any(|n| n == t), "{t} in {after:?}");
    }

    let r = rpc(&pool, &w.agent, 4, "tools/call", json!({ "name": "intake_create", "arguments": filing("m1", "C1") })).await;
    let task = tool_text(&r);
    assert_eq!(task["status"], "triage", "{r}");
    let r = rpc(&pool, &w.agent, 5, "tools/call", json!({ "name": "intake_create", "arguments": filing("m1", "C1") })).await;
    assert_eq!(r["result"]["isError"], true);
    assert!(tool_text(&r).as_str().unwrap().contains(task["id"].as_str().unwrap()));
    let r = rpc(&pool, &w.agent, 6, "tools/call", json!({ "name": "intake_create", "arguments": { "title": "x" } })).await;
    assert_eq!(r["result"]["isError"], true);
    assert!(tool_text(&r).as_str().unwrap().contains("missing field"), "{r}");

    let r = rpc(&pool, &w.agent, 7, "tools/call", json!({ "name": "intake_append",
        "arguments": { "taskId": task["id"], "source": source("m2", "C1"), "text": "again" } })).await;
    assert_eq!(tool_text(&r)["kind"], "note", "{r}");
    let r = rpc(&pool, &w.agent, 8, "tools/call", json!({ "name": "intake_recent", "arguments": { "days": 7 } })).await;
    assert_eq!(tool_text(&r)[0]["id"], task["id"]);
}

#[sqlx::test]
async fn filing_leaves_the_inbox_alone(pool: PgPool) {
    let w = world(&pool, true).await;
    let inbox = |pool: PgPool, token: String| async move {
        let request = Request::builder()
            .uri("/api/agent/inbox")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let response = acp_server::app::app(state(&pool)).oneshot(request).await.unwrap();
        response.into_body().collect().await.unwrap().to_bytes()
    };
    let before = inbox(pool.clone(), w.agent.clone()).await;
    let t = file(&pool, &w, "m1").await;
    ok(&pool, "POST", &format!("/api/agent/intake/{}/append", t["id"].as_str().unwrap()), &w.agent,
        json!({ "source": source("m2", "C1") })).await;
    ok(&pool, "POST", &format!("/api/user/tasks/{}/accept", t["id"].as_str().unwrap()), &w.owner, Value::Null).await;
    assert_eq!(inbox(pool.clone(), w.agent.clone()).await, before, "byte-stable");
}
