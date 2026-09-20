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
    let b = Request::builder().method(method).uri(uri)
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
async fn artifacts_attach_to_a_parent_and_list_back(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", "/api/user/artifacts", &token, Some(serde_json::json!({
            "parentType": "project", "parentId": project_id,
            "kind": "pr", "url": "https://github.com/Anmol-Srv/loop/pull/1", "title": "P0"
        })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_of(response).await["data"]["kind"], "pr");

    let response = acp_server::app::app(state)
        .oneshot(req("GET", &format!("/api/user/artifacts?parentType=project&parentId={project_id}"), &token, None))
        .await.unwrap();
    let json = json_of(response).await;
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["title"], "P0");
}

#[sqlx::test]
async fn an_unknown_artifact_kind_is_rejected(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state)
        .oneshot(req("POST", "/api/user/artifacts", &token, Some(serde_json::json!({
            "parentType": "project", "parentId": project_id, "kind": "spreadsheet", "url": "https://x"
        })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn an_unknown_parent_type_is_rejected(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state)
        .oneshot(req("POST", "/api/user/artifacts", &token, Some(serde_json::json!({
            "parentType": "sprint", "parentId": project_id, "kind": "link", "url": "https://x"
        })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
