//! Agent sessions: the task page's session section, Home's "Agents at work",
//! the Agents cards and the three-step Connect flow, driven from fixture JSON
//! in the shapes of docs/superpowers/plans/2026-09-24-agent-sessions-ui.md.
//!
//! The behaviour tests run with the suite. `renders` draws every surface at
//! 1440 and 820 into docs/design-mocks/render/agent-sessions/:
//!   cargo test --features app --test agent_sessions_ui -- --ignored --nocapture
#![cfg(feature = "app")]

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use acp_server::desktop::design::theme;
use acp_server::desktop::net::Net;
use acp_server::desktop::{views, App, Tab};
use chrono::Utc;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use serde_json::{json, Value};

const ME: &str = "aaaaaaaa-0000-0000-0000-000000000001";
const PRIYA: &str = "aaaaaaaa-0000-0000-0000-000000000002";
const TASK: &str = "22222222-0000-0000-0000-000000000009";
const HERMES: &str = "99999999-0000-0000-0000-000000000001";
const CLAUDE: &str = "99999999-0000-0000-0000-000000000002";
const NEW: &str = "99999999-0000-0000-0000-000000000009";

const NOW: &str = "Running the rubric service tests against the new bucket names";
const QUESTION: &str = "Should unscored leads fall into the default bucket, or be left out of the payload?";
const INSTRUCTION: &str = "Keep the v3 bucket names; the rubric doc is out of date.";

