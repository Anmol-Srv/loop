//! Archiving and deleting projects and tasks: who may, what disappears from
//! which list, what a delete takes with it, and the agent that has to be
//! asked first.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use acp_server::controllers::token;
use acp_server::db::AppState;

fn state(pool: &PgPool) -> AppState {
    AppState { db: pool.clone() }
}

/// A person with a session: (token, id).
async fn person(pool: &PgPool, email: &str, role: &str) -> (String, Uuid) {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO person (email, name, role, department)
         VALUES ($1, initcap(split_part($1, '@', 1)), $2, 'backend') RETURNING id",
    )
    .bind(email)
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    let (raw, _) = token::mint_session(&state(pool), email).await.unwrap();
    (raw, id)
}

async fn send(pool: &PgPool, method: &str, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("host", "acp.test")
        .header("authorization", format!("Bearer {token}"))
        .body(if body.is_null() { Body::empty() } else { Body::from(body.to_string()) })
        .unwrap();
    let response = acp_server::app::app(state(pool)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

async fn get(pool: &PgPool, token: &str, uri: &str) -> Value {
    let (status, json) = send(pool, "GET", uri, token, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {json}");
    json["data"].clone()
}

fn message(json: &Value) -> &str {
    json["error"]["message"].as_str().unwrap_or_default()
}

/// A project made by `token` with one task assigned to `assignee`: (project, task).
async fn project(pool: &PgPool, token: &str, name: &str, assignee: Uuid) -> (String, String) {
    let (status, json) = send(pool, "POST", "/api/user/projects", token, json!({
        "name": name, "tasks": [{ "title": format!("{name} work"), "assigneeId": assignee }]
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let id = json["data"]["entity"]["id"].as_str().unwrap().to_owned();
    let tasks = get(pool, token, &format!("/api/user/tasks?projectId={id}")).await;
    (id, tasks[0]["id"].as_str().unwrap().to_owned())
}

fn ids(rows: &Value) -> Vec<&str> {
    rows.as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect()
}

#[sqlx::test]
async fn the_creator_archives_and_restores_and_the_lists_follow(pool: PgPool) {
    let (dhaval, dhaval_id) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (p, t) = project(&pool, &dhaval, "Checkout", dhaval_id).await;
    let active = |c: &Value| (c["myOpen"].as_i64().unwrap(), c["activeProjects"].as_i64().unwrap());
    assert_eq!(active(&get(&pool, &dhaval, "/api/user/counts").await), (1, 1));

    let (status, json) = send(&pool, "POST", &format!("/api/user/projects/{p}/archive"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["data"]["archivedAt"].is_string());
    assert_eq!(json["data"]["status"], "active", "archiving leaves the status alone");

    // Gone from every default view, its task with it.
    assert!(ids(&get(&pool, &dhaval, "/api/user/projects").await).is_empty());
    assert!(ids(&get(&pool, &dhaval, "/api/user/tasks").await).is_empty());
    assert!(ids(&get(&pool, &dhaval, "/api/user/tasks/mine").await).is_empty());
    let home = get(&pool, &dhaval, "/api/user/home").await;
    assert!(home["myTasks"].as_array().unwrap().is_empty());
    assert!(home["projects"].as_array().unwrap().is_empty());
    assert_eq!(home["team"][0]["open"], 0);
    assert_eq!(active(&get(&pool, &dhaval, "/api/user/counts").await), (0, 0));

    // And there when asked for.
    assert_eq!(ids(&get(&pool, &dhaval, "/api/user/projects?archived=true").await), [p.as_str()]);
    let archived = get(&pool, &dhaval, "/api/user/tasks?archived=true").await;
    assert_eq!(ids(&archived), [t.as_str()]);
    assert!(archived[0]["projectArchivedAt"].is_string());
    assert_eq!(ids(&get(&pool, &dhaval, "/api/user/tasks/mine?archived=true").await), [t.as_str()]);
    // The project's own page still lists its tasks.
    assert_eq!(ids(&get(&pool, &dhaval, &format!("/api/user/tasks?projectId={p}")).await), [t.as_str()]);

    // A task in an archived project comes back with the project, not alone.
    let (status, json) = send(&pool, "POST", &format!("/api/user/tasks/{t}/restore"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");

    let (status, json) = send(&pool, "POST", &format!("/api/user/projects/{p}/restore"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["data"]["archivedAt"].is_null());
    assert_eq!(ids(&get(&pool, &dhaval, "/api/user/tasks").await), [t.as_str()]);
    assert_eq!(active(&get(&pool, &dhaval, "/api/user/counts").await), (1, 1));

    // A task on its own: out of the lists and its project's counts.
    let (status, json) = send(&pool, "POST", &format!("/api/user/tasks/{t}/archive"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["data"]["archivedAt"].is_string());
    assert!(ids(&get(&pool, &dhaval, &format!("/api/user/tasks?projectId={p}")).await).is_empty());
    assert_eq!(ids(&get(&pool, &dhaval, &format!("/api/user/tasks?projectId={p}&archived=true")).await), [t.as_str()]);
    assert_eq!(get(&pool, &dhaval, "/api/user/projects").await[0]["total"], 0);
    assert_eq!(active(&get(&pool, &dhaval, "/api/user/counts").await), (0, 1));

    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE patch ? 'archived'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit, 3);
}

#[sqlx::test]
async fn only_the_creators_or_an_admin_may(pool: PgPool) {
    let (dhaval, dhaval_id) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (anmol, anmol_id) = person(&pool, "anmol@airtribe.live", "member").await;
    let (evana, _) = person(&pool, "evana@airtribe.live", "manager").await;
    let (boss, _) = person(&pool, "boss@airtribe.live", "admin").await;
    let (p, _) = project(&pool, &dhaval, "Checkout", dhaval_id).await;

    for (token, can) in [(&dhaval, true), (&anmol, false), (&boss, true)] {
        let shown = get(&pool, token, &format!("/api/user/projects/{p}")).await;
        assert_eq!((shown["canArchive"].as_bool(), shown["canDelete"].as_bool()), (Some(can), Some(can)));
    }
    for (method, uri) in [("POST", format!("/api/user/projects/{p}/archive")), ("DELETE", format!("/api/user/projects/{p}"))] {
        let (status, json) = send(&pool, method, &uri, &anmol, Value::Null).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        let verb = if method == "POST" { "archive" } else { "delete" };
        assert_eq!(message(&json), format!("Only Dhaval, who created this project, or an admin can {verb} it."));
    }

    // Anmol adds a task to Dhaval's project: it is Anmol's and Dhaval's.
    let (status, json) =
        send(&pool, "POST", &format!("/api/user/projects/{p}/tasks"), &anmol, json!({ "title": "Refunds", "assigneeId": anmol_id }))
            .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let t = json["data"]["id"].as_str().unwrap().to_owned();
    for (token, can) in [(&anmol, true), (&dhaval, true), (&evana, false), (&boss, true)] {
        assert_eq!(get(&pool, token, &format!("/api/user/tasks/{t}")).await["canDelete"], can);
    }
    let (status, json) = send(&pool, "DELETE", &format!("/api/user/tasks/{t}"), &evana, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(
        message(&json),
        "Only Anmol, who created this task, Dhaval, who created its project, or an admin can delete it."
    );
    let (status, _) = send(&pool, "POST", &format!("/api/user/tasks/{t}/archive"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::OK);

    // A row nobody is recorded as creating is an admin's.
    sqlx::query("UPDATE project SET created_by = NULL").execute(&pool).await.unwrap();
    let (status, json) = send(&pool, "POST", &format!("/api/user/projects/{p}/archive"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(message(&json), "Only an admin can archive this project \u{2014} nobody is recorded as creating it.");
    let (status, json) = send(&pool, "POST", &format!("/api/user/projects/{p}/archive"), &boss, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

#[sqlx::test]
async fn a_token_without_write_cannot_propose_either(pool: PgPool) {
    let (dhaval, dhaval_id) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (p, t) = project(&pool, &dhaval, "Checkout", dhaval_id).await;
    let (raw, _) = token::mint(&state(&pool), "ci", "dhaval@airtribe.live", vec!["read".into(), "propose".into()], 1)
        .await
        .unwrap();
    for (method, uri) in [
        ("DELETE", format!("/api/user/projects/{p}")),
        ("POST", format!("/api/user/projects/{p}/archive")),
        ("DELETE", format!("/api/user/tasks/{t}")),
    ] {
        let (status, json) = send(&pool, method, &uri, &raw, Value::Null).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: {json}");
    }
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE state = 'pending'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pending, 0);
}

#[sqlx::test]
async fn deleting_a_project_leaves_nothing_behind(pool: PgPool) {
    let (dhaval, dhaval_id) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (p, t) = project(&pool, &dhaval, "Checkout", dhaval_id).await;
    let (other, waiting) = project(&pool, &dhaval, "Refunds", dhaval_id).await;
    let (status, _) =
        send(&pool, "PATCH", &format!("/api/user/tasks/{waiting}/blockers"), &dhaval, json!({ "blockedBy": [t] })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&pool, "POST", &format!("/api/user/tasks/{t}/notes"), &dhaval, json!({ "body": "started" })).await;
    assert_eq!(status, StatusCode::OK);
    for (kind, id) in [("project", &p), ("task", &t), ("project", &other)] {
        let (status, json) = send(&pool, "POST", "/api/user/artifacts", &dhaval, json!({
            "parentType": kind, "parentId": id, "kind": "link", "url": "https://example.com"
        }))
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
    }

    let (status, json) = send(&pool, "DELETE", &format!("/api/user/projects/{p}"), &dhaval, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let left: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM project WHERE id = $1),
                (SELECT count(*) FROM phase WHERE project_id = $1),
                (SELECT count(*) FROM task WHERE id = $2),
                (SELECT count(*) FROM note WHERE task_id = $2),
                (SELECT count(*) FROM artifact WHERE parent_id IN ($1, $2))",
    )
    .bind(p.parse::<Uuid>().unwrap())
    .bind(t.parse::<Uuid>().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(left, (0, 0, 0, 0, 0));
    // The other project keeps its link, and stops waiting on a task that is gone.
    assert_eq!(get(&pool, &dhaval, &format!("/api/user/artifacts?parentType=project&parentId={other}")).await.as_array().unwrap().len(), 1);
    assert_eq!(get(&pool, &dhaval, &format!("/api/user/tasks/{waiting}")).await["blockedBy"], json!([]));
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE op = 'delete' AND target_type = 'project'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit, 1);

    let (status, json) = send(&pool, "DELETE", &format!("/api/user/tasks/{waiting}"), &dhaval, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(ids(&get(&pool, &dhaval, &format!("/api/user/tasks?projectId={other}")).await).is_empty());
}

#[sqlx::test]
async fn a_task_an_agent_holds_is_taken_back_first(pool: PgPool) {
    let (dhaval, dhaval_id) = person(&pool, "dhaval@airtribe.live", "member").await;
    let (p, t) = project(&pool, &dhaval, "Checkout", dhaval_id).await;
    let (status, json) =
        send(&pool, "POST", "/api/user/agents", &dhaval, json!({ "handle": "hermes", "name": "Hermes", "runtime": "hermes" })).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let agent = json["data"]["agent"]["id"].as_str().unwrap().to_owned();
    let (status, _) =
        send(&pool, "POST", &format!("/api/user/tasks/{t}/handoff"), &dhaval, json!({ "agentId": agent })).await;
    assert_eq!(status, StatusCode::OK);

    for (method, uri) in [("POST", format!("/api/user/tasks/{t}/archive")), ("DELETE", format!("/api/user/tasks/{t}"))] {
        let (status, json) = send(&pool, method, &uri, &dhaval, Value::Null).await;
        assert_eq!(status, StatusCode::CONFLICT, "{json}");
        assert_eq!(message(&json), "Take it back from Hermes first.");
    }
    let (status, json) = send(&pool, "DELETE", &format!("/api/user/projects/{p}"), &dhaval, Value::Null).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(message(&json), "Hermes still holds \u{201c}Checkout work\u{201d}. Take it back from Hermes first.");

    let (status, _) = send(&pool, "POST", &format!("/api/user/tasks/{t}/takeback"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, json) = send(&pool, "POST", &format!("/api/user/projects/{p}/archive"), &dhaval, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

/// Rows made before `created_by` existed get it from their first create in
/// the audit trail: by email for a person, `on_behalf_of` otherwise.
#[sqlx::test(migrations = false)]
async fn the_backfill_finds_creators_in_the_audit_trail(pool: PgPool) {
    let all = sqlx::migrate!();
    let before = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.migrations.iter().filter(|m| m.version <= 20260923000013).cloned().collect(),
        ),
        ..sqlx::migrate!()
    };
    before.run(&pool).await.unwrap();
    let (dhaval, anmol): (Uuid, Uuid) = sqlx::query_as(
        "WITH p AS (INSERT INTO person (email, name) VALUES ('dhaval@airtribe.live', 'Dhaval'), ('anmol@airtribe.live', 'Anmol')
                    RETURNING id, name)
         SELECT (SELECT id FROM p WHERE name = 'Dhaval'), (SELECT id FROM p WHERE name = 'Anmol')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let (project, task, orphan): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "WITH pr AS (INSERT INTO project (key, name, status) VALUES ('acp', 'ACP', 'archived') RETURNING id),
              ph AS (INSERT INTO phase (project_id, name, position) SELECT id, 'Work', 0 FROM pr RETURNING id),
              t AS (INSERT INTO task (phase_id, title) SELECT id, x FROM ph, unnest(ARRAY['a', 'b']) x RETURNING id, title)
         SELECT (SELECT id FROM pr), (SELECT id FROM t WHERE title = 'a'), (SELECT id FROM t WHERE title = 'b')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO change (actor, on_behalf_of, target_type, target_id, op, patch, state) VALUES
           ('dhaval@airtribe.live', $1, 'project', $3, 'create', '{}', 'applied'),
           ('ci (agent)', $2, 'task', $4, 'create', '{}', 'approved'),
           ('anmol@airtribe.live', $2, 'task', $5, 'create', '{}', 'rejected')",
    )
    .bind(dhaval)
    .bind(anmol)
    .bind(project)
    .bind(task)
    .bind(orphan)
    .execute(&pool)
    .await
    .unwrap();
    all.run(&pool).await.unwrap();

    let (by, status, archived): (Option<Uuid>, String, bool) =
        sqlx::query_as("SELECT created_by, status, archived_at IS NOT NULL FROM project WHERE id = $1")
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((by, status.as_str(), archived), (Some(dhaval), "done", true));
    let by: Vec<Option<Uuid>> = sqlx::query_scalar("SELECT created_by FROM task WHERE id IN ($1, $2) ORDER BY title")
        .bind(task)
        .bind(orphan)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(by, [Some(anmol), None]);
}
