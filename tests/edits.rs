//! Editing after creation: the flows that make this a tracker rather than a
//! form. Each test is a rule that would quietly corrupt the board if it broke.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn person(pool: &PgPool, email: &str, department: &str) -> (String, Uuid) {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO person (email, name, department) VALUES ($1, $1, $2) RETURNING id",
    )
    .bind(email)
    .bind(department)
    .fetch_one(pool)
    .await
    .unwrap();
    let state = acp_server::db::AppState { db: pool.clone() };
    let (raw, _) = acp_server::controllers::token::mint(
        &state, email, email, vec!["read".into(), "write".into()], 30,
    )
    .await
    .unwrap();
    (raw, id)
}

async fn project(pool: &PgPool) -> (Uuid, Uuid) {
    let project: Uuid = sqlx::query_scalar(
        "INSERT INTO project (key, name, start_date, target_date)
         VALUES ('p', 'P', '2026-09-01', '2026-10-01') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let phase: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Work', 0) RETURNING id",
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .unwrap();
    (project, phase)
}

async fn call(pool: &PgPool, method: &str, uri: &str, token: &str, body: serde_json::Value)
    -> (StatusCode, serde_json::Value)
{
    let state = acp_server::db::AppState { db: pool.clone() };
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = acp_server::app::app(state).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[sqlx::test]
async fn a_project_patch_can_clear_a_date_and_checks_the_result(pool: PgPool) {
    let (token, _) = person(&pool, "a@airtribe.live", "backend").await;
    let (id, _) = project(&pool).await;

    // Moving only the start past the existing target is the case a check on
    // the patch alone would miss.
    let (status, _) = call(&pool, "PATCH", &format!("/api/user/projects/{id}"), &token,
        serde_json::json!({ "startDate": "2026-12-01" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // `null` clears; absent leaves alone.
    let (status, body) = call(&pool, "PATCH", &format!("/api/user/projects/{id}"), &token,
        serde_json::json!({ "targetDate": null, "status": "paused" })).await;
    assert_eq!(status, StatusCode::OK);
    let project = &body["data"]["entity"];
    assert!(project["targetDate"].is_null(), "null cleared the target");
    assert_eq!(project["startDate"], "2026-09-01", "an absent field is left alone");
    assert_eq!(project["status"], "paused");
}

#[sqlx::test]
async fn reassigning_across_tracks_resets_a_status_the_new_track_lacks(pool: PgPool) {
    let (token, _) = person(&pool, "a@airtribe.live", "backend").await;
    let (_, designer) = person(&pool, "d@airtribe.live", "design").await;
    let (_, engineer) = person(&pool, "e@airtribe.live", "frontend").await;
    let (_, phase) = project(&pool).await;

    let task: Uuid = sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, status, assignee_kind, assignee_person_id)
         VALUES ($1, 'screens', 'handoff', 'human', $2) RETURNING id",
    )
    .bind(phase).bind(designer).fetch_one(&pool).await.unwrap();

    let (status, body) = call(&pool, "PATCH", &format!("/api/user/tasks/{task}/details"), &token,
        serde_json::json!({ "assigneeId": engineer })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["entity"]["status"], "open",
        "handoff is not an engineering state; the engineer starts from open");

    let (_, body) = call(&pool, "PATCH", &format!("/api/user/tasks/{task}/details"), &token,
        serde_json::json!({ "assigneeId": null })).await;
    assert!(body["data"]["entity"]["assigneePersonId"].is_null(), "null unassigns");
}

#[sqlx::test]
async fn a_design_handoff_unblocks_the_work_waiting_on_it(pool: PgPool) {
    let (token, designer) = person(&pool, "d@airtribe.live", "design").await;
    let (_, phase) = project(&pool).await;

    let design: Uuid = sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, status, assignee_kind, assignee_person_id)
         VALUES ($1, 'screens', 'in_progress', 'human', $2) RETURNING id",
    )
    .bind(phase).bind(designer).fetch_one(&pool).await.unwrap();
    let build: Uuid = sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, blocked_by) VALUES ($1, 'build it', ARRAY[$2]) RETURNING id",
    )
    .bind(phase).bind(design).fetch_one(&pool).await.unwrap();

    let resolved = |pool: PgPool| async move {
        let (_, body) = call(&pool, "GET", &format!("/api/user/tasks/{build}"), &token_of(&pool).await,
            serde_json::Value::Null).await;
        body["data"]["blockersDone"].as_i64().unwrap()
    };
    assert_eq!(resolved(pool.clone()).await, 0);

    sqlx::query("INSERT INTO artifact (parent_type, parent_id, kind, url) VALUES ('task', $1, 'figma', 'https://f')")
        .bind(design).execute(&pool).await.unwrap();
    let (status, _) = call(&pool, "PATCH", &format!("/api/user/tasks/{design}"), &token,
        serde_json::json!({ "status": "handoff" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resolved(pool).await, 1, "handoff is the moment engineering can start");
}

async fn token_of(pool: &PgPool) -> String {
    let state = acp_server::db::AppState { db: pool.clone() };
    let email: String = sqlx::query_scalar("SELECT email FROM person LIMIT 1")
        .fetch_one(pool).await.unwrap();
    acp_server::controllers::token::mint(&state, "reader", &email, vec!["read".into()], 1)
        .await.unwrap().0
}

#[sqlx::test]
async fn a_removed_link_leaves_an_audit_row(pool: PgPool) {
    let (token, _) = person(&pool, "a@airtribe.live", "backend").await;
    let (project_id, _) = project(&pool).await;
    let artifact: Uuid = sqlx::query_scalar(
        "INSERT INTO artifact (parent_type, parent_id, kind, url) VALUES ('project', $1, 'doc', 'https://d') RETURNING id",
    )
    .bind(project_id).fetch_one(&pool).await.unwrap();

    let (status, _) = call(&pool, "DELETE", &format!("/api/user/artifacts/{artifact}"), &token,
        serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&pool, "DELETE", &format!("/api/user/artifacts/{artifact}"), &token,
        serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a second remove says it is already gone");

    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM change WHERE target_type = 'artifact' AND target_id = $1 AND op = 'delete'",
    )
    .bind(artifact).fetch_one(&pool).await.unwrap();
    assert_eq!(audited, 1, "the link existed, and the trail says so");
}