fn ago(minutes: i64) -> String {
    (Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339()
}

fn me(person: &str, role: &str) -> Value {
    let (name, email) = if person == ME {
        ("Anmol Srivastava", "anmol@airtribe.live")
    } else {
        ("Priya Nair", "priya@airtribe.live")
    };
    json!({"personId": person, "email": email, "name": name, "role": role, "scopes": ["read", "write"]})
}

fn delegate(state: &str) -> Value {
    let working = matches!(state, "working" | "acknowledged");
    json!({
        "id": HERMES, "handle": "hermes", "name": "Hermes (Anmol's Mac)", "state": state, "lastSeenAt": ago(1),
        "now": if working { json!(NOW) } else { Value::Null }, "nowAt": if working { json!(ago(1)) } else { Value::Null },
        "delegatedAt": ago(130), "runtime": "hermes", "ownerName": "Anmol Srivastava",
    })
}

fn task(state: &str, private: bool) -> Value {
    json!({
        "id": TASK, "title": "Rate leads by role bucket", "status": "in_progress", "discipline": "backend",
        "body": "Backend-engineering leads on the weighted-rubric path get their role bucket from Jev.\n\nExpose the bucket on the lead payload.",
        "priority": 1, "projectId": "11111111-0000-0000-0000-000000000001", "projectName": "Lead Rating",
        "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "assigneeEmail": "anmol@airtribe.live",
        "assigneeKind": "person", "createdAt": ago(4000), "updatedAt": ago(10),
        "doneAt": if state == "done" { json!(ago(2)) } else { Value::Null },
        "delegate": delegate(state), "reviewTarget": if state == "in_review" || state == "done" { json!("completed") } else { Value::Null },
        "canSeeAgentPrivate": private,
    })
}

fn agent_note(id: &str, kind: &str, body: &str, minutes: i64) -> Value {
    json!({"id": id, "kind": kind, "body": body, "authorName": null,
           "agent": {"id": HERMES, "name": "Hermes (Anmol's Mac)"}, "createdAt": ago(minutes)})
}

fn owner_note(id: &str, kind: &str, body: &str, minutes: i64) -> Value {
    json!({"id": id, "kind": kind, "body": body, "authorName": "Anmol Srivastava", "agent": null, "createdAt": ago(minutes)})
}

/// A session's notes up to `state`. Private kinds are included for every
/// viewer: the server filters them, and the page must not show them anyway.
fn notes(state: &str) -> Value {
    let mut n = vec![
        json!({"id": "n0", "kind": "note", "body": "Jev's bucket list is in the rubric doc \u{2014} use the v3 names.",
               "authorName": "Priya Nair", "agent": null, "createdAt": ago(300)}),
        agent_note("n1", "progress", "Picked this up. Reading the lead payload and the rubric service.", 120),
        agent_note("n2", "question", QUESTION, 100),
        owner_note("n3", "answer", "Default bucket \u{2014} call it \u{201c}unrated\u{201d}.", 95),
        owner_note("n4", "instruction", INSTRUCTION, 60),
        agent_note("n5", "progress", "Added roleBucket to the payload; writing tests for the unscored path.", 30),
    ];
    match state {
        "needs_input" => n.push(agent_note("n6", "question", "The rubric service times out on staging. Retry, or mark those leads unrated?", 5)),
        "in_review" | "done" => n.push(agent_note(
            "n6",
            "submission",
            "Added roleBucket to the lead payload behind the rubric check; unscored leads get \u{201c}unrated\u{201d}. Tests cover both paths.",
            4,
        )),
        _ => {}
    }
    if state == "done" {
        n.push(owner_note("n7", "review", "Approved.", 2));
    }
    Value::Array(n)
}

fn artifacts() -> Value {
    json!([
        {"id": "a1", "kind": "pr", "url": "https://github.com/airtribe/mycohort-api/pull/4821", "title": "Role bucket on lead payload"},
        {"id": "a2", "kind": "commit", "url": "a1b2c3d4e5", "title": "Default unscored leads to unrated"},
        {"id": "a3", "kind": "figma", "url": "https://figma.com/file/abc/lead-rating", "title": "Lead card states"},
    ])
}

fn my_agents() -> Value {
    json!([
        {"id": HERMES, "handle": "hermes", "name": "Hermes (Anmol's Mac)", "runtime": "hermes", "status": "connected",
         "connectedAt": ago(3000), "lastSeenAt": ago(1), "createdAt": ago(3100), "activeTasks": 1,
         "setup": {"skill": "1.4.0", "mcp": true, "watcher": true},
         "currentTask": {"id": TASK, "title": "Rate leads by role bucket", "state": "working", "now": NOW},
         "activity": [0, 2, 5, 3, 0, 0, 4, 7, 6, 2, 0, 3, 8, 5]},
        {"id": CLAUDE, "handle": "claude-mac", "name": "Claude Code", "runtime": "claude-code", "status": "connected",
         "connectedAt": ago(9000), "lastSeenAt": ago(400), "createdAt": ago(9100), "activeTasks": 0,
         "setup": {"skill": "1.3.2", "mcp": true, "watcher": false}, "currentTask": null,
         "activity": [1, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]},
        {"id": "99999999-0000-0000-0000-000000000003", "handle": "codex", "name": "Codex", "runtime": "codex", "status": "waiting",
         "connectedAt": null, "lastSeenAt": null, "createdAt": ago(20), "activeTasks": 0},
        {"id": "99999999-0000-0000-0000-000000000004", "handle": "hermes-old", "name": "Hermes (old Mac)", "runtime": "hermes",
         "status": "revoked", "connectedAt": ago(90000), "lastSeenAt": ago(80000), "createdAt": ago(90000), "activeTasks": 0},
        {"id": "99999999-0000-0000-0000-000000000005", "handle": "codex-old", "name": "Codex (old)", "runtime": "codex",
         "status": "revoked", "connectedAt": ago(95000), "lastSeenAt": ago(94000), "createdAt": ago(95000), "activeTasks": 0},
    ])
}

fn active() -> Value {
    json!([
        {"agent": {"id": HERMES, "name": "Hermes (Anmol's Mac)", "handle": "hermes", "runtime": "hermes"},
         "owner": {"id": ME, "name": "Anmol Srivastava"},
         "task": {"id": TASK, "title": "Rate leads by role bucket", "projectName": "Lead Rating"},
         "state": "working", "now": NOW, "nowAt": ago(1), "lastSeenAt": ago(1), "delegatedAt": ago(130)},
        {"agent": {"id": "p1", "name": "Claude Code (Priya's Mac)", "handle": "claude-priya", "runtime": "claude-code"},
         "owner": {"id": PRIYA, "name": "Priya Nair"},
         "task": {"id": "t2", "title": "Lead list pagination", "projectName": "Lead Rating"},
         "state": "needs_input", "now": null, "nowAt": null, "lastSeenAt": ago(3), "delegatedAt": ago(45)},
        {"agent": {"id": "p2", "name": "Codex", "handle": "codex-rahul", "runtime": "codex"},
         "owner": {"id": "r1", "name": "Rahul Mehta"},
         "task": {"id": "t3", "title": "Checkout copy pass", "projectName": "Checkout redesign"},
         "state": "in_review", "now": null, "nowAt": null, "lastSeenAt": ago(8), "delegatedAt": ago(1500)},
    ])
}

type Fixtures = Vec<(&'static str, Value)>;

fn base(viewer: Value) -> Fixtures {
    vec![
        ("__me", viewer),
        ("sidebar:counts", json!({"myOpen": 4, "activeProjects": 2})),
        ("__tracks", json!({})),
        ("board:people", json!([])),
        ("task:artifacts", artifacts()),
    ]
}

fn task_fixtures(state: &str, owner: bool) -> Fixtures {
    let mut f = base(if owner { me(ME, "member") } else { me(PRIYA, "member") });
    f.push(("agents:mine", if owner { my_agents() } else { json!([]) }));
    f.push(("task:one", task(state, owner)));
    f.push(("task:notes", notes(state)));
    f
}

fn home_fixtures(agents_active: Value) -> Fixtures {
    let mut f = base(me(ME, "member"));
    f.push(("agents:mine", my_agents()));
    f.push(("agents:active", agents_active));
    f.push(("home", json!({"myTasks": [], "waitingOnMe": [], "team": [], "needsAttention": [],
        "projects": [{"id": "11111111-0000-0000-0000-000000000001", "name": "Lead Rating", "status": "active", "done": 3, "total": 8}]})));
    let mut t = task("working", true);
    t["blockersDone"] = json!(0);
    t["blockersTotal"] = json!(0);
    f.push(("home:tasks", json!([t,
        {"id": "t2", "title": "Lead list pagination", "status": "in_progress", "discipline": "frontend", "priority": 2,
         "projectId": "11111111-0000-0000-0000-000000000001", "projectName": "Lead Rating", "assigneeName": "Priya Nair",
         "createdAt": ago(5000), "updatedAt": ago(50), "doneAt": null, "delegate": null},
        {"id": "t3", "title": "Checkout copy pass", "status": "in_progress", "discipline": "design", "priority": 1,
         "projectId": "11111111-0000-0000-0000-000000000001", "projectName": "Lead Rating", "assigneeName": "Rahul Mehta",
         "createdAt": ago(6000), "updatedAt": ago(90), "doneAt": null, "delegate": null},
    ])));
    f
}

struct Page<'a> {
    harness: Harness<'a>,
    app: &'a RefCell<Option<App>>,
}

impl Page<'_> {
    fn seed(&self, key: &str, value: Value) {
        self.app.borrow_mut().as_mut().unwrap().net.as_mut().unwrap().seed(key, value);
    }
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
    fn placeholder(&self, starts: &str) -> bool {
        self.harness.query_by(|n| n.placeholder().is_some_and(|h| h.starts_with(starts))).is_some()
    }
    fn type_into(&mut self, starts: &str, text: &str) {
        self.harness.get_by(|n| n.placeholder().is_some_and(|h| h.starts_with(starts))).focus();
        self.steps(1);
        self.harness.get_by(|n| n.placeholder().is_some_and(|h| h.starts_with(starts))).type_text(text);
        self.steps(2);
    }
    fn disabled(&self, label: &str) -> bool {
        self.harness.get_by_label(label).accesskit_node().is_disabled()
    }
    /// The shortest wake-up any frame asked for, over `frames` frames with no
    /// input: what an eframe loop would draw at when the window sits idle.
    fn fastest_wake(&mut self, frames: usize) -> Duration {
        let fastest = Arc::new(Mutex::new(Duration::MAX));
        let f = fastest.clone();
        self.harness.ctx.set_request_repaint_callback(move |info| {
            let mut slot = f.lock().unwrap();
            *slot = (*slot).min(info.delay);
        });
        let mut causes: Vec<String> = Vec::new();
        for _ in 0..frames {
            self.harness.step();
            for c in self.harness.ctx.repaint_causes() {
                let c = c.to_string();
                if !causes.contains(&c) {
                    causes.push(c);
                }
            }
        }
        let d = *fastest.lock().unwrap();
        eprintln!("fastest wake {d:?}; causes: {}", causes.join(" | "));
        d
    }
}

