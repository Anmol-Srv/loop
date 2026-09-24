//! The agent control plane's screens, driven from fixture JSON.
//!
//! The real `App::frame`, with every payload a page reads seeded into `Net`
//! under the keys the views ask for, so nothing here needs a server: the
//! wire shapes are the ones in docs/superpowers/plans/2026-09-23-agent-control-plane.md.
//! Requests the fixtures do not cover go to a closed port and fail.
//!
//! The behaviour tests run with the suite. `renders` draws the pages at three
//! widths into docs/design-mocks/render/agents/ for looking at:
//!   cargo test --features app --test agents_ui -- --ignored --nocapture
#![cfg(feature = "app")]

use std::cell::RefCell;

use acp_server::desktop::design::theme;
use acp_server::desktop::net::Net;
use acp_server::desktop::{views, App, Tab};
use chrono::{Duration, Utc};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use serde_json::{json, Value};

const ME: &str = "aaaaaaaa-0000-0000-0000-000000000001";
const TASK: &str = "22222222-0000-0000-0000-000000000009";
const HERMES: &str = "99999999-0000-0000-0000-000000000001";

fn ago(minutes: i64) -> String {
    (Utc::now() - Duration::minutes(minutes)).to_rfc3339()
}

fn agents() -> Value {
    json!([
        {"id": HERMES, "handle": "hermes", "name": "Hermes (Anmol's Mac)", "runtime": "hermes",
         "status": "connected", "connectedAt": ago(3000), "lastSeenAt": ago(3), "createdAt": ago(3100), "activeTasks": 2},
        {"id": "99999999-0000-0000-0000-000000000002", "handle": "claude-mac", "name": "Claude Code", "runtime": "claude-code",
         "status": "waiting", "connectedAt": null, "lastSeenAt": null, "createdAt": ago(20), "activeTasks": 0},
        {"id": "99999999-0000-0000-0000-000000000003", "handle": "codex-old", "name": "Codex", "runtime": "codex",
         "status": "revoked", "connectedAt": ago(90000), "lastSeenAt": ago(80000), "createdAt": ago(90000), "activeTasks": 0},
    ])
}

/// A task of mine, in progress, with `delegate` as given.
fn task(delegate: Value, review_target: Value) -> Value {
    json!({
        "id": TASK, "title": "Rate leads by role bucket", "status": "in_progress", "discipline": "backend",
        "body": "Backend-engineering leads on the weighted-rubric path get their role bucket from Jev.\n\nExpose the bucket on the lead payload.",
        "priority": 1, "projectId": "11111111-0000-0000-0000-000000000001", "projectName": "Lead Rating",
        "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "assigneeEmail": "anmol@airtribe.live",
        "assigneeKind": "person", "createdAt": ago(4000), "updatedAt": ago(10), "doneAt": null,
        "delegate": delegate, "reviewTarget": review_target,
    })
}

fn delegate(state: &str) -> Value {
    json!({"id": HERMES, "handle": "hermes", "name": "Hermes (Anmol's Mac)", "state": state, "lastSeenAt": ago(2)})
}

fn notes(last: Value) -> Value {
    json!([
        {"id": "n1", "body": "Jev's bucket list is in the rubric doc — use the v3 names.", "kind": "note",
         "authorName": "Priya Nair", "agent": null, "createdAt": ago(300)},
        {"id": "n2", "body": "Picked this up. Reading the lead payload and the rubric service.", "kind": "progress",
         "authorName": null, "agent": {"id": HERMES, "name": "Hermes (Anmol's Mac)"}, "createdAt": ago(120)},
        last,
    ])
}

fn question() -> Value {
    json!({"id": "n3", "body": "Should leads with no rubric score fall into the default bucket, or be left out of the payload?",
           "kind": "question", "authorName": null, "agent": {"id": HERMES, "name": "Hermes (Anmol's Mac)"}, "createdAt": ago(6)})
}

fn submission() -> Value {
    json!({"id": "n4", "body": "Added roleBucket to the lead payload behind the rubric check; unscored leads get \"unrated\". Tests cover both paths.",
           "kind": "submission", "authorName": null, "agent": {"id": HERMES, "name": "Hermes (Anmol's Mac)"}, "createdAt": ago(4)})
}

