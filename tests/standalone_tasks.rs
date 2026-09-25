//! Standalone tasks: made with no project, listed everywhere a task is,
//! moved into a project and out again, managed by their creator or an admin,
//! archived on their own flag, and handed to an agent like any other.

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
        "INSERT INTO person (email, name, role, department)
         VALUES ($1, initcap(split_part($1, '@', 1)), $2, 'backend') RETURNING id",
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
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

async fn ok(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> Value {
    let (status, v) = send(pool, method, uri, token, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {v}");
    v["data"].clone()
}

fn ids(rows: &Value) -> Vec<&str> {
    rows.as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect()
}

fn message(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

/// A project made by `token`, with no tasks: its id.
async fn project(pool: &PgPool, token: &str, name: &str) -> String {
    let v = ok(pool, "POST", "/api/user/projects", token, json!({ "name": name })).await;
    v["entity"]["id"].as_str().unwrap().to_owned()
}

#[sqlx::test]
async fn a_task_with_no_project_is_standalone_and_listed_everywhere(pool: PgPool) {
    let (anmol, anmol_id) = person(&pool, "anmol@airtribe.live", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;

    let t = ok(&pool, "POST", "/api/user/tasks", &anmol, json!({
        "title": "  Rotate the Razorpay key  ", "body": "Before Friday", "assigneeId": anmol_id,
        "priority": 1, "category": "chore"
    })).await;
    let id = t["id"].as_str().unwrap().to_owned();
    assert_eq!(t["title"], "Rotate the Razorpay key");
    assert!(t["projectId"].is_null() && t["projectName"].is_null() && t["phaseId"].is_null(), "{t}");
    assert_eq!((t["priority"].as_i64(), t["category"].as_str()), (Some(1), Some("chore")));
    assert_eq!(t["canArchive"], true, "its creator manages it");
    let created_by: Option<Uuid> = sqlx::query_scalar("SELECT created_by FROM task WHERE id = $1::uuid")
        .bind(&id).fetch_one(&pool).await.unwrap();
    assert_eq!(created_by, Some(anmol_id));
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM change WHERE target_type = 'task' AND target_id = $1::uuid AND op = 'create'",
    )
    .bind(&id).fetch_one(&pool).await.unwrap();
    assert_eq!(audited, 1);

    assert_eq!(ids(&ok(&pool, "GET", "/api/user/tasks", &dhaval, Value::Null).await), [id.as_str()]);
    assert_eq!(ids(&ok(&pool, "GET", "/api/user/tasks/mine", &anmol, Value::Null).await), [id.as_str()]);
    assert_eq!(ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &dhaval, Value::Null).await["canArchive"], false);
    let home = ok(&pool, "GET", "/api/user/home", &anmol, Value::Null).await;
    assert_eq!(ids(&home["myTasks"]), [id.as_str()]);
    assert_eq!(home["team"].as_array().unwrap().iter().find(|p| p["email"] == "anmol@airtribe.live").unwrap()["open"], 1);
    let counts = ok(&pool, "GET", "/api/user/counts", &anmol, Value::Null).await;
    assert_eq!(counts["myOpen"], 1);

    // Bad values say what is wrong.
    let (status, v) = send(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": " " })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (status, v) = send(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": "x", "category": "idea" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message(&v).contains("category"), "{v}");
}

#[sqlx::test]
async fn created_into_a_project_it_lands_in_the_first_phase(pool: PgPool) {
    let (anmol, _) = person(&pool, "anmol@airtribe.live", "member").await;
    let p = project(&pool, &anmol, "Checkout").await;
    let t = ok(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": "Saved cards", "projectId": p })).await;
    assert_eq!(t["projectId"], json!(p));
    assert_eq!(t["projectName"], "Checkout");
    assert_eq!(t["phaseName"], "Work");
    assert!(t["assigneePersonId"].is_null(), "no assignee unless asked");

    let (status, v) = send(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": "x", "projectId": Uuid::new_v4() })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(message(&v), "That project does not exist.");
    ok(&pool, "POST", &format!("/api/user/projects/{p}/archive"), &anmol, json!({})).await;
    let (status, v) = send(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": "x", "projectId": p })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message(&v).contains("archived"), "{v}");
}

#[sqlx::test]
async fn it_moves_into_a_project_and_out_again(pool: PgPool) {
    let (anmol, _) = person(&pool, "anmol@airtribe.live", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (root, _) = person(&pool, "root@airtribe.live", "admin").await;
    let p = project(&pool, &dhaval, "Checkout").await;
    let t = ok(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": "Saved cards" })).await;
    let id = t["id"].as_str().unwrap();
    let details = format!("/api/user/tasks/{id}/details");

    // Only who may manage it moves it.
    let (status, v) = send(&pool, "PATCH", &details, &dhaval, json!({ "projectId": p })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert!(message(&v).contains("Anmol"), "{v}");

    ok(&pool, "PATCH", &details, &anmol, json!({ "projectId": p })).await;
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &anmol, Value::Null).await;
    assert_eq!((row["projectName"].as_str(), row["phaseName"].as_str()), (Some("Checkout"), Some("Work")));
    assert_eq!(ids(&ok(&pool, "GET", &format!("/api/user/tasks?projectId={p}"), &anmol, Value::Null).await), [id]);
    // In the project now, its creator may manage it too.
    assert_eq!(ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &dhaval, Value::Null).await["canArchive"], true);
    let flow = ok(&pool, "GET", &format!("/api/user/projects/{p}/flow"), &anmol, Value::Null).await;
    assert_eq!(flow["total"], 1, "{flow}");

    ok(&pool, "PATCH", &details, &root, json!({ "projectId": null })).await;
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &anmol, Value::Null).await;
    assert!(row["projectId"].is_null() && row["phaseName"].is_null(), "{row}");
    assert!(ids(&ok(&pool, "GET", &format!("/api/user/tasks?projectId={p}"), &anmol, Value::Null).await).is_empty());
    assert_eq!(ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &dhaval, Value::Null).await["canArchive"], false);

    // Other edits leave where it is alone.
    ok(&pool, "PATCH", &details, &anmol, json!({ "projectId": p })).await;
    ok(&pool, "PATCH", &details, &dhaval, json!({ "title": "Saved cards v2" })).await;
    let row = ok(&pool, "GET", &format!("/api/user/tasks/{id}"), &anmol, Value::Null).await;
    assert_eq!((row["title"].as_str(), row["projectName"].as_str()), (Some("Saved cards v2"), Some("Checkout")));

    let (status, v) = send(&pool, "PATCH", &details, &anmol, json!({ "projectId": Uuid::new_v4() })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(message(&v), "That project does not exist.");
    let q = project(&pool, &anmol, "Old").await;
    ok(&pool, "POST", &format!("/api/user/projects/{q}/archive"), &anmol, json!({})).await;
    let (status, v) = send(&pool, "PATCH", &details, &anmol, json!({ "projectId": q })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(message(&v), "That project is archived \u{2014} pick a live project.");
}

#[sqlx::test]
async fn it_archives_on_its_own_flag_and_outlives_a_project_delete(pool: PgPool) {
    let (anmol, _) = person(&pool, "anmol@airtribe.live", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (root, _) = person(&pool, "root@airtribe.live", "admin").await;
    let t = ok(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": "Standalone" })).await;
    let id = t["id"].as_str().unwrap().to_owned();
    let p = project(&pool, &anmol, "Checkout").await;
    ok(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": "In project", "projectId": p })).await;

    // Archiving and deleting a project leaves a standalone task alone.
    ok(&pool, "POST", &format!("/api/user/projects/{p}/archive"), &anmol, json!({})).await;
    assert_eq!(ids(&ok(&pool, "GET", "/api/user/tasks", &anmol, Value::Null).await), [id.as_str()]);
    ok(&pool, "DELETE", &format!("/api/user/projects/{p}"), &anmol, Value::Null).await;
    assert_eq!(ids(&ok(&pool, "GET", "/api/user/tasks", &anmol, Value::Null).await), [id.as_str()]);

    let (status, _) = send(&pool, "POST", &format!("/api/user/tasks/{id}/archive"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let row = ok(&pool, "POST", &format!("/api/user/tasks/{id}/archive"), &anmol, json!({})).await;
    assert!(row["archivedAt"].is_string() && row["projectArchivedAt"].is_null());
    assert!(ids(&ok(&pool, "GET", "/api/user/tasks", &anmol, Value::Null).await).is_empty());
    assert_eq!(ids(&ok(&pool, "GET", "/api/user/tasks?archived=true", &anmol, Value::Null).await), [id.as_str()]);
    ok(&pool, "POST", &format!("/api/user/tasks/{id}/restore"), &root, json!({})).await;
    assert_eq!(ids(&ok(&pool, "GET", "/api/user/tasks", &anmol, Value::Null).await), [id.as_str()]);
    ok(&pool, "DELETE", &format!("/api/user/tasks/{id}"), &root, Value::Null).await;
    assert!(ids(&ok(&pool, "GET", "/api/user/tasks", &anmol, Value::Null).await).is_empty());
}

#[sqlx::test]
async fn an_agent_works_a_standalone_task(pool: PgPool) {
    let (anmol, me) = person(&pool, "anmol@airtribe.live", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    let minted = ok(&pool, "POST", "/api/user/agents", &anmol, json!({ "handle": "hermes", "runtime": "hermes" })).await;
    let agent_id = minted["agent"]["id"].as_str().unwrap().to_owned();
    let agent = minted["token"].as_str().unwrap().to_owned();
    let t = ok(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": "Rotate the key", "assigneeId": me })).await;
    let id = t["id"].as_str().unwrap();

    ok(&pool, "POST", &format!("/api/user/tasks/{id}/handoff"), &anmol, json!({ "agentId": agent_id })).await;
    assert_eq!(ids(&ok(&pool, "GET", "/api/agent/tasks", &agent, Value::Null).await), [id]);
    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{id}"), &agent, Value::Null).await;
    assert!(ctx["project"].is_null(), "{ctx}");
    assert_eq!(ctx["task"]["title"], "Rotate the key");
    ok(&pool, "POST", &format!("/api/agent/tasks/{id}/ack"), &agent, Value::Null).await;
    ok(&pool, "POST", &format!("/api/agent/tasks/{id}/now"), &agent, json!({ "text": "rotating" })).await;
    let active = ok(&pool, "GET", "/api/user/agents/active", &dhaval, Value::Null).await;
    assert_eq!(active[0]["task"]["title"], "Rotate the key");
    assert!(active[0]["task"]["projectName"].is_null());

    // Held by the agent, it cannot be archived out from under it.
    let (status, _) = send(&pool, "POST", &format!("/api/user/tasks/{id}/archive"), &anmol, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn mcp_task_create_takes_a_project_or_none(pool: PgPool) {
    sqlx::query("INSERT INTO person (email, name) VALUES ('anmol@airtribe.live', 'Anmol')").execute(&pool).await.unwrap();
    let (raw, _) = token::mint(&state(&pool), "mcp", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30)
        .await
        .unwrap();
    let (session, _) = token::mint_session(&state(&pool), "anmol@airtribe.live").await.unwrap();
    let p = project(&pool, &session, "Checkout").await;
    let call = |args: Value| {
        let pool = pool.clone();
        let raw = raw.clone();
        async move {
            let request = Request::builder()
                .method("POST")
                .uri("/api/services/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {raw}"))
                .body(Body::from(
                    json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                            "params": { "name": "task_create", "arguments": args } })
                    .to_string(),
                ))
                .unwrap();
            let response = acp_server::app::app(state(&pool)).oneshot(request).await.unwrap();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            let text = v["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{v}")).to_owned();
            serde_json::from_str::<Value>(&text).unwrap()
        }
    };
    let alone = call(json!({ "title": "standalone" })).await;
    assert!(alone["entity"]["phaseId"].is_null(), "{alone}");
    let inside = call(json!({ "title": "in project", "projectId": p })).await;
    let phase: Uuid = sqlx::query_scalar("SELECT id FROM phase WHERE project_id = $1::uuid")
        .bind(&p).fetch_one(&pool).await.unwrap();
    assert_eq!(inside["entity"]["phaseId"], json!(phase));
}
