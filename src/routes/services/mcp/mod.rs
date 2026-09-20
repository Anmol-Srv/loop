//! The MCP surface: a thin JSON-RPC 2.0 layer over the existing controllers.
//!
//! Responses here are raw JSON-RPC, **not** the `{ success, data }` envelope the
//! rest of this API uses. MCP clients parse the standard shape and nothing else,
//! so wrapping it would make the endpoint unusable by the tools it exists for.
//! Authentication is the same bearer token as everywhere else (`Caller`), and
//! the tool list is filtered by that token's scopes.

pub mod protocol;
pub mod tools;

use axum::extract::State;
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

const PROTOCOL_VERSION: &str = "2024-11-05";
const DOC_URI_PREFIX: &str = "acp://docs/";

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/services/mcp", post(handle))
}

async fn handle(State(state): State<AppState>, caller: Caller, body: String) -> Json<JsonRpcResponse> {
    // ponytail: malformed JSON and a malformed envelope both read as -32700.
    // Splitting them would need a second parse for no caller benefit.
    let request: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => return Json(JsonRpcResponse::err(None, PARSE_ERROR, e.to_string())),
    };
    let id = request.id.clone();

    let result = match request.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {}, "resources": {} },
            "serverInfo": { "name": "acp", "version": env!("CARGO_PKG_VERSION") },
        })),
        "tools/list" => Ok(json!({ "tools": tool_list(&caller) })),
        "tools/call" => call_tool(&state, &caller, &request.params).await.map(|v| {
            json!({ "content": [{ "type": "text", "text": v.to_string() }] })
        }),
        "resources/list" => Ok(json!({ "resources": resource_list(&caller) })),
        "resources/read" => read_resource(&caller, &request.params),
        other => {
            return Json(JsonRpcResponse::err(
                id,
                METHOD_NOT_FOUND,
                format!("unknown method '{other}'"),
            ))
        }
    };

    match result {
        Ok(value) => Json(JsonRpcResponse::ok(id, value)),
        Err(e) => {
            let (code, message) = protocol::code_and_message(&e);
            Json(JsonRpcResponse::err(id, code, message))
        }
    }
}

fn tool_list(caller: &Caller) -> Vec<Value> {
    tools::for_scopes(&caller.scopes)
        .into_iter()
        .map(|t| json!({
            "name": t.name,
            "description": t.description,
            "inputSchema": t.input_schema,
        }))
        .collect()
}

fn resource_list(caller: &Caller) -> Vec<Value> {
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

fn read_resource(caller: &Caller, params: &Value) -> AppResult<Value> {
    let uri = str_arg(params, "uri")?;
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

async fn call_tool(state: &AppState, caller: &Caller, params: &Value) -> AppResult<Value> {
    let name = str_arg(params, "name")?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

    // The scope filter is the same one `tools/list` uses, so a tool the caller
    // cannot see is also a tool it cannot call.
    if !tools::for_scopes(&caller.scopes).iter().any(|t| t.name == name) {
        return Err(AppError::BadRequest(format!("unknown tool '{name}'")));
    }
    let actor = &caller.actor;

    match name.as_str() {
        "project_list" => to_value(controllers::project::list(state).await?),
        "phase_status" => {
            to_value(controllers::phase::list(state, uuid_arg(&args, "projectId")?).await?)
        }
        "task_search" => {
            let filter = TaskFilter {
                project_id: opt_uuid_arg(&args, "projectId")?,
                phase_id: opt_uuid_arg(&args, "phaseId")?,
                status: opt_str_arg(&args, "status"),
                assignee_email: opt_str_arg(&args, "assigneeEmail"),
                assignee_kind: opt_str_arg(&args, "assigneeKind"),
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
            controllers::artifact::list(state, str_arg(&args, "parentType")?, uuid_arg(&args, "parentId")?)
                .await?,
        ),
        "task_create" => to_value(
            controllers::task::create(
                state,
                actor,
                uuid_arg(&args, "phaseId")?,
                str_arg(&args, "title")?,
                opt_str_arg(&args, "body").unwrap_or_default(),
                args.get("priority").and_then(Value::as_i64).unwrap_or(2) as i32,
                opt_str_arg(&args, "discipline"),
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
                    to_value(controllers::task::set_status(state, actor, id, status).await?)
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
        // The lease is always taken under the caller's own label — there is no
        // argument for it, so a worker cannot claim or heartbeat as someone else.
        "work_claim" => to_value(
            controllers::work::claim(state, &actor.label, opt_uuid_arg(&args, "taskId")?).await?,
        ),
        "work_heartbeat" => to_value(
            controllers::work::heartbeat(state, &actor.label, uuid_arg(&args, "taskId")?).await?,
        ),
        "work_release" => to_value(
            controllers::work::release(state, &actor.label, uuid_arg(&args, "taskId")?).await?,
        ),
        "run_log_append" => {
            let lines = args
                .get("lines")
                .and_then(Value::as_array)
                .ok_or_else(|| AppError::BadRequest("'lines' is required".into()))?
                .iter()
                .map(|l| {
                    l.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| AppError::BadRequest("'lines' must be strings".into()))
                })
                .collect::<AppResult<Vec<String>>>()?;
            to_value(
                controllers::run_log::append(state, &actor.label, uuid_arg(&args, "taskId")?, lines)
                    .await?,
            )
        }
        other => Err(AppError::BadRequest(format!("unknown tool '{other}'"))),
    }
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
