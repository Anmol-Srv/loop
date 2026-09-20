use serde_json::Value;

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
        Self { base_url, token, http: reqwest::Client::new() }
    }

    pub fn from_env() -> Result<Self, String> {
        let base_url = std::env::var("ACP_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        let token = std::env::var("ACP_TOKEN")
            .map_err(|_| "ACP_TOKEN is not set. Mint one with: acp-admin mint <label> --owner <email>".to_string())?;
        Ok(Self::new(base_url, token))
    }

    pub async fn get(&self, path: &str) -> Result<Value, String> {
        self.send(reqwest::Method::GET, path, Value::Null).await
    }

    pub async fn send(&self, method: reqwest::Method, path: &str, body: Value) -> Result<Value, String> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token);

        if !body.is_null() {
            request = request.json(&body);
        }

        let response = request.send().await.map_err(|e| format!("request failed: {e}"))?;
        let json: Value = response.json().await.map_err(|e| format!("bad response body: {e}"))?;

        unwrap_envelope(json)
    }
}
