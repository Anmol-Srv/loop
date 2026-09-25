//! The agent page: opened from its card and from its name on a task, its
//! sections, their empty states, a run row with its error, and that it sits
//! still when nothing is working. Driven from fixture JSON in the overview's
//! shape (`GET /api/user/agents/{id}/overview`).
//!
//! `renders` draws a worker agent and an intake agent at 1440 and 820 into
//! docs/design-mocks/render/agent-page/:
//!   cargo test --features app --test agent_page_ui -- --ignored --nocapture
#![cfg(feature = "app")]

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use acp_server::desktop::design::theme;
use acp_server::desktop::net::Net;
use acp_server::desktop::{views, App, Tab};
use chrono::{Local, Utc};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value};

const ME: &str = "aaaaaaaa-0000-0000-0000-000000000001";
const TASK: &str = "22222222-0000-0000-0000-000000000009";
const HERMES: &str = "99999999-0000-0000-0000-000000000001";
const SLACK: &str = "99999999-0000-0000-0000-000000000002";
const FRESH: &str = "99999999-0000-0000-0000-000000000003";

const NOW: &str = "Running the rubric service tests against the new bucket names";

fn ago(minutes: i64) -> String {
    (Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339()
}

fn day(back: i64) -> String {
    (Local::now().date_naive() - chrono::Duration::days(back)).format("%Y-%m-%d").to_string()
}

fn me() -> Value {
    json!({"personId": ME, "email": "anmol@airtribe.live", "name": "Anmol Srivastava", "role": "member", "scopes": ["read", "write"]})
}

fn hermes(working: bool) -> Value {
    let current = if working {
        json!({"id": TASK, "title": "Rate leads by role bucket", "state": "working", "now": NOW})
    } else {
        Value::Null
    };
    json!({"id": HERMES, "handle": "hermes", "name": "Hermes (Anmol's Mac)", "runtime": "hermes", "status": "connected",
           "connectedAt": ago(30000), "lastSeenAt": ago(1), "createdAt": ago(30100), "activeTasks": 2,
           "setup": {"skill": "1.4.0", "mcp": true, "watcher": true}, "currentTask": current,
           "activity": [0, 2, 5, 3, 0, 0, 4, 7, 6, 2, 0, 3, 8, 5], "canIntake": false,
           "intakeStats": {"triage": 0, "accepted": 0, "dismissed": 0}})
}

fn slack() -> Value {
    json!({"id": SLACK, "handle": "slack-agent", "name": "Slack Agent", "runtime": "claude-code", "status": "connected",
           "connectedAt": ago(9000), "lastSeenAt": ago(4), "createdAt": ago(9100), "activeTasks": 0,
           "setup": {"skill": "1.4.0", "mcp": true, "watcher": false}, "currentTask": null,
           "activity": [1, 0, 3, 2, 0, 0, 1, 4, 2, 0, 0, 1, 3, 2], "canIntake": true,
           "intakeStats": {"triage": 4, "accepted": 14, "dismissed": 5}})
}

fn fresh() -> Value {
    json!({"id": FRESH, "handle": "codex", "name": "Codex", "runtime": "codex", "status": "connected",
           "connectedAt": ago(20), "lastSeenAt": ago(2), "createdAt": ago(30), "activeTasks": 0,
           "setup": {"skill": "1.4.0", "mcp": true, "watcher": true}, "currentTask": null, "activity": [], "canIntake": false})
}

fn mine() -> Value {
    json!([hermes(true), slack(), fresh()])
}

fn stats(v: Value) -> Value {
    let mut s = json!({"tasksHandled": 0, "tasksDone": 0, "inReviewNow": 0, "questionsAsked": 0, "medianAckMinutes": null,
                       "filed": 0, "accepted": 0, "dismissed": 0, "acceptRate": null});
    for (k, x) in v.as_object().unwrap() {
        s[k] = x.clone();
    }
    s
}

fn days(f: impl Fn(i64) -> (i64, i64, i64)) -> Value {
    Value::Array(
        (0..30)
            .map(|i| {
                let (notes, filed, events) = f(i);
                json!({"day": day(29 - i), "notes": notes, "filed": filed, "events": events})
            })
            .collect(),
    )
}

fn row(id: &str, title: &str, status: &str, state: Option<&str>, project: Option<&str>, updated: i64, filed: bool) -> Value {
    json!({"id": id, "title": title, "status": status, "priority": 2, "agentState": state, "projectName": project,
           "updatedAt": ago(updated), "delegatedAt": ago(updated + 60), "filed": filed})
}

fn recent(kind: &str, minutes: i64, task: &str, text: &str) -> Value {
    json!({"at": ago(minutes), "kind": kind, "taskId": TASK, "taskTitle": task, "text": text})
}

fn hermes_overview() -> Value {
    json!({
        "agent": hermes(true),
        "stats": stats(json!({"tasksHandled": 12, "tasksDone": 9, "inReviewNow": 1, "questionsAsked": 4, "medianAckMinutes": 3.4})),
        "activity": days(|i| (((i * 7) % 5 + (i % 3)) * i64::from(i % 6 != 0), 0, (i % 4) * i64::from(i % 6 != 0))),
        "recent": [
            recent("progress", 6, "Rate leads by role bucket", "Added roleBucket to the payload; writing tests for the unscored path."),
            recent("instruction", 60, "Rate leads by role bucket", "Keep the v3 bucket names; the rubric doc is out of date."),
            recent("answer", 95, "Rate leads by role bucket", "Default bucket \u{2014} call it \u{201c}unrated\u{201d}."),
            recent("question", 100, "Rate leads by role bucket", "Should unscored leads fall into the default bucket, or be left out of the payload?"),
            recent("handed_off", 130, "Rate leads by role bucket", ""),
            recent("submission", 1500, "Refund webhook retries", "Retries now back off exponentially up to 5 times; the dead-letter queue catches the rest."),
            recent("approved", 2900, "Payout CSV export", ""),
            recent("changes_requested", 3100, "Payout CSV export", "Use the finance column names, not ours."),
        ],
        "tasks": {
            "active": [
                row(TASK, "Rate leads by role bucket", "in_progress", Some("working"), Some("Lead Rating"), 6, false),
                row("t2", "Refund webhook retries", "in_progress", Some("in_review"), Some("Payments"), 1500, false),
            ],
            "recent": [
                row("t3", "Payout CSV export", "completed", Some("done"), Some("Payments"), 2900, false),
                row("t4", "Lead list pagination", "open", None, Some("Lead Rating"), 7000, false),
            ],
        },
        "runs": [],
        "logs": [
            {"taskId": "t2", "taskTitle": "Refund webhook retries", "seq": 41, "text": "$ npx jest api/webhooks/refund --silent", "at": ago(1510)},
            {"taskId": "t2", "taskTitle": "Refund webhook retries", "seq": 42, "text": "PASS  api/webhooks/refund/retry.spec.js (6 tests)", "at": ago(1509)},
            {"taskId": TASK, "taskTitle": "Rate leads by role bucket", "seq": 1, "text": "$ git switch -c feat/role-bucket", "at": ago(120)},
            {"taskId": TASK, "taskTitle": "Rate leads by role bucket", "seq": 2, "text": "$ rg -n \"roleBucket\" api/", "at": ago(119)},
            {"taskId": TASK, "taskTitle": "Rate leads by role bucket", "seq": 3, "text": "api/routes/admin/leads.js:88  // TODO roleBucket", "at": ago(118)},
            {"taskId": TASK, "taskTitle": "Rate leads by role bucket", "seq": 4, "text": "$ npx jest api/services/rubric --silent", "at": ago(20)},
            {"taskId": TASK, "taskTitle": "Rate leads by role bucket", "seq": 5, "text": "PASS  api/services/rubric/bucket.spec.js (4 tests)", "at": ago(19)},
        ],
    })
}

fn run(id: i64, minutes: i64, status: &str, counts: Value, summary: &str, error: Option<&str>) -> Value {
    json!({"id": id, "startedAt": ago(minutes + 1), "finishedAt": ago(minutes), "status": status, "summary": summary,
           "counts": counts, "error": error})
}

const RUN_ERROR: &str = "conversations.history for #checkout-bugs: 429 ratelimited (retry after 30s)";

fn slack_overview() -> Value {
    let mut runs = vec![
        run(40, 12, "ok", json!({"filed": 2, "appended": 1, "alreadyFiled": 0, "skipped": 5}), "Read #issues-and-feedback, #checkout-bugs", None),
        run(39, 27, "partial", json!({"filed": 1, "appended": 0, "alreadyFiled": 2, "skipped": 3}), "Read 2 of 3 channels", Some(RUN_ERROR)),
        run(38, 42, "ok", json!({}), "Read #issues-and-feedback, #checkout-bugs", None),
        run(37, 57, "failed", json!({}), "Did not run", Some("Slack token expired: invalid_auth")),
    ];
    for i in 0..10 {
        runs.push(run(36 - i, 72 + i * 15, "ok", json!({"skipped": (i % 3) + 1}), "Read #issues-and-feedback, #checkout-bugs", None));
    }
    json!({
        "agent": slack(),
        "stats": stats(json!({"filed": 23, "accepted": 14, "dismissed": 5, "acceptRate": 0.7368})),
        "activity": days(|i| (0, (i * 3 % 4) * i64::from(i % 7 != 5), 0)),
        "recent": [
            recent("filed", 12, "Checkout fails for saved cards", "#checkout-bugs"),
            recent("filed", 13, "Refund email has the wrong amount", "#issues-and-feedback"),
            recent("filed", 1700, "Coupon field accepts expired codes", "#issues-and-feedback"),
        ],
        "tasks": {
            "active": [
                row("f1", "Checkout fails for saved cards", "triage", None, None, 12, true),
                row("f2", "Refund email has the wrong amount", "triage", None, None, 13, true),
            ],
            "recent": [row("f3", "Coupon field accepts expired codes", "open", None, None, 1600, true)],
        },
        "runs": runs,
        "logs": [],
    })
}

fn empty_overview() -> Value {
    json!({"agent": fresh(), "stats": stats(json!({})), "activity": days(|_| (0, 0, 0)), "recent": [],
           "tasks": {"active": [], "recent": []}, "runs": [], "logs": []})
}

fn task_json() -> Value {
    json!({
        "id": TASK, "title": "Rate leads by role bucket", "status": "in_progress", "discipline": "backend",
        "body": "Expose the bucket on the lead payload.", "priority": 1,
        "projectId": "11111111-0000-0000-0000-000000000001", "projectName": "Lead Rating",
        "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "assigneeEmail": "anmol@airtribe.live",
        "assigneeKind": "person", "createdAt": ago(4000), "updatedAt": ago(10), "doneAt": null,
        "delegate": {"id": HERMES, "handle": "hermes", "name": "Hermes (Anmol's Mac)", "state": "working", "lastSeenAt": ago(1),
                     "now": NOW, "nowAt": ago(1), "delegatedAt": ago(130), "runtime": "hermes", "ownerName": "Anmol Srivastava"},
        "reviewTarget": null, "canSeeAgentPrivate": true,
    })
}

type Fixtures = Vec<(String, Value)>;

fn fixtures(working: bool) -> Fixtures {
    let mut agents = mine();
    agents[0] = hermes(working);
    let mut h = hermes_overview();
    h["agent"] = hermes(working);
    vec![
        ("__me".into(), me()),
        ("sidebar:counts".into(), json!({"myOpen": 4, "activeProjects": 2})),
        ("__tracks".into(), json!({})),
        ("board:people".into(), json!([])),
        ("agents:mine".into(), agents),
        (format!("agent:overview:{HERMES}"), h),
        (format!("agent:overview:{SLACK}"), slack_overview()),
        (format!("agent:overview:{FRESH}"), empty_overview()),
        ("task:one".into(), task_json()),
        ("task:notes".into(), json!([])),
        ("task:artifacts".into(), json!([])),
    ]
}

struct Page<'a> {
    harness: Harness<'a>,
}

