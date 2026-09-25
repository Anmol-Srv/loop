//! Labels as searchable, creatable badges; repositories with each person's
//! private folder; the task page's hint when the agent has no folder to use.
//! Driven from fixture JSON like tests/archive_ui.rs.
//!
//! `renders` draws them into docs/design-mocks/render/repos-labels/:
//!   cargo test --features app --test repos_labels_ui -- --ignored --nocapture
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
const TASK: &str = "22222222-0000-0000-0000-000000000001";
const BACKEND: &str = "33333333-0000-0000-0000-000000000001";
const Q4: &str = "33333333-0000-0000-0000-000000000002";
const INFRA: &str = "33333333-0000-0000-0000-000000000003";
const PLATFORM: &str = "33333333-0000-0000-0000-000000000004";
const MY_PATH: &str = "/Users/anmol/code/payments-api";

fn ago(minutes: i64) -> String {
    (Utc::now() - Duration::minutes(minutes)).to_rfc3339()
}

fn label(id: &str, name: &str, colour: &str) -> Value {
    json!({"id": id, "name": name, "colour": colour})
}

fn labels() -> Value {
    json!([label(BACKEND, "Backend", "green"), label(INFRA, "Infra", "slate"), label(Q4, "Q4", "amber")])
}

fn project(my_path: Option<&str>) -> Value {
    json!({
        "id": PROJECT, "key": "payments", "name": "Payments", "status": "active", "priority": 1,
        "description": "One payment sheet for every checkout.", "startDate": null, "targetDate": null,
        "labels": [label(BACKEND, "Backend", "green"), label(INFRA, "Infra", "slate"), label(Q4, "Q4", "amber")],
        "repos": [
            {"id": "44444444-0000-0000-0000-000000000001", "name": "API", "url": "git@github.com:airtribe/payments-api.git",
             "myPath": my_path, "canEdit": true},
            {"id": "44444444-0000-0000-0000-000000000002", "name": "Web", "url": "https://github.com/airtribe/payments-web",
             "myPath": null, "canEdit": false},
        ],
        "archivedAt": null, "canArchive": true, "canDelete": true, "taskCount": 1,
        "createdAt": ago(9000), "updatedAt": ago(20), "done": 0, "total": 1,
    })
}

fn task() -> Value {
    json!({
        "id": TASK, "title": "Wire checkout", "status": "in_progress", "discipline": "backend", "priority": 1,
        "body": "Move the card form onto the new sheet.", "projectId": PROJECT, "projectName": "Payments",
        "phaseName": "Work", "assigneePersonId": ME, "assigneeName": "Anmol Srivastava",
        "assigneeEmail": "anmol@airtribe.live", "assigneeKind": "human", "createdAt": ago(4000), "updatedAt": ago(10),
        "doneAt": null, "blockedBy": [], "blockersTotal": 0, "blockersDone": 0, "archivedAt": null,
        "projectArchivedAt": null, "canArchive": true, "canDelete": true, "canSeeAgentPrivate": true,
        "delegate": {"id": "55555555-0000-0000-0000-000000000001", "handle": "hermes", "name": "Hermes (Anmol's Mac)",
            "state": "working", "now": "reading the checkout controller", "nowAt": ago(1), "lastSeenAt": ago(1),
            "delegatedAt": ago(30), "runtime": "hermes", "ownerName": "Anmol Srivastava"},
    })
}

