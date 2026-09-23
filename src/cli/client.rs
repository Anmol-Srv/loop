use std::time::Duration;

use serde_json::Value;

/// Long enough for a slow link, short enough that a stalled request surfaces
/// as an error instead of a page that spins forever.
const TIMEOUT: Duration = Duration::from_secs(15);

/// Unwrap the `{ success, data }` envelope every endpoint returns.
pub fn unwrap_envelope(body: Value) -> Result<Value, String> {
    match body.get("success").and_then(Value::as_bool) {
        Some(true) => Ok(body.get("data").cloned().unwrap_or(Value::Null)),
        Some(false) => Err(body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("request failed")
            .to_string()),
        None => Err(format!("unexpected response from server: {body}")),
    }
}

pub struct Client {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base_url: String, token: String) -> Self {
        let http = reqwest::Client::builder().timeout(TIMEOUT).build().unwrap_or_default();
        Self { base_url, token, http }
    }

    pub fn from_env() -> Result<Self, String> {
        let base_url = super::creds::base_url();
        let token = std::env::var("ACP_TOKEN")
            .map_err(|_| "ACP_TOKEN is not set. Mint one with: acp-admin mint <label> --owner <email>".to_string())?;
        Ok(Self::new(base_url, token))
    }

    pub async fn get(&self, path: &str) -> Result<Value, String> {
        self.send(reqwest::Method::GET, path, Value::Null).await
    }

    /// Like `send`, but with the HTTP status beside the result (0 when the
    /// request never got a response), so a UI can tell an ended session from
    /// any other failure.
    pub async fn request(&self, method: reqwest::Method, path: &str, body: Value) -> (u16, Result<Value, String>) {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token);

        if !body.is_null() {
            request = request.json(&body);
        }

        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => return (0, Err(format!("request failed: {e}"))),
        };
        let status = response.status().as_u16();
        let result = match response.json::<Value>().await {
            Ok(json) => unwrap_envelope(json),
            Err(e) => Err(format!("bad response body: {e}")),
        };
        (status, result)
    }

    /// Send and return the raw body, without unwrapping any envelope. The MCP
    /// endpoint speaks JSON-RPC, which has no `{success, data}` wrapper.
    pub async fn send_raw(&self, method: reqwest::Method, path: &str, body: Value) -> Result<Value, String> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token);

        if !body.is_null() {
            request = request.json(&body);
        }

        let response = request.send().await.map_err(|e| format!("request failed: {e}"))?;
        response.json().await.map_err(|e| format!("bad response body: {e}"))
    }

    pub async fn send(&self, method: reqwest::Method, path: &str, body: Value) -> Result<Value, String> {
        self.request(method, path, body).await.1
    }
}
