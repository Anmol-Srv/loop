//! The action menus: a right-click on any row, and the page's "⋯", driven
//! from fixture JSON the way tests/archive_ui.rs does.
//!
//! The behaviour tests run with the suite. `renders` draws the menus into
//! docs/design-mocks/render/context-menu/ for looking at:
//!   cargo test --features app --test context_menu_ui -- --ignored --nocapture
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
const TITLE: &str = "Payment sheet card form";

fn ago(minutes: i64) -> String {
    (Utc::now() - Duration::minutes(minutes)).to_rfc3339()
}

fn task(n: usize, title: &str, status: &str) -> Value {
    json!({
        "id": if n == 0 { TASK.to_owned() } else { format!("22222222-0000-0000-0000-00000000000{n}") },
        "title": title, "status": status, "discipline": "backend", "priority": 1 + n % 3,
        "body": "Move the card form onto the new payment sheet and keep the saved cards.",
        "projectId": PROJECT, "projectName": NAME, "phaseName": "Work",
        "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "assigneeEmail": "anmol@airtribe.live",
        "assigneeKind": "human", "createdAt": ago(4000 + n as i64 * 60), "updatedAt": ago(10), "doneAt": null,
        "delegate": null, "blockedBy": [], "blockersTotal": 0, "blockersDone": 0,
        "archivedAt": null, "projectArchivedAt": null, "canArchive": true, "canDelete": true,
    })
}

fn project(can_delete: bool) -> Value {
    json!({
        "id": PROJECT, "key": "checkout-redesign", "name": NAME, "status": "active", "priority": 1,
        "description": "One payment sheet for every checkout, saved cards included.",
        "startDate": null, "targetDate": null, "labels": [], "archivedAt": null,
        "canArchive": can_delete, "canDelete": can_delete, "taskCount": 3,
        "createdAt": ago(9000), "updatedAt": ago(20), "done": 1, "total": 3,
    })
}

fn fixtures(can_delete: bool) -> Vec<(String, Value)> {
    let tasks = json!([
        task(0, TITLE, "in_progress"),
        task(1, "Saved cards on the sheet", "open"),
        task(2, "Remove the old checkout", "shipped"),
    ]);
    vec![
        ("__me".into(), json!({"personId": ME, "email": "anmol@airtribe.live", "name": "Anmol Srivastava", "role": "member", "scopes": ["read", "write"]})),
        ("sidebar:counts".into(), json!({"myOpen": 4, "activeProjects": 2})),
        ("board:people".into(), json!([])),
        ("projects:labels".into(), json!([])),
        ("agents:mine".into(), json!([
            {"id": "99999999-0000-0000-0000-000000000001", "name": "Hermes (Anmol's Mac)", "status": "connected"},
            {"id": "99999999-0000-0000-0000-000000000002", "name": "Claude Code", "status": "waiting"},
        ])),
        ("__tracks".into(), json!({})),
        ("board:projects".into(), json!([project(can_delete)])),
        (format!("board:project:{PROJECT}"), project(can_delete)),
        (format!("board:flow:{PROJECT}"), json!({"id": PROJECT, "key": "checkout-redesign", "name": NAME, "status": "active",
            "priority": 1, "targetDate": null, "done": 1, "total": 3, "disciplines": [], "activePhase": "Work", "activePeople": 1})),
        (format!("board:tasks:{PROJECT}"), tasks.clone()),
        (format!("project:artifacts:{PROJECT}"), json!([])),
        ("home".into(), json!({"myTasks": [], "waitingOnMe": [], "team": [], "needsAttention": [],
            "projects": [{"id": PROJECT, "name": NAME, "status": "active", "done": 1, "total": 3}]})),
        ("home:tasks".into(), tasks.clone()),
        ("mytasks:mine".into(), tasks),
        ("task:one".into(), task(0, TITLE, "in_progress")),
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
    fn steps(&mut self, n: usize) {
        for _ in 0..n {
            self.harness.step();
        }
    }
    fn open(&self) -> (Option<String>, Option<String>) {
        let app = self.app.borrow();
        let a = app.as_ref().unwrap();
        (a.project.clone(), a.task.clone())
    }
    fn right_click(&mut self, label: &str) {
        self.harness.get_by_label(label).click_secondary();
        self.steps(3);
    }
    fn more(&mut self) {
        self.harness.get_all_by_label("More actions").next().unwrap().click();
        self.steps(3);
    }
    /// A menu item, not the column header or rail label that shares its words.
    fn item<'s>(&'s self, label: &'s str) -> Option<egui_kittest::Node<'s>> {
        self.harness.query_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(label))
    }
    fn has(&self, label: &str) -> bool {
        self.item(label).is_some() || self.harness.query_by_label(label).is_some()
    }
    fn disabled(&self, label: &str) -> bool {
        self.item(label).unwrap().accesskit_node().is_disabled()
    }
}

fn page<'a>(
    app: &'a RefCell<Option<App>>,
    fixtures: &'a [(String, Value)],
    tab: Tab,
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
                tab,
                project: project.map(str::to_owned),
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
    Page { harness, app }
}

/// What every task menu carries, wherever it is opened from.
const TASK_ITEMS: [&str; 6] = ["Hand off to", "Priority", "Copy title", "Copy ID", "Archive\u{2026}", "Delete\u{2026}"];

