//! Bridge between the GUI thread and the API.
//!
//! egui redraws on one thread and must never block, so every request goes to a
//! background tokio runtime and comes back through a channel. Requests are
//! keyed strings rather than an enum: a view issues `net.get("board", path)`
//! and later reads `net.take("board")`. That keeps views from sharing a type
//! nobody owns, which is what lets them be written independently.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

use serde_json::Value;

use crate::cli::client::Client;

pub struct Job {
    pub key: String,
    pub method: reqwest::Method,
    pub path: String,
    pub body: Value,
}

pub struct Reply {
    pub key: String,
    pub result: Result<Value, String>,
}

pub struct Net {
    jobs: Sender<Job>,
    replies: Receiver<Reply>,
    /// Latest result per key, drained from the channel each frame.
    pub results: HashMap<String, Result<Value, String>>,
    /// Keys with a request currently in flight, so a view can show a spinner
    /// and avoid firing the same request every frame.
    pub inflight: HashMap<String, bool>,
}

impl Net {
    /// Spawn the background runtime. `base_url` and `token` are fixed for the
    /// life of the bridge; changing identity means building a new `Net`.
    pub fn spawn(base_url: String, token: String, repaint: egui::Context) -> Self {
        let (job_tx, job_rx) = channel::<Job>();
        let (reply_tx, reply_rx) = channel::<Reply>();

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = reply_tx.send(Reply {
                        key: "__runtime".into(),
                        result: Err(format!("could not start the network runtime: {e}")),
                    });
                    return;
                }
            };

            let client = Client::new(base_url, token);

            rt.block_on(async {
                while let Ok(job) = job_rx.recv() {
                    let result = client.send_raw_result(job.method, &job.path, job.body).await;
                    if reply_tx.send(Reply { key: job.key, result }).is_err() {
                        return;
                    }
                    // Wake the GUI thread; otherwise the reply sits until the
                    // next unrelated repaint.
                    repaint.request_repaint();
                }
            });
        });

        Self {
            jobs: job_tx,
            replies: reply_rx,
            results: HashMap::new(),
            inflight: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &str, path: &str) {
        self.send(key, reqwest::Method::GET, path, Value::Null);
    }

    pub fn post(&mut self, key: &str, path: &str, body: Value) {
        self.send(key, reqwest::Method::POST, path, body);
    }

    pub fn patch(&mut self, key: &str, path: &str, body: Value) {
        self.send(key, reqwest::Method::PATCH, path, body);
    }

    pub fn send(&mut self, key: &str, method: reqwest::Method, path: &str, body: Value) {
        self.inflight.insert(key.to_string(), true);
        let _ = self.jobs.send(Job {
            key: key.to_string(),
            method,
            path: path.to_string(),
            body,
        });
    }

    /// Issue the request only if nothing is in flight and no result is cached.
    /// Views call this from `ui()`, which runs every frame.
    pub fn get_once(&mut self, key: &str, path: &str) {
        if !self.results.contains_key(key) && !self.is_loading(key) {
            self.get(key, path);
        }
    }

    pub fn is_loading(&self, key: &str) -> bool {
        *self.inflight.get(key).unwrap_or(&false)
    }

    /// Drain replies into `results`. Call once at the top of each frame.
    pub fn pump(&mut self) {
        while let Ok(reply) = self.replies.try_recv() {
            self.inflight.insert(reply.key.clone(), false);
            self.results.insert(reply.key, reply.result);
        }
    }

    pub fn peek(&self, key: &str) -> Option<&Result<Value, String>> {
        self.results.get(key)
    }

    /// Read a successful payload, if one has arrived.
    pub fn data(&self, key: &str) -> Option<&Value> {
        match self.results.get(key) {
            Some(Ok(v)) => Some(v),
            _ => None,
        }
    }

    pub fn error(&self, key: &str) -> Option<&str> {
        match self.results.get(key) {
            Some(Err(e)) => Some(e.as_str()),
            _ => None,
        }
    }

    /// Forget a cached result so the next `get_once` refetches it.
    pub fn invalidate(&mut self, key: &str) {
        self.results.remove(key);
    }

    pub fn invalidate_prefix(&mut self, prefix: &str) {
        self.results.retain(|k, _| !k.starts_with(prefix));
    }
}
