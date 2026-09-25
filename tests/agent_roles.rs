//! Agent roles: working on tasks (`can_work`) and filing them (`can_intake`).
//! An intake-only agent never takes a hand-off, an agent always has a role,
//! and turning work off never strands a task — each as the server enforces it.

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

async fn person(pool: &PgPool, email: &str) -> (String, Uuid) {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO person (email, name, department) VALUES ($1, initcap(split_part($1, '@', 1)), 'backend')
         RETURNING id",
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .unwrap();
    let (raw, _) = token::mint_session(&state(pool), email).await.unwrap();
    (raw, id)
}

async fn task(pool: &PgPool, assignee: Uuid) -> Uuid {
    sqlx::query_scalar(
        "WITH p AS (INSERT INTO project (key, name, description) VALUES (gen_random_uuid()::text, 'Payments', 'x') RETURNING id),
              ph AS (INSERT INTO phase (project_id, name, position) SELECT id, 'Build', 0 FROM p RETURNING id)
         INSERT INTO task (phase_id, title, status, assignee_kind, assignee_person_id)
         SELECT id, 'Wire checkout', 'open', 'human', $1 FROM ph RETURNING id",
    )
    .bind(assignee)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn send(
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
        .body(Body::from(body.to_string()))
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

fn message(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

async fn create(pool: &PgPool, owner: &str, body: Value) -> (StatusCode, Value) {
    send(pool, "POST", "/api/user/agents", owner, body).await
}

async fn agent(pool: &PgPool, owner: &str, body: Value) -> Value {
    let (status, v) = create(pool, owner, body).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v["data"]["agent"].clone()
}

#[sqlx::test]
async fn an_intake_only_agent_is_refused_a_hand_off(pool: PgPool) {
    let (owner, owner_id) = person(&pool, "anmol@airtribe.live").await;
    let slack = agent(&pool, &owner,
        json!({ "handle": "slacker", "name": "Slack Agent", "runtime": "other", "canWork": false, "canIntake": true })).await;
    assert_eq!(slack["canWork"], false);
    assert_eq!(slack["canIntake"], true);
    let worker = agent(
        &pool,
        &owner,
        json!({ "handle": "claude", "name": "Airtribe", "runtime": "claude-code" }),
    )
    .await;
    assert_eq!(worker["canWork"], true, "working on tasks is the default");
    assert_eq!(worker["canIntake"], false);

    let t = task(&pool, owner_id).await;
    let uri = format!("/api/user/tasks/{t}/handoff");
    let (status, v) = send(
        &pool,
        "POST",
        &uri,
        &owner,
        json!({ "agentId": slack["id"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(
        message(&v),
        "Slack Agent only files tasks; it doesn't take hand-offs. Hand this to an agent that works on tasks."
    );
    let held: Option<Uuid> = sqlx::query_scalar("SELECT delegate_agent_id FROM task WHERE id = $1")
        .bind(t)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        held, None,
        "a refused hand-off leaves the task with its person"
    );

    let (status, v) = send(
        &pool,
        "POST",
        &uri,
        &owner,
        json!({ "agentId": worker["id"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
}

#[sqlx::test]
async fn an_agent_needs_a_role_and_only_its_owner_changes_them(pool: PgPool) {
    let (owner, owner_id) = person(&pool, "anmol@airtribe.live").await;
    let (other, _) = person(&pool, "dhaval@airtribe.live").await;

    let (status, v) = create(
        &pool,
        &owner,
        json!({ "handle": "idle", "runtime": "other", "canWork": false }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(message(&v).contains("at least one role"), "{v}");

    let a = agent(
        &pool,
        &owner,
        json!({ "handle": "hermes", "runtime": "hermes" }),
    )
    .await;
    let uri = format!("/api/user/agents/{}", a["id"].as_str().unwrap());
    let (status, _) = send(
        &pool,
        "PATCH",
        &uri,
        &other,
        json!({ "canWork": false, "canIntake": true }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "someone else's agent does not exist for them"
    );

    let (status, v) = send(&pool, "PATCH", &uri, &owner, json!({ "canWork": false })).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "work off with intake off leaves no role: {v}"
    );
    assert!(message(&v).contains("at least one role"), "{v}");

    let (status, v) = send(&pool, "PATCH", &uri, &owner, json!({ "canIntake": true })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (v["data"]["canWork"].clone(), v["data"]["canIntake"].clone()),
        (json!(true), json!(true))
    );

    // Work off while it holds a task: refused, naming the task.
    let t = task(&pool, owner_id).await;
    let (status, _) = send(
        &pool,
        "POST",
        &format!("/api/user/tasks/{t}/handoff"),
        &owner,
        json!({ "agentId": a["id"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, v) = send(&pool, "PATCH", &uri, &owner, json!({ "canWork": false })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert!(message(&v).contains("Wire checkout"), "{v}");

    // Taken back, it can stop working on tasks.
    let (status, _) = send(
        &pool,
        "POST",
        &format!("/api/user/tasks/{t}/takeback"),
        &owner,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, v) = send(&pool, "PATCH", &uri, &owner, json!({ "canWork": false })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (v["data"]["canWork"].clone(), v["data"]["canIntake"].clone()),
        (json!(false), json!(true))
    );
}

#[sqlx::test]
async fn the_migration_makes_intake_agents_intake_only(pool: PgPool) {
    let (_, owner_id) = person(&pool, "anmol@airtribe.live").await;
    // As before the migration: no can_work column, one intake agent and one not.
    sqlx::query("ALTER TABLE agent DROP COLUMN can_work")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO agent (owner_id, handle, name, runtime, can_intake)
         VALUES ($1, 'slacker', 'Slack Agent', 'other', true), ($1, 'hermes', 'Hermes', 'hermes', false)",
    )
    .bind(owner_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/20260925000021_agent_can_work.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();

    let rows: Vec<(String, bool)> =
        sqlx::query_as("SELECT handle, can_work FROM agent ORDER BY handle")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![("hermes".to_owned(), true), ("slacker".to_owned(), false)]
    );
}
