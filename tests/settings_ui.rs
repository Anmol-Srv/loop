//! Settings' Folders section, and the task page's Folder picker it feeds.
//! Driven from fixture JSON like tests/standalone_ui.rs.
#![cfg(feature = "app")]

use std::cell::RefCell;

use acp_server::desktop::design::theme;
use acp_server::desktop::net::Net;
use acp_server::desktop::{views, App, Tab};
use chrono::Utc;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use serde_json::{json, Value};

const ME: &str = "aaaaaaaa-0000-0000-0000-000000000001";
const ALONE: &str = "33333333-0000-0000-0000-000000000001";
const ALONE_TITLE: &str = "Rotate the Razorpay key";
const API_ID: &str = "44444444-0000-0000-0000-000000000001";
const WEB_ID: &str = "44444444-0000-0000-0000-000000000002";

fn ago(minutes: i64) -> String {
    (Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339()
}

fn folders() -> Value {
    json!([
        {"id": API_ID, "name": "mycohort-api", "path": "/Users/anmol/code/mycohort-api", "isDefault": true},
        {"id": WEB_ID, "name": "dr-doom", "path": "/Users/anmol/code/dr-doom", "isDefault": false},
    ])
}

fn standalone() -> Value {
    json!({"id": ALONE, "title": ALONE_TITLE, "status": "open", "priority": 1, "discipline": "backend",
        "body": "The live key leaked into a screenshot. Rotate it and update the env.",
        "projectId": null, "projectName": null, "phaseId": null, "phaseName": null, "category": "chore",
        "folderName": null, "assigneePersonId": ME, "assigneeName": "Anmol Srivastava",
        "assigneeEmail": "anmol@airtribe.live", "createdAt": ago(30), "updatedAt": ago(30), "doneAt": null,
        "delegate": null, "canArchive": true, "canDelete": true})
}

type Fixtures = Vec<(&'static str, Value)>;

fn base() -> Fixtures {
    vec![
        ("__me", json!({"personId": ME, "email": "anmol@airtribe.live", "name": "Anmol Srivastava",
            "role": "member", "scopes": ["read", "write"]})),
        ("sidebar:counts", json!({"myOpen": 1, "activeProjects": 0, "triage": 0})),
        ("__tracks", json!({})),
        ("agents:mine", json!([])),
        ("board:people", json!([])),
        ("settings:folders", folders()),
    ]
}

fn task_page() -> Fixtures {
    let mut f = base();
    f.push(("task:one", standalone()));
    f.push(("task:notes", json!([])));
    f.push(("task:artifacts", json!([])));
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
    fn click(&mut self, role: egui::accesskit::Role, label: &str) {
        self.harness
            .get_all_by(|n| n.role() == role && n.label().as_deref() == Some(label))
            .next()
            .unwrap_or_else(|| panic!("no {role:?} {label}"))
            .click();
        self.steps(3);
    }
    fn button(&mut self, label: &str) {
        self.click(egui::accesskit::Role::Button, label);
    }
    /// A request is out: `Net::inflight` has at least one `true`.
    fn request_out(&self) -> bool {
        self.app.borrow().as_ref().unwrap().net.as_ref().unwrap().inflight.values().any(|b| *b)
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

fn page<'a>(app: &'a RefCell<Option<App>>, fixtures: &'a Fixtures, tab: Tab, task: Option<&'a str>) -> Page<'a> {
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 1200.0)).with_pixels_per_point(2.0).build_ui(
        move |ui| {
            let mut slot = app.borrow_mut();
            if slot.is_none() {
                theme::install(ui.ctx());
                ui.ctx().all_styles_mut(|s| s.animation_time = egui::Style::default().animation_time);
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
        },
    );
    for _ in 0..6 {
        harness.step();
    }
    Page { harness, app }
}

// ------------------------------------------------------------------ the Settings page

#[test]
fn folders_list_with_a_default_badge_and_the_helper_copy() {
    let f = base();
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::Settings, None);
    assert!(p.has("mycohort-api"));
    assert!(p.has("/Users/anmol/code/mycohort-api"));
    assert!(p.has("dr-doom"));
    assert!(p.has("Default"), "the default badge");
    assert!(p.harness.query_by_label_contains("Folders your agent can work in").is_some());
}

