//! Labels on tasks, intake filings as standalone tasks wearing their
//! source's label (and the migration that moved the old ones), and the files
//! that come with a filed message: upload limits, dedupe, and who may read
//! them — a direct message's files are the owner's and admins'.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
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

/// Status, content type and raw body.
async fn raw(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> (StatusCode, String, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(if body.is_null() { Body::empty() } else { Body::from(body.to_string()) })
        .unwrap();
    let response = acp_server::app::app(state(pool)).oneshot(request).await.unwrap();
    let status = response.status();
    let mime = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    let bytes = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, mime, bytes)
}

async fn send(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    let (status, _, bytes) = raw(pool, method, uri, token, body).await;
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

fn names(labels: &Value) -> Vec<&str> {
    labels.as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect()
}

// ---- labels -----------------------------------------------------------------

#[sqlx::test]
async fn tasks_carry_labels_set_on_create_and_replaced_by_details(pool: PgPool) {
    let (anmol, _) = person(&pool, "anmol@airtribe.live", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    let bug = ok(&pool, "POST", "/api/user/labels", &anmol, json!({ "name": "bug", "colour": "red" })).await;
    let infra = ok(&pool, "POST", "/api/user/labels", &anmol, json!({ "name": "infra", "colour": "blue" })).await;

    let t = ok(&pool, "POST", "/api/user/tasks", &anmol,
        json!({ "title": "Retry sheet writes", "labelIds": [infra["id"], bug["id"]] })).await;
    assert_eq!(names(&t["labels"]), ["bug", "infra"], "by name");
    assert_eq!(t["labels"][0]["colour"], "red");
    let id = t["id"].as_str().unwrap();

    // Any writer replaces the set; a list without labelIds leaves it.
    ok(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &dhaval, json!({ "labelIds": [infra["id"]] })).await;
    ok(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &dhaval, json!({ "priority": 1 })).await;
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &dhaval, Value::Null).await;
    assert_eq!(names(&row["labels"]), ["infra"]);
    let all = ok(&pool, "GET", "/api/user/tasks", &dhaval, Value::Null).await;
    assert_eq!(names(&all[0]["labels"]), ["infra"], "lists carry them too");

    let (status, v) = send(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &dhaval,
        json!({ "labelIds": [Uuid::new_v4()] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message(&v).contains("labels does not exist"), "{v}");
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &dhaval, Value::Null).await;
    assert_eq!(names(&row["labels"]), ["infra"], "a refused edit changes nothing");

    ok(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &anmol, json!({ "labelIds": [] })).await;
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &anmol, Value::Null).await;
    assert_eq!(row["labels"], json!([]));

    // A label deleted goes from every task that wore it.
    ok(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &anmol, json!({ "labelIds": [bug["id"]] })).await;
    sqlx::query("DELETE FROM label WHERE name = 'bug'").execute(&pool).await.unwrap();
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &anmol, Value::Null).await;
    assert_eq!(row["labels"], json!([]));
}

// ---- intake + files -----------------------------------------------------------

struct World {
    owner: String,
    other: String,
    admin: String,
    agent: String,
}

async fn world(pool: &PgPool) -> World {
    let (owner, _) = person(pool, "anmol@airtribe.live", "member").await;
    let (other, _) = person(pool, "dhaval@airtribe.live", "member").await;
    let (admin, _) = person(pool, "root@airtribe.live", "admin").await;
    let minted = ok(pool, "POST", "/api/user/agents", &owner,
        json!({ "handle": "slacker", "name": "Slack Agent", "runtime": "hermes", "canIntake": true })).await;
    World { owner, other, admin, agent: minted["token"].as_str().unwrap().to_owned() }
}

fn source(key: &str, channel: &str) -> Value {
    json!({
        "kind": "slack", "key": key, "url": format!("https://slack.test/{key}"), "channel": channel,
        "channelName": "#issues-and-feedback", "author": "Priya", "text": "Checkout *fails*",
        "receivedAt": "2026-09-25T10:00:00Z",
    })
}

