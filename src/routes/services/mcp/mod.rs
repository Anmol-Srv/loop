//! The MCP surface: a thin JSON-RPC 2.0 layer over the existing controllers,
//! served as Streamable HTTP with plain JSON responses.
//!
//! Responses here are raw JSON-RPC, **not** the `{ success, data }` envelope the
//! rest of this API uses. MCP clients parse the standard shape and nothing else,
//! so wrapping it would make the endpoint unusable by the tools it exists for.
//! Authentication is the same bearer token as everywhere else (`Caller`). A
//! person's token sees the board tools filtered by its scopes; an agent's token
//! sees the agent tools and the skill.
//!
//! What real clients (the Python MCP SDK Hermes uses, Claude Code's http
//! transport) need beyond plain JSON-RPC: the protocol version negotiated, a
//! notification answered `202` with no body, and `GET` — the optional
//! server-to-client stream, which this server does not offer — refused `405`.

pub mod protocol;
pub mod tools;

use axum::extract::State;
use axum::http::header::ALLOW;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::controllers;
use crate::controllers::task::Assignee;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::auth::Caller;
use crate::models::task::{Task, TaskFilter, TASK_COLUMNS};
use protocol::{JsonRpcRequest, JsonRpcResponse, METHOD_NOT_FOUND, PARSE_ERROR};
use tools::ToolDef;

/// Newest first; the first is what a client asking for anything else gets.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const DOC_URI_PREFIX: &str = "acp://docs/";
const SKILL_URI: &str = "acp://skill";

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/services/mcp", post(handle).get(no_stream).delete(no_stream))
}

/// No server-initiated stream and no sessions to end: `405` is how the
/// transport says so, and clients carry on with POST alone.
async fn no_stream() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(ALLOW, "POST")]).into_response()
}

async fn handle(State(state): State<AppState>, caller: Caller, headers: HeaderMap, body: String) -> Response {
    // ponytail: malformed JSON and a malformed envelope both read as -32700.
    // Splitting them would need a second parse for no caller benefit.
    let request: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => return Json(JsonRpcResponse::err(None, PARSE_ERROR, e.to_string())).into_response(),
    };
    // A notification (no id) — `notifications/initialized` above all — wants
    // no answer. Anything else with no id is a response to a request this
    // server never sends. Either way: accepted, nothing to say.
    let Some(id) = request.id.clone() else {
        return StatusCode::ACCEPTED.into_response();
    };
    let id = Some(id);
    let agent = caller.agent_id.is_some();

    let result = match request.method.as_str() {
        "initialize" => Ok(initialize(&state, &caller, &request.params).await),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_list(&state, &caller).await })),
        "tools/call" => {
            let name = str_arg(&request.params, "name").unwrap_or_default();
            // The same filter `tools/list` uses, so a tool the caller cannot
            // see is also a tool it cannot call — a protocol error, not a tool
            // result.
            if !visible(&state, &caller).await.iter().any(|t| t.name == name) {
                Err(AppError::BadRequest(format!("unknown tool '{name}'")))
            } else {
                let args = request.params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let outcome = if agent {
                    call_agent_tool(&state, &caller, &name, &args).await
                } else {
                    call_tool(&state, &caller, &name, &args).await
                };
                // A refused call is the tool's answer, not a protocol failure:
                // the model reads the sentence and corrects course.
                Ok(match outcome {
                    Ok(Value::String(text)) => json!({ "content": [{ "type": "text", "text": text }] }),
                    Ok(v) => json!({ "content": [{ "type": "text", "text": v.to_string() }] }),
                    Err(e) => {
                        let (_, message) = protocol::code_and_message(&e);
                        json!({ "content": [{ "type": "text", "text": message }], "isError": true })
                    }
                })
            }
        }
        "resources/list" => Ok(json!({ "resources": resource_list(&caller) })),
        "resources/read" => read_resource(&state, &caller, &headers, &request.params).await,
        other => {
            return Json(JsonRpcResponse::err(id, METHOD_NOT_FOUND, format!("unknown method '{other}'")))
                .into_response()
        }
    };

    match result {
        Ok(value) => Json(JsonRpcResponse::ok(id, value)).into_response(),
        Err(e) => {
            let (code, message) = protocol::code_and_message(&e);
            Json(JsonRpcResponse::err(id, code, message)).into_response()
        }
    }
}

/// Answer with the client's version when this server speaks it, otherwise the
/// newest this server knows — the client then decides whether it can go on.
async fn initialize(state: &AppState, caller: &Caller, params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked.filter(|v| PROTOCOL_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSIONS[0]);
    let mut result = json!({
        "protocolVersion": version,
        "capabilities": { "tools": {}, "resources": {} },
        "serverInfo": { "name": "acp", "version": env!("CARGO_PKG_VERSION") },
    });
    if let Some(agent) = caller.agent_id {
        let handle: Option<String> = sqlx::query_scalar("SELECT handle FROM agent WHERE id = $1")
            .bind(agent)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
        result["instructions"] = json!(format!(
            "You are agent '{}' on Airtribe Control Plane. Read the {SKILL_URI} resource (the \
             airtribe-agent skill) before acting, and start every wake-up with agent_inbox.",
            handle.unwrap_or_default()
        ));
    }
    result
}

