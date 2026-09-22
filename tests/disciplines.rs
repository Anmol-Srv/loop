//! Disciplines, human claims, and blocker cycles.
//!
//! Three tests, each for a rule that would be expensive to get wrong: the
//! availability rule (which decides what the whole team sees), the claim race
//! (which decides who owns a piece of work), and the cycle guard (which, if it
//! failed, would deadlock the board permanently).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn person(pool: &PgPool, email: &str, disciplines: &[&str]) -> (String, Uuid) {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO person (email, name, disciplines) VALUES ($1, $1, $2) RETURNING id",
    )
    .bind(email)
    .bind(disciplines.iter().map(|d| d.to_string()).collect::<Vec<_>>())
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

async fn task(pool: &PgPool, phase_id: Uuid, title: &str, discipline: Option<&str>) -> Uuid {
    sqlx::query_scalar("INSERT INTO task (phase_id, title, discipline) VALUES ($1,$2,$3) RETURNING id")
        .bind(phase_id)
        .bind(title)
        .bind(discipline)
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
async fn available_needs_my_discipline_no_assignee_and_finished_blockers(pool: PgPool) {
    let (token, person_id) = person(&pool, "anmol@airtribe.live", &["backend"]).await;
    let phase_id = phase(&pool).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    let blocker = task(&pool, phase_id, "ship the migration", Some("backend")).await;
    let blocked = task(&pool, phase_id, "use the new column", Some("backend")).await;
    sqlx::query("UPDATE task SET blocked_by = ARRAY[$2::uuid] WHERE id = $1")
        .bind(blocked).bind(blocker).execute(&pool).await.unwrap();

    // Wrong discipline, and already assigned: neither should ever appear.
    task(&pool, phase_id, "redraw the icons", Some("design")).await;
    let taken = task(&pool, phase_id, "someone has this", Some("backend")).await;
    sqlx::query("UPDATE task SET assignee_kind='human', assignee_person_id=$2 WHERE id = $1")
        .bind(taken).bind(person_id).execute(&pool).await.unwrap();

    let titles = |json: serde_json::Value| -> Vec<String> {
        json["data"].as_array().unwrap().iter()
            .map(|t| t["title"].as_str().unwrap().to_string())
            .collect()
    };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("GET", "/api/user/tasks/available", &token, None)).await.unwrap();
    let before = titles(json_of(response).await);
    assert_eq!(before, vec!["ship the migration"], "a task with an unfinished blocker is not available");

    // Only the assignee moves a task, so the blocker has to be theirs before
    // they can finish it. An unassigned task cannot be completed by anyone.
    let response = acp_server::app::app(state.clone())
        .oneshot(req("PATCH", &format!("/api/user/tasks/{blocker}"), &token,
            Some(serde_json::json!({ "status": "done" })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN, "nobody owns it, so nobody can finish it");

    sqlx::query("UPDATE task SET assignee_kind='human', assignee_person_id=$2 WHERE id = $1")
        .bind(blocker).bind(person_id).execute(&pool).await.unwrap();
    let response = acp_server::app::app(state.clone())
        .oneshot(req("PATCH", &format!("/api/user/tasks/{blocker}"), &token,
            Some(serde_json::json!({ "status": "done" })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = acp_server::app::app(state)
        .oneshot(req("GET", "/api/user/tasks/available", &token, None)).await.unwrap();
    let after = titles(json_of(response).await);
    assert_eq!(after, vec!["use the new column"],
        "finishing the blocker releases the blocked task, and the finished one leaves the list");
}

#[sqlx::test]
async fn claiming_a_claimed_task_conflicts_and_leaves_the_owner_alone(pool: PgPool) {
    let (mine, my_id) = person(&pool, "anmol@airtribe.live", &["backend"]).await;
    let (theirs, _) = person(&pool, "raj@airtribe.live", &["backend"]).await;
    let phase_id = phase(&pool).await;
    let task_id = task(&pool, phase_id, "wire auth", Some("backend")).await;
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
    let (token, _) = person(&pool, "anmol@airtribe.live", &["backend"]).await;
    let phase_id = phase(&pool).await;
    let a = task(&pool, phase_id, "A", Some("backend")).await;
    let b = task(&pool, phase_id, "B", Some("backend")).await;
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
