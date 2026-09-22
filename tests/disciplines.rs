//! Departments, human claims, and blocker cycles.
//!
//! A task has no discipline of its own: it takes one from whoever holds it,
//! so the first test is that the derived value follows the assignee and that
//! filtering on it works. The other two are the claim race (who owns a piece
//! of work) and the cycle guard (which, if it failed, would deadlock the
//! board permanently).

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

async fn phase(pool: &PgPool) -> Uuid {
    let project_id: Uuid =
        sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query_scalar("INSERT INTO phase (project_id, name, position) VALUES ($1,'Build',1) RETURNING id")
        .bind(project_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn task(pool: &PgPool, phase_id: Uuid, title: &str, assignee: Option<Uuid>) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, assignee_kind, assignee_person_id)
         VALUES ($1, $2, CASE WHEN $3::uuid IS NULL THEN NULL ELSE 'human' END, $3)
         RETURNING id",
    )
    .bind(phase_id)
    .bind(title)
    .bind(assignee)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn req(method: &str, uri: &str, token: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    match body {
        Some(v) => b.body(Body::from(v.to_string())).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn json_of(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test]
async fn a_tasks_discipline_is_its_assignees_department(pool: PgPool) {
    let (token, backend) = person(&pool, "anmol@airtribe.live", "backend").await;
    let (_, designer) = person(&pool, "evana@airtribe.live", "design").await;
    let phase_id = phase(&pool).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    task(&pool, phase_id, "ship the migration", Some(backend)).await;
    task(&pool, phase_id, "redraw the icons", Some(designer)).await;
    task(&pool, phase_id, "nobody has this", None).await;

    let rows = |json: serde_json::Value| -> Vec<(String, Option<String>)> {
        json["data"].as_array().unwrap().iter()
            .map(|t| {
                (
                    t["title"].as_str().unwrap().to_string(),
                    t["discipline"].as_str().map(str::to_string),
                )
            })
            .collect()
    };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("GET", "/api/user/tasks", &token, None)).await.unwrap();
    let mut all = rows(json_of(response).await);
    all.sort();
    assert_eq!(
        all,
        vec![
            ("nobody has this".to_string(), None),
            ("redraw the icons".to_string(), Some("design".to_string())),
            ("ship the migration".to_string(), Some("backend".to_string())),
        ],
        "the discipline is read off the assignee, and an unassigned task has none"
    );

    let response = acp_server::app::app(state.clone())
        .oneshot(req("GET", "/api/user/tasks?discipline=design", &token, None)).await.unwrap();
    assert_eq!(
        rows(json_of(response).await),
        vec![("redraw the icons".to_string(), Some("design".to_string()))],
        "filtering on discipline filters on the assignee's department"
    );

    // Move the designer to backend and the task moves with them: that is the
    // point of storing the department once, on the person.
    sqlx::query("UPDATE person SET department = 'backend' WHERE id = $1")
        .bind(designer).execute(&pool).await.unwrap();
    let response = acp_server::app::app(state)
        .oneshot(req("GET", "/api/user/tasks?discipline=design", &token, None)).await.unwrap();
    assert!(
        rows(json_of(response).await).is_empty(),
        "nothing is in design once its only designer moves to backend"
    );
}

#[sqlx::test]
async fn claiming_a_claimed_task_conflicts_and_leaves_the_owner_alone(pool: PgPool) {
    let (mine, my_id) = person(&pool, "anmol@airtribe.live", "backend").await;
    let (theirs, _) = person(&pool, "raj@airtribe.live", "backend").await;
    let phase_id = phase(&pool).await;
    let task_id = task(&pool, phase_id, "wire auth", None).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/tasks/{task_id}/claim"), &mine, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = json_of(response).await;
    assert_eq!(json["data"]["entity"]["assigneeKind"], "human");
    assert_eq!(json["data"]["entity"]["assigneePersonId"], my_id.to_string());
    assert!(json["data"]["entity"]["claimedBy"].is_null(), "a human claim is not an agent lease");

    let response = acp_server::app::app(state)
        .oneshot(req("POST", &format!("/api/user/tasks/{task_id}/claim"), &theirs, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let owner: Option<Uuid> = sqlx::query_scalar("SELECT assignee_person_id FROM task WHERE id = $1")
        .bind(task_id).fetch_one(&pool).await.unwrap();
    assert_eq!(owner, Some(my_id), "the losing claim must not move the task");
}

#[sqlx::test]
async fn a_blocker_cycle_is_refused(pool: PgPool) {
    let (token, _) = person(&pool, "anmol@airtribe.live", "backend").await;
    let phase_id = phase(&pool).await;
    let a = task(&pool, phase_id, "A", None).await;
    let b = task(&pool, phase_id, "B", None).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    // B waits on A.
    let response = acp_server::app::app(state.clone())
        .oneshot(req("PATCH", &format!("/api/user/tasks/{b}/blockers"), &token,
            Some(serde_json::json!({ "blockedBy": [a] })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // A waiting on B would close the ring.
    let response = acp_server::app::app(state.clone())
        .oneshot(req("PATCH", &format!("/api/user/tasks/{a}/blockers"), &token,
            Some(serde_json::json!({ "blockedBy": [b] })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // And so would a task waiting on itself.
    let response = acp_server::app::app(state)
        .oneshot(req("PATCH", &format!("/api/user/tasks/{a}/blockers"), &token,
            Some(serde_json::json!({ "blockedBy": [a] })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let still: Vec<Uuid> = sqlx::query_scalar("SELECT blocked_by FROM task WHERE id = $1")
        .bind(a).fetch_one(&pool).await.unwrap();
    assert!(still.is_empty(), "a refused cycle writes nothing");
}