fn artifacts() -> Value {
    json!([
        {"id": "a1", "kind": "pr", "url": "https://github.com/airtribe/mycohort-api/pull/4821", "title": "Role bucket on lead payload"},
        {"id": "a2", "kind": "commit", "url": "a1b2c3d4", "title": "Default unscored leads to unrated"},
    ])
}

fn base() -> Vec<(&'static str, Value)> {
    vec![
        ("__me", json!({"personId": ME, "email": "anmol@airtribe.live", "name": "Anmol Srivastava", "role": "admin", "scopes": ["read", "write"]})),
        ("sidebar:counts", json!({"myOpen": 4, "activeProjects": 2})),
        ("agents:mine", agents()),
        ("__tracks", json!({})),
        ("board:people", json!([])),
        ("task:logs", json!([])),
        ("task:artifacts", artifacts()),
    ]
}

fn home_fixtures() -> Vec<(&'static str, Value)> {
    let mut f = base();
    f.push(("home", json!({
        "myTasks": [], "waitingOnMe": [], "team": [],
        "projects": [{"id": "11111111-0000-0000-0000-000000000001", "name": "Lead Rating", "status": "active", "done": 3, "total": 8}],
        "needsAttention": [
            {"kind": "question", "taskId": TASK, "title": "Rate leads by role bucket", "agentName": "Hermes", "body": question()["body"]},
            {"kind": "review", "taskId": "t2", "title": "Checkout copy pass", "agentName": "Hermes", "body": "Copy updated on all three steps."},
        ],
    })));
    let mut delegated = task(delegate("needs_input"), Value::Null);
    delegated["blockersDone"] = json!(0);
    delegated["blockersTotal"] = json!(0);
    f.push(("home:tasks", json!([
        delegated,
        {"id": "t3", "title": "Lead list pagination", "status": "open", "discipline": "frontend", "priority": 2,
         "projectId": "11111111-0000-0000-0000-000000000001", "projectName": "Lead Rating", "assigneeName": "Priya Nair",
         "createdAt": ago(5000), "updatedAt": ago(50), "doneAt": null, "delegate": null},
    ])));
    f
}

type Fixtures = Vec<(&'static str, Value)>;

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
}

/// The app on `tab` (or on `task`), every fixture re-seeded before each frame
/// so a view that invalidates a key finds it again rather than a failure.
fn page<'a>(
    app: &'a RefCell<Option<App>>,
    fixtures: &'a Fixtures,
    tab: Tab,
    task: Option<&'a str>,
    width: f32,
    gpu: bool,
) -> Page<'a> {
    let builder = Harness::builder().with_size(egui::vec2(width, 1300.0)).with_pixels_per_point(2.0);
    let builder = if gpu { builder.wgpu() } else { builder };
    let mut harness = builder.build_ui(move |ui| {
        let mut slot = app.borrow_mut();
        if slot.is_none() {
            theme::install(ui.ctx());
            ui.ctx().set_zoom_factor(1.15);
            *slot = Some(App {
                net: Some(Net::spawn("http://127.0.0.1:9".into(), "test".into(), ui.ctx().clone())),
                tab,
                project: None,
                task: task.map(str::to_owned),
                scopes: Vec::new(),
                login: views::login::State::default(),
                board: views::board::State::default(),
                palette: views::palette::State::default(),
            });
            return;
        }
        let a = slot.as_mut().unwrap();
        for (k, v) in fixtures {
            a.net.as_mut().unwrap().seed(k, v.clone());
        }
        a.frame(ui);
    });
    for _ in 0..4 {
        harness.step();
    }
    Page { harness, app }
}

fn task_fixtures(state: &str) -> Fixtures {
    let mut f = base();
    let (target, last) = match state {
        "in_review" => (json!("completed"), submission()),
        _ => (Value::Null, question()),
    };
    f.push(("task:one", task(delegate(state), target)));
    f.push(("task:notes", notes(last)));
    f
}

// ------------------------------------------------------------------ behaviour