/// A server that takes the connection and never answers, so a request a
/// test sends stays in flight until the test seeds its reply — a refused
/// connection would fail it first, and race the seed.
fn silent_server() -> &'static str {
    static URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    URL.get_or_init(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::mem::forget(listener);
        url
    })
}

/// `still` turns motion off the way `AIRTRIBE_REDUCE_MOTION` does, without
/// touching the process environment the other tests share.
#[allow(clippy::too_many_arguments)]
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
            // The harness turns animation off; the app runs with it on.
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
    Page { harness, app }
}

fn task_page<'a>(app: &'a RefCell<Option<App>>, f: &'a Fixtures) -> Page<'a> {
    page(app, f, Tab::Home, Some(TASK), (1440.0, 1600.0), false, false)
}

// ------------------------------------------------------------------ behaviour

#[test]
fn stepper_follows_the_state() {
    for (state, expect) in [
        ("handed_off", ["Handed off: current", "Working: next", "In review: next", "Done: next"]),
        ("working", ["Handed off: done", "Working: current", "In review: next", "Done: next"]),
        ("needs_input", ["Handed off: done", "Working: done", "Needs you: current", "Done: next"]),
        ("in_review", ["Handed off: done", "Working: done", "In review: current", "Done: next"]),
        ("done", ["Handed off: done", "Working: done", "In review: done", "Done: done"]),
    ] {
        let f = task_fixtures(state, true);
        let app = RefCell::new(None);
        let p = task_page(&app, &f);
        for label in expect {
            assert!(p.has(label), "{state}: {label}");
        }
    }
    // A teammate reads whose turn it is, not "you".
    let f = task_fixtures("needs_input", false);
    let app = RefCell::new(None);
    let p = task_page(&app, &f);
    assert!(p.has("Needs Anmol: current"));
    assert!(p.has("Waiting on Anmol\u{2019}s answer"));
}

