//! Repositories on a project, each person's private folder for them, the
//! agent's view of both, and labels as a many-to-many set.

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

/// The body as text: the agent inbox is plain text.
async fn text(pool: &PgPool, uri: &str, token: &str) -> String {
    let request = Request::builder().uri(uri).header("host", "acp.test")
        .header("authorization", format!("Bearer {token}")).body(Body::empty()).unwrap();
    let response = acp_server::app::app(state(pool)).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

async fn ok(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> Value {
    let (status, v) = send(pool, method, uri, token, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {v}");
    v["data"].clone()
}

async fn refused(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value, want: StatusCode) -> String {
    let (status, v) = send(pool, method, uri, token, body).await;
    assert_eq!(status, want, "{method} {uri}: {v}");
    v["error"]["message"].as_str().unwrap_or_default().to_owned()
}

async fn project(pool: &PgPool, token: &str, body: Value) -> Value {
    ok(pool, "POST", "/api/user/projects", token, body).await["entity"].clone()
}

#[sqlx::test]
async fn repos_are_shared_and_folders_are_private(pool: PgPool) {
    let (anmol, _) = person(&pool, "anmol@airtribe.live", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (root, _) = person(&pool, "root@airtribe.live", "admin").await;

    // The create form's one URL becomes a repo named after the project.
    let p = project(&pool, &anmol, json!({ "name": "Payments", "repoUrl": " git@github.com:airtribe/payments-api.git " })).await;
    let pid = p["id"].as_str().unwrap();
    let detail = ok(&pool, "GET", &format!("/api/user/projects/{pid}"), &anmol, Value::Null).await;
    assert_eq!(detail["repos"][0]["name"], "Payments");
    assert_eq!(detail["repos"][0]["url"], "git@github.com:airtribe/payments-api.git");
    assert!(detail["repos"][0]["myPath"].is_null());
    assert_eq!(detail["repos"][0]["canEdit"], true);
    // List rows carry no repos at all.
    let list = ok(&pool, "GET", "/api/user/projects", &anmol, Value::Null).await;
    assert!(list[0].get("repos").is_none(), "{list}");

    // Anyone who can write adds one; a bad URL says what a good one looks like.
    let msg = refused(&pool, "POST", &format!("/api/user/projects/{pid}/repos"), &dhaval,
        json!({ "name": "Web", "url": "file:///etc" }), StatusCode::BAD_REQUEST).await;
    assert!(msg.contains("https://"), "{msg}");
    refused(&pool, "POST", &format!("/api/user/projects/{pid}/repos"), &dhaval,
        json!({ "name": " ", "url": "https://github.com/airtribe/web" }), StatusCode::BAD_REQUEST).await;
    refused(&pool, "POST", &format!("/api/user/projects/{}/repos", Uuid::new_v4()), &dhaval,
        json!({ "name": "Web", "url": "https://github.com/airtribe/web" }), StatusCode::NOT_FOUND).await;
    let web = ok(&pool, "POST", &format!("/api/user/projects/{pid}/repos"), &dhaval,
        json!({ "name": "Web", "url": "https://github.com/airtribe/web" })).await;
    let web_id = web["id"].as_str().unwrap().to_owned();
    let api_id = detail["repos"][0]["id"].as_str().unwrap().to_owned();
    let repos = ok(&pool, "GET", &format!("/api/user/projects/{pid}/repos"), &anmol, Value::Null).await;
    assert_eq!(repos.as_array().unwrap().len(), 2);
    assert_eq!(repos[1]["canEdit"], false, "Anmol did not add Web");

    // Folders: validated, private, cleared by null or "".
    for bad in ["relative/path", "/two\nlines", &format!("/{}", "x".repeat(500))] {
        refused(&pool, "PUT", &format!("/api/user/repos/{api_id}/path"), &anmol, json!({ "path": bad }),
            StatusCode::BAD_REQUEST).await;
    }
    let mine = ok(&pool, "PUT", &format!("/api/user/repos/{api_id}/path"), &anmol,
        json!({ "path": " /Users/anmol/code/payments-api " })).await;
    assert_eq!(mine["myPath"], "/Users/anmol/code/payments-api");
    ok(&pool, "PUT", &format!("/api/user/repos/{api_id}/path"), &dhaval, json!({ "path": "/Users/dhaval/api" })).await;

    let seen = ok(&pool, "GET", &format!("/api/user/projects/{pid}"), &dhaval, Value::Null).await;
    assert_eq!(seen["repos"][0]["myPath"], "/Users/dhaval/api", "Dhaval sees his own, never Anmol's");
    let body = seen.to_string();
    assert!(!body.contains("/Users/anmol"), "{body}");
    let admin = ok(&pool, "GET", &format!("/api/user/projects/{pid}/repos"), &root, Value::Null).await;
    assert!(admin[0]["myPath"].is_null(), "not even an admin reads another's folder");
    assert!(!admin.to_string().contains("/Users/"), "{admin}");

    let cleared = ok(&pool, "PUT", &format!("/api/user/repos/{api_id}/path"), &anmol, json!({ "path": "" })).await;
    assert!(cleared["myPath"].is_null());
    let cleared = ok(&pool, "PUT", &format!("/api/user/repos/{api_id}/path"), &dhaval, json!({ "path": null })).await;
    assert!(cleared["myPath"].is_null());

    // Edit and remove: the adder or an admin.
    let msg = refused(&pool, "PATCH", &format!("/api/user/repos/{web_id}"), &anmol, json!({ "name": "Site" }),
        StatusCode::FORBIDDEN).await;
    assert!(msg.contains("Dhaval, who added this repository"), "{msg}");
    refused(&pool, "DELETE", &format!("/api/user/repos/{web_id}"), &anmol, Value::Null, StatusCode::FORBIDDEN).await;
    refused(&pool, "PATCH", &format!("/api/user/repos/{web_id}"), &dhaval, json!({ "url": "ftp://x" }),
        StatusCode::BAD_REQUEST).await;
    let renamed = ok(&pool, "PATCH", &format!("/api/user/repos/{web_id}"), &dhaval, json!({ "name": " Site " })).await;
    assert_eq!(renamed["name"], "Site");
    assert_eq!(renamed["url"], "https://github.com/airtribe/web");
    ok(&pool, "DELETE", &format!("/api/user/repos/{web_id}"), &root, Value::Null).await;
    refused(&pool, "DELETE", &format!("/api/user/repos/{web_id}"), &dhaval, Value::Null, StatusCode::NOT_FOUND).await;

    // A bad URL on the create form refuses the whole project.
    refused(&pool, "POST", "/api/user/projects", &anmol, json!({ "name": "Other", "repoUrl": "not a url" }),
        StatusCode::BAD_REQUEST).await;
}

#[sqlx::test]
async fn the_agent_reads_its_owners_folder(pool: PgPool) {
    let (anmol, me) = person(&pool, "anmol@airtribe.live", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    let p = project(&pool, &anmol, json!({
        "name": "Payments", "repoUrl": "https://github.com/airtribe/payments-api",
        "tasks": [{ "title": "Wire checkout", "assigneeId": me }],
    })).await;
    let pid = p["id"].as_str().unwrap();
    let repo = ok(&pool, "GET", &format!("/api/user/projects/{pid}/repos"), &anmol, Value::Null).await[0].clone();
    let repo_id = repo["id"].as_str().unwrap();
    let task: Uuid = sqlx::query_scalar(
        "SELECT t.id FROM task t JOIN phase ph ON ph.id = t.phase_id WHERE ph.project_id = $1::uuid",
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();

    let minted = ok(&pool, "POST", "/api/user/agents", &anmol, json!({ "handle": "hermes", "runtime": "hermes" })).await;
    let agent_id = minted["agent"]["id"].as_str().unwrap().to_owned();
    let agent = minted["token"].as_str().unwrap().to_owned();
    ok(&pool, "POST", &format!("/api/user/tasks/{task}/handoff"), &anmol, json!({ "agentId": agent_id })).await;
    let inbox_before = text(&pool, "/api/agent/inbox", &agent).await;

    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{task}"), &agent, Value::Null).await;
    assert_eq!(ctx["project"]["repos"], json!([{ "name": "Payments", "url": "https://github.com/airtribe/payments-api", "localPath": null }]));

    ok(&pool, "PUT", &format!("/api/user/repos/{repo_id}/path"), &dhaval, json!({ "path": "/Users/dhaval/api" })).await;
    ok(&pool, "PUT", &format!("/api/user/repos/{repo_id}/path"), &anmol, json!({ "path": "/Users/anmol/api" })).await;
    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{task}"), &agent, Value::Null).await;
    assert_eq!(ctx["project"]["repos"][0]["localPath"], "/Users/anmol/api", "the owner's folder, not Dhaval's");

    // The MCP tool reads the same context.
    let (status, v) = send(&pool, "POST", "/api/services/mcp", &agent, json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "task_context", "arguments": { "taskId": task } },
    })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(v.to_string().contains("/Users/anmol/api"), "{v}");
    assert!(!v.to_string().contains("/Users/dhaval"), "{v}");

    // A path change tells the agent nothing: the inbox is as it was.
    let inbox_after = text(&pool, "/api/agent/inbox", &agent).await;
    assert_eq!(inbox_before, inbox_after);
    assert!(!inbox_after.contains("/Users/"));
}

#[sqlx::test]
async fn a_project_holds_several_labels(pool: PgPool) {
    let (anmol, _) = person(&pool, "anmol@airtribe.live", "member").await;
    let backend = ok(&pool, "POST", "/api/user/labels", &anmol, json!({ "name": "Backend", "colour": "green" })).await;
    let q4 = ok(&pool, "POST", "/api/user/labels", &anmol, json!({ "name": "Q4", "colour": "amber" })).await;
    let infra = ok(&pool, "POST", "/api/user/labels", &anmol, json!({ "name": "Infra" })).await;
    assert_eq!(infra["colour"], "slate");
    // Creating one that exists returns it.
    let again = ok(&pool, "POST", "/api/user/labels", &anmol, json!({ "name": "Backend", "colour": "red" })).await;
    assert_eq!(again["id"], backend["id"]);
    refused(&pool, "POST", "/api/user/labels", &anmol, json!({ "name": "X", "colour": "teal" }), StatusCode::BAD_REQUEST).await;

    let p = project(&pool, &anmol, json!({ "name": "Payments", "labelIds": [backend["id"], q4["id"]] })).await;
    let names: Vec<&str> = p["labels"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Backend", "Q4"]);
    let pid = p["id"].as_str().unwrap();

    let updated = ok(&pool, "PATCH", &format!("/api/user/projects/{pid}"), &anmol,
        json!({ "labelIds": [q4["id"], infra["id"], backend["id"]] })).await["entity"].clone();
    assert_eq!(updated["labels"].as_array().unwrap().len(), 3);
    let list = ok(&pool, "GET", "/api/user/projects", &anmol, Value::Null).await;
    assert_eq!(list[0]["labels"].as_array().unwrap().len(), 3);

    let updated = ok(&pool, "PATCH", &format!("/api/user/projects/{pid}"), &anmol, json!({ "labelIds": [infra["id"]] }))
        .await["entity"].clone();
    assert_eq!(updated["labels"], json!([{ "id": infra["id"], "name": "Infra", "colour": "slate" }]));
    ok(&pool, "PATCH", &format!("/api/user/projects/{pid}"), &anmol, json!({ "labelIds": [] })).await;
    let detail = ok(&pool, "GET", &format!("/api/user/projects/{pid}"), &anmol, Value::Null).await;
    assert_eq!(detail["labels"], json!([]));
}