impl Page<'_> {
    fn steps(&mut self, n: usize) {
        for _ in 0..n {
            self.harness.step();
        }
    }
    fn has(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }
    fn has_part(&self, label: &str) -> bool {
        self.harness.query_all_by_label_contains(label).next().is_some()
    }
    fn click(&mut self, label: &str) {
        self.harness.get_all_by_label(label).next().unwrap_or_else(|| panic!("no {label}")).click_accesskit();
        self.steps(3);
    }
    /// Through accessibility, as a screen reader would: a pointer at the
    /// card's centre can land on the task link inside it, which opens the
    /// task instead.
    fn open(&mut self, name: &str) {
        let label = format!("Open {name}");
        self.harness.get_all_by_label(&label).next().unwrap_or_else(|| panic!("no {label}")).click_accesskit();
        self.steps(3);
    }
    fn fastest_wake(&mut self, frames: usize) -> Duration {
        let fastest = Arc::new(Mutex::new(Duration::MAX));
        let f = fastest.clone();
        self.harness.ctx.set_request_repaint_callback(move |info| {
            let mut slot = f.lock().unwrap();
            *slot = (*slot).min(info.delay);
        });
        self.steps(frames);
        let d = *fastest.lock().unwrap();
        d
    }
}

fn silent_server() -> &'static str {
    static URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    URL.get_or_init(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::mem::forget(listener);
        url
    })
}