#[test]
fn connect_validates_the_handle_and_shows_the_prompt_once() {
    let app = RefCell::new(None);
    let fixtures = base();
    let mut p = page(&app, &fixtures, Tab::Agents, None, 1100.0, false);

    p.harness.get_by_label("Connect an agent").click();
    p.steps(2);
    p.harness.get_by(|n| n.placeholder() == Some("hermes")).focus();
    p.steps(1);
    p.harness.get_by(|n| n.placeholder() == Some("hermes")).type_text("Hermes Bot");
    p.steps(2);
    assert!(p.harness.query_by_label_contains("Lowercase letters, digits and dashes").is_some());
    assert!(p.harness.get_by_label("Continue").accesskit_node().is_disabled());

    // Fix the handle (still focused), name it, create.
    for _ in 0.."Hermes Bot".len() {
        p.harness.key_press(egui::Key::Backspace);
    }
    p.steps(1);
    p.harness.get_by(|n| n.placeholder() == Some("hermes")).type_text("hermes");
    p.steps(1);
    p.harness.get_by(|n| n.placeholder().is_some_and(|h| h.starts_with("Hermes ("))).focus();
    p.steps(1);
    p.harness.get_by(|n| n.placeholder().is_some_and(|h| h.starts_with("Hermes ("))).type_text("Hermes");
    p.steps(2);
    assert!(p.harness.query_by_label_contains("Lowercase letters").is_none());
    assert!(!p.harness.get_by_label("Continue").accesskit_node().is_disabled());
    p.harness.get_by_label("Continue").click();
    p.steps(1);

    let prompt = "You are being connected to Airtribe Control Plane as agent \"hermes\" for Anmol. Token: acp_secret123";
    p.seed("agents:action", json!({"agent": {"id": HERMES, "name": "Hermes"}, "token": "acp_secret123", "prompt": prompt}));
    p.steps(3);
    assert!(p.harness.query_by_label("Paste this into Hermes").is_some());
    assert!(p.harness.query_by_label_contains("Shown only once").is_some());
    assert!(p.harness.query_by_value(prompt).is_some(), "the prompt is on screen");

    p.harness.get_by_label("Copy").click();
    p.steps(1);
    let copied = p.harness.output().platform_output.commands.iter().any(|c| {
        matches!(c, egui::OutputCommand::CopyText(t) if t == prompt)
    });
    assert!(copied, "Copy puts the prompt on the clipboard");
    p.steps(1);
    assert!(p.harness.query_by_label("Copied").is_some());

    p.harness.get_by_label("Close").click();
    p.steps(2);
    assert!(p.harness.query_by_value(prompt).is_none(), "closing forgets the prompt");
}

#[test]
fn revoke_asks_first() {
    let app = RefCell::new(None);
    let fixtures = base();
    let mut p = page(&app, &fixtures, Tab::Agents, None, 1440.0, false);
    // Two live agents, so two cards with a menu; the revoked one is folded away.
    assert_eq!(p.harness.get_all_by_label("More actions").count(), 2);
    p.harness.get_all_by_label("More actions").next().unwrap().click();
    p.steps(2);
    p.harness.get_by_label("Revoke\u{2026}").click();
    p.steps(2);
    assert!(p.harness.query_by_label("Revoke Hermes (Anmol's Mac)?").is_some());
    p.harness.get_by_label("Cancel").click();
    p.steps(2);
    assert!(p.harness.query_by_label("Revoke Hermes (Anmol's Mac)?").is_none());
}

#[test]
fn question_takes_an_answer() {
    let app = RefCell::new(None);
    let fixtures = task_fixtures("needs_input");
    let mut p = page(&app, &fixtures, Tab::Home, Some(TASK), 1440.0, false);
    assert!(p.harness.query_by_label("asked").is_some());
    assert!(p.harness.query_by_label("Needs your input").is_some());
    assert!(p.harness.query_by_label("Take back").is_some());
    assert!(p.harness.get_by_label("Send answer").accesskit_node().is_disabled());

    let answer = p.harness.get_by(|n| n.placeholder().is_some_and(|h| h.starts_with("Answer Hermes")));
    answer.focus();
    p.steps(1);
    p.harness.get_by(|n| n.placeholder().is_some_and(|h| h.starts_with("Answer Hermes"))).type_text("Default bucket.");
    p.steps(2);
    p.harness.get_by_label("Send answer").click();
    p.steps(1);
    p.seed("task:agent", json!({}));
    p.steps(2);
    assert!(p.harness.query_by_label("Answer sent.").is_some());
}

