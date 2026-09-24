//! Archive and delete on the project and task pages, driven from fixture JSON
//! the way tests/agents_ui.rs drives the agent screens.
//!
//! The behaviour test runs with the suite. `renders` draws the pages and their
//! dialogs into docs/design-mocks/render/archive/ for looking at:
//!   cargo test --features app --test archive_ui -- --ignored --nocapture
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
const PROJECT: &str = "11111111-0000-0000-0000-000000000001";
const TASK: &str = "22222222-0000-0000-0000-000000000009";
const NAME: &str = "Checkout redesign";

fn ago(minutes: i64) -> String {
    (Utc::now() - Duration::minutes(minutes)).to_rfc3339()
}

fn task(n: usize, title: &str, status: &str, archived: bool) -> Value {
    json!({
        "id": if n == 0 { TASK.to_owned() } else { format!("22222222-0000-0000-0000-00000000000{n}") },
        "title": title, "status": status, "discipline": "backend", "priority": 1 + n % 3,
        "body": "Move the card form onto the new payment sheet and keep the saved cards.",
        "projectId": PROJECT, "projectName": NAME, "phaseName": "Work",
        "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "assigneeEmail": "anmol@airtribe.live",
        "assigneeKind": "human", "createdAt": ago(4000 + n as i64 * 60), "updatedAt": ago(10), "doneAt": null,
        "delegate": null, "blockedBy": [], "blockersTotal": 0, "blockersDone": 0,
        "archivedAt": if archived { json!(ago(30)) } else { Value::Null }, "projectArchivedAt": null,
        "canArchive": true, "canDelete": true,
    })
}

fn project(archived: bool) -> Value {
    json!({
        "id": PROJECT, "key": "checkout-redesign", "name": NAME, "status": "active", "priority": 1,
        "description": "One payment sheet for every checkout, saved cards included.",
        "startDate": null, "targetDate": null, "labels": [],
        "archivedAt": if archived { json!(ago(60)) } else { Value::Null },
        "canArchive": true, "canDelete": true, "taskCount": 3,
        "createdAt": ago(9000), "updatedAt": ago(20), "done": 1, "total": 3,
    })
}

fn fixtures(archived: bool) -> Vec<(String, Value)> {
    let tasks = json!([
        task(0, "Payment sheet card form", "in_progress", false),
        task(1, "Saved cards on the sheet", "open", false),
        task(2, "Remove the old checkout", "shipped", false),
    ]);
    vec![
        ("__me".into(), json!({"personId": ME, "email": "anmol@airtribe.live", "name": "Anmol Srivastava", "role": "member", "scopes": ["read", "write"]})),
        ("sidebar:counts".into(), json!({"myOpen": 4, "activeProjects": 2})),
        ("board:people".into(), json!([])),
        ("projects:labels".into(), json!([])),
        ("agents:mine".into(), json!([])),
        ("__tracks".into(), json!({})),
        ("board:projects".into(), json!([project(false)])),
        ("board:projects:archived".into(), json!([project(true)])),
        (format!("board:project:{PROJECT}"), project(archived)),
        (format!("board:flow:{PROJECT}"), json!({"id": PROJECT, "key": "checkout-redesign", "name": NAME, "status": "active",
            "priority": 1, "targetDate": null, "done": 1, "total": 3, "disciplines": [], "activePhase": "Work", "activePeople": 1})),
        (format!("board:tasks:{PROJECT}"), tasks),
        (format!("board:tasks:{PROJECT}:archived"), json!([task(3, "Old coupon banner", "open", true)])),
        (format!("project:artifacts:{PROJECT}"), json!([])),
        ("task:one".into(), task(0, "Payment sheet card form", "in_progress", archived)),
        ("task:notes".into(), json!([])),
        ("task:artifacts".into(), json!([])),
        ("task:logs".into(), json!([])),
    ]
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
    fn project(&self) -> Option<String> {
        self.app.borrow().as_ref().unwrap().project.clone()
    }
}