fn page<'a>(
    app: &'a RefCell<Option<App>>,
    fixtures: &'a Fixtures,
    tab: Tab,
    task: Option<&'a str>,
    size: (f32, f32),
    gpu: bool,
    still: bool,
) -> Page<'a> {
    let builder = Harness::builder().with_size(egui::vec2(size.0, size.1)).with_pixels_per_point(2.0);
    let builder = if gpu { builder.wgpu() } else { builder };
    let mut harness = builder.build_ui(move |ui| {
        let mut slot = app.borrow_mut();
        if slot.is_none() {
            theme::install(ui.ctx());
            let time = if still { 0.0 } else { egui::Style::default().animation_time };
            ui.ctx().all_styles_mut(|s| s.animation_time = time);
            ui.ctx().set_zoom_factor(1.15);
            *slot = Some(App {
                net: Some(Net::spawn(silent_server().into(), "test".into(), ui.ctx().clone())),
                tab,
                project: None,
                task: task.map(str::to_owned),
                scopes: Vec::new(),
                login: views::login::State::default(),
                board: views::board::State::default(),
                palette: views::palette::State::default(),
                settings: views::settings::State::default(),
            });
            return;
        }
        let a = slot.as_mut().unwrap();
        for (k, v) in fixtures {
            a.net.as_mut().unwrap().seed(k, v.clone());
        }
        a.frame(ui);
    });
    for _ in 0..6 {
        harness.step();
    }
    Page { harness }
}