#[test]
fn review_needs_a_note_to_request_changes() {
    let app = RefCell::new(None);
    let fixtures = task_fixtures("in_review");
    let mut p = page(&app, &fixtures, Tab::Home, Some(TASK), 1440.0, false);
    assert!(p.harness.query_by_label_contains("submitted this for review").is_some());
    assert!(!p.harness.get_by_label("Approve").accesskit_node().is_disabled());
    // Request changes opens the note it needs; Send back waits for it.
    p.harness.get_by_label("Request changes").click();
    p.steps(2);
    assert!(p.harness.get_by_label("Send back").accesskit_node().is_disabled());

    let note = |n: &egui_kittest::kittest::AccessKitNode<'_>| {
        n.placeholder().is_some_and(|h| h.starts_with("What should Hermes change"))
    };
    p.harness.get_by(note).focus();
    p.steps(1);
    p.harness.get_by(note).type_text("Unscored leads should be left out.");
    p.steps(2);
    assert!(!p.harness.get_by_label("Send back").accesskit_node().is_disabled());
    p.harness.get_by_label("Send back").click();
    p.steps(1);
    p.seed("task:agent", json!({}));
    p.steps(2);
    assert!(p.harness.query_by_label("Changes requested.").is_some());
}

#[test]
fn hand_off_is_the_assignees_and_not_for_finished_work() {
    // Mine, not delegated, one live agent besides a waiting one: a picker.
    let mut f = base();
    f.push(("task:one", task(Value::Null, Value::Null)));
    f.push(("task:notes", json!([])));
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::Home, Some(TASK), 1440.0, false);
    assert!(p.harness.query_by_label("Hand off to\u{2026}").is_some());
    drop(p);

    // One agent: a button that names it.
    let mut f = base();
    f.retain(|(k, _)| *k != "agents:mine");
    f.push(("agents:mine", json!([agents()[0].clone()])));
    f.push(("task:one", task(Value::Null, Value::Null)));
    f.push(("task:notes", json!([])));
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::Home, Some(TASK), 1440.0, false);
    assert!(!p.harness.get_by_label("Hand off to Hermes (Anmol's Mac)").accesskit_node().is_disabled());
    drop(p);

    // Finished: still there, disabled.
    let mut done = task(Value::Null, Value::Null);
    done["status"] = json!("shipped");
    done["doneAt"] = json!(ago(5));
    let mut f2 = f.clone();
    f2.retain(|(k, _)| *k != "task:one");
    f2.push(("task:one", done));
    let app = RefCell::new(None);
    let p = page(&app, &f2, Tab::Home, Some(TASK), 1440.0, false);
    assert!(p.harness.get_by_label("Hand off to Hermes (Anmol's Mac)").accesskit_node().is_disabled());
    drop(p);

    // Someone else's task: nothing.
    let mut theirs = task(Value::Null, Value::Null);
    theirs["assigneePersonId"] = json!("someone-else");
    f.retain(|(k, _)| *k != "task:one");
    f.push(("task:one", theirs));
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::Home, Some(TASK), 1440.0, false);
    assert!(p.harness.query_by_label_contains("Hand off").is_none());
}

#[test]
fn home_lists_agent_questions_and_reviews_first() {
    let app = RefCell::new(None);
    let fixtures = home_fixtures();
    let p = page(&app, &fixtures, Tab::Home, None, 1440.0, false);
    assert!(p.harness.query_by_label("Question").is_some());
    assert!(p.harness.query_by_label("Review").is_some());
    assert!(p.harness.query_by_label_contains("Hermes asks: Should leads").is_some());
}

// -------------------------------------------------------------------- renders

const WIDTHS: [(f32, &str); 3] = [(1440.0, "wide"), (1100.0, "mid"), (820.0, "narrow")];