/// The app on Projects with `project` (and `task`) open, every fixture
/// re-seeded before each frame.
fn page<'a>(
    app: &'a RefCell<Option<App>>,
    fixtures: &'a [(String, Value)],
    project: Option<&'a str>,
    task: Option<&'a str>,
    width: f32,
    gpu: bool,
) -> Page<'a> {
    let builder = Harness::builder().with_size(egui::vec2(width, 1000.0)).with_pixels_per_point(2.0);
    let builder = if gpu { builder.wgpu() } else { builder };
    let mut harness = builder.build_ui(move |ui| {
        let mut slot = app.borrow_mut();
        if slot.is_none() {
            theme::install(ui.ctx());
            ui.ctx().set_zoom_factor(1.15);
            *slot = Some(App {
                net: Some(Net::spawn("http://127.0.0.1:9".into(), "test".into(), ui.ctx().clone())),
                tab: Tab::Projects,
                project: project.map(str::to_owned),
                task: task.map(str::to_owned),
                scopes: vec!["read".into(), "write".into()],
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

fn open_menu_and_pick(p: &mut Page<'_>, item: &str) {
    p.harness.get_all_by_label("More actions").next().unwrap().click();
    p.steps(2);
    p.harness.get_by_label(item).click();
    p.steps(2);
}

/// Into the dialog's one input, the only one on the project page.
fn type_name(p: &mut Page<'_>, text: &str) {
    let input = |n: &egui_kittest::kittest::AccessKitNode<'_>| n.role() == egui::accesskit::Role::TextInput;
    p.harness.get_by(input).focus();
    p.steps(1);
    p.harness.get_by(input).type_text(text);
    p.steps(2);
}

// ------------------------------------------------------------------ behaviour

#[test]
fn deleting_a_project_needs_its_name_typed() {
    let app = RefCell::new(None);
    let f = fixtures(false);
    let mut p = page(&app, &f, Some(PROJECT), None, 1440.0, false);

    open_menu_and_pick(&mut p, "Delete\u{2026}");
    assert!(p.harness.query_by_label(&format!("Delete {NAME}?")).is_some());
    assert!(p.harness.query_by_label_contains("Its 3 tasks go with it").is_some());
    assert!(p.harness.get_by_label("Delete project").accesskit_node().is_disabled());

    type_name(&mut p, "Checkout");
    assert!(p.harness.get_by_label("Delete project").accesskit_node().is_disabled(), "a prefix is not the name");
    type_name(&mut p, " redesign");
    assert!(!p.harness.get_by_label("Delete project").accesskit_node().is_disabled());

    p.harness.get_by_label("Delete project").click();
    p.steps(1);
    p.seed("projects:action", json!({"id": PROJECT, "deleted": true}));
    p.steps(3);
    assert!(p.harness.query_by_label(&format!("Delete {NAME}?")).is_none(), "the dialog closes once sent");
    assert_eq!(p.project(), None, "a deleted project's page goes back to Projects");
    assert!(p.harness.query_by_label(&format!("{NAME} deleted.")).is_some());
}

#[test]
fn archive_asks_once_and_cancel_keeps_it() {
    let app = RefCell::new(None);
    let f = fixtures(false);
    let mut p = page(&app, &f, Some(PROJECT), None, 1440.0, false);
    open_menu_and_pick(&mut p, "Archive\u{2026}");
    assert!(p.harness.query_by_label(&format!("Archive {NAME}?")).is_some());
    p.harness.get_by_label("Cancel").click();
    p.steps(2);
    assert!(p.harness.query_by_label(&format!("Archive {NAME}?")).is_none());
    assert_eq!(p.project().as_deref(), Some(PROJECT));
}

#[test]
fn nothing_to_use_without_the_right() {
    let mut f = fixtures(false);
    for (k, v) in f.iter_mut() {
        if k == &format!("board:project:{PROJECT}") || k == "task:one" {
            v["canArchive"] = json!(false);
            v["canDelete"] = json!(false);
        }
    }
    // Still offered, greyed: the copies are anyone's, and a missing Delete
    // would teach nobody it exists.
    for (project, task) in [(Some(PROJECT), None), (None, Some(TASK))] {
        let app = RefCell::new(None);
        let mut p = page(&app, &f, project, task, 1440.0, false);
        p.harness.get_all_by_label("More actions").next().unwrap().click();
        p.steps(2);
        assert!(p.harness.get_by_label("Delete\u{2026}").accesskit_node().is_disabled());
        assert!(p.harness.get_by_label("Archive\u{2026}").accesskit_node().is_disabled());
    }
}

// -------------------------------------------------------------------- renders

fn save(p: &mut Page<'_>, name: &str, label: &str) {
    let path = format!("docs/design-mocks/render/archive/{name}-{label}.png");
    std::fs::create_dir_all("docs/design-mocks/render/archive").unwrap();
    p.harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs: cargo test --features app --test archive_ui -- --ignored"]
fn renders() {
    for (width, label) in [(1440.0, "wide"), (820.0, "narrow")] {
        let f = fixtures(false);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Some(PROJECT), None, width, true);
        p.harness.get_all_by_label("More actions").next().unwrap().click();
        p.steps(3);
        save(&mut p, "project-menu", label);
        p.harness.get_by_label("Delete\u{2026}").click();
        p.steps(2);
        type_name(&mut p, "Checkout");
        p.steps(2);
        save(&mut p, "project-delete", label);
        p.harness.get_by_label("Cancel").click();
        p.steps(2);
        open_menu_and_pick(&mut p, "Archive\u{2026}");
        p.steps(2);
        save(&mut p, "project-archive", label);
        p.harness.get_by_label("Cancel").click();
        p.steps(2);
        drop(p);

        let f = fixtures(true);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Some(PROJECT), None, width, true);
        p.harness.get_by_label("Archived").click();
        p.steps(3);
        save(&mut p, "project-archived", label);
        drop(p);

        let f = fixtures(false);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, None, None, width, true);
        p.harness.get_by_label("Archived").click();
        p.steps(3);
        save(&mut p, "projects-archived", label);
        drop(p);

        for archived in [false, true] {
            let f = fixtures(archived);
            let app = RefCell::new(None);
            let mut p = page(&app, &f, None, Some(TASK), width, true);
            if archived {
                save(&mut p, "task-archived", label);
                continue;
            }
            p.harness.get_all_by_label("More actions").next().unwrap().click();
            p.steps(3);
            save(&mut p, "task-menu", label);
            p.harness.get_by_label("Delete\u{2026}").click();
            p.steps(3);
            save(&mut p, "task-delete", label);
            p.harness.get_by_label("Cancel").click();
            p.steps(2);
        }
    }
}
