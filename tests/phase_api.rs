use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn setup(pool: &PgPool) -> (String, Uuid) {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live").bind("Anmol").execute(pool).await.unwrap();
    let state = acp_server::db::AppState { db: pool.clone() };
    let (raw, _) = acp_server::controllers::token::mint(
        &state, "test", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30,
    ).await.unwrap();
    let project_id: Uuid = sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
        .fetch_one(pool).await.unwrap();
    (raw, project_id)
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
async fn phases_are_created_listed_in_order_and_updated(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    for (pos, name) in [(2, "Build"), (1, "Spec")] {
        let response = acp_server::app::app(state.clone())
            .oneshot(req("POST", &format!("/api/user/projects/{project_id}/phases"), &token,
                Some(serde_json::json!({ "name": name, "position": pos }))))
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "creating phase {name}");
    }

    let response = acp_server::app::app(state.clone())
        .oneshot(req("GET", &format!("/api/user/projects/{project_id}/phases"), &token, None))
        .await.unwrap();
    let json = json_of(response).await;
    assert_eq!(json["data"][0]["name"], "Spec", "phases list by position");
    assert_eq!(json["data"][1]["name"], "Build");
    assert_eq!(json["data"][0]["status"], "planned");

    let phase_id = json["data"][0]["id"].as_str().unwrap().to_string();
    let response = acp_server::app::app(state)
        .oneshot(req("PATCH", &format!("/api/user/phases/{phase_id}"), &token,
            Some(serde_json::json!({ "status": "active" }))))
        .await.unwrap();
    assert_eq!(json_of(response).await["data"]["status"], "active");

    let changes: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE target_type = 'phase'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(changes, 3, "two creates and one update");
}

#[sqlx::test]
async fn duplicate_positions_in_one_project_are_rejected(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    for _ in 0..2 {
        let _ = acp_server::app::app(state.clone())
            .oneshot(req("POST", &format!("/api/user/projects/{project_id}/phases"), &token,
                Some(serde_json::json!({ "name": "Spec", "position": 1 }))))
            .await.unwrap();
    }

    let response = acp_server::app::app(state)
        .oneshot(req("POST", &format!("/api/user/projects/{project_id}/phases"), &token,
            Some(serde_json::json!({ "name": "Other", "position": 1 }))))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[sqlx::test]
async fn an_invalid_status_is_rejected(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/projects/{project_id}/phases"), &token,
            Some(serde_json::json!({ "name": "Spec", "position": 1 }))))
        .await.unwrap();
    let phase_id = json_of(response).await["data"]["id"].as_str().unwrap().to_string();

    let response = acp_server::app::app(state)
        .oneshot(req("PATCH", &format!("/api/user/phases/{phase_id}"), &token,
            Some(serde_json::json!({ "status": "banana" }))))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
