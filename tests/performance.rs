//! The performance pass, held to its promises: every query that got cheaper
//! still returns what it did, and the caching layer only ever skips a body the
//! client already has.

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
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

async fn phase(pool: &PgPool, key: &str, status: &str) -> Uuid {
    let project: Uuid = sqlx::query_scalar(
        "INSERT INTO project (key, name, status) VALUES ($1, $1, $2) RETURNING id",
    )
    .bind(key)
    .bind(status)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Work', 0) RETURNING id",
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// A task in a given state; `done` stamps `done_at`, as a finishing transition would.
async fn task(
    pool: &PgPool,
    phase: Uuid,
    status: &str,
    done: bool,
    assignee: Option<Uuid>,
    blocked_by: &[Uuid],
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, status, done_at, assignee_kind, assignee_person_id, blocked_by)
         VALUES ($1, 't', $2, CASE WHEN $3 THEN now() END,
                 CASE WHEN $4::uuid IS NULL THEN NULL ELSE 'human' END, $4, $5)
         RETURNING id",
    )
    .bind(phase)
    .bind(status)
    .bind(done)
    .bind(assignee)
    .bind(blocked_by)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn send(pool: &PgPool, method: &str, uri: &str, token: &str, etag: Option<&str>) -> Response<Body> {
    let state = acp_server::db::AppState { db: pool.clone() };
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    if let Some(tag) = etag {
        request = request.header("if-none-match", tag);
    }
    let body = if method == "GET" { Body::empty() } else { Body::from("{}") };
    acp_server::app::app(state).oneshot(request.body(body).unwrap()).await.unwrap()
}

async fn json(pool: &PgPool, uri: &str, token: &str) -> serde_json::Value {
    let response = send(pool, "GET", uri, token, None).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test]
async fn team_capacity_counts_every_kind_of_blocker_correctly(pool: PgPool) {
    let (_, a) = person(&pool, "a@airtribe.live", "backend").await;
    let (_, b) = person(&pool, "b@airtribe.live", "design").await;
    let (_, c) = person(&pool, "c@airtribe.live", "frontend").await;
    let (_, idle) = person(&pool, "idle@airtribe.live", "backend").await;
    let (_, gone) = person(&pool, "gone@airtribe.live", "backend").await;
    let ph = phase(&pool, "p", "active").await;

    // A: three open tasks, only one of which anything live is waiting on.
    let a1 = task(&pool, ph, "open", false, Some(a), &[]).await;
    let a2 = task(&pool, ph, "in_progress", false, Some(a), &[]).await;
    let a3 = task(&pool, ph, "blocked", false, Some(a), &[]).await;
    task(&pool, ph, "open", false, None, &[a1]).await;
    task(&pool, ph, "dropped", false, None, &[a2]).await; // a dropped waiter waits on nothing
    task(&pool, ph, "shipped", true, None, &[a3]).await; // a finished waiter neither

    // B: a handoff is a resolved blocker even with someone waiting on it.
    let b1 = task(&pool, ph, "handoff", false, Some(b), &[]).await;
    task(&pool, ph, "open", false, None, &[b1]).await;
    task(&pool, ph, "completed", false, Some(b), &[]).await;
    task(&pool, ph, "shipped", true, Some(b), &[]).await;

    // C: two waiters on one task is still one blocking task; a waiter that is
    // itself someone's open work counts for its own owner too.
    let c1 = task(&pool, ph, "open", false, Some(c), &[]).await;
    task(&pool, ph, "open", false, Some(a), &[c1]).await;
    task(&pool, ph, "blocked", false, None, &[c1, a1]).await;
    task(&pool, ph, "dropped", false, Some(c), &[]).await;

    task(&pool, ph, "open", false, Some(gone), &[]).await;
    sqlx::query("UPDATE person SET deleted_at = now() WHERE id = $1")
        .bind(gone).execute(&pool).await.unwrap();

    let state = acp_server::db::AppState { db: pool.clone() };
    let team = acp_server::controllers::home::team_capacity(&state).await.unwrap();
    let counts = |id: Uuid| {
        let row = team.iter().find(|r| r.person_id == id).expect("person present");
        (row.open, row.review, row.blocking)
    };
    assert_eq!(counts(a), (4, 0, 1));
    assert_eq!(counts(b), (0, 2, 0));
    assert_eq!(counts(c), (1, 0, 1));
    assert_eq!(counts(idle), (0, 0, 0));
    assert!(team.iter().all(|r| r.person_id != gone), "the deleted are not on the team");
}

