use serde_json::Value;

use super::client::Client;

/// Forward one JSON-RPC line to the server and return the response line.
///
/// A malformed line is answered with a JSON-RPC parse error rather than being
/// dropped: an MCP client is blocked waiting on a response, so silence would
/// hang it forever.
pub async fn forward(client: &Client, line: &str) -> String {
    let parsed: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return error_line(Value::Null, -32700, &format!("parse error: {e}"));
        }
    };

    let id = parsed.get("id").cloned().unwrap_or(Value::Null);

    match client.send_raw(reqwest::Method::POST, "/api/services/mcp", parsed).await {
        Ok(response) => response.to_string(),
        Err(e) => error_line(id, -32603, &e),
    }
}

fn error_line(id: Value, code: i64, message: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
    .to_string()
}