fn fixtures(my_path: Option<&str>) -> Vec<(String, Value)> {
    let row = {
        let mut p = project(my_path);
        p.as_object_mut().unwrap().remove("repos");
        p
    };
    let mut crowded = row.clone();
    crowded["id"] = json!("11111111-0000-0000-0000-000000000002");
    crowded["name"] = json!("Checkout redesign");
    crowded["labels"] = json!([label(BACKEND, "Backend", "green")]);
    vec![
        ("__me".into(), json!({"personId": ME, "email": "anmol@airtribe.live", "name": "Anmol Srivastava", "role": "member", "scopes": ["read", "write"]})),
        ("sidebar:counts".into(), json!({"myOpen": 1, "activeProjects": 2})),
        ("board:people".into(), json!([])),
        ("projects:labels".into(), labels()),
        ("agents:mine".into(), json!([])),
        ("__tracks".into(), json!({})),
        ("board:projects".into(), json!([row, crowded])),
        (format!("board:project:{PROJECT}"), project(my_path)),
        (format!("board:flow:{PROJECT}"), json!({"id": PROJECT, "key": "payments", "name": "Payments", "status": "active",
            "priority": 1, "targetDate": null, "done": 0, "total": 1, "disciplines": [], "activePhase": "Work", "activePeople": 1})),
        (format!("board:tasks:{PROJECT}"), json!([])),
        (format!("project:artifacts:{PROJECT}"), json!([])),
        ("task:one".into(), task()),
        ("task:notes".into(), json!([])),
        ("task:artifacts".into(), json!([])),
    ]
}

struct Page<'a> {
    harness: Harness<'a>,
    app: &'a RefCell<Option<App>>,
    fixtures: &'a RefCell<Vec<(String, Value)>>,
}

impl Page<'_> {
    fn steps(&mut self, n: usize) {
        for _ in 0..n {
            self.harness.step();
        }
    }
    fn with_net<R>(&self, f: impl FnOnce(&mut Net) -> R) -> R {
        f(self.app.borrow_mut().as_mut().unwrap().net.as_mut().unwrap())
    }
    /// Replace one fixture, re-seeded every frame from now on.
    fn fixture(&self, key: &str, value: Value) {
        let mut f = self.fixtures.borrow_mut();
        match f.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => f.push((key.to_owned(), value)),
        }
    }
    fn draft_labels(&self) -> Vec<String> {
        self.app.borrow().as_ref().unwrap().board.creating.as_ref().map(|d| d.labels.clone()).unwrap_or_default()
    }
    /// A row in the open picker, not a badge of the same name.
    fn pick(&mut self, name: &str) {
        self.harness
            .get_by(|n| n.label().as_deref() == Some(name) && n.role() != egui::accesskit::Role::Label)
            .click();
        self.steps(1);
    }
    fn type_search(&mut self, text: &str) {
        self.harness.get_by_label("Search labels").type_text(text);
        self.steps(2);
    }
}

fn page<'a>(
    app: &'a RefCell<Option<App>>,
    fixtures: &'a RefCell<Vec<(String, Value)>>,
    project: Option<&'a str>,
    task: Option<&'a str>,
    width: f32,
    gpu: bool,
) -> Page<'a> {
    let builder = Harness::builder().with_size(egui::vec2(width, 1100.0)).with_pixels_per_point(2.0);
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
                settings: views::settings::State::default(),
            });
            return;
        }
        let a = slot.as_mut().unwrap();
        for (k, v) in fixtures.borrow().iter() {
            a.net.as_mut().unwrap().seed(k, v.clone());
        }
        a.frame(ui);
    });
    for _ in 0..4 {
        harness.step();
    }
    Page { harness, app, fixtures }
}

/// The create form, with its label picker open.
fn open_create_picker(p: &mut Page<'_>) {
    p.harness.get_by_label("Create project").click();
    p.steps(2);
    p.harness.get_by_label("Add labels").click();
    p.steps(3);
}

// ------------------------------------------------------------------ behaviour

