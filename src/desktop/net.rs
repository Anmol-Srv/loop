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
    /// The tag of the copy already held, sent as `If-None-Match`.
    etag: Option<String>,
    /// A binary GET: the reply lands in `blobs`, not `results`.
    raw: bool,
}

pub struct Reply {
    pub key: String,
    pub result: Result<Value, String>,
    /// The HTTP status, 0 when there was no response at all.
    status: u16,
    seq: u64,
    etag: Option<String>,
    /// Set for a binary GET, in place of `result`.
    bytes: Option<Result<Vec<u8>, String>>,
}

pub struct Net {
    /// The server this bridge talks to, for showing the user which one.
    pub base_url: String,
    jobs: UnboundedSender<Job>,
    replies: Receiver<Reply>,
    /// Latest result per key, drained from the channel each frame. Shared,
    /// so a view can hold a payload across frames without copying it.
    pub results: HashMap<String, Result<Arc<Value>, String>>,
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
    /// The server's `ETag` for each key's current payload, so a refetch of
    /// something unchanged costs a `304` and no parse.
    etags: HashMap<String, String>,
    /// Which reply each key's payload came from: a view that derives rows
    /// from a payload redoes the work only when this moves. A `304` leaves it.
    gens: HashMap<String, u64>,
    /// Binary payloads (a filed message's images) by key. Never refreshed:
    /// the bytes behind a file id do not change.
    blobs: HashMap<String, Result<Arc<Vec<u8>>, String>>,
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
                        etag: None,
                        bytes: None,
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
                        let reply = if job.raw {
                            let (status, bytes) = client.get_bytes(&job.path).await;
                            Reply { key: job.key, result: Ok(Value::Null), status, seq: job.seq, etag: None, bytes: Some(bytes) }
                        } else {
                            let (status, result, etag) =
                                client.request_cached(job.method, &job.path, job.body, job.etag.as_deref()).await;
                            Reply { key: job.key, result, status, seq: job.seq, etag, bytes: None }
                        };
                        let _ = reply_tx.send(reply);
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
            etags: HashMap::new(),
            gens: HashMap::new(),
            blobs: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &str, path: &str) {
        self.paths.insert(key.to_string(), path.to_string());
        // Only a copy still on hand can be revalidated; without one a `304`
        // would leave nothing to show.
        let etag = self.data(key).and(self.etags.get(key)).cloned();
        self.dispatch(key, reqwest::Method::GET, path, Value::Null, etag);
    }

    pub fn post(&mut self, key: &str, path: &str, body: Value) {
        self.send(key, reqwest::Method::POST, path, body);
    }

    pub fn patch(&mut self, key: &str, path: &str, body: Value) {
        self.send(key, reqwest::Method::PATCH, path, body);
    }

    pub fn send(&mut self, key: &str, method: reqwest::Method, path: &str, body: Value) {
        self.dispatch(key, method, path, body, None);
    }

    fn dispatch(&mut self, key: &str, method: reqwest::Method, path: &str, body: Value, etag: Option<String>) {
        self.dispatch_job(key, method, path, body, etag, false);
    }

    fn dispatch_job(&mut self, key: &str, method: reqwest::Method, path: &str, body: Value, etag: Option<String>, raw: bool) {
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
            etag,
            raw,
        });
    }

    /// Fetch bytes once per key: a file's image. Read them with `bytes`.
    pub fn get_bytes_once(&mut self, key: &str, path: &str) {
        if !self.blobs.contains_key(key) && !self.is_loading(key) {
            self.dispatch_job(key, reqwest::Method::GET, path, Value::Null, None, true);
        }
    }

    pub fn bytes(&self, key: &str) -> Option<&Result<Arc<Vec<u8>>, String>> {
        self.blobs.get(key)
    }

    /// `seed` for bytes: a UI test's image, without a server.
    pub fn seed_bytes(&mut self, key: &str, bytes: Vec<u8>) {
        self.seq += 1;
        self.latest.insert(key.to_string(), self.seq);
        self.inflight.insert(key.to_string(), false);
        self.blobs.insert(key.to_string(), Ok(Arc::new(bytes)));
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
            if let Some(bytes) = reply.bytes {
                self.blobs.insert(reply.key, bytes.map(Arc::new));
                continue;
            }
            if reply.status == 304 {
                // Unchanged: keep what is held. If it was invalidated while the
                // request was out there is nothing to keep, so ask in full.
                if self.data(&reply.key).is_none() {
                    self.etags.remove(&reply.key);
                    if let Some(path) = self.paths.get(&reply.key).cloned() {
                        self.get(&reply.key, &path);
                    }
                }
                continue;
            }
            match reply.etag {
                Some(tag) => self.etags.insert(reply.key.clone(), tag),
                None => self.etags.remove(&reply.key),
            };
            self.gens.insert(reply.key.clone(), reply.seq);
            self.results.insert(reply.key, reply.result.map(Arc::new));
        }
    }

    pub fn peek(&self, key: &str) -> Option<&Result<Arc<Value>, String>> {
        self.results.get(key)
    }

    /// Read a successful payload, if one has arrived.
    pub fn data(&self, key: &str) -> Option<&Value> {
        match self.results.get(key) {
            Some(Ok(v)) => Some(v),
            _ => None,
        }
    }

    /// The payload itself, to hold past this borrow of `net` without copying.
    pub fn shared(&self, key: &str) -> Option<Arc<Value>> {
        match self.results.get(key) {
            Some(Ok(v)) => Some(v.clone()),
            _ => None,
        }
    }

    /// Moves each time a new payload lands for `key`, and not on a `304`; 0
    /// while there is none. What a view derives from `key` is good for as long
    /// as this stays put.
    pub fn generation(&self, key: &str) -> u64 {
        if self.data(key).is_none() {
            return 0;
        }
        self.gens.get(key).copied().unwrap_or(0)
    }

    pub fn error(&self, key: &str) -> Option<&str> {
        match self.results.get(key) {
            Some(Err(e)) => Some(e.as_str()),
            _ => None,
        }
    }

    /// Put a payload under `key` as if its reply had just landed, superseding
    /// any request still out for it. For offscreen renders and UI tests, which
    /// draw real views from fixture JSON instead of a server.
    pub fn seed(&mut self, key: &str, value: Value) {
        self.seq += 1;
        self.latest.insert(key.to_string(), self.seq);
        self.inflight.insert(key.to_string(), false);
        self.gens.insert(key.to_string(), self.seq);
        self.results.insert(key.to_string(), Ok(Arc::new(value)));
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

/// Work a view derives from its payloads, done once rather than every frame:
/// `f` runs again only when `stamp` changes — the generations it read, and
/// whatever else it depends on. Held in egui's temp store under `id`, beside
/// the views' filters.
pub fn memo<S, T>(ctx: &egui::Context, id: egui::Id, stamp: S, f: impl FnOnce() -> T) -> Arc<T>
where
    S: PartialEq + Clone + Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    if let Some((held, value)) = ctx.data(|d| d.get_temp::<(S, Arc<T>)>(id)) {
        if held == stamp {
            return value;
        }
    }
    let value = Arc::new(f());
    ctx.data_mut(|d| d.insert_temp(id, (stamp, value.clone())));
    value
}
