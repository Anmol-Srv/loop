//! A person's own folders: where their agent works when a task has no
//! project, or its project has no folder for them. Private per person, like
//! a repo's local path (see `repos_are_shared_and_folders_are_private` in
//! tests/project_repos.rs) — and `task_context`'s resolution of a task's
//! `folderName` against them.

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

async fn refused(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value, want: StatusCode) -> String {
    let (status, v) = send(pool, method, uri, token, body).await;
    assert_eq!(status, want, "{method} {uri}: {v}");
    v["error"]["message"].as_str().unwrap_or_default().to_owned()
}

#[sqlx::test]
async fn folders_are_private_and_the_first_is_the_default(pool: PgPool) {
    let (anmol, _) = person(&pool, "anmol@airtribe.live", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "admin").await;

    // Bad input, before anything exists.
    refused(&pool, "POST", "/api/user/folders", &anmol, json!({ "name": "", "path": "/x" }), StatusCode::BAD_REQUEST).await;
    let msg = refused(
        &pool, "POST", "/api/user/folders", &anmol,
        json!({ "name": "api", "path": "code/api" }),
        StatusCode::BAD_REQUEST,
    ).await;
    assert!(msg.contains("absolute"), "{msg}");

    // The first folder becomes the default whether asked or not.
    let api = ok(&pool, "POST", "/api/user/folders", &anmol, json!({ "name": "api", "path": "/Users/anmol/code/api" })).await;
    assert_eq!(api["isDefault"], true);
    let web = ok(&pool, "POST", "/api/user/folders", &anmol, json!({ "name": "web", "path": "/Users/anmol/code/web" })).await;
    assert_eq!(web["isDefault"], false);

    // A duplicate name for the same person is refused.
    refused(
        &pool, "POST", "/api/user/folders", &anmol,
        json!({ "name": "api", "path": "/Users/anmol/code/api2" }),
        StatusCode::BAD_REQUEST,
    ).await;
    // The same name for a different person is fine.
    ok(&pool, "POST", "/api/user/folders", &dhaval, json!({ "name": "api", "path": "/Users/dhaval/code/api" })).await;

    // Private: not even an admin sees or edits another's folders.
    let seen = ok(&pool, "GET", "/api/user/folders", &dhaval, Value::Null).await;
    assert_eq!(seen.as_array().unwrap().len(), 1, "only Dhaval's own");
    assert!(!seen.to_string().contains("/Users/anmol"), "{seen}");
    refused(
        &pool, "DELETE", &format!("/api/user/folders/{}", api["id"].as_str().unwrap()), &dhaval,
        Value::Null, StatusCode::NOT_FOUND,
    ).await;
    refused(
        &pool, "POST", &format!("/api/user/folders/{}/default", web["id"].as_str().unwrap()), &dhaval,
        Value::Null, StatusCode::NOT_FOUND,
    ).await;

    // Setting a new default unsets the old one.
    let now_default = ok(&pool, "POST", &format!("/api/user/folders/{}/default", web["id"].as_str().unwrap()), &anmol, Value::Null).await;
    assert_eq!(now_default["isDefault"], true);
    let list = ok(&pool, "GET", "/api/user/folders", &anmol, Value::Null).await;
    let by_name = |n: &str| list.as_array().unwrap().iter().find(|f| f["name"] == n).unwrap().clone();
    assert_eq!(by_name("api")["isDefault"], false);
    assert_eq!(by_name("web")["isDefault"], true);

    // Remove: gone, and removing it again is a 404.
    ok(&pool, "DELETE", &format!("/api/user/folders/{}", api["id"].as_str().unwrap()), &anmol, Value::Null).await;
    refused(
        &pool, "DELETE", &format!("/api/user/folders/{}", api["id"].as_str().unwrap()), &anmol,
        Value::Null, StatusCode::NOT_FOUND,
    ).await;
    let list = ok(&pool, "GET", "/api/user/folders", &anmol, Value::Null).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

/// `task_context` — what an agent's worker reads — resolving a folder to
/// work in for a task with no project, or one whose project gives it no
/// path: the pin, else the default, else nothing.
#[sqlx::test]
async fn task_context_resolves_the_owners_folder(pool: PgPool) {
    let (anmol, _) = person(&pool, "anmol@airtribe.live", "member").await;

    let task = ok(&pool, "POST", "/api/user/tasks", &anmol, json!({ "title": "Fix the sync job" })).await;
    let task_id = task["id"].as_str().unwrap().to_owned();
    ok(&pool, "POST", &format!("/api/user/tasks/{task_id}/assign"), &anmol, json!({ "personEmail": "anmol@airtribe.live" })).await;

    let minted = ok(&pool, "POST", "/api/user/agents", &anmol, json!({ "handle": "hermes", "runtime": "hermes" })).await;
    let agent_id = minted["agent"]["id"].as_str().unwrap().to_owned();
    let agent = minted["token"].as_str().unwrap().to_owned();
    ok(&pool, "POST", &format!("/api/user/tasks/{task_id}/handoff"), &anmol, json!({ "agentId": agent_id })).await;

    // No project, no folders yet: nothing to offer.
    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{task_id}"), &agent, Value::Null).await;
    assert!(ctx["project"].is_null());
    assert!(ctx["folder"].is_null(), "{ctx}");

    // A default folder resolves once there is one.
    ok(&pool, "POST", "/api/user/folders", &anmol, json!({ "name": "mycohort-api", "path": "/Users/anmol/code/mycohort-api" })).await;
    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{task_id}"), &agent, Value::Null).await;
    assert_eq!(
        ctx["folder"],
        json!({ "name": "mycohort-api", "path": "/Users/anmol/code/mycohort-api", "source": "default" }),
    );

    // A pinned folder wins over the default.
    ok(&pool, "POST", "/api/user/folders", &anmol, json!({ "name": "dr-doom", "path": "/Users/anmol/code/dr-doom" })).await;
    ok(&pool, "PATCH", &format!("/api/user/tasks/{task_id}/details"), &anmol, json!({ "folderName": "dr-doom" })).await;
    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{task_id}"), &agent, Value::Null).await;
    assert_eq!(
        ctx["folder"],
        json!({ "name": "dr-doom", "path": "/Users/anmol/code/dr-doom", "source": "pinned" }),
    );

    // A pin naming nothing real falls back to the default rather than to
    // nothing: it may simply have been renamed or removed since.
    ok(&pool, "PATCH", &format!("/api/user/tasks/{task_id}/details"), &anmol, json!({ "folderName": "no-such-folder" })).await;
    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{task_id}"), &agent, Value::Null).await;
    assert_eq!(ctx["folder"]["source"], "default");
    assert_eq!(ctx["folder"]["name"], "mycohort-api");

    // Unpinning goes back to the default too.
    ok(&pool, "PATCH", &format!("/api/user/tasks/{task_id}/details"), &anmol, json!({ "folderName": null })).await;
    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{task_id}"), &agent, Value::Null).await;
    assert_eq!(ctx["folder"]["source"], "default");

    // A project with a repo that has no path for the owner yet: still no
    // reason to prefer it over a folder.
    let p = ok(
        &pool, "POST", "/api/user/projects", &anmol,
        json!({ "name": "Payments", "repoUrl": "https://github.com/airtribe/payments-api" }),
    ).await["entity"].clone();
    let pid = p["id"].as_str().unwrap();
    ok(&pool, "PATCH", &format!("/api/user/tasks/{task_id}/details"), &anmol, json!({ "projectId": pid })).await;
    let repo_id = ok(&pool, "GET", &format!("/api/user/projects/{pid}/repos"), &anmol, Value::Null).await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{task_id}"), &agent, Value::Null).await;
    assert!(ctx["project"]["repos"][0]["localPath"].is_null());
    assert_eq!(ctx["folder"]["source"], "default", "no repo path yet, so the folder still stands in");

    // Once the repo has a path for the owner, it wins and the folder drops out.
    ok(&pool, "PUT", &format!("/api/user/repos/{repo_id}/path"), &anmol, json!({ "path": "/Users/anmol/code/payments-api" })).await;
    let ctx = ok(&pool, "GET", &format!("/api/agent/tasks/{task_id}"), &agent, Value::Null).await;
    assert_eq!(ctx["project"]["repos"][0]["localPath"], "/Users/anmol/code/payments-api");
    assert!(ctx["folder"].is_null(), "a project repo with a path wins over any folder");
}
