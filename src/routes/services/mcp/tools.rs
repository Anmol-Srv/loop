//! The MCP tool registry.
//!
//! `scope` is the scope that unlocks the tool. Mutating tools are unlocked by
//! `propose` *or* `write` — the difference is what happens when they run, not
//! whether they are visible.

use serde_json::{json, Value};

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub scope: &'static str,
    pub input_schema: Value,
    pub doc: &'static str,
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

pub fn all() -> Vec<ToolDef> {
    let uuid = json!({ "type": "string", "format": "uuid" });
    vec![
        ToolDef {
            name: "project_list",
            description: "List every project in the control plane.",
            scope: "read",
            input_schema: schema(json!({}), &[]),
            doc: include_str!("docs/project_list.md"),
        },
        ToolDef {
            name: "phase_status",
            description: "List a project's phases with their status, position, and gate flag.",
            scope: "read",
            input_schema: schema(json!({ "projectId": uuid }), &["projectId"]),
            doc: include_str!("docs/phase_status.md"),
        },
        ToolDef {
            name: "task_search",
            description: "Search tasks by project, phase, status, or assignee. All filters optional.",
            scope: "read",
            input_schema: schema(
                json!({
                    "projectId": uuid,
                    "phaseId": uuid,
                    "status": { "type": "string", "enum": crate::models::task::ALL_STATUSES },
                    "assigneeEmail": { "type": "string" },
                    "assigneeKind": { "type": "string", "enum": ["human", "agent"] },
                }),
                &[],
            ),
            doc: include_str!("docs/task_search.md"),
        },
        ToolDef {
            name: "task_get",
            description: "Fetch one task by id.",
            scope: "read",
            input_schema: schema(json!({ "taskId": uuid }), &["taskId"]),
            doc: include_str!("docs/task_get.md"),
        },
        ToolDef {
            name: "artifact_list",
            description: "List artifacts (PRs, docs, links) attached to a project, phase, or task.",
            scope: "read",
            input_schema: schema(
                json!({
                    "parentType": { "type": "string", "enum": crate::models::artifact::PARENT_TYPES },
                    "parentId": uuid,
                }),
                &["parentType", "parentId"],
            ),
            doc: include_str!("docs/artifact_list.md"),
        },
        ToolDef {
            name: "task_create",
            description: "Create a task in a phase. With only the 'propose' scope this queues a \
                          pending change instead of creating anything.",
            scope: "propose",
            input_schema: schema(
                json!({
                    "phaseId": uuid,
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "priority": { "type": "integer", "minimum": 0, "maximum": 4, "default": 2 },
                }),
                &["phaseId", "title"],
            ),
            doc: include_str!("docs/task_create.md"),
        },
        ToolDef {
            name: "task_update",
            description: "Change a task's status, or assign it to a person or agent. Give either \
                          'status' or one of 'personEmail' / 'agentLabel'.",
            scope: "propose",
            input_schema: schema(
                json!({
                    "taskId": uuid,
                    "status": { "type": "string", "enum": crate::models::task::ALL_STATUSES },
                    "personEmail": { "type": "string" },
                    "agentLabel": { "type": "string" },
                }),
                &["taskId"],
            ),
            doc: include_str!("docs/task_update.md"),
        },
        ToolDef {
            name: "artifact_add",
            description: "Attach an artifact (PR, doc, or link) to a project, phase, or task.",
            scope: "propose",
            input_schema: schema(
                json!({
                    "parentType": { "type": "string", "enum": crate::models::artifact::PARENT_TYPES },
                    "parentId": uuid,
                    "kind": { "type": "string", "enum": crate::models::artifact::ARTIFACT_KINDS },
                    "url": { "type": "string" },
                    "title": { "type": "string" },
                }),
                &["parentType", "parentId", "kind", "url"],
            ),
            doc: include_str!("docs/artifact_add.md"),
        },
        ToolDef {
            name: "work_claim",
            description: "Lease the next task assigned to an agent, or a named one. Returns null \
                          when nothing is claimable. See the doc for the claim/heartbeat/log/\
                          propose/release loop.",
            scope: "claim",
            input_schema: schema(json!({ "taskId": uuid }), &[]),
            doc: include_str!("docs/work_claim.md"),
        },
        ToolDef {
            name: "work_heartbeat",
            description: "Extend your lease on a task by another 5 minutes. Call while working.",
            scope: "claim",
            input_schema: schema(json!({ "taskId": uuid }), &["taskId"]),
            doc: include_str!("docs/work_heartbeat.md"),
        },
        ToolDef {
            name: "work_release",
            description: "Drop your lease and return the task to 'open' for another worker.",
            scope: "claim",
            input_schema: schema(json!({ "taskId": uuid }), &["taskId"]),
            doc: include_str!("docs/work_release.md"),
        },
        ToolDef {
            name: "run_log_append",
            description: "Append lines to a task's run log. Requires that you hold its lease.",
            scope: "claim",
            input_schema: schema(
                json!({
                    "taskId": uuid,
                    "lines": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                }),
                &["taskId", "lines"],
            ),
            doc: include_str!("docs/run_log_append.md"),
        },
    ]
}

/// A tool the caller cannot use is absent from the list, not merely rejected on
/// call — an agent should never see a tool it will be refused.
pub fn for_scopes(scopes: &[String]) -> Vec<ToolDef> {
    let has = |s: &str| scopes.iter().any(|x| x == s);
    all()
        .into_iter()
        .filter(|t| match t.scope {
            "propose" => has("propose") || has("write"),
            other => has(other),
        })
        .collect()
}
