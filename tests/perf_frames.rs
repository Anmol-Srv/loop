//! Frame time per page, against a real server seeded at scale.
//!
//! Drives the real `App::frame` the way `tests/page_render.rs` does, but
//! without a GPU: this times the CPU side of a frame (views + egui layout and
//! tessellation), which is what grows with the data. Per page it reports:
//!   - cold load: wall time until every request landed, and the requests made
//!     (in the order they were first issued, with the frame that issued them,
//!     so sequential waves show up as later frames)
//!   - 60 frames with the page loaded and no input: `App::frame` alone, and the
//!     whole `harness.step()` (adds egui end-of-frame + kittest's AccessKit)
//!   - idle: frames an eframe-style reactive loop would draw in 3 s with no input
//!
//! `#[ignore]`d: needs a seeded server. `FRAMES=1 scripts/perf.sh` runs it.
//!   ACP_PERF_URL=http://localhost:8141 ACP_PERF_TOKEN=... \
//!     cargo test --release --features app --test perf_frames -- --ignored --nocapture
#![cfg(feature = "app")]

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use acp_server::desktop::design::theme;
use acp_server::desktop::net::Net;
use acp_server::desktop::{views, App, Tab};
use egui_kittest::Harness;

// Fixed ids from tests/fixtures/scale-seed.sql: the biggest project, and a
// task in it.
const PROJECT: &str = "33333333-0000-0000-0000-000000000001";
const TASK: &str = "44444444-0000-0000-0000-000000000001";

const PAGES: &[(&str, Tab, Option<&str>, Option<&str>)] = &[
    ("home", Tab::Home, None, None),
    ("mytasks", Tab::MyTasks, None, None),
    ("projects", Tab::Projects, None, None),
    ("project", Tab::Projects, Some(PROJECT), None),
    ("task", Tab::Home, None, Some(TASK)),
];

#[test]
#[ignore = "needs a seeded server: FRAMES=1 scripts/perf.sh"]
fn frames() {
    let url = std::env::var("ACP_PERF_URL").expect("ACP_PERF_URL");
    let token = std::env::var("ACP_PERF_TOKEN").expect("ACP_PERF_TOKEN");
    eprintln!(
        "\n{:<9} {:>8} {:>5} {:>10} {:>10} {:>10} {:>10} {:>11}",
        "page", "load_ms", "reqs", "app_mean", "app_p95", "step_mean", "step_p95", "idle_fps"
    );
    for page in PAGES {
        measure(page, &url, &token);
    }
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1]
}

