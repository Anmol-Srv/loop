//! JSON-RPC 2.0 envelope types for the MCP surface.
//!
//! Unlike the rest of the API, these are *not* wrapped in `{ success, data }`:
//! MCP clients speak plain JSON-RPC and will not parse anything else.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::AppError;

pub const PARSE_ERROR: i32 = -32700;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: Option<Value>, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    pub fn err(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message: message.into() }),
        }
    }
}

/// 4xx is the caller's fault (bad arguments, missing row, wrong scope) and maps
/// to "invalid params"; 5xx is ours.
pub fn code_and_message(error: &AppError) -> (i32, String) {
    if error.status().is_server_error() {
        tracing::error!(error = %error, "mcp internal error");
        (INTERNAL_ERROR, "internal server error".to_string())
    } else {
        (INVALID_PARAMS, error.to_string())
    }
}
