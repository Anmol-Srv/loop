//! Who may remove a resource: only whoever added it. An agent's attachment is
//! its owner's; a row with nobody recorded is an admin's. No role overrides
//! that, and neither a proposal nor its approval is a way round it.

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

/// A person with a session: (token, id).
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

async fn project(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp', 'ACP') RETURNING id")
        .fetch_one(pool)
        .await
        .unwrap()
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

/// A project link added by `token`; its id.
async fn add(pool: &PgPool, token: &str, project: Uuid) -> String {
    let (status, json) = send(pool, "POST", "/api/user/artifacts", token, json!({
        "parentType": "project", "parentId": project, "kind": "link", "url": "https://example.com/spec"
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json["data"]["entity"]["id"].as_str().unwrap().to_owned()
}

async fn remove(pool: &PgPool, token: &str, id: &str) -> (StatusCode, Value) {
    send(pool, "DELETE", &format!("/api/user/artifacts/{id}"), token, Value::Null).await
}

async fn list(pool: &PgPool, token: &str, project: Uuid) -> Vec<Value> {
    let uri = format!("/api/user/artifacts?parentType=project&parentId={project}");
    send(pool, "GET", &uri, token, Value::Null).await.1["data"].as_array().unwrap().clone()
}

fn message(json: &Value) -> &str {
    json["error"]["message"].as_str().unwrap_or_default()
}

#[sqlx::test]
async fn the_adder_removes_their_own_and_nobody_else_can(pool: PgPool) {
    let p = project(&pool).await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (anmol, _) = person(&pool, "anmol@airtribe.live", "member").await;
    let (boss, _) = person(&pool, "boss@airtribe.live", "admin").await;
    let (manager, _) = person(&pool, "manny@airtribe.live", "manager").await;
    let id = add(&pool, &dhaval, p).await;

    for token in [&anmol, &boss, &manager] {
        let (status, json) = remove(&pool, token, &id).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert_eq!(message(&json), "Only Dhaval can remove this link \u{2014} they added it.");
    }
    let (status, json) = remove(&pool, &dhaval, &id).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"]["status"], "applied");
    assert!(list(&pool, &dhaval, p).await.is_empty());
}

#[sqlx::test]
async fn a_row_nobody_added_is_an_admins_to_remove(pool: PgPool) {
    let p = project(&pool).await;
    let (member, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (boss, _) = person(&pool, "boss@airtribe.live", "admin").await;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO artifact (parent_type, parent_id, kind, url) VALUES ('project', $1, 'pr', 'https://x/pull/1')
         RETURNING id",
    )
    .bind(p)
    .fetch_one(&pool)
    .await
    .unwrap();

    let rows = list(&pool, &member, p).await;
    assert!(rows[0]["addedBy"].is_null());
    assert_eq!(rows[0]["canRemove"], false);
    assert_eq!(list(&pool, &boss, p).await[0]["canRemove"], true);

    let (status, json) = remove(&pool, &member, &id.to_string()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(message(&json), "Only an admin can remove this PR \u{2014} nobody is recorded as adding it.");
    assert_eq!(remove(&pool, &boss, &id.to_string()).await.0, StatusCode::OK);
}

#[sqlx::test]
async fn list_says_who_added_it_and_whether_you_may_remove_it(pool: PgPool) {
    let p = project(&pool).await;
    let (dhaval, dhaval_id) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (boss, _) = person(&pool, "boss@airtribe.live", "admin").await;
    add(&pool, &dhaval, p).await;

    let mine = &list(&pool, &dhaval, p).await[0];
    assert_eq!(mine["addedBy"], json!({ "id": dhaval_id, "name": "Dhaval" }));
    assert!(mine["addedByAgent"].is_null());
    assert_eq!(mine["canRemove"], true);
    assert_eq!(mine["url"], "https://example.com/spec", "existing fields stay");
    assert_eq!(list(&pool, &boss, p).await[0]["canRemove"], false);
}

#[sqlx::test]
async fn a_proposal_to_remove_someone_elses_is_refused_up_front(pool: PgPool) {
    let p = project(&pool).await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    person(&pool, "anmol@airtribe.live", "member").await;
    let id = add(&pool, &dhaval, p).await;
    let (proposer, _) = token::mint(&state(&pool), "bot", "anmol@airtribe.live", vec!["read".into(), "propose".into()], 30)
        .await
        .unwrap();

    let (status, json) = remove(&pool, &proposer, &id).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE state = 'pending'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pending, 0, "nothing queued that the owner could not honour");

    // The adder's own proposal queues, and approving it removes the link.
    let (own, _) = token::mint(&state(&pool), "bot", "dhaval@airtribe.live", vec!["read".into(), "propose".into()], 30)
        .await
        .unwrap();
    let (status, json) = remove(&pool, &own, &id).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let change = json["data"]["changeId"].as_str().unwrap();
    let (status, json) = send(&pool, "POST", &format!("/api/user/changes/{change}/approve"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(list(&pool, &dhaval, p).await.is_empty());
}

#[sqlx::test]
async fn approving_a_removal_checks_the_proposer_not_the_approver(pool: PgPool) {
    let p = project(&pool).await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (_, anmol_id) = person(&pool, "anmol@airtribe.live", "member").await;
    let id = add(&pool, &dhaval, p).await;
    // Queued before the rule existed: Anmol's bot asking to remove Dhaval's link.
    let change: Uuid = sqlx::query_scalar(
        "INSERT INTO change (actor, on_behalf_of, target_type, target_id, op, patch, state)
         VALUES ('bot', $1, 'artifact', $2, 'delete', jsonb_build_object('id', $2), 'pending') RETURNING id",
    )
    .bind(anmol_id)
    .bind(id.parse::<Uuid>().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();

    // Even Dhaval approving it does not make it Anmol's to remove.
    let (status, json) = send(&pool, "POST", &format!("/api/user/changes/{change}/approve"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(list(&pool, &dhaval, p).await.len(), 1);
    let state: String = sqlx::query_scalar("SELECT state FROM change WHERE id = $1")
        .bind(change)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "pending");
}

#[sqlx::test]
async fn an_agents_attachment_belongs_to_its_owner(pool: PgPool) {
    let (anmol, anmol_id) = person(&pool, "anmol@airtribe.live", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "admin").await;
    let p = project(&pool).await;
    let phase: Uuid = sqlx::query_scalar("INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 0) RETURNING id")
        .bind(p)
        .fetch_one(&pool)
        .await
        .unwrap();
    let t: Uuid = sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, status, assignee_kind, assignee_person_id)
         VALUES ($1, 'Wire checkout', 'in_progress', 'human', $2) RETURNING id",
    )
    .bind(phase)
    .bind(anmol_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let (_, json) = send(&pool, "POST", "/api/user/agents", &anmol, json!({ "handle": "hermes", "name": "Hermes", "runtime": "hermes" })).await;
    let agent = json["data"]["agent"]["id"].as_str().unwrap().to_owned();
    let agent_token = json["data"]["token"].as_str().unwrap().to_owned();
    let (status, json) = send(&pool, "POST", &format!("/api/user/tasks/{t}/handoff"), &anmol, json!({ "agentId": agent })).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (status, json) = send(&pool, "POST", &format!("/api/agent/tasks/{t}/attach"), &agent_token,
        json!({ "kind": "pr", "url": "https://github.com/x/y/pull/1" })).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let id = json["data"]["id"].as_str().unwrap().to_owned();

    let uri = format!("/api/user/artifacts?parentType=task&parentId={t}");
    let row = send(&pool, "GET", &uri, &anmol, Value::Null).await.1["data"][0].clone();
    assert_eq!(row["addedBy"]["name"], "Anmol");
    assert_eq!(row["addedByAgent"], "Hermes");
    assert_eq!(row["canRemove"], true);

    let (status, json) = remove(&pool, &dhaval, &id).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(message(&json), "Only Anmol can remove this PR \u{2014} their agent Hermes attached it.");
    assert_eq!(remove(&pool, &anmol, &id).await.0, StatusCode::OK);
}
