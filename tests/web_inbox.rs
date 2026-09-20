//! `describe` is the only place in the codebase that turns data into prose, so
//! it is the only place in the web UI worth a unit test.

use acp_server::controllers::approval::ChangeRow;
use acp_server::routes::web::inbox::describe;
use serde_json::{json, Value};
use uuid::Uuid;

fn change(target_type: &str, op: &str, patch: Value) -> ChangeRow {
    ChangeRow {
        id: Uuid::nil(),
        actor: "claude-1".into(),
        on_behalf_of: None,
        target_type: target_type.into(),
        target_id: Uuid::nil(),
        op: op.into(),
        patch,
        state: "pending".into(),
        applied_at: None,
        created_at: chrono::Utc::now(),
    }
}

#[test]
fn describe_covers_every_operation_shape() {
    let cases = [
        (
            "project/create",
            change("project", "create", json!({"key": "acme", "name": "Acme"})),
            "",
            r#"Create project "Acme" with key acme."#,
        ),
        (
            "phase/create",
            change(
                "phase",
                "create",
                json!({"project_id": Uuid::nil(), "name": "Design", "position": 2}),
            ),
            "Acme",
            r#"Add phase "Design" at position 2 to project "Acme"."#,
        ),
        (
            "phase/update",
            change("phase", "update", json!({"status": "done"})),
            "Design",
            r#"Move phase "Design" to done."#,
        ),
        (
            "task/create",
            change(
                "task",
                "create",
                json!({"phase_id": Uuid::nil(), "title": "wire MCP auth", "body": "", "priority": 1}),
            ),
            "Design",
            r#"Add task "wire MCP auth" to phase "Design"."#,
        ),
        (
            "task/update status",
            change("task", "update", json!({"status": "in_review"})),
            "wire MCP auth",
            r#"Move task "wire MCP auth" to in_review."#,
        ),
        (
            "task/update assign person",
            change("task", "update", json!({"person_email": "anmol@airtribe.live"})),
            "wire MCP auth",
            r#"Assign task "wire MCP auth" to anmol@airtribe.live."#,
        ),
        (
            "task/update assign agent",
            change("task", "update", json!({"agent_label": "claude-1"})),
            "wire MCP auth",
            r#"Assign task "wire MCP auth" to agent claude-1."#,
        ),
        (
            "task/update unassign",
            change("task", "update", json!({"person_email": Value::Null})),
            "wire MCP auth",
            r#"Unassign task "wire MCP auth"."#,
        ),
        (
            "artifact/create",
            change(
                "artifact",
                "create",
                json!({
                    "parent_type": "task",
                    "parent_id": Uuid::nil(),
                    "kind": "pr",
                    "url": "https://example.test/pr/1",
                    "title": "Inbox",
                }),
            ),
            "wire MCP auth",
            r#"Attach pr "Inbox" (https://example.test/pr/1) to task "wire MCP auth"."#,
        ),
        (
            "unrecognised shape stays honest",
            change("task", "delete", json!({"why": "oops"})),
            "wire MCP auth",
            r#"Unrecognised delete on task "wire MCP auth": {"why":"oops"}"#,
        ),
    ];

    for (name, row, context, expected) in cases {
        assert_eq!(describe(&row, context), expected, "case: {name}");
    }
}
