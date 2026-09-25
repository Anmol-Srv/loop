//! Standalone tasks in the app: the New task dialog (from My Tasks, Home,
//! the palette and `C`), "—" in the tables' Project column, and the task
//! page's "No project" breadcrumb and Project picker.
//!
//! `renders` draws each surface at 1440 and 820 into
//! docs/design-mocks/render/standalone/:
//!   cargo test --features app --test standalone_ui -- --ignored --nocapture
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
const PRIYA: &str = "aaaaaaaa-0000-0000-0000-000000000002";
const CHECKOUT: &str = "11111111-0000-0000-0000-000000000002";
const ALONE: &str = "33333333-0000-0000-0000-000000000001";
const ALONE_TITLE: &str = "Rotate the Razorpay key";

fn ago(minutes: i64) -> String {
    (Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339()
}

fn standalone() -> Value {
    json!({"id": ALONE, "title": ALONE_TITLE, "status": "open", "priority": 1, "discipline": "backend",
        "body": "The live key leaked into a screenshot. Rotate it and update the env.",
        "projectId": null, "projectName": null, "phaseId": null, "phaseName": null, "category": "chore",
        "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "assigneeEmail": "anmol@airtribe.live",
        "createdAt": ago(30), "updatedAt": ago(30), "doneAt": null, "delegate": null,
        "canArchive": true, "canDelete": true})
}

fn in_project() -> Value {
    json!({"id": "t1", "title": "Rate leads by role bucket", "status": "in_progress", "priority": 2,
        "discipline": "backend", "body": "Expose the bucket on the lead payload.", "projectId": CHECKOUT,
        "projectName": "Checkout redesign", "assigneePersonId": ME, "assigneeName": "Anmol Srivastava",
        "assigneeEmail": "anmol@airtribe.live", "createdAt": ago(4000), "updatedAt": ago(10), "doneAt": null,
        "delegate": null})
}

type Fixtures = Vec<(&'static str, Value)>;

fn base() -> Fixtures {
    vec![
        ("__me", json!({"personId": ME, "email": "anmol@airtribe.live", "name": "Anmol Srivastava",
            "role": "member", "scopes": ["read", "write"]})),
        ("sidebar:counts", json!({"myOpen": 2, "activeProjects": 1, "triage": 0})),
        ("__tracks", json!({})),
        ("agents:mine", json!([])),
        ("agents:active", json!([])),
        ("board:people", json!([
            {"id": ME, "name": "Anmol Srivastava", "department": "backend"},
            {"id": PRIYA, "name": "Priya Nair", "department": "design"},
        ])),
        ("board:projects", json!([
            {"id": CHECKOUT, "name": "Checkout redesign", "status": "active"},
            {"id": "p9", "name": "Old launch", "status": "done", "archivedAt": ago(9000)},
        ])),
        ("mytasks:mine", json!([standalone(), in_project()])),
        ("home", json!({"myTasks": [], "waitingOnMe": [], "team": [], "needsAttention": [],
            "projects": [{"id": CHECKOUT, "name": "Checkout redesign", "status": "active", "done": 0, "total": 1}]})),
        ("home:tasks", json!([standalone(), in_project()])),
        ("palette:tasks", json!([standalone(), in_project()])),
        ("palette:projects", json!([{"id": CHECKOUT, "name": "Checkout redesign", "status": "active"}])),
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
    fn dialog_open(&self) -> bool {
        self.has("Create task")
    }
    fn type_title(&mut self, s: &str) {
        self.harness.get_by(|n| n.placeholder().is_some_and(|h| h.starts_with("e.g. Fix the saved-card"))).focus();
        self.steps(1);
        self.harness.get_by(|n| n.placeholder().is_some_and(|h| h.starts_with("e.g. Fix the saved-card"))).type_text(s);
        self.steps(2);
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
) -> Page<'a> {
    let builder = Harness::builder().with_size(egui::vec2(size.0, size.1)).with_pixels_per_point(2.0);
    let builder = if gpu { builder.wgpu() } else { builder };
    let mut harness = builder.build_ui(move |ui| {
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

// ------------------------------------------------------------------ behaviour

#[test]
fn new_task_from_my_tasks_creates_and_closes() {
    let f = base();
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::MyTasks, None, (1440.0, 1000.0), false);
    p.button("New task");
    assert!(p.dialog_open());
    // Assigned to me and P2 until changed; no project is the default.
    assert!(p.has("Anmol Srivastava \u{00B7} backend"));
    assert!(p.has("P2 Normal") || p.has("P2"));
    assert!(p.has("No category") && p.has("No project"));

    // Nothing to create without a title.
    let create = |p: &Page<'_>| p.harness.get_by_label("Create task").accesskit_node().is_disabled();
    assert!(create(&p));
    p.type_title("Rotate the webhook secret");
    assert!(!create(&p));

    p.click(egui::accesskit::Role::ComboBox, "No project");
    assert!(p.has("Checkout redesign"));
    assert!(!p.has("Old launch"), "archived projects are not offered");
    p.click(egui::accesskit::Role::Button, "Checkout redesign");

    p.button("Create task");
    assert!(p.has("Creating\u{2026}"));
    p.seed("newtask:create", json!({"id": "new", "title": "Rotate the webhook secret"}));
    p.steps(3);
    assert!(!p.dialog_open(), "closed on success");
    assert!(p.has("Created \u{201c}Rotate the webhook secret\u{201d}."));
}

#[test]
fn escape_closes_and_cmd_enter_submits() {
    let f = base();
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, None, (1440.0, 1300.0), false);
    p.button("New task");
    assert!(p.dialog_open(), "Home's table header opens it too");
    p.harness.key_press(egui::Key::Escape);
    p.steps(3);
    assert!(!p.dialog_open());

    p.button("New task");
    p.type_title("From the keyboard");
    p.harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Enter);
    p.steps(2);
    assert!(p.has("Creating\u{2026}"), "Cmd+Enter submits");
    p.seed("newtask:create", json!({"id": "new"}));
    p.steps(3);
    assert!(!p.dialog_open());
}

#[test]
fn c_and_the_palette_open_it() {
    let f = base();
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, None, (1440.0, 1300.0), false);
    p.harness.key_press(egui::Key::C);
    p.steps(3);
    assert!(p.dialog_open(), "C with nothing focused");
    // Typing a C into the title is a C, not a second dialog.
    p.type_title("C");
    assert!(p.dialog_open());
    p.harness.key_press(egui::Key::Escape);
    p.steps(3);

    p.harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    p.steps(3);
    p.harness.get_by(|n| n.placeholder().is_some_and(|h| h.starts_with("Search tasks and projects"))).type_text("new");
    p.steps(2);
    p.harness.key_press(egui::Key::Enter);
    p.steps(3);
    assert!(p.dialog_open(), "the palette's New task");
}

#[test]
fn tables_show_a_dash_for_no_project() {
    let f = base();
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, None, (1440.0, 1000.0), false);
    assert!(p.has(ALONE_TITLE));
    assert!(p.has("\u{2014}"), "My Tasks' Project column");
    assert!(p.harness.query_all_by_label("Checkout redesign").next().is_some());
    drop(p);

    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::Home, None, (1440.0, 1300.0), false);
    assert!(p.has(ALONE_TITLE));
    assert!(p.has("\u{2014}"), "Home's Project column");
}