#[test]
fn owner_sees_the_private_parts_and_a_teammate_does_not() {
    let f = task_fixtures("working", true);
    let app = RefCell::new(None);
    let p = task_page(&app, &f);
    assert!(p.has("Agent session"));
    assert!(p.has(NOW), "the now line");
    assert!(p.has("Logs"));
    assert!(p.placeholder("Tell Hermes"));
    assert!(p.has("Only you can see this"));
    assert!(p.has(INSTRUCTION));
    assert!(p.has(QUESTION));
    drop(p);

    let f = task_fixtures("working", false);
    let app = RefCell::new(None);
    let p = task_page(&app, &f);
    assert!(p.has("Agent session"));
    assert!(p.has(NOW), "the now line is team-visible");
    assert!(p.has_part("Added roleBucket to the payload"), "progress is team-visible");
    assert!(!p.has("Logs"));
    assert!(!p.placeholder("Tell Hermes"));
    assert!(!p.has("Only you can see this"));
    assert!(!p.has(INSTRUCTION));
    assert!(!p.has(QUESTION));
    assert!(!p.has_part("Default bucket"));
    // The team thread keeps what people said to each other.
    assert!(p.has_part("use the v3 names"));
}

#[test]
fn logs_open_on_demand_and_copy() {
    let f = task_fixtures("working", true);
    let app = RefCell::new(None);
    let mut p = task_page(&app, &f);
    assert!(!p.has("Copy log"), "nothing fetched or shown until opened");
    p.harness.get_by_label("Logs").click();
    p.steps(2);
    p.seed("task:logs", json!([
        {"seq": 1, "text": "$ cargo test -p rubric"},
        {"seq": 2, "text": "test bucket::unscored_is_unrated ... ok"},
    ]));
    p.steps(3);
    assert!(p.has("test bucket::unscored_is_unrated ... ok"));
    p.harness.get_by_label("Copy log").click();
    p.steps(1);
    let copied = p.harness.output().platform_output.commands.iter().any(|c| {
        matches!(c, egui::OutputCommand::CopyText(t) if t.contains("$ cargo test -p rubric\ntest bucket"))
    });
    assert!(copied, "Copy log puts every line on the clipboard");
}

#[test]
fn instruct_sends_on_cmd_enter() {
    let f = task_fixtures("working", true);
    let app = RefCell::new(None);
    let mut p = task_page(&app, &f);
    assert!(p.disabled("Send to Hermes"));
    p.type_into("Tell Hermes", "Skip the staging retries.");
    assert!(!p.disabled("Send to Hermes"));
    p.harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Enter);
    p.steps(1);
    p.seed("task:agent", json!({}));
    p.steps(2);
    assert!(p.has_part("the agent hears it at its next step"));
}