fn save(p: &mut Page<'_>, name: &str, label: &str) {
    let path = format!("docs/design-mocks/render/agents/{name}-{label}.png");
    std::fs::create_dir_all("docs/design-mocks/render/agents").unwrap();
    p.harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs: cargo test --features app --test agents_ui -- --ignored"]
fn renders() {
    for (width, label) in WIDTHS {
        let f = base();
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Agents, None, width, true);
        save(&mut p, "agents", label);
        drop(p);

        let mut f = base();
        f.retain(|(k, _)| *k != "agents:mine");
        f.push(("agents:mine", json!([])));
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Agents, None, width, true);
        save(&mut p, "agents-empty", label);
        drop(p);

        let f = base();
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Agents, None, width, true);
        p.harness.get_by_label("Connect an agent").click();
        p.steps(2);
        p.harness.get_by(|n| n.placeholder() == Some("hermes")).focus();
        p.steps(1);
        p.harness.get_by(|n| n.placeholder() == Some("hermes")).type_text("Hermes Bot");
        p.steps(3);
        save(&mut p, "agents-connect", label);
        p.harness.get_by_label("Cancel").click();
        p.steps(2);
        p.harness.get_all_by_label("More actions").next().unwrap().click();
        p.steps(2);
        p.harness.get_by_label("Rotate token\u{2026}").click();
        p.steps(2);
        p.harness.get_by_label("Rotate token").click();
        p.steps(1);
        p.seed("agents:action", json!({
            "agent": {"id": HERMES, "name": "Hermes (Anmol's Mac)"},
            "token": "acp_7f3c9e21d4b84a0c9f6e2b1a5d8c3e7f",
            "prompt": "You are being connected to Airtribe Control Plane as agent \"hermes\" for Anmol. Server: https://acp.airtribe.live Token: acp_7f3c9e21d4b84a0c9f6e2b1a5d8c3e7f (secret \u{2014} store it, never print it again). Fetch your setup steps with\ncurl -fsS -H \"Authorization: Bearer <token>\" \"https://acp.airtribe.live/api/agent/onboarding?runtime=hermes\"\nand follow them, then confirm.",
        }));
        p.steps(4);
        save(&mut p, "agents-prompt", label);
        // The page's dialogs live in a thread-local, as the one app window's
        // would: close this one so the next width starts on the bare page.
        p.harness.get_by_label("Close").click();
        p.steps(2);
        drop(p);

        for state in ["needs_input", "in_review"] {
            let f = task_fixtures(state);
            let app = RefCell::new(None);
            let mut p = page(&app, &f, Tab::Home, Some(TASK), width, true);
            save(&mut p, &format!("task-{}", if state == "in_review" { "review" } else { "question" }), label);
        }

        let f = home_fixtures();
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Home, None, width, true);
        save(&mut p, "home", label);
    }
}

/// The same pages against a real server, so the wire shapes are the server's
/// and not these fixtures'. Needs a running server and an owner session:
///   ACP_LIVE_URL=http://localhost:8080 ACP_LIVE_TOKEN=… ACP_LIVE_TASKS=id,id \
///   cargo test --features app --test agents_ui live_renders -- --ignored
#[test]
#[ignore = "needs a live server: see the comment above"]
fn live_renders() {
    let url = std::env::var("ACP_LIVE_URL").expect("ACP_LIVE_URL");
    let token = std::env::var("ACP_LIVE_TOKEN").expect("ACP_LIVE_TOKEN");
    let tasks = std::env::var("ACP_LIVE_TASKS").unwrap_or_default();
    let mut shots: Vec<(String, Tab, Option<String>)> =
        vec![("live-agents".into(), Tab::Agents, None), ("live-home".into(), Tab::Home, None)];
    for (i, id) in tasks.split(',').filter(|s| !s.is_empty()).enumerate() {
        shots.push((format!("live-task-{}", i + 1), Tab::Home, Some(id.to_owned())));
    }
    for (name, tab, task) in shots {
        let app: RefCell<Option<App>> = RefCell::new(None);
        let (url, token) = (url.clone(), token.clone());
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1500.0))
            .with_pixels_per_point(2.0)
            .wgpu()
            .build_ui(|ui| {
                let mut slot = app.borrow_mut();
                if slot.is_none() {
                    theme::install(ui.ctx());
                    ui.ctx().set_zoom_factor(1.15);
                    *slot = Some(App {
                        net: Some(Net::spawn(url.clone(), token.clone(), ui.ctx().clone())),
                        tab,
                        project: None,
                        task: task.clone(),
                        scopes: Vec::new(),
                        login: views::login::State::default(),
                        board: views::board::State::default(),
                        palette: views::palette::State::default(),
                    });
                    return;
                }
                slot.as_mut().unwrap().frame(ui);
            });
        // Real requests: give them time to land, then let the page settle.
        for _ in 0..30 {
            harness.step();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        std::fs::create_dir_all("docs/design-mocks/render/agents").unwrap();
        let path = format!("docs/design-mocks/render/agents/{name}.png");
        harness.render().expect("render").save(&path).expect("write png");
        eprintln!("wrote {path}");
    }
}
