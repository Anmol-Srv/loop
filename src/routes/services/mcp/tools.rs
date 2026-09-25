//! The MCP tool registry.
//!
//! Two sets. A person's token sees the board tools, filtered by scope:
//! `scope` is the scope that unlocks the tool, and mutating tools are unlocked
//! by `propose` *or* `write` — the difference is what happens when they run,
//! not whether they are visible. An agent's token sees only the agent tools,
//! which act on the tasks handed to it and nothing else.

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

/// What a personal agent may call: the `/api/agent` routes, as tools. Their
/// documentation is the skill (`acp://skill`), not a page per tool.
pub fn agent_tools() -> Vec<ToolDef> {
    let uuid = json!({ "type": "string", "format": "uuid" });
    let task_only = || schema(json!({ "taskId": uuid }), &["taskId"]);
    let with_body = || schema(json!({ "taskId": uuid, "body": { "type": "string" } }), &["taskId", "body"]);
    let tool = |name: &'static str, description: &'static str, input_schema: Value| ToolDef { name, description, scope: "agent", input_schema, doc: "" };
    vec![
        tool(
            "agent_inbox",
            "What needs you now: tasks handed to you and not yet acknowledged, and events you have not \
             handled, oldest first. Start every wake-up here.",
            schema(json!({}), &[]),
        ),
        tool("agent_tasks", "Every task currently handed to you.", schema(json!({}), &[])),
        tool(
            "task_context",
            "Everything about one of your tasks: the task with its track and allowed next statuses, the \
             project (with its repositories and localPath, the folder on your owner's Mac to work in), \
             the owner, every note oldest first, artifacts by kind, and related work (tasks it \
             waits on and tasks waiting on it, with their PRs, commits and Figma links).",
            task_only(),
        ),
        tool("task_ack", "Acknowledge a task handed to you, so your owner sees you have it.", task_only()),
        tool(
            "task_update",
            "Post progress on your task, optionally moving it to open, in_progress or blocked. Finishing \
             moves go through task_submit.",
            schema(
                json!({
                    "taskId": uuid,
                    "body": { "type": "string" },
                    "status": { "type": "string", "enum": ["open", "in_progress", "blocked"] },
                    "expectedStatus": { "type": "string", "description": "The status you last read; the move is refused if someone changed it since." },
                    "now": { "type": "string", "maxLength": 120, "description": "Optionally, a new now line (see task_now)." },
                }),
                &["taskId", "body"],
            ),
        ),
        tool(
            "task_ask",
            "Ask your owner a question that only they can decide. The task shows as waiting on them until \
             they answer.",
            with_body(),
        ),
        tool(
            "task_attach",
            "Attach evidence or context to your task: a pr, commit, figma, doc or link.",
            schema(
                json!({
                    "taskId": uuid,
                    "kind": { "type": "string", "enum": crate::models::artifact::ARTIFACT_KINDS },
                    "url": { "type": "string", "description": "A web link; for a commit, its hash." },
                    "title": { "type": "string" },
                }),
                &["taskId", "kind", "url"],
            ),
        ),
        tool(
            "task_now",
            "Say in one short line what you are doing right now on your task (1-120 characters, e.g. \
             \"running the checkout tests\"). The whole team sees it live, and it marks you working. \
             Update it whenever you move to a new step; it clears itself when you ask, submit or stop.",
            schema(
                json!({ "taskId": uuid, "text": { "type": "string", "minLength": 1, "maxLength": 120 } }),
                &["taskId", "text"],
            ),
        ),
        tool(
            "task_log",
            "Append lines to your task's step log: the commands you ran and what they printed, oldest \
             first. Up to 200 lines per call, each at most 2000 characters. Only your owner (and admins) \
             can read it, so it is the place for detail that would clutter progress notes.",
            schema(
                json!({
                    "taskId": uuid,
                    "lines": { "type": "array", "items": { "type": "string", "maxLength": 2000 }, "minItems": 1, "maxItems": 200 },
                }),
                &["taskId", "lines"],
            ),
        ),
        tool("task_note", "Leave a plain note on your task for the team.", with_body()),
        tool(
            "task_submit",
            "Submit your task for your owner to approve, asking to move it to a finishing status \
             (engineering: completed or shipped; design: handoff or completed). Evidence is checked now: \
             engineering completed needs a pr or commit attached, design handoff needs a figma link, \
             unless you give manualReason.",
            schema(
                json!({
                    "taskId": uuid,
                    "target": { "type": "string", "enum": ["completed", "shipped", "handoff"] },
                    "summary": { "type": "string", "description": "What you did and how you verified it, checkable in two minutes." },
                    "manualReason": { "type": "string", "description": "Why this finished without the usual evidence." },
                }),
                &["taskId", "target", "summary"],
            ),
        ),
        tool(
            "events_ack",
            "Mark events as handled, through the given event id (the #number in your inbox).",
            schema(json!({ "through": { "type": "integer" } }), &["through"]),
        ),
    ]
}