#[test]
fn question_takes_an_answer() {
    let f = task_fixtures("needs_input", true);
    let app = RefCell::new(None);
    let mut p = task_page(&app, &f);
    assert!(p.has("asked"));
    assert!(p.disabled("Send answer"));
    p.type_into("Answer Hermes", "Mark them unrated.");
    p.harness.get_by_label("Send answer").click();
    p.steps(1);
    p.seed("task:agent", json!({}));
    p.steps(2);
    assert!(p.has("Answer sent."));
}

#[test]
fn report_card_reviews() {
    let f = task_fixtures("in_review", true);
    let app = RefCell::new(None);
    let mut p = task_page(&app, &f);
    assert!(p.has("Report"));
    assert!(p.has("Pull request: Role bucket on lead payload"));
    assert!(p.has("Commit: a1b2c3d Default unscored leads to unrated"));
    assert!(!p.disabled("Approve"));
    p.harness.get_by_label("Request changes").click();
    p.steps(2);
    assert!(p.placeholder("What should Hermes change?"));
    assert!(p.disabled("Send back"));
    p.type_into("What should Hermes change?", "Leave unscored leads out.");
    assert!(!p.disabled("Send back"));
    p.harness.get_by_label("Send back").click();
    p.steps(1);
    p.seed("task:agent", json!({}));
    p.steps(2);
    assert!(p.has("Changes requested."));
    drop(p);

    // A teammate reads the report, not the controls.
    let f = task_fixtures("in_review", false);
    let app = RefCell::new(None);
    let p = task_page(&app, &f);
    assert!(p.has("Report"));
    assert!(!p.has("Approve"));
    assert!(p.has("Waiting on Anmol\u{2019}s review."));
}

#[test]
fn home_strip_shows_agents_at_work_and_hides_when_empty() {
    let f = home_fixtures(json!([]));
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::Home, None, (1440.0, 1300.0), false, false);
    assert!(!p.has("Agents at work"));
    drop(p);

    let f = home_fixtures(active());
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, None, (1440.0, 1300.0), false, false);
    assert!(p.has("Agents at work"));
    assert!(p.has("Waiting on Priya"));
    p.harness.get_by_label("Hermes, Anmol\u{2019}s agent, on Rate leads by role bucket").click();
    p.steps(2);
    assert_eq!(app.borrow().as_ref().unwrap().task.as_deref(), Some(TASK));
}

#[test]
fn working_ring_repaints_only_while_seen_and_moving() {
    let slow = Duration::from_millis(500);
    // Nobody at work: Home sleeps.
    let f = home_fixtures(json!([]));
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, None, (1440.0, 1300.0), false, false);
    assert!(p.fastest_wake(20) > slow, "idle Home must not tick");
    drop(p);

    // A working agent on screen: the ring asks for its next frame. (It asks
    // for 33 ms; egui takes its predicted frame time off before it reports.)
    let f = home_fixtures(active());
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, None, (1440.0, 1300.0), false, false);
    let wake = p.fastest_wake(20);
    assert!(wake <= Duration::from_millis(40), "{wake:?}");
    drop(p);

    // The same, scrolled out of sight: nothing.
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, None, (1440.0, 420.0), false, false);
    assert!(p.fastest_wake(20) > slow, "an off-screen ring must not tick");
    drop(p);

    // Reduced motion: a still ring, no frames.
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, None, (1440.0, 1300.0), false, true);
    assert!(p.has("Agents at work"));
    assert!(p.fastest_wake(20) > slow, "reduced motion must not tick");
}