#[test]
fn adding_a_folder_validates_before_it_sends() {
    let f = base();
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Settings, None);
    p.button("+ Add folder");

    // Nothing typed: Add stays off.
    assert!(p.harness.get_by_label("Add").accesskit_node().is_disabled());

    let name = |n: &egui_kittest::kittest::AccessKitNode<'_>| {
        n.placeholder().is_some_and(|h| h.starts_with("e.g. mycohort-api"))
    };
    let path = |n: &egui_kittest::kittest::AccessKitNode<'_>| n.placeholder() == Some("/Users/you/code/mycohort-api");

    p.harness.get_by(name).focus();
    p.steps(1);
    p.harness.get_by(name).type_text("logan");
    p.steps(2);
    assert!(p.harness.get_by_label("Add").accesskit_node().is_disabled(), "no path yet");

    // A relative path is refused before anything is sent.
    p.harness.get_by(path).focus();
    p.steps(1);
    p.harness.get_by(path).type_text("code/logan");
    p.steps(2);
    assert!(p.harness.query_by_label_contains("Needs a full path starting with /").is_some());
    assert!(p.harness.get_by_label("Add").accesskit_node().is_disabled());
    assert!(!p.request_out());

    // An absolute one enables Add, and clicking it sends the request.
    for _ in 0.."code/logan".len() {
        p.harness.key_press(egui::Key::Backspace);
    }
    p.steps(1);
    p.harness.get_by(path).type_text("/Users/anmol/code/logan");
    p.steps(2);
    assert!(!p.harness.get_by_label("Add").accesskit_node().is_disabled());
    p.button("Add");
    assert!(p.request_out(), "the add request went out");
}

#[test]
fn setting_a_default_sends_immediately() {
    let f = base();
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Settings, None);

    // dr-doom is the second row (mycohort-api is the default, listed first).
    let mores: Vec<_> = p.harness.get_all_by_label("More actions").collect();
    assert_eq!(mores.len(), 2);
    mores[1].click();
    p.steps(2);
    // dr-doom is not the default, so its menu offers making it one.
    assert!(!p.harness.get_by_label("Set as default").accesskit_node().is_disabled());
    p.harness.get_by_label("Set as default").click();
    p.steps(2);
    assert!(p.request_out(), "no confirmation needed to set a default");
}

#[test]
fn removing_a_folder_asks_first() {
    let f = base();
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Settings, None);

    let mores: Vec<_> = p.harness.get_all_by_label("More actions").collect();
    mores[1].click();
    p.steps(2);
    p.harness.get_by_label("Remove\u{2026}").click();
    p.steps(2);
    assert!(p.harness.query_by_label_contains("Remove the \u{201c}dr-doom\u{201d} folder?").is_some());
    assert!(!p.request_out(), "asking first, not sending yet");

    p.harness.get_by_label("Cancel").click();
    p.steps(2);
    assert!(p.has("dr-doom"), "kept");
    assert!(!p.request_out());
}

// ------------------------------------------------------------------ the task page's Folder picker

#[test]
fn the_task_page_offers_the_default_folder_when_there_is_no_project() {
    let f = task_page();
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(ALONE));
    assert!(p.has("No project"), "the breadcrumb");
    // Unpinned: shown as the default, named.
    assert!(p.has("mycohort-api (default)"));
}

#[test]
fn picking_a_folder_pins_it() {
    let f = task_page();
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::MyTasks, Some(ALONE));
    p.click(egui::accesskit::Role::ComboBox, "mycohort-api (default)");
    assert!(p.has("dr-doom"), "the other folder is offered");
    p.click(egui::accesskit::Role::Button, "dr-doom");
    p.seed("task:details", json!({"id": ALONE, "folderName": "dr-doom"}));
    p.steps(3);
    assert!(p.has("Saved."));
}
