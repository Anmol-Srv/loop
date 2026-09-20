use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn send(state: &acp_server::db::AppState, token: &str, method: &str, params: Value) -> Value {
    let request = Request::builder()
        .method("POST")
        .uri("/api/services/mcp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string(),
        ))
        .unwrap();

    let response = acp_server::app::app(state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn tool_names(response: &Value) -> Vec<String> {
    response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

/// The `claim` scope shows the work tools and nothing editorial, `read` shows
/// neither, and a claim through MCP actually takes the lease.
#[sqlx::test]
async fn claim_scope_sees_work_tools_and_can_lease_a_task(pool: PgPool) {
    let state = acp_server::db::AppState { db: pool.clone() };

    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .execute(&pool)
        .await
        .unwrap();

    let mint = |label: &'static str, scopes: Vec<String>| {
        let state = state.clone();
        async move {
            acp_server::controllers::token::mint(&state, label, "anmol@airtribe.live", scopes, 30)
                .await
                .unwrap()
        }
    };
    let (worker_raw, worker) = mint("hermes", vec!["read".into(), "claim".into()]).await;
    let (reader_raw, _) = mint("watcher", vec!["read".into()]).await;

    let names = tool_names(&send(&state, &worker_raw, "tools/list", json!({})).await);
    for tool in ["work_claim", "work_heartbeat", "work_release", "run_log_append"] {
        assert!(names.contains(&tool.to_string()), "claim token missing {tool}: {names:?}");
    }
    assert!(!names.contains(&"task_create".to_string()), "claim token saw task_create: {names:?}");

    let names = tool_names(&send(&state, &reader_raw, "tools/list", json!({})).await);
    assert!(!names.contains(&"work_claim".to_string()), "read-only token saw {names:?}");

    let project_id: Uuid =
        sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let phase_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 1) RETURNING id",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let task_id: Uuid = sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, assignee_kind, assignee_token_id)
         VALUES ($1, 'wire the reaper', 'agent', $2) RETURNING id",
    )
    .bind(phase_id)
    .bind(worker.id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let response = send(&state, &worker_raw, "tools/call", json!({
        "name": "work_claim", "arguments": {}
    }))
    .await;
    let claimed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(claimed["id"], task_id.to_string());
    assert_eq!(claimed["claimedBy"], "hermes");
    assert_eq!(claimed["status"], "in_progress");
}