#[test]
fn connect_goes_through_three_steps() {
    let mut f = base(me(ME, "member"));
    f.retain(|(k, _)| *k != "task:artifacts");
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Agents, None, (1440.0, 1300.0), false, false);
    p.seed("agents:mine", my_agents());
    p.steps(2);

    p.harness.get_by_label("Connect an agent").click();
    p.steps(2);
    assert!(p.has("Name it: current"));
    p.harness.get_by(|n| n.role() == egui::accesskit::Role::RadioButton && n.label().as_deref() == Some("Claude Code")).click();
    p.steps(2);
    assert!(p.placeholder("claude-code"), "the handle hint follows the runtime");
    assert!(p.disabled("Continue"));
    p.type_into("claude-code", "Claude Mac");
    assert!(p.has_part("Lowercase letters, digits and dashes"));
    for _ in 0.."Claude Mac".len() {
        p.harness.key_press(egui::Key::Backspace);
    }
    p.steps(1);
    p.harness.get_by(|n| n.placeholder() == Some("claude-code")).type_text("claude-mac");
    p.steps(1);
    p.type_into("Claude Code (Anmol", "Claude Code (studio)");
    assert!(!p.disabled("Continue"));
    p.harness.get_by_label("Continue").click();
    p.steps(1);

    let prompt = "You are being connected to Airtribe Control Plane as agent \"claude-mac\". Token: acp_secret123";
    p.seed("agents:action", json!({"agent": {"id": NEW, "name": "Claude Code (studio)"}, "token": "acp_secret123", "prompt": prompt}));
    p.steps(3);
    assert!(p.has("Paste this into Claude Code"));
    assert!(p.has("Name it: done") && p.has("Paste the prompt: current"));
    assert!(p.has_part("Shown only once"));
    assert!(p.harness.query_by_value(prompt).is_some());
    p.harness.get_by_label("Copy").click();
    p.steps(2);
    assert!(p.has("Copied"));
    p.harness.get_by_label("I\u{2019}ve pasted it").click();
    p.steps(2);
    assert!(p.harness.query_by_value(prompt).is_none(), "the prompt is gone once past it");
    assert!(p.has("Waiting for Claude Code to say hello\u{2026}"));
    assert!(p.has("First contact: current"));

    let mut mine = my_agents();
    mine.as_array_mut().unwrap().push(json!({"id": NEW, "handle": "claude-mac", "name": "Claude Code (studio)",
        "runtime": "claude-code", "status": "connected", "lastSeenAt": ago(0), "createdAt": ago(1), "activeTasks": 0,
        "setup": {"skill": "1.4.0", "mcp": true, "watcher": false}}));
    p.seed("agents:mine", mine);
    p.steps(3);
    assert!(p.has("Claude Code is connected"));
    assert!(p.has("First contact: done"));
    assert!(p.has("Skill installed: yes"));
    assert!(p.has("MCP server registered: yes"));
    assert!(p.has("Watcher running: no"));
    p.harness.get_by_label("Done").click();
    p.steps(2);
    assert!(!p.has("Claude Code is connected"));
}

#[test]
fn agents_page_is_cards_with_a_menu_and_revoked_tucked_away() {
    let mut f = base(me(ME, "member"));
    f.push(("agents:mine", my_agents()));
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Agents, None, (1440.0, 1300.0), false, false);
    assert!(p.has("Skill: yes") && p.has("Watcher: no"));
    assert!(p.has("Continue setup"), "a waiting agent offers the way back in");
    assert!(p.has("Revoked"));
    assert!(!p.has("Hermes (old Mac)"), "revoked agents stay folded");
    p.harness.get_by_label("Revoked").click();
    p.steps(2);
    assert!(p.has("Hermes (old Mac)"));

    p.harness.get_all_by_label("More actions").next().unwrap().click();
    p.steps(2);
    p.harness.get_by_label("Revoke\u{2026}").click();
    p.steps(2);
    assert!(p.has("Revoke Hermes (Anmol's Mac)?"));
    p.harness.get_by_label("Cancel").click();
    p.steps(2);

    // Continue setup reopens the last step for that agent.
    p.harness.get_all_by_label("Continue setup").next().unwrap().click();
    p.steps(2);
    assert!(p.has("Waiting for Codex to say hello\u{2026}"));
    p.harness.key_press(egui::Key::Escape);
    p.steps(2);
    assert!(!p.has("Waiting for Codex to say hello\u{2026}"), "Escape leaves safely");
}

// -------------------------------------------------------------------- renders

const WIDTHS: [(f32, &str); 2] = [(1440.0, "1440"), (820.0, "820")];
const DIR: &str = "docs/design-mocks/render/agent-sessions";