#[sqlx::test]
async fn counts_are_my_live_tasks_and_active_projects(pool: PgPool) {
    let (token, me) = person(&pool, "a@airtribe.live", "backend").await;
    let (_, other) = person(&pool, "b@airtribe.live", "backend").await;
    let ph = phase(&pool, "p", "active").await;
    phase(&pool, "q", "active").await;
    phase(&pool, "r", "paused").await;

    task(&pool, ph, "open", false, Some(me), &[]).await;
    task(&pool, ph, "completed", false, Some(me), &[]).await; // unshipped is still mine
    task(&pool, ph, "shipped", true, Some(me), &[]).await;
    task(&pool, ph, "dropped", false, Some(me), &[]).await;
    task(&pool, ph, "open", false, Some(other), &[]).await;
    task(&pool, ph, "triage", false, Some(me), &[]).await; // counted apart, not as open

    let body = json(&pool, "/api/user/counts", &token).await;
    assert_eq!(body["data"], serde_json::json!({ "myOpen": 2, "activeProjects": 2, "triage": 1 }));
}

#[sqlx::test]
async fn the_project_list_carries_the_same_progress_as_flow(pool: PgPool) {
    let (token, me) = person(&pool, "a@airtribe.live", "backend").await;
    let ph = phase(&pool, "p", "active").await;
    phase(&pool, "empty", "active").await;
    task(&pool, ph, "open", false, Some(me), &[]).await;
    task(&pool, ph, "shipped", true, Some(me), &[]).await;
    task(&pool, ph, "completed", true, None, &[]).await;
    task(&pool, ph, "dropped", false, None, &[]).await;

    let list = json(&pool, "/api/user/projects", &token).await;
    let rows = list["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let flow = json(&pool, &format!("/api/user/projects/{}/flow", row["id"].as_str().unwrap()), &token).await;
        assert_eq!(row["done"], flow["data"]["done"], "{}", row["key"]);
        assert_eq!(row["total"], flow["data"]["total"], "{}", row["key"]);
    }
    let p = rows.iter().find(|r| r["key"] == "p").unwrap();
    assert_eq!((p["done"].as_i64(), p["total"].as_i64()), (Some(2), Some(3)));
}

#[sqlx::test]
async fn a_repeat_get_with_its_etag_is_a_bodiless_304(pool: PgPool) {
    let (token, _) = person(&pool, "a@airtribe.live", "backend").await;
    phase(&pool, "p", "active").await;

    let first = send(&pool, "GET", "/api/user/projects", &token, None).await;
    assert_eq!(first.status(), StatusCode::OK);
    let tag = first.headers()["etag"].to_str().unwrap().to_owned();
    assert!(tag.starts_with("W/\""), "a weak tag: {tag}");

    let again = send(&pool, "GET", "/api/user/projects", &token, Some(&tag)).await;
    assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(again.headers()["etag"], tag.as_str());
    assert!(again.into_body().collect().await.unwrap().to_bytes().is_empty());

    // A stale tag gets the body.
    let stale = send(&pool, "GET", "/api/user/projects", &token, Some("W/\"nope\"")).await;
    assert_eq!(stale.status(), StatusCode::OK);

    phase(&pool, "q", "active").await;
    let changed = send(&pool, "GET", "/api/user/projects", &token, Some(&tag)).await;
    assert_eq!(changed.status(), StatusCode::OK);
    assert_ne!(changed.headers()["etag"], tag.as_str(), "a new body, a new tag");
    let bytes = changed.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["data"].as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn only_successful_user_gets_are_tagged(pool: PgPool) {
    let (token, _) = person(&pool, "a@airtribe.live", "backend").await;
    let id = Uuid::new_v4();

    let missing = send(&pool, "GET", &format!("/api/user/projects/{id}"), &token, None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(missing.headers().get("etag").is_none());

    let unauthorised = send(&pool, "GET", "/api/user/projects", "bogus", None).await;
    assert_eq!(unauthorised.status(), StatusCode::UNAUTHORIZED);
    assert!(unauthorised.headers().get("etag").is_none());

    let write = send(&pool, "PATCH", &format!("/api/user/projects/{id}"), &token, None).await;
    assert!(write.headers().get("etag").is_none());

    let health = send(&pool, "GET", "/health", &token, None).await;
    assert_eq!(health.status(), StatusCode::OK);
    assert!(health.headers().get("etag").is_none());
}

#[sqlx::test]
async fn a_credential_is_touched_at_most_every_five_minutes(pool: PgPool) {
    let (token, _) = person(&pool, "a@airtribe.live", "backend").await;
    let last_used = || async {
        sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
            "SELECT last_used_at FROM credential",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    };

    acp_server::middleware::auth::resolve(&pool, &token).await.unwrap();
    let first = last_used().await.expect("the first use is recorded");
    acp_server::middleware::auth::resolve(&pool, &token).await.unwrap();
    assert_eq!(last_used().await, Some(first), "a second use a moment later writes nothing");

    sqlx::query("UPDATE credential SET last_used_at = now() - interval '6 minutes'")
        .execute(&pool).await.unwrap();
    acp_server::middleware::auth::resolve(&pool, &token).await.unwrap();
    assert!(last_used().await.unwrap() > first, "an older touch is refreshed");
}
