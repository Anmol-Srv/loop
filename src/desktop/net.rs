//! Bridge between the GUI thread and the API.
//!
//! egui redraws on one thread and must never block, so every request goes to a
//! background tokio runtime and comes back through a channel. Requests are
//! keyed strings rather than an enum: a view issues `net.get("board", path)`
//! and later reads `net.take("board")`. That keeps views from sharing a type
//! nobody owns, which is what lets them be written independently.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::cli::client::Client;

/// How often the page on screen is fetched again while the window has focus.
/// Five people share one board; a page older than this is likely to be wrong.
const REFRESH: Duration = Duration::from_secs(30);

pub struct Job {
    pub key: String,
    pub method: reqwest::Method,
    pub path: String,
    pub body: Value,
    seq: u64,
}

pub struct Reply {
    pub key: String,
    pub result: Result<Value, String>,
    /// The HTTP status, 0 when there was no response at all.
    status: u16,
    seq: u64,
}

pub struct Net {
    /// The server this bridge talks to, for showing the user which one.
    pub base_url: String,
    jobs: UnboundedSender<Job>,
    replies: Receiver<Reply>,
    /// Latest result per key, drained from the channel each frame.
    pub results: HashMap<String, Result<Value, String>>,
    /// Keys with a request currently in flight, so a view can show a spinner
    /// and avoid firing the same request every frame.
    pub inflight: HashMap<String, bool>,
    /// Set by any 401: the token is no longer good, whichever view asked.
    pub session_ended: bool,
    /// The newest request per key. Requests run concurrently, so an older
    /// reply can land after a newer one; only the newest is kept.
    seq: u64,
    latest: HashMap<String, u64>,
    /// The path each GET key was last fetched from, so it can be fetched again.
    paths: HashMap<String, String>,
    /// Keys a view asked for with `get_once` since the last `pump`: what the
    /// page on screen is made of.
    touched: HashSet<String>,
    /// Keys whose in-flight request is a background refresh, which an
    /// `invalidate` supersedes rather than waits for.
    refreshing: HashSet<String>,
    focused: Option<bool>,
    refreshed_at: Instant,
}

impl Net {
    /// Spawn the background runtime. `base_url` and `token` are fixed for the
    /// life of the bridge; changing identity means building a new `Net`.
    pub fn spawn(base_url: String, token: String, repaint: egui::Context) -> Self {
        let (job_tx, mut job_rx) = unbounded_channel::<Job>();
        let (reply_tx, reply_rx) = channel::<Reply>();
        let url = base_url.clone();

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = reply_tx.send(Reply {
                        key: "__runtime".into(),
                        result: Err(format!("could not start the network runtime: {e}")),
                        status: 0,
                        seq: 0,
                    });
                    return;
                }
            };

            let client = Arc::new(Client::new(url, token));

            // Each job is its own task, so one slow request no longer holds up
            // every page behind it.
            rt.block_on(async {
                while let Some(job) = job_rx.recv().await {
                    let (client, reply_tx, repaint) = (client.clone(), reply_tx.clone(), repaint.clone());
                    tokio::spawn(async move {
                        let (status, result) = client.request(job.method, &job.path, job.body).await;
                        let _ = reply_tx.send(Reply { key: job.key, result, status, seq: job.seq });
                        // Wake the GUI thread; otherwise the reply sits until
                        // the next unrelated repaint.
                        repaint.request_repaint();
                    });
                }
            });
        });

        Self {
            base_url,
            jobs: job_tx,
            replies: reply_rx,
            results: HashMap::new(),
            inflight: HashMap::new(),
            session_ended: false,
            seq: 0,
            latest: HashMap::new(),
            paths: HashMap::new(),
            touched: HashSet::new(),
            refreshing: HashSet::new(),
            focused: None,
            refreshed_at: Instant::now(),
        }
    }

    pub fn get(&mut self, key: &str, path: &str) {
        self.paths.insert(key.to_string(), path.to_string());
        self.send(key, reqwest::Method::GET, path, Value::Null);
    }

    pub fn post(&mut self, key: &str, path: &str, body: Value) {
        self.send(key, reqwest::Method::POST, path, body);
    }

    pub fn patch(&mut self, key: &str, path: &str, body: Value) {
        self.send(key, reqwest::Method::PATCH, path, body);
    }

    pub fn send(&mut self, key: &str, method: reqwest::Method, path: &str, body: Value) {
        self.seq += 1;
        self.latest.insert(key.to_string(), self.seq);
        self.refreshing.remove(key);
        self.inflight.insert(key.to_string(), true);
        let _ = self.jobs.send(Job {
            key: key.to_string(),
            method,
            path: path.to_string(),
            body,
            seq: self.seq,
        });
    }

    /// Issue the request only if nothing is in flight and no result is cached.
    /// Views call this from `ui()`, which runs every frame.
    pub fn get_once(&mut self, key: &str, path: &str) {
        self.touched.insert(key.to_string());
        if !self.results.contains_key(key) && !self.is_loading(key) {
            self.get(key, path);
        }
    }

    pub fn is_loading(&self, key: &str) -> bool {
        *self.inflight.get(key).unwrap_or(&false)
    }

    /// Drain replies into `results`. Call once at the top of each frame.
    pub fn pump(&mut self) {
        self.touched.clear();
        while let Ok(reply) = self.replies.try_recv() {
            if reply.status == 401 {
                self.session_ended = true;
            }
            if self.latest.get(&reply.key).is_some_and(|s| *s != reply.seq) {
                continue;
            }
            self.refreshing.remove(&reply.key);
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
        self.drop_refresh(key);
    }

    pub fn invalidate_prefix(&mut self, prefix: &str) {
        self.results.retain(|k, _| !k.starts_with(prefix));
        let keys: Vec<String> = self.refreshing.iter().filter(|k| k.starts_with(prefix)).cloned().collect();
        for key in keys {
            self.drop_refresh(&key);
        }
    }

    /// A background refresh in flight may have been read before the change
    /// that invalidated it, so its reply is ignored and the next `get_once`
    /// asks again.
    fn drop_refresh(&mut self, key: &str) {
        if self.refreshing.remove(key) {
            self.seq += 1;
            self.latest.insert(key.to_string(), self.seq);
            self.inflight.insert(key.to_string(), false);
        }
    }

    /// Keep the page on screen current: fetch it again when the window comes
    /// back into focus, and every `REFRESH` while it has focus. Call once at
    /// the end of each frame, after the views have asked for what they show.
    ///
    /// Only what the page is made of is refetched, in place: the old values
    /// stay on screen until the new ones land, so nothing blinks. Keys under
    /// `__` (identity, the track table) do not change while the app runs.
    pub fn tick(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.focused);
        let regained = focused && self.focused == Some(false);
        self.focused = Some(focused);
        if !focused {
            return;
        }
        if regained || self.refreshed_at.elapsed() >= REFRESH {
            self.refreshed_at = Instant::now();
            let due: Vec<(String, String)> = self
                .touched
                .iter()
                .filter(|k| !k.starts_with("__") && self.results.contains_key(*k) && !self.is_loading(k))
                .filter_map(|k| Some((k.clone(), self.paths.get(k)?.clone())))
                .collect();
            for (key, path) in due {
                self.get(&key, &path);
                self.refreshing.insert(key);
            }
        }
        ctx.request_repaint_after(REFRESH.saturating_sub(self.refreshed_at.elapsed()));
    }
}