fn save(p: &mut Page<'_>, name: &str, label: &str) {
    std::fs::create_dir_all(DIR).unwrap();
    let path = format!("{DIR}/{name}-{label}.png");
    p.harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs: cargo test --features app --test agent_sessions_ui -- --ignored"]
fn renders() {
    for (width, label) in WIDTHS {
        // Task page as the owner: working, the log open.
        let f = task_fixtures("working", true);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Home, Some(TASK), (width, 2000.0), true, false);
        p.harness.get_by_label("Logs").click();
        p.steps(2);
        p.seed("task:logs", json!([
            {"seq": 1, "text": "$ git switch -c feat/role-bucket"},
            {"seq": 2, "text": "$ rg -n \"roleBucket\" api/"},
            {"seq": 3, "text": "api/routes/admin/leads.js:88  // TODO roleBucket"},
            {"seq": 4, "text": "$ npx jest api/services/rubric --silent"},
            {"seq": 5, "text": "PASS  api/services/rubric/bucket.spec.js (4 tests)"},
            {"seq": 6, "text": "$ npx jest api/routes/admin/leads.spec.js"},
            {"seq": 7, "text": "  \u{2713} returns roleBucket for scored leads (41 ms)"},
            {"seq": 8, "text": "  \u{2713} marks unscored leads unrated (12 ms)"},
        ]));
        p.steps(4);
        save(&mut p, "task-owner-working", label);
        // The page's session state is per thread, as the one window's is:
        // fold the log again so the next render starts closed.
        p.harness.get_by_label("Logs").click();
        p.steps(2);
        drop(p);

        // The same task as a teammate.
        let f = task_fixtures("working", false);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Home, Some(TASK), (width, 1700.0), true, false);
        save(&mut p, "task-teammate", label);
        drop(p);

        // The report card, as the owner, with Request changes open.
        let f = task_fixtures("in_review", true);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Home, Some(TASK), (width, 1900.0), true, false);
        p.harness.get_by_label("Request changes").click();
        p.steps(3);
        save(&mut p, "task-report", label);
        p.harness.get_by_label("Cancel").click();
        p.steps(2);
        drop(p);

        // Home with agents at work.
        let f = home_fixtures(active());
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Home, None, (width, 1300.0), true, false);
        save(&mut p, "home", label);
        drop(p);

        // The Agents cards, revoked opened. Seeded once rather than every
        // frame, so the connect flow below can move the list on.
        let f = base(me(ME, "member"));
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Agents, None, (width, 1100.0), true, false);
        p.seed("agents:mine", my_agents());
        p.steps(3);
        p.harness.get_by_label("Revoked").click();
        p.steps(3);
        save(&mut p, "agents", label);

        // Connect: 1, 2, 3, connected.
        p.harness.get_by_label("Connect an agent").click();
        p.steps(2);
        p.type_into("Hermes (Anmol", "Hermes (studio Mac)");
        p.type_into("hermes", "hermes-studio");
        save(&mut p, "connect-1", label);
        p.harness.get_by_label("Continue").click();
        p.steps(1);
        p.seed("agents:action", json!({
            "agent": {"id": NEW, "name": "Hermes (studio Mac)"}, "token": "acp_7f3c9e21d4b84a0c9f6e2b1a5d8c3e7f",
            "prompt": "You are being connected to Airtribe Control Plane as agent \"hermes-studio\" for Anmol. Server: https://acp.airtribe.live Token: acp_7f3c9e21d4b84a0c9f6e2b1a5d8c3e7f (secret \u{2014} store it, never print it again). Fetch your setup steps with\ncurl -fsS -H \"Authorization: Bearer <token>\" \"https://acp.airtribe.live/api/agent/onboarding?runtime=hermes\"\nand follow them, then confirm.",
        }));
        p.seed("agents:mine", my_agents());
        p.steps(4);
        save(&mut p, "connect-2", label);
        p.harness.get_by_label("Copy").click();
        p.steps(1);
        p.harness.get_by_label("I\u{2019}ve pasted it").click();
        p.steps(3);
        // The poll behind step 3 has no server here; put the list back.
        p.seed("agents:mine", my_agents());
        p.steps(2);
        save(&mut p, "connect-3", label);
        let mut mine = my_agents();
        mine.as_array_mut().unwrap().push(json!({"id": NEW, "handle": "hermes-studio", "name": "Hermes (studio Mac)",
            "runtime": "hermes", "status": "connected", "lastSeenAt": ago(0), "createdAt": ago(1), "activeTasks": 0,
            "setup": {"skill": "1.4.0", "mcp": true, "watcher": true}}));
        p.seed("agents:mine", mine);
        p.steps(40);
        save(&mut p, "connect-connected", label);
        p.harness.get_by_label("Done").click();
        p.steps(2);
    }
}
