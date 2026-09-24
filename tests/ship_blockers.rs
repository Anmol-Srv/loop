//! The rules the ship-readiness review found missing, one test per rule, each
//! with the case that used to get through. If one of these fails, a person on
//! a shared board can skip a check or undo someone else's work again.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use acp_server::controllers::{people, token};
use acp_server::db::AppState;
use acp_server::errors::AppError;

fn state(pool: &PgPool) -> AppState {
    AppState { db: pool.clone() }
}

/// A person with a real session — the kind the app holds — so the credential
/// kind on `Caller` is `session`, not whatever a test helper would mint.
async fn person(pool: &PgPool, email: &str, department: &str, role: &str) -> (String, Uuid) {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO person (email, name, department, role) VALUES ($1, $1, $2, $3) RETURNING id",
    )
    .bind(email)
    .bind(department)
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    let (raw, _) = token::mint_session(&state(pool), email).await.unwrap();
    (raw, id)
}

async fn phase(pool: &PgPool) -> (Uuid, Uuid) {
    let project: Uuid = sqlx::query_scalar(
        "INSERT INTO project (key, name) VALUES ('p', 'P') RETURNING id",
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

async fn task(pool: &PgPool, phase: Uuid, status: &str, assignee: Option<Uuid>) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, status, assignee_kind, assignee_person_id)
         VALUES ($1, 't', $2, CASE WHEN $3::uuid IS NULL THEN NULL ELSE 'human' END, $3)
         RETURNING id",
    )
    .bind(phase)
    .bind(status)
    .bind(assignee)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn call(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(if body.is_null() { Body::empty() } else { Body::from(body.to_string()) })
        .unwrap();
    let response = acp_server::app::app(state(pool)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

async fn status_of(pool: &PgPool, id: Uuid) -> (String, bool) {
    sqlx::query_as("SELECT status, done_at IS NOT NULL FROM task WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

// ---- H1: transitions follow the track ------------------------------------

#[sqlx::test]
async fn the_tracks_route_serves_the_table_set_status_enforces(pool: PgPool) {
    let (t, _) = person(&pool, "a@airtribe.live", "backend", "member").await;
    let (status, body) = call(&pool, "GET", "/api/user/tracks", &t, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    let table = &body["data"];
    assert_eq!(*table, acp_server::models::task::tracks_table(), "generated, not written out");
    assert_eq!(table["eng"]["open"], json!(["in_progress", "blocked", "dropped"]));
    assert_eq!(table["eng"]["completed"], json!(["shipped", "in_progress", "blocked", "dropped"]));
    assert_eq!(table["design"]["in_progress"], json!(["handoff", "open", "blocked", "dropped"]));
    assert_eq!(table["eng"]["dropped"], json!(["open"]));
    assert_eq!(table["evidence"]["eng"]["completed"], json!(["pr", "commit"]));
    assert_eq!(table["evidence"]["design"]["handoff"], json!(["figma"]));
    assert_eq!(table["anyone"], json!(["shipped"]));
    assert!(table["design"].get("shipped").is_none(), "design has no shipped state");
}

/// The review's finding, as it happened: Dhaval shipped Anmol's open task.
#[sqlx::test]
async fn nobody_ships_an_open_task(pool: PgPool) {
    let (_, anmol) = person(&pool, "anmol@airtribe.live", "backend", "member").await;
    let (dhaval, _) = person(&pool, "dhaval@airtribe.live", "backend", "member").await;
    let (_, ph) = phase(&pool).await;
    let id = task(&pool, ph, "open", Some(anmol)).await;

    let (status, body) =
        call(&pool, "PATCH", &format!("/api/user/tasks/{id}"), &dhaval, json!({ "status": "shipped" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"]["message"].as_str().unwrap().contains("in_progress"), "names the legal moves");
    assert_eq!(status_of(&pool, id).await, ("open".into(), false), "nothing stamped");

    // From `completed` anyone may still ship it.
    sqlx::query("UPDATE task SET status = 'completed' WHERE id = $1").bind(id).execute(&pool).await.unwrap();
    let (status, _) =
        call(&pool, "PATCH", &format!("/api/user/tasks/{id}"), &dhaval, json!({ "status": "shipped" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_of(&pool, id).await, ("shipped".into(), true));
}

#[sqlx::test]
async fn finishing_moves_only_come_from_in_progress(pool: PgPool) {
    let (eng_t, eng) = person(&pool, "e@airtribe.live", "backend", "member").await;
    let (des_t, des) = person(&pool, "d@airtribe.live", "design", "member").await;
    let (_, ph) = phase(&pool).await;

    // Even with the evidence in hand, the order is the rule.
    let e = task(&pool, ph, "open", Some(eng)).await;
    let (status, _) = call(&pool, "PATCH", &format!("/api/user/tasks/{e}"), &eng_t,
        json!({ "status": "completed", "manualReason": "done in the console" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "completed only from in_progress");

    let d = task(&pool, ph, "open", Some(des)).await;
    sqlx::query("INSERT INTO artifact (parent_type, parent_id, kind, url) VALUES ('task', $1, 'figma', 'https://f')")
        .bind(d).execute(&pool).await.unwrap();
    let (status, _) =
        call(&pool, "PATCH", &format!("/api/user/tasks/{d}"), &des_t, json!({ "status": "handoff" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "handoff only from in_progress");

    // A dropped task comes back through `open`, not straight into work.
    let x = task(&pool, ph, "dropped", Some(eng)).await;
    let (status, _) =
        call(&pool, "PATCH", &format!("/api/user/tasks/{x}"), &eng_t, json!({ "status": "in_progress" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) =
        call(&pool, "PATCH", &format!("/api/user/tasks/{x}"), &eng_t, json!({ "status": "open" })).await;
    assert_eq!(status, StatusCode::OK);
}

// ---- H2: a move states what it moved from ---------------------------------

#[sqlx::test]
async fn a_stale_move_is_a_409_and_changes_nothing(pool: PgPool) {
    let (t, me) = person(&pool, "chinmay@airtribe.live", "backend", "member").await;
    let (_, ph) = phase(&pool).await;
    let id = task(&pool, ph, "in_progress", Some(me)).await;

    let (status, _) = call(&pool, "PATCH", &format!("/api/user/tasks/{id}"), &t,
        json!({ "status": "completed", "manualReason": "console", "expectedStatus": "in_progress" })).await;
    assert_eq!(status, StatusCode::OK);

    // The same screen, not refreshed, tries to send it back.
    let (status, body) = call(&pool, "PATCH", &format!("/api/user/tasks/{id}"), &t,
        json!({ "status": "open", "expectedStatus": "in_progress" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "CONFLICT");
    assert!(body["error"]["message"].as_str().unwrap().contains("completed"), "{body}");
    assert_eq!(status_of(&pool, id).await.0, "completed");

    // Without the precondition the old behaviour stands: the move applies.
    let (status, _) =
        call(&pool, "PATCH", &format!("/api/user/tasks/{id}"), &t, json!({ "status": "in_progress" })).await;
    assert_eq!(status, StatusCode::OK);
}

// ---- H3: edits state what they edited --------------------------------------

#[sqlx::test]
async fn a_stale_task_edit_is_a_409_and_keeps_the_other_edit(pool: PgPool) {
    let (t, _) = person(&pool, "a@airtribe.live", "backend", "member").await;
    let (_, ph) = phase(&pool).await;
    let id = task(&pool, ph, "open", None).await;

    let (_, body) = call(&pool, "GET", &format!("/api/user/tasks/{id}"), &t, Value::Null).await;
    let seen = body["data"]["updatedAt"].clone();

    let (status, body) = call(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &t,
        json!({ "body": "theirs", "expectedUpdatedAt": seen })).await;
    assert_eq!(status, StatusCode::OK);
    let fresh = body["data"]["entity"]["updatedAt"].clone();

    let (status, body) = call(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &t,
        json!({ "title": "mine", "body": "stale draft", "expectedUpdatedAt": seen })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (title, text): (String, String) = sqlx::query_as("SELECT title, body FROM task WHERE id = $1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!((title.as_str(), text.as_str()), ("t", "theirs"), "the stale save wrote nothing");

    let (status, _) = call(&pool, "PATCH", &format!("/api/user/tasks/{id}/details"), &t,
        json!({ "title": "mine", "expectedUpdatedAt": fresh })).await;
    assert_eq!(status, StatusCode::OK, "reapplied against the fresh version");
}

#[sqlx::test]
async fn a_stale_project_edit_is_a_409(pool: PgPool) {
    let (t, _) = person(&pool, "a@airtribe.live", "backend", "member").await;
    let (id, _) = phase(&pool).await;

    let (_, body) = call(&pool, "GET", &format!("/api/user/projects/{id}"), &t, Value::Null).await;
    let seen = body["data"]["updatedAt"].clone();

    let (status, _) = call(&pool, "PATCH", &format!("/api/user/projects/{id}"), &t,
        json!({ "description": "theirs", "expectedUpdatedAt": seen })).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(&pool, "PATCH", &format!("/api/user/projects/{id}"), &t,
        json!({ "name": "Mine", "expectedUpdatedAt": seen })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let name: String = sqlx::query_scalar("SELECT name FROM project WHERE id = $1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(name, "P");
}

// ---- H4, H5: accounts -------------------------------------------------------

#[sqlx::test]
async fn setting_a_password_by_either_path_ends_existing_sessions(pool: PgPool) {
    let s = state(&pool);
    let (old, _) = person(&pool, "a@airtribe.live", "backend", "member").await;
    let (other, _) = person(&pool, "b@airtribe.live", "backend", "member").await;
    assert_eq!(call(&pool, "GET", "/api/user/me", &old, Value::Null).await.0, StatusCode::OK);

    // Setup code: the re-invite after a leak.
    let code = people::invite(&s, "a@airtribe.live").await.unwrap();
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/setup")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "email": "a@airtribe.live", "code": code, "password": "a long new password" }).to_string(),
        ))
        .unwrap();
    let response = acp_server::app::app(s.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let fresh = serde_json::from_slice::<Value>(&bytes).unwrap()["data"]["token"].as_str().unwrap().to_owned();

    assert_eq!(call(&pool, "GET", "/api/user/me", &old, Value::Null).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&pool, "GET", "/api/user/me", &fresh, Value::Null).await.0, StatusCode::OK);
    assert_eq!(call(&pool, "GET", "/api/user/me", &other, Value::Null).await.0, StatusCode::OK,
        "only that person's sessions end");

    // acp-admin set-password.
    people::set_password_directly(&s, "a@airtribe.live", "another long password").await.unwrap();
    assert_eq!(call(&pool, "GET", "/api/user/me", &fresh, Value::Null).await.0, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn a_session_ends_ninety_days_after_sign_in_however_recently_used(pool: PgPool) {
    let (t, _) = person(&pool, "a@airtribe.live", "backend", "member").await;
    let age = |days: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "UPDATE credential SET created_at = now() - ($1 || ' days')::interval,
                                       expires_at = now() + interval '20 days'",
            )
            .bind(days.to_string())
            .execute(&pool)
            .await
            .unwrap();
        }
    };

    age(89).await;
    assert_eq!(call(&pool, "GET", "/api/user/me", &t, Value::Null).await.0, StatusCode::OK);
    let expires: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT expires_at FROM credential").fetch_one(&pool).await.unwrap();
    assert!(expires < chrono::Utc::now() + chrono::Duration::days(2), "the slide stops at the cap");

    age(91).await;
    assert_eq!(call(&pool, "GET", "/api/user/me", &t, Value::Null).await.0, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn set_password_refuses_under_twelve_characters(pool: PgPool) {
    let s = state(&pool);
    person(&pool, "a@airtribe.live", "backend", "member").await;

    let short = people::set_password_directly(&s, "a@airtribe.live", "abc").await;
    assert!(matches!(short, Err(AppError::BadRequest(_))), "{short:?}");
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM person")
        .fetch_one(&pool).await.unwrap();
    assert!(hash.is_none(), "nothing was set");

    people::set_password_directly(&s, "a@airtribe.live", "twelve chars").await.unwrap();
}

// ---- H7: departments and roles are admin actions ---------------------------

#[sqlx::test]
async fn a_person_cannot_change_their_own_department(pool: PgPool) {
    let (t, id) = person(&pool, "chinmay@airtribe.live", "backend", "member").await;
    let (status, body) =
        call(&pool, "PATCH", "/api/user/people/me", &t, json!({ "department": "design" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let dept: String = sqlx::query_scalar("SELECT department FROM person WHERE id = $1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(dept, "backend");
}

#[sqlx::test]
async fn an_admin_sets_department_and_role_and_a_member_cannot(pool: PgPool) {
    let (admin, _) = person(&pool, "anmol@airtribe.live", "backend", "admin").await;
    let (member, id) = person(&pool, "dhaval@airtribe.live", "backend", "member").await;
    let uri = format!("/api/admin/people/{id}");

    let (status, _) = call(&pool, "PATCH", &uri, &member, json!({ "role": "admin" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = call(&pool, "PATCH", &uri, &admin, json!({ "role": "boss" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&pool, "PATCH", &uri, &admin, json!({ "department": "sales" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&pool, "PATCH", &uri, &admin, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) =
        call(&pool, "PATCH", &uri, &admin, json!({ "department": "frontend", "role": "manager" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["department"], "frontend");
    assert_eq!(body["data"]["role"], "manager");
    assert_eq!(call(&pool, "GET", "/api/user/me", &member, Value::Null).await.0, StatusCode::UNAUTHORIZED,
        "a role change ends the sessions minted under the old one");

    // The older email-keyed route takes `manager` too.
    let (status, body) = call(&pool, "POST", "/api/admin/role", &admin,
        json!({ "email": "dhaval@airtribe.live", "role": "manager" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[sqlx::test]
async fn a_department_change_resets_in_flight_tasks_like_reassignment(pool: PgPool) {
    let (admin, _) = person(&pool, "anmol@airtribe.live", "backend", "admin").await;
    let (_, evana) = person(&pool, "evana@airtribe.live", "design", "member").await;
    let (_, ph) = phase(&pool).await;
    let handoff = task(&pool, ph, "handoff", Some(evana)).await;
    let working = task(&pool, ph, "in_progress", Some(evana)).await;
    let finished = task(&pool, ph, "completed", Some(evana)).await;
    sqlx::query("UPDATE task SET done_at = now() WHERE id = $1").bind(finished).execute(&pool).await.unwrap();

    let (status, _) = call(&pool, "PATCH", &format!("/api/admin/people/{evana}"), &admin,
        json!({ "department": "backend" })).await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(status_of(&pool, handoff).await, ("open".into(), false), "engineering has no handoff");
    assert_eq!(status_of(&pool, working).await, ("in_progress".into(), false), "a shared state stays");
    assert_eq!(status_of(&pool, finished).await, ("completed".into(), true), "finished work is history");

    // Back to design through the CLI's path: an engineering `completed` is
    // design's finish line, so `done_at` is recomputed and stamped.
    sqlx::query("UPDATE task SET status = 'completed' WHERE id = $1").bind(working).execute(&pool).await.unwrap();
    let actor = acp_server::models::change::Actor { label: "acp-admin".into(), person_id: None, can_apply: true };
    let s = state(&pool);
    people::set_department(&s, &actor, evana, "design").await.unwrap();
    assert_eq!(status_of(&pool, working).await, ("completed".into(), true));
}

#[sqlx::test]
async fn set_role_takes_manager_and_keeps_one_admin(pool: PgPool) {
    let s = state(&pool);
    let (_, admin) = person(&pool, "anmol@airtribe.live", "backend", "admin").await;
    let (_, member) = person(&pool, "dhaval@airtribe.live", "backend", "member").await;

    let (p, _) = people::set_role(&s, member, "manager").await.unwrap();
    assert_eq!(p.role, "manager");
    assert!(matches!(people::set_role(&s, member, "boss").await, Err(AppError::BadRequest(_))));
    assert!(matches!(people::set_role(&s, admin, "member").await, Err(AppError::Conflict(_))),
        "the last admin cannot be demoted");
}

// ---- Mediums ---------------------------------------------------------------

async fn mint(pool: &PgPool, token: &str, handle: &str) -> (StatusCode, Value) {
    call(pool, "POST", "/api/user/agents", token, json!({ "handle": handle })).await
}

#[sqlx::test]
async fn an_agent_cannot_mint_an_agent(pool: PgPool) {
    let (t, _) = person(&pool, "a@airtribe.live", "backend", "member").await;
    let (status, body) = mint(&pool, &t, "bot").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let agent = body["data"]["token"].as_str().unwrap().to_owned();

    let (status, body) = mint(&pool, &agent, "bot-2").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[sqlx::test]
async fn an_agents_token_lives_ninety_days(pool: PgPool) {
    let (t, _) = person(&pool, "a@airtribe.live", "backend", "member").await;
    let now = chrono::Utc::now();
    mint(&pool, &t, "bot").await;
    let expiry: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT expires_at FROM credential WHERE kind = 'agent'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let ninety = now + chrono::Duration::days(90);
    assert!(expiry > ninety - chrono::Duration::minutes(1) && expiry < ninety + chrono::Duration::minutes(1));
}

#[sqlx::test]
async fn artifact_links_must_be_web_links_except_commits(pool: PgPool) {
    let (t, _) = person(&pool, "a@airtribe.live", "backend", "member").await;
    let (_, ph) = phase(&pool).await;
    let id = task(&pool, ph, "open", None).await;
    let add = |kind: &'static str, url: &'static str| {
        let (pool, t) = (pool.clone(), t.clone());
        async move {
            call(&pool, "POST", "/api/user/artifacts", &t,
                json!({ "parentType": "task", "parentId": id, "kind": kind, "url": url })).await.0
        }
    };

    assert_eq!(add("link", "file:///etc/passwd").await, StatusCode::BAD_REQUEST);
    assert_eq!(add("pr", "javascript:alert(1)").await, StatusCode::BAD_REQUEST);
    assert_eq!(add("figma", "HTTPS://figma.com/f/x").await, StatusCode::OK);
    assert_eq!(add("doc", "http://intranet/doc").await, StatusCode::OK);
    assert_eq!(add("commit", "3f9a2c1").await, StatusCode::OK, "a commit holds a hash");
}

#[sqlx::test]
async fn add_person_normalises_and_checks_the_domain(pool: PgPool) {
    let s = state(&pool);
    assert!(people::add_person(&s, "  Pratik@Airtribe.LIVE ", "").await.unwrap());
    assert!(!people::add_person(&s, "pratik@airtribe.live", "Pratik").await.unwrap(), "exists already");
    let (email, name): (String, String) = sqlx::query_as("SELECT email, name FROM person")
        .fetch_one(&pool).await.unwrap();
    assert_eq!((email.as_str(), name.as_str()), ("pratik@airtribe.live", "pratik"));

    assert!(matches!(people::add_person(&s, "x@gmail.com", "X").await, Err(AppError::BadRequest(_))));
}