async fn visible(state: &AppState, caller: &Caller) -> Vec<ToolDef> {
    let Some(agent) = caller.agent_id else {
        return tools::for_scopes(&caller.scopes);
    };
    let mut tools = tools::agent_tools();
    // Read per request, so the owner's toggle takes effect on the next call.
    let intake: bool = sqlx::query_scalar("SELECT can_intake FROM agent WHERE id = $1")
        .bind(agent)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or(false);
    if intake {
        tools.extend(tools::intake_tools());
    }
    tools
}

async fn tool_list(state: &AppState, caller: &Caller) -> Vec<Value> {
    visible(state, caller)
        .await
        .into_iter()
        .map(|t| json!({
            "name": t.name,
            "description": t.description,
            "inputSchema": t.input_schema,
        }))
        .collect()
}

fn resource_list(caller: &Caller) -> Vec<Value> {
    if caller.agent_id.is_some() {
        return vec![json!({
            "uri": SKILL_URI,
            "name": "airtribe-agent skill",
            "description": "How to work the tasks your owner hands you. Read before acting.",
            "mimeType": "text/markdown",
        })];
    }
    tools::for_scopes(&caller.scopes)
        .into_iter()
        .map(|t| json!({
            "uri": format!("{DOC_URI_PREFIX}{}", t.name),
            "name": format!("{} documentation", t.name),
            "description": t.description,
            "mimeType": "text/markdown",
        }))
        .collect()
}

async fn read_resource(state: &AppState, caller: &Caller, headers: &HeaderMap, params: &Value) -> AppResult<Value> {
    let uri = str_arg(params, "uri")?;
    if let (SKILL_URI, Some(agent)) = (uri.as_str(), caller.agent_id) {
        let text = controllers::agent::skill(state, agent, None, &crate::routes::agent::server_url(headers)).await?;
        return Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": text }] }));
    }
    let name = uri
        .strip_prefix(DOC_URI_PREFIX)
        .ok_or_else(|| AppError::BadRequest(format!("unknown resource '{uri}'")))?;

    let tool = tools::for_scopes(&caller.scopes)
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| AppError::BadRequest(format!("unknown resource '{uri}'")))?;

    Ok(json!({
        "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": tool.doc }],
    }))
}

async fn call_tool(state: &AppState, caller: &Caller, name: &str, args: &Value) -> AppResult<Value> {
    let args = args.clone();
    let actor = &caller.actor;

    match name {
        "project_list" => to_value(controllers::project::list(state, actor.person_id, false).await?),
        "phase_status" => {
            to_value(controllers::phase::list(state, uuid_arg(&args, "projectId")?).await?)
        }
        "task_search" => {
            let filter = TaskFilter {
                department: args.get("discipline").and_then(|v| v.as_str()).map(str::to_string),
                project_id: opt_uuid_arg(&args, "projectId")?,
                phase_id: opt_uuid_arg(&args, "phaseId")?,
                status: opt_str_arg(&args, "status"),
                assignee_email: opt_str_arg(&args, "assigneeEmail"),
                assignee_kind: opt_str_arg(&args, "assigneeKind"),
                archived: false,
            };
            to_value(controllers::task::search(state, filter).await?)
        }
        "task_get" => {
            let id = uuid_arg(&args, "taskId")?;
            let task: Task = sqlx::query_as(&format!(
                "SELECT {TASK_COLUMNS} FROM task WHERE id = $1"
            ))
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::NotFound("task not found".into()))?;
            to_value(task)
        }
        "artifact_list" => to_value(
            controllers::artifact::list(state, actor.person_id, str_arg(&args, "parentType")?, uuid_arg(&args, "parentId")?)
                .await?,
        ),
        "task_create" => to_value(
            controllers::task::create(
                state,
                actor,
                opt_uuid_arg(&args, "phaseId")?,
                opt_uuid_arg(&args, "projectId")?,
                str_arg(&args, "title")?,
                opt_str_arg(&args, "body").unwrap_or_default(),
                args.get("priority").and_then(Value::as_i64).unwrap_or(2) as i32,
            )
            .await?,
        ),
        "task_update" => {
            let id = uuid_arg(&args, "taskId")?;
            match (opt_str_arg(&args, "status"), opt_str_arg(&args, "personEmail"), opt_str_arg(&args, "agentLabel")) {
                (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => {
                    Err(AppError::BadRequest(
                        "give exactly one of status, personEmail, or agentLabel".into(),
                    ))
                }
                (Some(status), None, None) => {
                    to_value(
                        controllers::task::set_status(
                            state,
                            actor,
                            id,
                            status,
                            opt_str_arg(&args, "manualReason"),
                            // An agent only ever proposes, and a proposal is
                            // checked when it is approved, not when it is made.
                            None,
                        )
                        .await?,
                    )
                }
                (None, person, agent) => {
                    let to = match (person, agent) {
                        (Some(email), None) => Assignee::Person(email),
                        (None, Some(label)) => Assignee::Agent(label),
                        _ => Assignee::Nobody,
                    };
                    to_value(controllers::task::assign(state, actor, id, to).await?)
                }
            }
        }
        "artifact_add" => to_value(
            controllers::artifact::add(
                state,
                actor,
                str_arg(&args, "parentType")?,
                uuid_arg(&args, "parentId")?,
                str_arg(&args, "kind")?,
                str_arg(&args, "url")?,
                opt_str_arg(&args, "title").unwrap_or_default(),
            )
            .await?,
        ),
        other => Err(AppError::BadRequest(format!("unknown tool '{other}'"))),
    }
}

