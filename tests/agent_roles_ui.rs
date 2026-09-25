//! Agent roles in the app: hand-off pickers list only agents that work on
//! tasks, the connect flow asks for roles, and cards and menus show them —
//! from fixture JSON, the way tests/context_menu_ui.rs does it.
#![cfg(feature = "app")]

use std::cell::RefCell;

use acp_server::desktop::design::theme;
use acp_server::desktop::net::Net;
use acp_server::desktop::{views, App, Tab};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use serde_json::{json, Value};

const ME: &str = "aaaaaaaa-0000-0000-0000-000000000001";
const TASK: &str = "22222222-0000-0000-0000-000000000009";
const TITLE: &str = "Payment sheet card form";
const SLACK: &str = "Slack Agent";
const WORKER: &str = "Airtribe";

fn slack() -> Value {
    json!({"id": "99999999-0000-0000-0000-000000000007", "handle": "slack-agent", "name": SLACK, "runtime": "other",
           "status": "connected", "activeTasks": 0, "currentTask": null, "canWork": false, "canIntake": true,
           "intakeStats": {"triage": 1, "accepted": 2, "dismissed": 0}})
}

fn worker() -> Value {
    json!({"id": "99999999-0000-0000-0000-000000000001", "handle": "airtribe", "name": WORKER, "runtime": "claude-code",
           "status": "connected", "activeTasks": 0, "currentTask": null, "canWork": true, "canIntake": false})
}

fn task() -> Value {
    json!({
        "id": TASK, "title": TITLE, "status": "in_progress", "discipline": "backend", "priority": 1,
        "body": "Move the card form onto the new payment sheet.", "projectId": null, "projectName": null,
        "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "assigneeEmail": "anmol@airtribe.live",
        "assigneeKind": "human", "createdAt": "2026-09-20T10:00:00Z", "updatedAt": "2026-09-25T10:00:00Z",
        "doneAt": null, "delegate": null, "blockedBy": [], "labels": [], "canArchive": true, "canDelete": true,
    })
}

fn fixtures(agents: Value) -> Vec<(&'static str, Value)> {
    vec![
        (
            "__me",
            json!({"personId": ME, "email": "anmol@airtribe.live", "name": "Anmol Srivastava", "role": "member", "scopes": ["read", "write"]}),
        ),
        ("sidebar:counts", json!({"myOpen": 1, "activeProjects": 0})),
        ("__tracks", json!({})),
        ("board:people", json!([])),
        ("board:projects", json!([])),
        ("projects:labels", json!([])),
        ("agents:mine", agents),
        ("mytasks:mine", json!([task()])),
        ("task:one", task()),
        ("task:notes", json!([])),
        ("task:artifacts", json!([])),
        ("task:logs", json!([])),
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
    fn button<'s>(&'s self, label: &'s str) -> egui_kittest::Node<'s> {
        self.harness
            .get_all_by(|n| {
                n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(label)
            })
            .next()
            .unwrap_or_else(|| panic!("no button {label}"))
    }
    fn toggled(&self, label: &str) -> Option<egui::accesskit::Toggled> {
        self.harness.get_by_label(label).accesskit_node().toggled()
    }
}

fn page<'a>(
    app: &'a RefCell<Option<App>>,
    fixtures: &'a [(&'static str, Value)],
    tab: Tab,
    task: Option<&'a str>,
) -> Page<'a> {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1100.0))
        .build_ui(move |ui| {
            let mut slot = app.borrow_mut();
            if slot.is_none() {
                theme::install(ui.ctx());
                *slot = Some(App {
                    net: Some(Net::spawn(
                        "http://127.0.0.1:9".into(),
                        "test".into(),
                        ui.ctx().clone(),
                    )),
                    tab,
                    project: None,
                    task: task.map(str::to_owned),
                    scopes: vec!["read".into(), "write".into()],
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
    for _ in 0..4 {
        harness.step();
    }
    Page { harness }
}

#[test]
fn the_task_page_hands_off_only_to_an_agent_that_works() {
    let f = fixtures(json!([slack(), worker()]));
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(TASK));
    // One working agent: a button that names it, not a picker with Slack in it.
    assert!(!p
        .button("Hand off to Airtribe")
        .accesskit_node()
        .is_disabled());
    assert!(!p.has("Hand off to Slack Agent"));
    assert!(!p.has("Hand off to\u{2026}"));
}

#[test]
fn with_only_an_intake_agent_there_is_no_hand_off() {
    let f = fixtures(json!([slack()]));
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(TASK));
    assert!(p.harness.query_by_label_contains("Hand off").is_none());
}

#[test]
fn an_agent_from_before_roles_still_takes_hand_offs() {
    let mut old = worker();
    old.as_object_mut().unwrap().remove("canWork");
    let f = fixtures(json!([old]));
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(TASK));
    assert!(p.has("Hand off to Airtribe"));
}

#[test]
fn the_row_menu_hands_off_only_to_an_agent_that_works() {
    let f = fixtures(json!([slack(), worker()]));
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::MyTasks, None);
    p.harness.get_by_label(TITLE).click_secondary();
    p.steps(3);
    assert!(p.has("Hand off to Airtribe"));
    assert!(!p.has("Hand off to Slack Agent"));
    assert!(!p.has(SLACK), "not in any submenu either");
}

#[test]
fn connect_asks_for_roles_and_needs_one() {
    let f = fixtures(json!([worker()]));
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Agents, None);
    p.button("Connect an agent").click();
    p.steps(3);
    let (work, intake) = ("Takes tasks I hand off", "Creates tasks for me");
    assert_eq!(
        p.toggled(work),
        Some(egui::accesskit::Toggled::True),
        "works on tasks unless told otherwise"
    );
    assert_eq!(
        p.toggled(intake),
        Some(egui::accesskit::Toggled::False),
        "files tasks only when asked"
    );
    assert!(!p.has("Turn on at least one role."));

    p.harness.get_by_label(work).click();
    p.steps(3);
    assert_eq!(p.toggled(work), Some(egui::accesskit::Toggled::False));
    assert!(p.has("Turn on at least one role."));
    assert!(p.button("Continue").accesskit_node().is_disabled());

    p.harness.get_by_label(intake).click();
    p.steps(3);
    assert!(!p.has("Turn on at least one role."));
}

#[test]
fn cards_show_roles_and_the_menu_switches_them() {
    let f = fixtures(json!([slack(), worker()]));
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Agents, None);
    assert_eq!(
        p.harness.query_all_by_label("Works on tasks").count(),
        1,
        "only the worker works on tasks"
    );
    assert_eq!(
        p.harness.query_all_by_label("Files tasks").count(),
        1,
        "only Slack Agent files them"
    );
    assert!(p.has("Files tasks; it doesn\u{2019}t take hand-offs"));

    // Slack Agent's card comes first; its menu offers to take hand-offs.
    p.harness
        .get_all_by_label("More actions")
        .next()
        .unwrap()
        .click();
    p.steps(3);
    assert!(p.has("Take tasks I hand off"));
    assert!(p.has("Stop creating tasks"));
}