async fn file_task(pool: &PgPool, w: &World, key: &str, channel: &str) -> String {
    let t = ok(pool, "POST", "/api/agent/intake", &w.agent, json!({
        "source": source(key, channel), "title": "Checkout fails", "category": "bug",
        "reason": "a bug", "confidence": 0.9,
    })).await;
    t["id"].as_str().unwrap().to_owned()
}

/// A real 1×1 PNG.
const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

fn png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(PNG_1X1).unwrap()
}

fn upload(name: &str, mime: &str, bytes: &[u8]) -> Value {
    json!({ "name": name, "mime": mime, "dataBase64": base64::engine::general_purpose::STANDARD.encode(bytes) })
}

#[sqlx::test]
async fn files_upload_with_limits_and_dedupe(pool: PgPool) {
    let w = world(&pool).await;
    let id = file_task(&pool, &w, "m1", "C1").await;
    let uri = format!("/api/agent/intake/{id}/files");

    let f = ok(&pool, "POST", &uri, &w.agent, upload("screen.png", "image/png", &png())).await;
    assert_eq!((f["name"].as_str(), f["mime"].as_str(), f["size"].as_i64()), (Some("screen.png"), Some("image/png"), Some(png().len() as i64)));
    assert_eq!((f["width"].as_i64(), f["height"].as_i64()), (Some(1), Some(1)), "read from the header");

    // The same name from the same message, once.
    let (status, v) = send(&pool, "POST", &uri, &w.agent, upload("screen.png", "image/png", &png())).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(message(&v).contains(f["id"].as_str().unwrap()), "{v}");

    // Types: images and PDFs only, and the bytes must be what they claim.
    let (status, v) = send(&pool, "POST", &uri, &w.agent, upload("notes.txt", "text/plain", b"hello")).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(message(&v).contains("only PNG, JPEG, GIF and WebP images and PDFs"), "{v}");
    let (status, v) = send(&pool, "POST", &uri, &w.agent, upload("fake.png", "image/png", b"<html></html>")).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(message(&v).contains("does not look like image/png"), "{v}");
    ok(&pool, "POST", &uri, &w.agent, upload("spec.pdf", "application/pdf", b"%PDF-1.7\n...")).await;

    // Size: 8 MB each, said in a sentence.
    let mut big = png();
    big.resize(8 * 1024 * 1024 + 1, 0);
    let (status, v) = send(&pool, "POST", &uri, &w.agent, upload("huge.png", "image/png", &big)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(message(&v).contains("limited to 8 MB"), "{v}");
    let (status, v) = send(&pool, "POST", &uri, &w.agent, json!({ "name": "x.png", "mime": "image/png", "dataBase64": "%%%" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message(&v).contains("base64"), "{v}");

    // Only on tasks this agent filed, and only with a message it knows.
    let (status, v) = send(&pool, "POST", &format!("/api/agent/intake/{}/files", Uuid::new_v4()), &w.agent,
        upload("a.png", "image/png", &png())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    let mut stray = upload("b.png", "image/png", &png());
    stray["sourceKey"] = json!("m-unknown");
    let (status, v) = send(&pool, "POST", &uri, &w.agent, stray).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message(&v).contains("not a message you filed or appended"), "{v}");
    let (status, _) = send(&pool, "POST", &uri, &w.owner, upload("c.png", "image/png", &png())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a person does not upload as the agent");

    // The source lists them, in upload order.
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &w.other, Value::Null).await;
    let files = row["source"]["files"].as_array().unwrap();
    assert_eq!(files.iter().map(|f| f["name"].as_str().unwrap()).collect::<Vec<_>>(), ["screen.png", "spec.pdf"]);
    assert!(files[0].get("bytes").is_none(), "metadata only");
}

#[sqlx::test]
async fn a_file_is_read_by_whoever_may_read_its_message(pool: PgPool) {
    let w = world(&pool).await;
    let public = file_task(&pool, &w, "m1", "C1").await;
    let dm = file_task(&pool, &w, "m2", "D42").await;
    let shot = ok(&pool, "POST", &format!("/api/agent/intake/{public}/files"), &w.agent, upload("s.png", "image/png", &png())).await;
    let secret = ok(&pool, "POST", &format!("/api/agent/intake/{dm}/files"), &w.agent, upload("s.png", "image/png", &png())).await;
    // A DM appended to the public task: its file is private though the task is not.
    ok(&pool, "POST", &format!("/api/agent/intake/{public}/append"), &w.agent, json!({ "source": source("m3", "D42") })).await;
    let mut from_dm = upload("dm.png", "image/png", &png());
    from_dm["sourceKey"] = json!("m3");
    let appended = ok(&pool, "POST", &format!("/api/agent/intake/{public}/files"), &w.agent, from_dm).await;

    let get = |id: &Value| format!("/api/user/files/{}", id.as_str().unwrap());
    let (status, mime, bytes) = raw(&pool, "GET", &get(&shot["id"]), &w.other, Value::Null).await;
    assert_eq!((status, mime.as_str()), (StatusCode::OK, "image/png"));
    assert_eq!(bytes, png(), "the bytes, not JSON");

    for id in [&secret["id"], &appended["id"]] {
        let (status, v) = send(&pool, "GET", &get(id), &w.other, Value::Null).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(message(&v), "This file came with a direct message; only Anmol and admins can open it.");
        for who in [&w.owner, &w.admin] {
            let (status, _, bytes) = raw(&pool, "GET", &get(id), who, Value::Null).await;
            assert_eq!((status, bytes.len()), (StatusCode::OK, png().len()));
        }
    }
    let (status, _) = send(&pool, "GET", &format!("/api/user/files/{}", Uuid::new_v4()), &w.owner, Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&pool, "GET", &get(&shot["id"]), &w.agent, Value::Null).await;
    assert_ne!(status, StatusCode::OK, "an agent token reads no user files");

    // The lists withhold the same files: hidden entirely, not blanked.
    let teammate = ok(&pool, "GET", &format!("/api/user/tasks/{dm}"), &w.other, Value::Null).await;
    assert_eq!(teammate["source"]["files"], json!([]));
    let teammate = ok(&pool, "GET", &format!("/api/user/tasks/{public}"), &w.other, Value::Null).await;
    assert_eq!(teammate["source"]["files"].as_array().unwrap().len(), 1, "the channel's file only");
    let owner = ok(&pool, "GET", &format!("/api/user/tasks/{public}"), &w.owner, Value::Null).await;
    assert_eq!(owner["source"]["files"].as_array().unwrap().len(), 2);

    // A deleted task takes its files with it.
    sqlx::query("DELETE FROM task WHERE id = $1").bind(dm.parse::<Uuid>().unwrap()).execute(&pool).await.unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM task_file").fetch_one(&pool).await.unwrap();
    assert_eq!(left, 2);
}

#[sqlx::test]
async fn intake_attach_is_an_mcp_tool_too(pool: PgPool) {
    let w = world(&pool).await;
    let id = file_task(&pool, &w, "m1", "C1").await;
    let call = |n: i64, args: Value| {
        let (pool, token) = (pool.clone(), w.agent.clone());
        async move {
            let body = json!({ "jsonrpc": "2.0", "id": n, "method": "tools/call",
                               "params": { "name": "intake_attach", "arguments": args } });
            let (_, v) = send(&pool, "POST", "/api/services/mcp", &token, body).await;
            v
        }
    };
    let mut args = upload("s.png", "image/png", &png());
    args["taskId"] = json!(id);
    let r = call(1, args.clone()).await;
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(text).unwrap()["name"], "s.png", "{r}");
    let r = call(2, args).await;
    assert_eq!(r["result"]["isError"], true);
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("already attached"));
}

// ---- the migration ------------------------------------------------------------

/// Filings made while intake used a per-owner project become standalone,
/// wear the source's label, and the emptied intake project is archived.
#[sqlx::test(migrations = false)]
async fn old_intake_tasks_leave_their_intake_projects(pool: PgPool) {
    let all = sqlx::migrate!();
    let before = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.migrations.iter().filter(|m| m.version <= 20260925000018).cloned().collect(),
        ),
        ..sqlx::migrate!()
    };
    before.run(&pool).await.unwrap();

    let (owner, agent, intake, kept, filed, manual, elsewhere): (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, Uuid) = sqlx::query_as(
        "WITH p AS (INSERT INTO person (email, name) VALUES ('anmol@airtribe.live', 'Anmol') RETURNING id),
              a AS (INSERT INTO agent (owner_id, handle, name, runtime, can_intake)
                    SELECT id, 'slacker', 'Slack Agent', 'hermes', true FROM p RETURNING id),
              pr AS (INSERT INTO project (key, name, created_by) SELECT x, y, (SELECT id FROM p)
                     FROM (VALUES ('intake-anmol-slack', 'Slack — Anmol'), ('intake-anmol-slack-2', 'Slack — Anmol'),
                                  ('pay', 'Payments')) v(x, y) RETURNING id, key),
              ph AS (INSERT INTO phase (project_id, name, position) SELECT id, 'Work', 0 FROM pr RETURNING id, project_id),
              t AS (INSERT INTO task (phase_id, title, status)
                    SELECT ph.id, v.title, 'triage' FROM ph JOIN pr ON pr.id = ph.project_id
                      JOIN (VALUES ('intake-anmol-slack', 'filed'), ('intake-anmol-slack-2', 'old filing'),
                                   ('intake-anmol-slack-2', 'by hand'), ('pay', 'accepted elsewhere')) v(k, title) ON v.k = pr.key
                    RETURNING id, title)
         SELECT (SELECT id FROM p), (SELECT id FROM a),
                (SELECT id FROM pr WHERE key = 'intake-anmol-slack'), (SELECT id FROM pr WHERE key = 'intake-anmol-slack-2'),
                (SELECT id FROM t WHERE title = 'filed'), (SELECT id FROM t WHERE title = 'by hand'),
                (SELECT id FROM t WHERE title = 'accepted elsewhere')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let old: Uuid = sqlx::query_scalar("SELECT id FROM task WHERE title = 'old filing'").fetch_one(&pool).await.unwrap();
    let _ = owner;
    // The first intake project is linked; the second only the audit trail knows.
    sqlx::query("UPDATE agent SET intake_project_id = $1 WHERE id = $2").bind(intake).bind(agent).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO change (actor, target_type, target_id, op, patch, state)
                 VALUES ('Slack Agent (agent)', 'project', $1, 'create', jsonb_build_object('intake_agent', $2::text), 'applied')")
        .bind(kept).bind(agent).execute(&pool).await.unwrap();
    for (task, key) in [(filed, "m1"), (old, "m2"), (elsewhere, "m3")] {
        sqlx::query("INSERT INTO task_source (task_id, agent_id, kind, source_key) VALUES ($1, $2, 'slack', $3)")
            .bind(task).bind(agent).bind(key).execute(&pool).await.unwrap();
    }
    all.run(&pool).await.unwrap();

    let phase = |t: Uuid| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, Option<Uuid>>("SELECT phase_id FROM task WHERE id = $1").bind(t).fetch_one(&pool).await.unwrap() }
    };
    assert!(phase(filed).await.is_none(), "a filing leaves the linked intake project");
    assert!(phase(old).await.is_none(), "and one the audit trail names");
    assert!(phase(manual).await.is_some(), "a task added by hand stays");
    assert!(phase(elsewhere).await.is_some(), "a filing accepted into a real project stays there");

    let labelled: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT tl.task_id, l.name, l.colour FROM task_label tl JOIN label l ON l.id = tl.label_id ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut expect = vec![filed, old];
    expect.sort();
    assert_eq!(labelled.iter().map(|r| r.0).collect::<Vec<_>>(), expect);
    assert!(labelled.iter().all(|r| r.1 == "Slack" && r.2 == "purple"));

    let archived: Vec<(Uuid, bool)> = sqlx::query_as("SELECT id, archived_at IS NOT NULL FROM project WHERE id IN ($1, $2)")
        .bind(intake).bind(kept).fetch_all(&pool).await.unwrap();
    for (id, a) in archived {
        assert_eq!(a, id == intake, "emptied: archived; still holding a hand-made task: left alone");
    }
    let link: Option<Uuid> = sqlx::query_scalar("SELECT intake_project_id FROM agent WHERE id = $1").bind(agent).fetch_one(&pool).await.unwrap();
    assert!(link.is_none());
}