/// The agent tools: `/api/agent` in tool form, for the agent behind the token.
async fn call_agent_tool(state: &AppState, caller: &Caller, name: &str, args: &Value) -> AppResult<Value> {
    use controllers::agent;
    let me = caller.agent()?;
    let task = || uuid_arg(args, "taskId");
    let body = || str_arg(args, "body");
    match name {
        "agent_inbox" => Ok(Value::String(agent::inbox(state, me).await?)),
        "agent_tasks" => to_value(agent::tasks(state, me).await?),
        "task_context" => agent::context(state, me, task()?).await,
        "task_ack" => to_value(agent::ack(state, me, task()?).await?),
        "task_update" => to_value(
            agent::update(
                state,
                me,
                task()?,
                &body()?,
                opt_str_arg(args, "status"),
                opt_str_arg(args, "expectedStatus"),
                opt_str_arg(args, "now").as_deref(),
            )
            .await?,
        ),
        "task_ask" => to_value(agent::ask(state, me, task()?, &body()?).await?),
        "task_attach" => to_value(
            agent::attach(
                state,
                me,
                task()?,
                &str_arg(args, "kind")?,
                &str_arg(args, "url")?,
                &opt_str_arg(args, "title").unwrap_or_default(),
            )
            .await?,
        ),
        "task_now" => to_value(agent::now(state, me, task()?, &str_arg(args, "text")?).await?),
        "task_log" => {
            let lines: Vec<String> = args
                .get("lines")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .ok_or_else(|| AppError::BadRequest("'lines' is required: an array of strings, oldest first".into()))?;
            agent::log(state, me, task()?, &lines).await
        }
        "task_note" => to_value(agent::note(state, me, task()?, &body()?).await?),
        "task_submit" => to_value(
            agent::submit(
                state,
                me,
                task()?,
                &str_arg(args, "target")?,
                &str_arg(args, "summary")?,
                opt_str_arg(args, "manualReason"),
            )
            .await?,
        ),
        "intake_create" => to_value(agent::intake(state, me, parse(args, "intake_create")?).await?),
        "intake_append" => {
            let source = parse(args.get("source").cloned().unwrap_or(Value::Null), "intake_append's source")?;
            to_value(
                agent::intake_append(state, me, task()?, source, &opt_str_arg(args, "text").unwrap_or_default())
                    .await?,
            )
        }
        "intake_recent" => to_value(
            agent::intake_recent(state, me, args.get("days").and_then(Value::as_i64).unwrap_or(14)).await?,
        ),
        "events_ack" => {
            let through = args
                .get("through")
                .and_then(Value::as_i64)
                .ok_or_else(|| AppError::BadRequest("'through' is required: the highest event id you handled".into()))?;
            Ok(json!({ "eventCursor": agent::ack_events(state, me, through).await? }))
        }
        other => Err(AppError::BadRequest(format!("unknown tool '{other}'"))),
    }
}

/// Tool arguments as a request body, with serde's complaint as the sentence.
fn parse<T: serde::de::DeserializeOwned>(args: impl std::borrow::Borrow<Value>, what: &str) -> AppResult<T> {
    serde_json::from_value(args.borrow().clone()).map_err(|e| AppError::BadRequest(format!("{what}: {e}")))
}

fn to_value<T: serde::Serialize>(value: T) -> AppResult<Value> {
    serde_json::to_value(value).map_err(|e| AppError::Internal(e.to_string()))
}

fn str_arg(args: &Value, key: &str) -> AppResult<String> {
    opt_str_arg(args, key).ok_or_else(|| AppError::BadRequest(format!("'{key}' is required")))
}

fn opt_str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn uuid_arg(args: &Value, key: &str) -> AppResult<Uuid> {
    opt_uuid_arg(args, key)?.ok_or_else(|| AppError::BadRequest(format!("'{key}' is required")))
}

fn opt_uuid_arg(args: &Value, key: &str) -> AppResult<Option<Uuid>> {
    match opt_str_arg(args, key) {
        None => Ok(None),
        Some(s) => s
            .parse()
            .map(Some)
            .map_err(|_| AppError::BadRequest(format!("'{key}' must be a uuid"))),
    }
}
