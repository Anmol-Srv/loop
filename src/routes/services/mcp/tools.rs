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
            description:
                "Search tasks by project, phase, status, or assignee. All filters optional.",
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
            description: "Create a task: in a phase, in a project's first phase (projectId), or \
                          standalone with neither. With only the 'propose' scope this queues a \
                          pending change instead of creating anything.",
            scope: "propose",
            input_schema: schema(
                json!({
                    "phaseId": uuid,
                    "projectId": uuid,
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "priority": { "type": "integer", "minimum": 0, "maximum": 4, "default": 2 },
                }),
                &["title"],
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
    let with_body = || {
        schema(
            json!({ "taskId": uuid, "body": { "type": "string" } }),
            &["taskId", "body"],
        )
    };
    let tool = |name: &'static str, description: &'static str, input_schema: Value| ToolDef {
        name,
        description,
        scope: "agent",
        input_schema,
        doc: "",
    };
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
             project (with its repositories and localPath, the folder on your owner's Mac to work in), or, \
             when there is none to work in, folder — a folder from your owner's own list ({name, path, \
             source}, source being 'pinned' if the task named it or 'default' otherwise) — \
             the owner, every note oldest first, artifacts by kind, and related work (tasks it \
             waits on and tasks waiting on it, with their PRs, commits and Figma links), brief (your \
             owner's own note on this hand-off, if they left one), plan (the current plan you sent, with \
             its decision), and, for a task filed from a message, source: the message, the earlier \
             messages in its thread and its files.",
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
            "Attach evidence or context to your task: a pr, commit, figma, doc or link. A pr is refused \
             until your plan for this hand-off is approved (task_plan).",
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
            "task_plan",
            "Send your owner a plan before you build, and wait for their approval: task_submit and \
             attaching a pr are refused until they approve it. Every call is a new revision; send another \
             after 'plan_changes' with what you changed. End your run once you've sent it — you'll get a \
             'plan_approved' or 'plan_changes' event when they decide.",
            schema(
                json!({
                    "taskId": uuid,
                    "summary": { "type": "string", "maxLength": 600, "description": "A short, plain-language line your owner can skim." },
                    "plan": { "type": "string", "maxLength": 20000, "description": "The concrete steps, files and approach." },
                }),
                &["taskId", "summary", "plan"],
            ),
        ),
        tool(
            "workspace_set",
            "Set where this task's code lives on your owner's Mac, instead of asking them to do it in the \
             app: their local path for one of the project's repositories (give repoId if it has more than \
             one), or, for a task with no project, a folder of theirs (made if it's new; name defaults to \
             path's last component). Use it when task_context's localPath or folder is missing or wrong — \
             ask your owner first (task_ask) if you aren't sure which folder is right.",
            schema(
                json!({
                    "taskId": uuid,
                    "path": { "type": "string", "description": "Absolute path on your owner's Mac." },
                    "name": { "type": "string", "description": "For a project-less task's folder; defaults to path's last component." },
                    "repoId": uuid,
                }),
                &["taskId", "path"],
            ),
        ),
        tool(
            "task_submit",
            "Submit your task for your owner to approve, asking to move it to a finishing status \
             (engineering: completed or shipped; design: handoff or completed). Refused until your plan for \
             this hand-off is approved (task_plan). Evidence is checked now: \
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

/// The intake tools, for an agent whose owner turned intake on. Absent from
/// the list otherwise, like any tool the caller would be refused.
pub fn intake_tools() -> Vec<ToolDef> {
    let uuid = json!({ "type": "string", "format": "uuid" });
    let source = json!({
        "type": "object",
        "description": "The message this came from.",
        "properties": {
            "kind": { "type": "string", "description": "Where it came from: slack, email, github." },
            "key": { "type": "string", "description": "Your stable id for the message (e.g. the Slack permalink). A key is never filed or appended twice." },
            "url": { "type": "string", "description": "Permalink to the message." },
            "channel": { "type": "string", "description": "Channel id; a Slack id starting with D is a direct message." },
            "channelName": { "type": "string", "description": "Human name, e.g. #issues-and-feedback." },
            "author": { "type": "string" },
            "text": { "type": "string", "description": "The message, verbatim." },
            "receivedAt": { "type": "string", "format": "date-time" },
            "private": { "type": "boolean", "description": "A direct message: only your owner and admins see its text and author." },
            "thread": {
                "type": "array",
                "maxItems": 30,
                "description": "The earlier messages in its thread, oldest first (at most 30, each text at most 4,000 characters), so whoever works the task reads the conversation. Private with the message.",
                "items": {
                    "type": "object",
                    "properties": {
                        "author": { "type": "string" },
                        "text": { "type": "string", "maxLength": 4000, "description": "Slack formatting, names resolved like source.text." },
                        "ts": { "type": "string", "description": "Its Slack ts." },
                        "receivedAt": { "type": "string", "format": "date-time" },
                    },
                    "required": ["author", "text", "receivedAt"],
                },
            },
        },
        "required": ["kind", "key", "url", "channel", "author", "text", "receivedAt"],
    });
    let tool = |name: &'static str, description: &'static str, input_schema: Value| ToolDef {
        name,
        description,
        scope: "agent",
        input_schema,
        doc: "",
    };
    vec![
        tool(
            "intake_create",
            "File a task for your owner from a message you read. It lands in Triage in your intake project, \
             assigned to your owner, who accepts or dismisses it. Check intake_recent first: filing the same \
             source.key twice is refused (409) with the task it was filed as.",
            schema(
                json!({
                    "source": source,
                    "title": { "type": "string", "description": "One line: what is wrong or wanted." },
                    "body": { "type": "string", "description": "What you understood, steps, who is affected." },
                    "category": { "type": "string", "enum": crate::models::task::CATEGORIES },
                    "reason": { "type": "string", "description": "One sentence: why this is a task." },
                    "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
                    "priority": { "type": "integer", "minimum": 0, "maximum": 4, "default": 2 },
                }),
                &["source", "title", "category", "reason", "confidence"],
            ),
        ),
        tool(
            "intake_append",
            "Add another message to a task you filed (a follow-up, another report of the same thing) as a \
             note with its permalink. Only on tasks you filed; a source.key already seen is refused (409).",
            schema(
                json!({ "taskId": uuid, "source": source, "text": { "type": "string", "description": "What it adds; the message text if empty." } }),
                &["taskId", "source"],
            ),
        ),
        tool(
            "intake_attach",
            "Attach a file that came with a message you filed or appended: a screenshot or a PDF, shown under \
             the message on the task. PNG, JPEG, GIF, WebP or PDF, 8 MB at most, base64-encoded. sourceKey is \
             the message it came with (the filed one if left out). The same name from the same message is \
             attached once (409 after).",
            schema(
                json!({
                    "taskId": uuid,
                    "name": { "type": "string", "description": "File name, e.g. screenshot.png." },
                    "mime": { "type": "string", "enum": ["image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf"] },
                    "dataBase64": { "type": "string", "description": "The file's bytes, base64 (standard alphabet)." },
                    "sourceKey": { "type": "string", "description": "The source.key of the message it came with." },
                }),
                &["taskId", "name", "mime", "dataBase64"],
            ),
        ),
        tool(
            "intake_recent",
            "What you filed in the last N days (default 14), newest first: id, title, category, status and \
             source {key, url, channel}. Read it before filing so you append instead of filing a duplicate.",
            schema(json!({ "days": { "type": "integer", "minimum": 1, "maximum": 90, "default": 14 } }), &[]),
        ),
    ]
}

/// Reporting a run, for every agent: one call per pass it makes on a
/// schedule, so its owner sees each pass and what it found.
pub fn run_tools() -> Vec<ToolDef> {
    let count = json!({ "type": "integer", "minimum": 0 });
    vec![ToolDef {
        name: "run_report",
        description: "Report one pass you made (an intake sweep of Slack, say) when it ends, ok or not. Your \
             owner sees each run on your page: when, the status, the counts and any error. status: ok (it \
             finished), partial (some items failed) or failed (it did not run; error required).",
        scope: "agent",
        input_schema: schema(
            json!({
                "startedAt": { "type": "string", "format": "date-time", "description": "When the pass began; now if left out." },
                "status": { "type": "string", "enum": ["ok", "partial", "failed"] },
                "summary": { "type": "string", "maxLength": 500, "description": "One line on what the pass did." },
                "counts": {
                    "type": "object",
                    "properties": { "filed": count, "appended": count, "alreadyFiled": count, "skipped": count },
                },
                "error": { "type": "string", "description": "What failed, if anything: the step and its message." },
            }),
            &["status"],
        ),
        doc: "",
    }]
}
