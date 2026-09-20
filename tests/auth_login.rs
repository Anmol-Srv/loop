use acp_server::controllers::people;
use acp_server::db::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

const EMAIL: &str = "anmol@airtribe.live";
const PASSWORD: &str = "correct horse battery";

async fn bootstrap(pool: &PgPool) -> (AppState, String) {
    let state = AppState { db: pool.clone() };
    let code = people::bootstrap_admin(&state, EMAIL, "Anmol", false).await.unwrap();
    (state, code)
}

async fn post(state: &AppState, uri: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(state, req).await
}

async fn send(state: &AppState, req: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = acp_server::app::app(state.clone()).oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn me(state: &AppState, token: &str) -> StatusCode {
    let req = Request::builder()
        .uri("/api/user/me")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    send(state, req).await.0
}

async fn login(state: &AppState, password: &str) -> (StatusCode, serde_json::Value) {
    post(state, "/api/auth/login", serde_json::json!({ "email": EMAIL, "password": password })).await
}

#[sqlx::test]
async fn setup_signs_you_in(pool: PgPool) {
    let (state, code) = bootstrap(&pool).await;

    let (status, body) = post(
        &state,
        "/api/auth/setup",
        serde_json::json!({ "email": EMAIL, "code": code, "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let data = &body["data"];
    assert_eq!(data["me"]["role"], "admin");
    assert_eq!(data["me"]["email"], EMAIL);
    assert!(data["expiresAt"].is_string());

    let token = data["token"].as_str().unwrap();
    assert_eq!(me(&state, token).await, StatusCode::OK, "the setup token must work straight away");
}

#[sqlx::test]
async fn right_password_works_wrong_one_does_not(pool: PgPool) {
    let (state, code) = bootstrap(&pool).await;
    people::set_password(&state, EMAIL, &code, PASSWORD).await.unwrap();

    let (status, body) = login(&state, PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me(&state, body["data"]["token"].as_str().unwrap()).await, StatusCode::OK);

    assert_eq!(login(&state, "not the password").await.0, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn an_unknown_address_is_indistinguishable_from_a_wrong_password(pool: PgPool) {
    let (state, code) = bootstrap(&pool).await;
    people::set_password(&state, EMAIL, &code, PASSWORD).await.unwrap();

    let wrong = login(&state, "not the password").await;
    let unknown = post(
        &state,
        "/api/auth/login",
        serde_json::json!({ "email": "nobody@airtribe.live", "password": "not the password" }),
    )
    .await;

    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong, unknown, "a wrong password and an unknown address must be identical");
}

#[sqlx::test]
async fn five_failures_lock_the_account(pool: PgPool) {
    let (state, code) = bootstrap(&pool).await;
    people::set_password(&state, EMAIL, &code, PASSWORD).await.unwrap();

    for _ in 0..5 {
        assert_eq!(login(&state, "not the password").await.0, StatusCode::UNAUTHORIZED);
    }

    let locked_until: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT locked_until FROM person WHERE email = $1")
            .bind(EMAIL)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(locked_until.is_some(), "five failures must lock the account");

    // Even the correct password is refused while the lock holds, and trying
    // must not push the unlock time further out.
    assert_eq!(login(&state, PASSWORD).await.0, StatusCode::UNAUTHORIZED);
    let after: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT locked_until FROM person WHERE email = $1")
            .bind(EMAIL)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(after, locked_until, "an attempt during lockout must not extend it");

    // Let the clock catch up.
    sqlx::query("UPDATE person SET locked_until = now() - interval '1 minute' WHERE email = $1")
        .bind(EMAIL)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(login(&state, PASSWORD).await.0, StatusCode::OK, "the lock must lift");
    let cleared: (i32, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT failed_attempts, locked_until FROM person WHERE email = $1")
            .bind(EMAIL)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(cleared, (0, None), "a successful login clears the counters");
}

#[sqlx::test]
async fn logout_revokes_only_that_session(pool: PgPool) {
    let (state, code) = bootstrap(&pool).await;
    people::set_password(&state, EMAIL, &code, PASSWORD).await.unwrap();

    let first = login(&state, PASSWORD).await.1["data"]["token"].as_str().unwrap().to_string();
    let second = login(&state, PASSWORD).await.1["data"]["token"].as_str().unwrap().to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header("authorization", format!("Bearer {first}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&state, req).await.0, StatusCode::OK);

    assert_eq!(me(&state, &first).await, StatusCode::UNAUTHORIZED);
    assert_eq!(me(&state, &second).await, StatusCode::OK, "logout must not end other sessions");
}

#[sqlx::test]
async fn a_session_slides_only_under_the_threshold(pool: PgPool) {
    let (state, code) = bootstrap(&pool).await;
    people::set_password(&state, EMAIL, &code, PASSWORD).await.unwrap();
    let token = login(&state, PASSWORD).await.1["data"]["token"].as_str().unwrap().to_string();

    async fn set_expiry(pool: &PgPool, days: &str) -> chrono::DateTime<chrono::Utc> {
        sqlx::query_scalar(&format!(
            "UPDATE credential SET expires_at = now() + interval '{days}'
              WHERE kind = 'session' RETURNING expires_at"
        ))
        .fetch_one(pool)
        .await
        .unwrap()
    }
    async fn expiry(pool: &PgPool) -> chrono::DateTime<chrono::Utc> {
        sqlx::query_scalar("SELECT expires_at FROM credential WHERE kind = 'session'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    let short = set_expiry(&pool, "10 days").await;
    assert_eq!(me(&state, &token).await, StatusCode::OK);
    assert!(expiry(&pool).await > short, "a session under 29 days must slide");

    let long = set_expiry(&pool, "30 days").await;
    assert_eq!(me(&state, &token).await, StatusCode::OK);
    assert_eq!(expiry(&pool).await, long, "a session with 30 days left must not be rewritten");
}