#[test]
fn the_task_page_says_no_project_and_moves_it() {
    let f = task_page();
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::MyTasks, Some(ALONE), (1440.0, 1300.0), false);
    assert!(p.has("No project"), "the breadcrumb");
    p.click(egui::accesskit::Role::ComboBox, "No project");
    assert!(p.has("Checkout redesign"));
    p.click(egui::accesskit::Role::Button, "Checkout redesign");
    p.seed("task:details", json!({"id": ALONE, "phaseId": "ph"}));
    p.steps(3);
    assert!(p.has("Saved."));
}

#[test]
fn a_teammate_reads_no_project_without_a_picker() {
    let mut f = task_page();
    let mut t = standalone();
    t["canArchive"] = json!(false);
    f.retain(|(k, _)| *k != "task:one");
    f.push(("task:one", t));
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(ALONE), (1440.0, 1300.0), false);
    assert!(p.has("No project"));
    assert!(
        p.harness
            .query_all_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.label().as_deref() == Some("No project"))
            .next()
            .is_none(),
        "only its creator or an admin moves it"
    );
}

// -------------------------------------------------------------------- renders

const WIDTHS: [(f32, &str); 2] = [(1440.0, "1440"), (820.0, "820")];
const DIR: &str = "docs/design-mocks/render/standalone";

fn save(p: &mut Page<'_>, name: &str, label: &str) {
    std::fs::create_dir_all(DIR).unwrap();
    let path = format!("{DIR}/{name}-{label}.png");
    p.harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs: cargo test --features app --test standalone_ui -- --ignored"]
fn renders() {
    for (width, label) in WIDTHS {
        let f = base();
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::MyTasks, None, (width, 1000.0), true);
        save(&mut p, "my-tasks", label);
        p.button("New task");
        p.type_title("Rotate the webhook secret");
        p.steps(10);
        save(&mut p, "new-task-dialog", label);
        p.click(egui::accesskit::Role::ComboBox, "No project");
        p.steps(3);
        save(&mut p, "new-task-project-menu", label);
        drop(p);

        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Home, None, (width, 1300.0), true);
        save(&mut p, "home", label);
        drop(p);

        let f = task_page();
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::MyTasks, Some(ALONE), (width, 1100.0), true);
        save(&mut p, "task-page", label);
        p.click(egui::accesskit::Role::ComboBox, "No project");
        p.steps(3);
        save(&mut p, "task-project-picker", label);
    }
}