#[test]
fn a_label_is_made_from_the_search_and_picked() {
    let app = RefCell::new(None);
    let f = RefCell::new(fixtures(None));
    let mut p = page(&app, &f, None, None, 1440.0, false);
    open_create_picker(&mut p);

    // Typing filters; an existing name, in any case, offers no create row.
    p.type_search("BACKEND");
    let row = |n: &egui_kittest::kittest::AccessKitNode<'_>, name: &str| {
        n.label().as_deref() == Some(name) && n.role() != egui::accesskit::Role::Label
    };
    assert!(p.harness.query_by(|n| row(n, "Backend")).is_some());
    assert!(p.harness.query_by(|n| row(n, "Q4")).is_none(), "filtered out");
    assert!(p.harness.query_by_label_contains("Create label").is_none());
    // Enter ticks the lit row.
    p.harness.key_press(egui::Key::Enter);
    p.steps(2);
    assert_eq!(p.draft_labels(), vec![BACKEND.to_owned()]);

    // Clearing the typed text takes no badge with it.
    for _ in 0.."BACKEND".len() {
        p.harness.key_press(egui::Key::Backspace);
    }
    p.steps(2);
    assert_eq!(p.draft_labels(), vec![BACKEND.to_owned()]);
    p.type_search("Platform");
    assert!(p.harness.query_by_label("Create label \u{201c}Platform\u{201d}").is_some());
    // The next unused colour is offered first; another can be picked.
    assert!(p.harness.get_by_label("Blue").accesskit_node().toggled() == Some(egui::accesskit::Toggled::True), "slate, green, amber are taken");
    p.harness.get_by_label("Purple").click();
    p.steps(2);
    p.harness.get_by_label("Create label \u{201c}Platform\u{201d}").click();
    p.steps(1);
    assert!(p.with_net(|n| n.is_loading("projects:new-label")), "the create is posted");

    // The server answers; the new label is picked and shows as a badge.
    p.fixture("projects:labels", json!([label(BACKEND, "Backend", "green"), label(INFRA, "Infra", "slate"),
        label(PLATFORM, "Platform", "purple"), label(Q4, "Q4", "amber")]));
    p.with_net(|n| n.seed("projects:new-label", label(PLATFORM, "Platform", "purple")));
    p.steps(3);
    assert!(p.draft_labels().contains(&PLATFORM.to_owned()), "{:?}", p.draft_labels());
    assert!(p.harness.query_by_label("Remove Platform").is_some());
}

#[test]
fn badges_are_removable_by_click_and_backspace() {
    let app = RefCell::new(None);
    let f = RefCell::new(fixtures(None));
    let mut p = page(&app, &f, None, None, 1440.0, false);
    open_create_picker(&mut p);
    for name in ["Backend", "Infra", "Q4"] {
        p.pick(name);
    }
    assert_eq!(p.draft_labels(), vec![BACKEND.to_owned(), INFRA.to_owned(), Q4.to_owned()], "several at once");

    // Backspace in the empty search takes the last one off.
    p.harness.key_press(egui::Key::Backspace);
    p.steps(2);
    assert_eq!(p.draft_labels(), vec![BACKEND.to_owned(), INFRA.to_owned()]);

    // Arrows move the lit row; Enter toggles it.
    p.harness.key_press(egui::Key::ArrowDown);
    p.steps(1);
    p.harness.key_press(egui::Key::Enter);
    p.steps(2);
    assert_eq!(p.draft_labels(), vec![BACKEND.to_owned()], "row two, Infra, unticked");

    // Escape closes the popup; the painted × asks before it removes a badge.
    p.harness.key_press(egui::Key::Escape);
    p.steps(2);
    assert!(p.harness.query_by_label("Search labels").is_none());
    p.harness.get_by_label("Remove Backend").click();
    p.steps(2);
    assert_eq!(p.draft_labels(), vec![BACKEND.to_owned()], "nothing comes off before the confirm");
    assert!(p.harness.query_by_label_contains("Remove the").is_some(), "a warning names the label");
    // The confirm's Cancel is the newest one; the create form has its own.
    p.harness.get_all_by_label("Cancel").last().unwrap().click();
    p.steps(2);
    assert_eq!(p.draft_labels(), vec![BACKEND.to_owned()], "Cancel keeps it");
    p.harness.get_by_label("Remove Backend").click();
    p.steps(2);
    p.harness.get_by_label("Remove").click();
    p.steps(2);
    assert!(p.draft_labels().is_empty(), "Remove takes it off");
}

#[test]
fn the_projects_table_shows_two_badges_then_a_count() {
    let app = RefCell::new(None);
    let f = RefCell::new(fixtures(None));
    let p = page(&app, &f, None, None, 1440.0, false);
    assert!(p.harness.query_by_label("+1").is_some(), "Payments has three labels");
    assert!(p.harness.query_all_by_label("Backend").count() >= 2);
}