fn agents_page<'a>(app: &'a RefCell<Option<App>>, f: &'a Fixtures) -> Page<'a> {
    page(app, f, Tab::Agents, None, (1440.0, 1400.0), false, false)
}

// ------------------------------------------------------------------ behaviour

#[test]
fn a_card_opens_the_page_and_back_returns() {
    let f = fixtures(true);
    let app = RefCell::new(None);
    let mut p = agents_page(&app, &f);
    assert!(p.has("Connect an agent"));
    assert!(p.has("Files into your Triage, each task labelled Slack."), "the card says where filings land");
    p.open("Hermes (Anmol's Mac)");
    assert_eq!(views::agents::opened().as_deref(), Some(HERMES));
    assert!(p.has("Last 30 days"));
    assert!(p.has("Handed to it: 12") && p.has("Approved: 9") && p.has("First reply: 3m"));
    assert!(p.has(NOW), "the now line heads the page");
    assert!(p.has("Only you can see this"));
    assert!(p.has("Skill installed: yes") || p.has("Skill: yes"), "the setup checklist");
    assert!(p.has_part("Activity, last 30 days"));

    p.click("All agents");
    assert!(views::agents::opened().is_none());
    assert!(p.has("Connect an agent"));
}

#[test]
fn a_narrow_window_folds_the_rail_and_back_still_works() {
    let f = fixtures(true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Agents, None, (820.0, 1400.0), false, false);
    p.open("Hermes (Anmol's Mac)");
    assert!(p.has("Skill: yes") && p.has("Handed to it: 12"), "facts in a line, figures under them");
    assert!(!p.has("Runtime"), "no rail at this width");
    p.click("All agents");
    assert!(views::agents::opened().is_none());
    assert!(p.has("Connect an agent"));
}

#[test]
fn sections_show_activity_tasks_and_logs() {
    let f = fixtures(true);
    let app = RefCell::new(None);
    let mut p = agents_page(&app, &f);
    p.open("Hermes (Anmol's Mac)");
    // Activity is the first tab: the timeline, across tasks.
    assert!(p.has_part("Added roleBucket to the payload"));
    assert!(p.has_part("Should unscored leads fall into"), "the owner reads questions here");
    assert!(p.has("posted an update") && p.has("submitted it for review") && p.has("asked for changes"));

    p.click("Tasks 4");
    assert!(p.has("Active") && p.has("Recent"));
    assert!(p.has("Refund webhook retries") && p.has("In review"));
    assert!(p.has("Payout CSV export"));

    p.click("Logs 7");
    assert!(p.has("PASS  api/services/rubric/bucket.spec.js (4 tests)"));
    p.harness.get_by_label("Copy all").click();
    p.steps(1);
    let copied = p.harness.output().platform_output.commands.iter().any(|c| {
        matches!(c, egui::OutputCommand::CopyText(t) if t.contains("# Refund webhook retries\n$ npx jest") && t.contains("# Rate leads by role bucket"))
    });
    assert!(copied, "Copy all takes every group");

    // A task opens from the page, and its Back comes back to the page.
    p.click("Tasks 4");
    p.harness.get_by_label("Payout CSV export").click();
    p.steps(3);
    assert_eq!(app.borrow().as_ref().unwrap().task.as_deref(), Some("t3"));
    app.borrow_mut().as_mut().unwrap().task = None;
    p.steps(3);
    assert!(p.has("Last 30 days"), "closing the task lands on the agent again");
    p.click("All agents");
}