// ------------------------------------------------------------------ behaviour

#[test]
fn right_click_on_a_project_row_opens_its_menu_and_escape_closes_it() {
    let app = RefCell::new(None);
    let f = fixtures(true);
    let mut p = page(&app, &f, Tab::Projects, None, None, 1440.0, false);
    p.right_click(NAME);
    for item in ["Open", "Copy name", "Copy ID", "Archive\u{2026}", "Delete\u{2026}"] {
        assert!(p.has(item), "{item} is on the menu");
        assert!(!p.disabled(item), "{item} is usable");
    }
    assert_eq!(p.open(), (None, None), "a right-click is not a visit");

    p.harness.key_press(egui::Key::Escape);
    p.steps(2);
    assert!(!p.has("Copy name"), "Escape closes the menu");
}

#[test]
fn the_keyboard_walks_the_menu() {
    let app = RefCell::new(None);
    let f = fixtures(true);
    let mut p = page(&app, &f, Tab::Projects, None, None, 1440.0, false);
    p.right_click(NAME);
    p.harness.key_press(egui::Key::ArrowDown);
    p.steps(2);
    assert!(p.harness.get_by_label("Open").is_focused(), "Down lands on the first item");
    p.harness.key_press(egui::Key::Enter);
    p.steps(3);
    assert_eq!(p.open().0.as_deref(), Some(PROJECT), "Enter picks it");
}

#[test]
fn delete_is_greyed_with_the_reason_without_the_right() {
    let app = RefCell::new(None);
    let f = fixtures(false);
    let mut p = page(&app, &f, Tab::Projects, None, None, 1440.0, false);
    p.right_click(NAME);
    assert!(p.disabled("Delete\u{2026}"));
    assert!(p.disabled("Archive\u{2026}"));
    p.harness.get_by_label("Delete\u{2026}").hover();
    p.steps(60);
    assert!(p
        .harness
        .query_by_label("Only whoever created this project, or an admin, can delete it.")
        .is_some());
}

#[test]
fn archive_from_a_home_row_asks_first() {
    let app = RefCell::new(None);
    let f = fixtures(true);
    let mut p = page(&app, &f, Tab::Home, None, None, 1440.0, false);
    p.right_click(TITLE);
    for item in TASK_ITEMS {
        assert!(p.has(item), "{item} is on the row's menu");
    }
    p.harness.get_by_label("Archive\u{2026}").click();
    p.steps(3);
    assert!(p.has(&format!("Archive \u{201c}{TITLE}\u{201d}?")));
    p.harness.get_by_label("Cancel").click();
    p.steps(2);
    assert!(!p.has(&format!("Archive \u{201c}{TITLE}\u{201d}?")));
}

#[test]
fn the_task_page_button_opens_the_same_menu_as_a_row() {
    // The row: on the project page, where Open is the extra.
    let app = RefCell::new(None);
    let f = fixtures(true);
    let mut p = page(&app, &f, Tab::Projects, Some(PROJECT), None, 1440.0, false);
    p.right_click(TITLE);
    for item in TASK_ITEMS.iter().chain(&["Open"]) {
        assert!(p.has(item), "{item} is on the row's menu");
    }
    drop(p);

    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Projects, None, Some(TASK), 1440.0, false);
    p.more();
    for item in TASK_ITEMS {
        assert!(p.has(item), "{item} is on the page's menu");
    }
    assert!(!p.has("Open"), "the page is already open");
    p.harness.key_press(egui::Key::Escape);
    p.steps(2);
    assert!(!p.has("Copy title"));

    // And the title answers a right-click with it too.
    p.right_click(TITLE);
    assert!(p.has("Copy title"));
}

#[test]
fn copy_says_so() {
    let app = RefCell::new(None);
    let f = fixtures(true);
    let mut p = page(&app, &f, Tab::MyTasks, None, None, 1440.0, false);
    p.right_click(TITLE);
    p.harness.get_by_label("Copy ID").click();
    p.steps(2);
    assert!(p.has("Copied."));
    assert!(!p.has("Copy ID"), "picking an item closes the menu");
}

// -------------------------------------------------------------------- renders

fn save(p: &mut Page<'_>, name: &str, label: &str) {
    let dir = "docs/design-mocks/render/context-menu";
    let path = format!("{dir}/{name}-{label}.png");
    std::fs::create_dir_all(dir).unwrap();
    p.harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs: cargo test --features app --test context_menu_ui -- --ignored"]
fn renders() {
    for (width, label) in [(1440.0, "wide"), (820.0, "narrow")] {
        let f = fixtures(true);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Projects, None, None, width, true);
        p.right_click(NAME);
        save(&mut p, "project-row-menu", label);
        drop(p);

        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Home, None, None, width, true);
        p.right_click(TITLE);
        p.harness.get_by_label("Hand off to").hover();
        p.steps(4);
        save(&mut p, "home-task-row-menu", label);
        drop(p);

        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Projects, None, Some(TASK), width, true);
        p.more();
        p.item("Priority").unwrap().hover();
        p.steps(4);
        save(&mut p, "task-page-more-menu", label);
        drop(p);

        let f = fixtures(false);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Projects, None, None, width, true);
        p.right_click(NAME);
        p.harness.get_by_label("Delete\u{2026}").hover();
        p.steps(60);
        save(&mut p, "disabled-with-reason", label);
    }
}