#[test]
fn repositories_show_my_folder_as_private() {
    let app = RefCell::new(None);
    let f = RefCell::new(fixtures(Some(MY_PATH)));
    let mut p = page(&app, &f, Some(PROJECT), None, 1440.0, false);
    assert!(p.harness.query_by_label("API").is_some());
    assert!(p.harness.query_by_label_contains("github.com:airtribe/payments-api").is_some());
    let field = p.harness.get_by_label("Folder for API on my Mac");
    assert_eq!(field.accesskit_node().value().as_deref(), Some(MY_PATH));
    assert_eq!(p.harness.query_all_by_label("Only you see this").count(), 2);

    // A relative folder is refused before it is sent.
    p.harness.get_by_label("Folder for Web on my Mac").focus();
    p.steps(1);
    p.harness.get_by_label("Folder for Web on my Mac").type_text("code/web");
    p.steps(2);
    assert!(p.harness.query_by_label_contains("Needs a full path starting with /").is_some());

    // Edit and remove only where the server says so.
    let mores: Vec<_> = p.harness.get_all_by_label("More actions").collect();
    mores[mores.len() - 1].click();
    p.steps(2);
    assert!(p.harness.get_by_label("Remove repository\u{2026}").accesskit_node().is_disabled(), "Web is not mine");
}

#[test]
fn the_task_page_says_when_the_agent_has_no_folder() {
    let app = RefCell::new(None);
    let f = RefCell::new(fixtures(None));
    let mut p = page(&app, &f, None, Some(TASK), 1440.0, false);
    let link = "set the folder for API on the project";
    assert!(p.harness.query_by_label_contains("Hermes won\u{2019}t know where the code is").is_some());
    p.harness.get_by_label(link).click();
    p.steps(2);
    let a = app.borrow();
    assert_eq!(a.as_ref().unwrap().project.as_deref(), Some(PROJECT), "the link opens the project");
    assert!(a.as_ref().unwrap().task.is_none());
    drop(a);

    // Every repo has a folder: no hint.
    let app = RefCell::new(None);
    let mut all = project(Some(MY_PATH));
    all["repos"][1]["myPath"] = json!("/Users/anmol/code/web");
    let mut fx = fixtures(Some(MY_PATH));
    fx.retain(|(k, _)| !k.starts_with("board:project:"));
    fx.push((format!("board:project:{PROJECT}"), all));
    let f = RefCell::new(fx);
    let p = page(&app, &f, None, Some(TASK), 1440.0, false);
    assert!(p.harness.query_by_label_contains("won\u{2019}t know where the code is").is_none());
}

// -------------------------------------------------------------------- renders

fn save(p: &mut Page<'_>, name: &str, label: &str) {
    let dir = "docs/design-mocks/render/repos-labels";
    std::fs::create_dir_all(dir).unwrap();
    let path = format!("{dir}/{name}-{label}.png");
    p.harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs: cargo test --features app --test repos_labels_ui -- --ignored"]
fn renders() {
    for (width, label) in [(1440.0, "wide"), (820.0, "narrow")] {
        let app = RefCell::new(None);
        let f = RefCell::new(fixtures(None));
        let mut p = page(&app, &f, None, None, width, true);
        save(&mut p, "projects", label);
        open_create_picker(&mut p);
        p.pick("Backend");
        p.pick("Q4");
        p.type_search("Plat");
        p.steps(2);
        save(&mut p, "create-picker", label);
        drop(p);

        let app = RefCell::new(None);
        let f = RefCell::new(fixtures(Some(MY_PATH)));
        let mut p = page(&app, &f, Some(PROJECT), None, width, true);
        save(&mut p, "project", label);
        p.harness.get_by_label("Add labels").click();
        p.steps(3);
        save(&mut p, "rail-picker", label);
        p.harness.key_press(egui::Key::Escape);
        p.steps(2);
        p.harness.get_by_label("+ Add repository").click();
        p.steps(2);
        save(&mut p, "repo-add", label);
        drop(p);

        let app = RefCell::new(None);
        let f = RefCell::new(fixtures(None));
        let mut p = page(&app, &f, None, Some(TASK), width, true);
        save(&mut p, "task-hint", label);
    }
}