#[test]
fn runs_show_each_pass_and_fold_the_error() {
    let f = fixtures(true);
    let app = RefCell::new(None);
    let mut p = agents_page(&app, &f);
    p.open("Slack Agent");
    assert!(p.has("Filed: 23") && p.has("Dismissed: 5") && p.has("Accept rate: 74%"));
    assert!(p.has_part("filed it from #checkout-bugs"), "filings are in the activity");
    p.click("Runs 14");
    assert!(p.has("Last 14 runs, 1 failed"));
    assert!(p.has_part("about every 15 min"));
    assert!(p.has("Partial") && p.has("Failed"));
    assert!(p.has("Read 2 of 3 channels"));
    assert!(!p.has(RUN_ERROR), "the error is folded until asked for");
    p.harness.get_all_by_label("Error").next().unwrap().click();
    p.steps(3);
    assert!(p.has(RUN_ERROR));
    p.click("All agents");
}

#[test]
fn empty_sections_teach_what_will_appear() {
    let f = fixtures(true);
    let app = RefCell::new(None);
    let mut p = agents_page(&app, &f);
    p.open("Codex");
    assert!(p.has("Nothing yet"));
    assert!(p.has("First reply: \u{2014}"), "no hand-offs, no median");
    assert!(p.has_part("No task right now"));
    p.click("Tasks");
    assert!(p.has("No tasks yet"));
    p.click("Runs");
    assert!(p.has("No runs reported yet"));
    assert!(p.has_part("A run is one pass an agent makes on a schedule"));
    p.click("Logs");
    assert!(p.has("No step log yet"));
    p.click("All agents");
}

#[test]
fn its_name_on_a_task_opens_it_and_back_returns_to_the_task() {
    let f = fixtures(true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, Some(TASK), (1440.0, 1600.0), false, false);
    assert!(p.has("Agent session"));
    p.open("Hermes (Anmol's Mac)");
    {
        let a = app.borrow();
        let a = a.as_ref().unwrap();
        assert!(a.task.is_none());
        assert!(a.tab == Tab::Agents);
    }
    assert!(p.has("Last 30 days"));
    p.click("Back");
    let a = app.borrow();
    let a = a.as_ref().unwrap();
    assert_eq!(a.task.as_deref(), Some(TASK));
    assert!(a.tab == Tab::Home);
}

#[test]
fn the_page_sleeps_unless_the_agent_is_working() {
    let slow = Duration::from_millis(500);
    let f = fixtures(false);
    let app = RefCell::new(None);
    let mut p = agents_page(&app, &f);
    p.open("Hermes (Anmol's Mac)");
    assert!(p.fastest_wake(20) > slow, "an idle agent's page must not tick");
    p.click("All agents");
    drop(p);

    let f = fixtures(true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Agents, None, (1440.0, 1400.0), false, true);
    p.open("Hermes (Anmol's Mac)");
    assert!(p.has(NOW));
    assert!(p.fastest_wake(20) > slow, "reduced motion: a still ring, no frames");
    p.click("All agents");
}

// -------------------------------------------------------------------- renders

const DIR: &str = "docs/design-mocks/render/agent-page";

fn save(p: &mut Page<'_>, name: &str, label: &str) {
    std::fs::create_dir_all(DIR).unwrap();
    let path = format!("{DIR}/{name}-{label}.png");
    p.harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs: cargo test --features app --test agent_page_ui -- --ignored"]
fn renders() {
    for (width, label) in [(1440.0, "1440"), (820.0, "820")] {
        let f = fixtures(true);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Agents, None, (width, 1500.0), true, true);
        save(&mut p, "agents", label);
        p.open("Hermes (Anmol's Mac)");
        p.steps(3);
        save(&mut p, "worker-activity", label);
        p.click("Tasks 4");
        save(&mut p, "worker-tasks", label);
        p.click("Logs 7");
        save(&mut p, "worker-logs", label);
        p.click("All agents");

        p.open("Slack Agent");
        p.click("Runs 14");
        p.click("Error");
        save(&mut p, "intake-runs", label);
        p.click("Activity");
        save(&mut p, "intake-activity", label);
        p.click("All agents");

        p.open("Codex");
        save(&mut p, "empty", label);
        p.click("All agents");
    }
}
