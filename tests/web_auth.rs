use acp_server::controllers::token;
use acp_server::db::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

/// The session cookie carries a token, not a copy of its authority. Revoking
/// the token in the database must end the browser session on the next request.
#[sqlx::test]
async fn revoking_a_token_ends_its_web_session(pool: PgPool) {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .execute(&pool)
        .await
        .unwrap();

    let state = AppState { db: pool.clone() };
    let (raw, row) = token::mint(&state, "browser", "anmol@airtribe.live", vec!["read".into()], 30)
        .await
        .unwrap();

    let app = || acp_server::app::app(AppState { db: pool.clone() });

    let login = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("token={raw}")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    assert_eq!(login.headers().get("location").unwrap(), "/");

    let cookie = login.headers().get("set-cookie").unwrap().to_str().unwrap().to_string();
    assert!(cookie.contains("acp_session="), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");

    let session = cookie.split(';').next().unwrap().to_string();
    let authed = |app: axum::Router, session: String| async move {
        app.oneshot(
            Request::builder()
                .uri("/login")
                .header("cookie", session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    };

    // While the token is live the cookie authenticates, so the login page
    // bounces the caller onward.
    let before = authed(app(), session.clone()).await;
    assert_eq!(before.status(), StatusCode::SEE_OTHER);
    assert_eq!(before.headers().get("location").unwrap(), "/");

    sqlx::query("UPDATE credential SET revoked_at = now() WHERE id = $1")
        .bind(row.id)
        .execute(&pool)
        .await
        .unwrap();

    let after = authed(app(), session).await;
    assert_eq!(after.status(), StatusCode::OK, "a revoked token must not authenticate");
}
