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
    let phase_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 1) RETURNING id")
        .bind(project_id).fetch_one(pool).await.unwrap();
    (raw, phase_id)
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
async fn tasks_are_created_and_filtered_by_status(pool: PgPool) {
    let (token, phase_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    for title in ["wire auth", "write docs"] {
        let response = acp_server::app::app(state.clone())
            .oneshot(req("POST", &format!("/api/user/phases/{phase_id}/tasks"), &token,
                Some(serde_json::json!({ "title": title }))))
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    let response = acp_server::app::app(state.clone())
        .oneshot(req("GET", "/api/user/tasks", &token, None)).await.unwrap();
    assert_eq!(json_of(response).await["data"].as_array().unwrap().len(), 2);

    let response = acp_server::app::app(state.clone())
        .oneshot(req("GET", "/api/user/tasks?status=done", &token, None)).await.unwrap();
    assert_eq!(json_of(response).await["data"].as_array().unwrap().len(), 0);

    let response = acp_server::app::app(state)
        .oneshot(req("GET", &format!("/api/user/tasks?phaseId={phase_id}"), &token, None)).await.unwrap();
    assert_eq!(json_of(response).await["data"].as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn a_task_can_be_assigned_to_a_person_and_to_an_agent(pool: PgPool) {
    let (token, phase_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool.clone() };
    acp_server::controllers::token::mint(&state, "hermes", "anmol@airtribe.live", vec!["claim".into()], 30)
        .await.unwrap();

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/phases/{phase_id}/tasks"), &token,
            Some(serde_json::json!({ "title": "migrate report" })))).await.unwrap();
    let task_id = json_of(response).await["data"]["entity"]["id"].as_str().unwrap().to_string();

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/tasks/{task_id}/assign"), &token,
            Some(serde_json::json!({ "personEmail": "anmol@airtribe.live" })))).await.unwrap();
    let json = json_of(response).await;
    assert_eq!(json["data"]["entity"]["assigneeKind"], "human");
    assert!(json["data"]["entity"]["assigneePersonId"].is_string());

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/tasks/{task_id}/assign"), &token,
            Some(serde_json::json!({ "agentLabel": "hermes" })))).await.unwrap();
    let json = json_of(response).await;
    assert_eq!(json["data"]["entity"]["assigneeKind"], "agent");
    assert!(json["data"]["entity"]["assigneePersonId"].is_null(), "switching to an agent clears the person");
    assert!(json["data"]["entity"]["assigneeTokenId"].is_string());

    let response = acp_server::app::app(state)
        .oneshot(req("GET", "/api/user/tasks?assigneeKind=agent", &token, None)).await.unwrap();
    assert_eq!(json_of(response).await["data"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn assigning_to_an_unknown_person_fails_and_records_nothing(pool: PgPool) {
    let (token, phase_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/phases/{phase_id}/tasks"), &token,
            Some(serde_json::json!({ "title": "x" })))).await.unwrap();
    let task_id = json_of(response).await["data"]["entity"]["id"].as_str().unwrap().to_string();

    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM change").fetch_one(&pool).await.unwrap();

    let response = acp_server::app::app(state)
        .oneshot(req("POST", &format!("/api/user/tasks/{task_id}/assign"), &token,
            Some(serde_json::json!({ "personEmail": "ghost@airtribe.live" })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM change").fetch_one(&pool).await.unwrap();
    assert_eq!(before, after, "a failed assignment must leave no change row");
}

#[sqlx::test]
async fn an_invalid_task_status_is_rejected(pool: PgPool) {
    let (token, phase_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/phases/{phase_id}/tasks"), &token,
            Some(serde_json::json!({ "title": "x" })))).await.unwrap();
    let task_id = json_of(response).await["data"]["entity"]["id"].as_str().unwrap().to_string();

    let response = acp_server::app::app(state)
        .oneshot(req("PATCH", &format!("/api/user/tasks/{task_id}"), &token,
            Some(serde_json::json!({ "status": "nope" })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
