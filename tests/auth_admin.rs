//! Self-serve agents and the admin endpoints. Auth is the exception to the
//! minimal-tests rule (§8): these are the checks that would otherwise be found
//! in production.

use acp_server::controllers::token;
use acp_server::db::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

const ADMIN: &str = "anmol@airtribe.live";
const MEMBER: &str = "ada@airtribe.live";
const OTHER: &str = "grace@airtribe.live";

async fn person(pool: &PgPool, email: &str, role: &str) {
    sqlx::query("INSERT INTO person (email, name, role) VALUES ($1, $2, $3)")
        .bind(email)
        .bind(email.split('@').next().unwrap())
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

/// A session credential for a seeded person, scopes derived from their role.
async fn session(pool: &PgPool, email: &str) -> String {
    let state = AppState { db: pool.clone() };
    token::mint_session(&state, email).await.unwrap().0
}

fn req(method: &str, uri: &str, token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn call(pool: &PgPool, request: Request<Body>) -> (StatusCode, Value) {
    let response = acp_server::app::app(AppState { db: pool.clone() })
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn mint_agent(pool: &PgPool, token: &str, label: &str, scopes: Value) -> (StatusCode, Value) {
    call(
        pool,
        req(
            "POST",
            "/api/user/agents",
            token,
            json!({ "label": label, "scopes": scopes }),
        ),
    )
    .await
}

#[sqlx::test]
async fn a_member_mints_a_read_only_agent_but_never_a_writing_one(pool: PgPool) {
    person(&pool, MEMBER, "member").await;
    let t = session(&pool, MEMBER).await;

    let (status, json) = mint_agent(&pool, &t, "hermes", json!(["read", "claim", "propose"])).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(!json["data"]["token"].as_str().unwrap().is_empty());
    assert_eq!(json["data"]["agent"]["label"], "hermes");

    let (status, json) = mint_agent(&pool, &t, "greedy", json!(["read", "write"])).await;
    assert!(status.is_client_error(), "got {status}: {json}");
    let message = json["error"]["message"].as_str().unwrap();
    assert!(message.contains("write") && message.contains("propose"), "got: {message}");
}

#[sqlx::test]
async fn agents_are_private_to_their_owner(pool: PgPool) {
    person(&pool, MEMBER, "member").await;
    person(&pool, OTHER, "member").await;
    let mine = session(&pool, MEMBER).await;
    let theirs = session(&pool, OTHER).await;

    let (_, json) = mint_agent(&pool, &theirs, "grace-bot", json!(["read"])).await;
    let id = json["data"]["agent"]["id"].as_str().unwrap().to_string();

    let (status, json) = call(
        &pool,
        req("DELETE", &format!("/api/user/agents/{id}"), &mine, json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a 403 would confirm the id exists: {json}");

    let (status, json) = call(&pool, req("GET", "/api/user/agents", &mine, json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"].as_array().unwrap().len(), 0, "{json}");

    // Still theirs, still live.
    let (_, json) = call(&pool, req("GET", "/api/user/agents", &theirs, json!({}))).await;
    assert_eq!(json["data"][0]["label"], "grace-bot");
}

#[sqlx::test]
async fn a_member_cannot_reach_the_admin_routes(pool: PgPool) {
    person(&pool, MEMBER, "member").await;
    let t = session(&pool, MEMBER).await;

    for (method, uri) in [
        ("POST", "/api/admin/invite"),
        ("POST", "/api/admin/role"),
        ("POST", "/api/admin/revoke"),
        ("GET", "/api/admin/sessions"),
    ] {
        let body = json!({ "email": MEMBER, "role": "admin" });
        let (status, json) = call(&pool, req(method, uri, &t, body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {json}");
    }
}

#[sqlx::test]
async fn the_last_admin_cannot_remove_themselves(pool: PgPool) {
    person(&pool, ADMIN, "admin").await;
    let t = session(&pool, ADMIN).await;

    let (status, json) = call(
        &pool,
        req("POST", "/api/admin/role", &t, json!({ "email": ADMIN, "role": "member" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert!(json["error"]["message"].as_str().unwrap().contains("only admin"));

    let (status, json) = call(
        &pool,
        req("POST", "/api/admin/revoke", &t, json!({ "email": ADMIN })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");

    // With a second admin in place, stepping down is allowed.
    person(&pool, OTHER, "admin").await;
    let (status, json) = call(
        &pool,
        req("POST", "/api/admin/role", &t, json!({ "email": ADMIN, "role": "member" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"]["role"], "member");
}

#[sqlx::test]
async fn listing_sessions_never_leaks_a_token_hash(pool: PgPool) {
    person(&pool, ADMIN, "admin").await;
    person(&pool, MEMBER, "member").await;
    let t = session(&pool, ADMIN).await;
    session(&pool, MEMBER).await;

    let (status, json) = call(&pool, req("GET", "/api/admin/sessions", &t, json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"].as_array().unwrap().len(), 2, "{json}");

    let body = json.to_string();
    assert!(!body.contains("tokenHash") && !body.contains("token_hash"), "{body}");
}