fn measure(page: &(&str, Tab, Option<&str>, Option<&str>), url: &str, token: &str) {
    let (name, tab, project, task) = *page;
    let app: RefCell<Option<App>> = RefCell::new(None);
    let app_ms: Cell<f64> = Cell::new(0.0);

    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui(|ui| {
        let mut slot = app.borrow_mut();
        if slot.is_none() {
            theme::install(ui.ctx());
            ui.ctx().set_zoom_factor(1.15);
            *slot = Some(App {
                net: Some(Net::spawn(url.to_owned(), token.to_owned(), ui.ctx().clone())),
                tab,
                project: project.map(str::to_owned),
                task: task.map(str::to_owned),
                scopes: Vec::new(),
                login: views::login::State::default(),
                board: views::board::State::default(),
                palette: views::palette::State::default(),
            });
            return;
        }
        let t = Instant::now();
        slot.as_mut().unwrap().frame(ui);
        app_ms.set(t.elapsed().as_secs_f64() * 1000.0);
    });

    // Cold load: step until nothing is in flight for a few frames.
    let start = Instant::now();
    let mut seen: Vec<(String, usize)> = Vec::new();
    let (mut quiet, mut frame, mut worst_load_frame) = (0, 0usize, 0.0_f64);
    while quiet < 6 && start.elapsed() < Duration::from_secs(30) {
        harness.step();
        frame += 1;
        worst_load_frame = worst_load_frame.max(app_ms.get());
        let a = app.borrow();
        let net = a.as_ref().unwrap().net.as_ref().unwrap();
        for k in net.inflight.keys() {
            if !seen.iter().any(|(s, _)| s == k) {
                seen.push((k.clone(), frame));
            }
        }
        let busy = net.inflight.values().any(|b| *b);
        drop(a);
        quiet = if busy { 0 } else { quiet + 1 };
        std::thread::sleep(Duration::from_millis(2));
    }
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;

    // Loaded: 60 frames, no input.
    let (mut app_t, mut step_t) = (Vec::new(), Vec::new());
    for _ in 0..60 {
        let t = Instant::now();
        harness.step();
        step_t.push(t.elapsed().as_secs_f64() * 1000.0);
        app_t.push(app_ms.get());
    }
    app_t.sort_by(f64::total_cmp);
    step_t.sort_by(f64::total_cmp);
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;

    // Idle: an eframe-style reactive loop. egui tells the integration when it
    // next wants a frame through the repaint callback (immediately, after a
    // delay, or from the network thread); draw only when that moment arrives.
    let wake: std::sync::Arc<std::sync::Mutex<Option<Instant>>> = Default::default();
    let w = wake.clone();
    harness.ctx.set_request_repaint_callback(move |info| {
        let at = Instant::now() + info.delay;
        let mut slot = w.lock().unwrap();
        *slot = Some(slot.map_or(at, |s| s.min(at)));
    });
    harness.step(); // one frame so this frame's own requests go through the callback
    let idle_start = Instant::now();
    let mut idle_frames = 0;
    let mut causes: Vec<String> = Vec::new();
    while idle_start.elapsed() < Duration::from_secs(3) {
        let due = wake.lock().unwrap().is_some_and(|at| Instant::now() >= at);
        if due {
            *wake.lock().unwrap() = None;
            harness.step();
            idle_frames += 1;
            for c in harness.ctx.repaint_causes() {
                let c = c.to_string();
                if !causes.contains(&c) {
                    causes.push(c);
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    eprintln!(
        "{name:<9} {load_ms:>8.0} {:>5} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>11.1}",
        seen.len(),
        mean(&app_t),
        pct(&app_t, 0.95),
        mean(&step_t),
        pct(&step_t, 0.95),
        idle_frames as f64 / 3.0,
    );
    eprintln!("          worst frame while loading: {worst_load_frame:.1} ms");
    let waves: Vec<String> = seen.iter().map(|(k, f)| format!("{k}@{f}")).collect();
    eprintln!("          requests (key@frame): {}", waves.join(" "));
    if !causes.is_empty() {
        eprintln!("          idle repaint causes: {}", causes.join(" | "));
    }
}

/// Where Home's frame goes: the work `views/home.rs` does before drawing
/// anything, timed on the real payloads. Per frame it only takes the shared
/// payloads; the sort and the index run once per new reply (`derive`).
#[test]
#[ignore = "needs a seeded server: FRAMES=1 scripts/perf.sh"]
fn home_breakdown() {
    use serde_json::Value;
    let url = std::env::var("ACP_PERF_URL").expect("ACP_PERF_URL");
    let token = std::env::var("ACP_PERF_TOKEN").expect("ACP_PERF_TOKEN");
    let client = acp_server::cli::client::Client::new(url, token);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let get = |path: &str| rt.block_on(client.request(reqwest::Method::GET, path, Value::Null)).1.unwrap();
    let tasks = get("/api/user/tasks");
    let home = get("/api/user/home");

    let time = |label: &str, f: &mut dyn FnMut()| {
        let mut v: Vec<f64> = (0..30)
            .map(|_| {
                let t = Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        v.sort_by(f64::total_cmp);
        eprintln!("  {label:<52} {:>7.2} ms", v[v.len() / 2]);
    };
    let n = tasks.as_array().map_or(0, Vec::len);
    eprintln!("\nhome per-frame work, median of 30, {n} tasks:");
    let (tasks_arc, home_arc) = (std::sync::Arc::new(tasks.clone()), std::sync::Arc::new(home.clone()));
    time("net.shared(TASKS)+shared(HOME)  home.rs (per frame)", &mut || {
        drop(std::hint::black_box((tasks_arc.clone(), home_arc.clone())));
    });
    fn str_at<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
        v.get(k).and_then(Value::as_str)
    }
    time("sort all by createdAt           home.rs derive (once)", &mut || {
        let mut all: Vec<&Value> = tasks.as_array().unwrap().iter().collect();
        all.sort_by(|a, b| str_at(b, "createdAt").unwrap_or_default().cmp(str_at(a, "createdAt").unwrap_or_default()));
        std::hint::black_box(all);
    });
    time("by_id HashMap                   home.rs derive (once)", &mut || {
        let m: std::collections::HashMap<&str, &Value> = tasks
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| Some((t.get("id")?.as_str()?, t)))
            .collect();
        std::hint::black_box(m);
    });
    time("serde_json parse of /tasks (net thread, once)", &mut || {
        let s = serde_json::to_string(&tasks).unwrap();
        drop(std::hint::black_box(serde_json::from_str::<Value>(&s).unwrap()));
    });
}
