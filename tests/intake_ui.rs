//! Intake agents: the Triage tab and My Tasks' line to it, the task page's
//! Source card and category, Accept / Dismiss from the row (hover icons, A/D),
//! the menu and the page, the
//! connect flow's intake role, the agent card's intake stats, and Home's
//! triage items — from fixture JSON in the shapes of
//! docs/superpowers/plans/2026-09-25-intake-agents.md.
//!
//! `renders` draws each surface at 1440 and 820 into
//! docs/design-mocks/render/intake/:
//!   cargo test --features app --test intake_ui -- --ignored --nocapture
//! and the Triage tab, My Tasks' line to it and the task page's decision into
//! docs/design-mocks/render/triage/.
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
const SLACK_AGENT: &str = "99999999-0000-0000-0000-000000000007";
const INTAKE_PROJECT: &str = "11111111-0000-0000-0000-000000000009";
const CHECKOUT: &str = "11111111-0000-0000-0000-000000000002";
const BUG: &str = "33333333-0000-0000-0000-000000000001";
const DM: &str = "33333333-0000-0000-0000-000000000002";
const IDEA: &str = "33333333-0000-0000-0000-000000000003";

const BUG_TITLE: &str = "Checkout fails for saved cards";
const DM_TITLE: &str = "Export cohort roster as CSV";
const IDEA_TITLE: &str = "Show cohort start dates on the invoice";
const BUG_TEXT: &str = "Checkout is failing for anyone paying with a saved card \u{2014} it spins and then says \u{201c}payment method invalid\u{201d}. New cards work. Started this morning.";
const DM_TEXT: &str =
    "Hey, could we get a CSV export of the roster? I copy it by hand every Monday.";

fn ago(minutes: i64) -> String {
    (Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339()
}

fn me(person: &str) -> Value {
    let (name, email) = if person == ME {
        ("Anmol Srivastava", "anmol@airtribe.live")
    } else {
        ("Priya Nair", "priya@airtribe.live")
    };
    json!({"personId": person, "email": email, "name": name, "role": "member", "scopes": ["read", "write"]})
}

fn source(private: bool, reader_is_owner: bool) -> Value {
    if private {
        let (text, author) = if reader_is_owner {
            (json!(DM_TEXT), json!("Rahul Mehta"))
        } else {
            (Value::Null, Value::Null)
        };
        return json!({"kind": "slack", "url": "https://airtribe.slack.com/archives/D04AB12CD/p1727251200", "channel": "D04AB12CD",
            "private": true, "author": author, "text": text, "receivedAt": ago(95), "agentName": "Slack Agent",
            "reason": "a feature ask sent to you directly", "confidence": 0.71});
    }
    json!({"kind": "slack", "url": "https://airtribe.slack.com/archives/C04AB12CD/p1727258000", "channel": "C04AB12CD",
        "channelName": "issues-and-feedback", "author": "Priya", "text": BUG_TEXT, "receivedAt": ago(12),
        "agentName": "Slack Agent", "reason": "looks like a bug: checkout fails for saved cards", "confidence": 0.86})
}

fn triage_task(id: &str, title: &str, category: &str, src: Value, minutes: i64) -> Value {
    json!({
        "id": id, "title": title, "status": "triage", "category": category, "discipline": "backend", "priority": 2,
        "body": "Filed from Slack by Slack Agent.", "projectId": null, "projectName": null,
        "labels": [{"id": "l-slack", "name": "Slack", "colour": "purple"}],
        "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "assigneeEmail": "anmol@airtribe.live",
        "createdAt": ago(minutes), "updatedAt": ago(minutes), "doneAt": null, "delegate": null, "source": src,
        "canArchive": true, "canDelete": true,
    })
}

fn triage() -> Vec<Value> {
    vec![
        triage_task(BUG, BUG_TITLE, "bug", source(false, true), 12),
        triage_task(
            IDEA,
            IDEA_TITLE,
            "feedback",
            json!({"kind": "slack", "url": "https://airtribe.slack.com/archives/C05/p1", "channel": "C05XY34EF",
                   "channelName": "sales-floor", "author": "Karan", "text": "Learners keep asking which cohort an invoice is for.",
                   "receivedAt": ago(48), "agentName": "Slack Agent", "reason": "feedback on invoices", "confidence": 0.64}),
            48,
        ),
        triage_task(DM, DM_TITLE, "feature", source(true, true), 95),
    ]
}

fn mine(with_triage: bool) -> Value {
    let mut rows = if with_triage { triage() } else { vec![] };
    rows.push(json!({"id": "t1", "title": "Rate leads by role bucket", "status": "in_progress", "priority": 1,
        "body": "Expose the bucket on the lead payload.", "projectId": "p1", "projectName": "Lead Rating",
        "assigneePersonId": ME, "createdAt": ago(4000), "updatedAt": ago(10), "doneAt": null}));
    rows.push(
        json!({"id": "t2", "title": "Payment sheet sync retries", "status": "open", "priority": 2,
        "body": "Retry the sheet write on 429.", "projectId": "p2", "projectName": "Sales tooling",
        "assigneePersonId": ME, "createdAt": ago(6000), "updatedAt": ago(300), "doneAt": null}),
    );
    Value::Array(rows)
}

fn agents(intake: bool) -> Value {
    json!([
        {"id": SLACK_AGENT, "handle": "slack-agent", "name": "Slack Agent", "runtime": "other", "status": "connected",
         "lastSeenAt": ago(2), "createdAt": ago(9000), "activeTasks": 0, "currentTask": null,
         "canIntake": intake, "intakeProjectId": if intake { json!(INTAKE_PROJECT) } else { Value::Null },
         "intakeProjectName": "Slack \u{2014} Anmol",
         "intakeStats": {"triage": 3, "accepted": 12, "dismissed": 4},
         "activity": [0, 3, 5, 2, 4, 6, 1, 0, 2, 7, 3, 4, 5, 3]},
        {"id": "99999999-0000-0000-0000-000000000001", "handle": "hermes", "name": "Hermes (Anmol's Mac)", "runtime": "hermes",
         "status": "connected", "lastSeenAt": ago(400), "createdAt": ago(9100), "activeTasks": 0, "currentTask": null,
         "canIntake": false, "setup": {"skill": "1.4.0", "mcp": true, "watcher": true}},
    ])
}

fn projects() -> Value {
    json!([
        {"id": INTAKE_PROJECT, "name": "Slack \u{2014} Anmol", "status": "active"},
        {"id": CHECKOUT, "name": "Checkout redesign", "status": "active"},
        {"id": "p1", "name": "Lead Rating", "status": "active"},
        {"id": "p9", "name": "Old launch", "status": "done", "archivedAt": ago(9000)},
    ])
}

type Fixtures = Vec<(&'static str, Value)>;

fn base(viewer: Value) -> Fixtures {
    vec![
        ("__me", viewer),
        (
            "sidebar:counts",
            json!({"myOpen": 2, "activeProjects": 3, "triage": 3}),
        ),
        ("__tracks", json!({})),
        ("board:people", json!([])),
        ("board:projects", projects()),
    ]
}

fn my_tasks(with_triage: bool, intake: bool) -> Fixtures {
    let mut f = base(me(ME));
    f.push(("agents:mine", agents(intake)));
    f.push(("mytasks:mine", mine(with_triage)));
    f
}

fn task_page_fixtures(id: &str, owner: bool) -> Fixtures {
    let mut f = base(me(if owner { ME } else { PRIYA }));
    f.push(("agents:mine", if owner { agents(true) } else { json!([]) }));
    let mut t = match id {
        DM => triage_task(DM, DM_TITLE, "feature", source(true, owner), 95),
        _ => triage_task(BUG, BUG_TITLE, "bug", source(false, owner), 12),
    };
    t["body"] = json!(if id == DM {
        "A CSV of the cohort roster from the roster page, so nobody copies it by hand each week."
    } else {
        "Saved-card payments fail at the confirm step with \u{201c}payment method invalid\u{201d}; new cards go through."
    });
    f.push(("task:one", t));
    f.push(("task:notes", json!([])));
    f.push(("task:artifacts", json!([])));
    f
}

fn home_fixtures() -> Fixtures {
    let mut f = base(me(ME));
    f.push(("agents:mine", agents(true)));
    f.push(("agents:active", json!([])));
    f.push(("home", json!({"myTasks": [], "waitingOnMe": [], "team": [],
        "needsAttention": [
            {"kind": "triage", "taskId": BUG, "title": BUG_TITLE, "agentName": "Slack Agent", "body": "bug"},
            {"kind": "triage", "taskId": IDEA, "title": "Show cohort start dates on the invoice", "agentName": "Slack Agent", "body": "feedback"},
            {"kind": "triage", "taskId": DM, "title": DM_TITLE, "agentName": "Slack Agent", "body": "feature"},
        ],
        "projects": [{"id": INTAKE_PROJECT, "name": "Slack \u{2014} Anmol", "status": "active", "done": 0, "total": 3}]})));
    let mut tasks = triage();
    tasks.push(json!({"id": "t1", "title": "Rate leads by role bucket", "status": "in_progress", "discipline": "backend",
        "priority": 1, "projectId": "p1", "projectName": "Lead Rating", "assigneeName": "Anmol Srivastava",
        "createdAt": ago(4000), "updatedAt": ago(10), "doneAt": null, "delegate": null}));
    f.push(("home:tasks", Value::Array(tasks)));
    f
}

struct Page<'a> {
    harness: Harness<'a>,
    app: &'a RefCell<Option<App>>,
}

impl Page<'_> {
    fn seed(&self, key: &str, value: Value) {
        self.app
            .borrow_mut()
            .as_mut()
            .unwrap()
            .net
            .as_mut()
            .unwrap()
            .seed(key, value);
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
        self.harness
            .query_all_by_label_contains(label)
            .next()
            .is_some()
    }
    fn button(&mut self, label: &str) {
        self.harness
            .get_all_by(|n| {
                n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(label)
            })
            .next()
            .unwrap_or_else(|| panic!("no button {label}"))
            .click();
        self.steps(3);
    }
    fn open(&self) -> (Option<String>, Option<String>) {
        let app = self.app.borrow();
        let a = app.as_ref().unwrap();
        (a.project.clone(), a.task.clone())
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
    let builder = Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(2.0);
    let builder = if gpu { builder.wgpu() } else { builder };
    let mut harness = builder.build_ui(move |ui| {
        let mut slot = app.borrow_mut();
        if slot.is_none() {
            theme::install(ui.ctx());
            ui.ctx()
                .all_styles_mut(|s| s.animation_time = egui::Style::default().animation_time);
            ui.ctx().set_zoom_factor(1.15);
            *slot = Some(App {
                net: Some(Net::spawn(
                    silent_server().into(),
                    "test".into(),
                    ui.ctx().clone(),
                )),
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

// ------------------------------------------------------------------ behaviour

fn tab(p: &Page<'_>) -> Tab {
    p.app.borrow().as_ref().unwrap().tab
}

/// The row itself, a button named by its title.
fn row_node<'a>(p: &'a Page<'_>, title: &'a str) -> egui_kittest::Node<'a> {
    p.harness
        .get_all_by(|n| {
            n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(title)
        })
        .next()
        .unwrap_or_else(|| panic!("no row {title}"))
}

fn hover_row(p: &mut Page<'_>, title: &str) {
    row_node(p, title).hover();
    p.steps(4);
}

#[test]
fn my_tasks_leads_with_one_line_to_triage() {
    let f = my_tasks(true, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::MyTasks, None, (1440.0, 1100.0), false);
    assert!(p.has("3 to triage"), "one line, not the group");
    assert!(!p.has(BUG_TITLE), "the filings live on the Triage tab");
    // Triage is not in the list below, which keeps the rest.
    assert!(p.has("2 tasks"));
    assert!(p.has("Rate leads by role bucket"));
    p.harness.get_by_label("3 to triage").click();
    p.steps(3);
    assert!(tab(&p) == Tab::Triage);
    assert!(p.has(BUG_TITLE));
}

#[test]
fn triage_is_a_tab_from_the_sidebar() {
    let f = my_tasks(true, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::MyTasks, None, (1440.0, 1100.0), false);
    p.harness
        .get_by(|n| {
            n.label().as_deref() == Some("Triage") && n.role() == egui::accesskit::Role::Button
        })
        .click();
    p.steps(3);
    assert!(tab(&p) == Tab::Triage);
    assert!(p.has("3 waiting on a yes or a no"));
    assert!(p.has(BUG_TITLE) && p.has(DM_TITLE));
    assert!(p.has_part("Slack \u{00B7} #issues-and-feedback \u{00B7} Priya \u{00B7} 12m"));
    assert!(p.has_part("Slack \u{00B7} Direct message \u{00B7} Rahul Mehta"));
    assert!(p.has("Bug") && p.has("Feature") && p.has("Feedback"));
    // A click on a row opens the task.
    row_node(&p, BUG_TITLE).click();
    p.steps(2);
    assert_eq!(p.open().1.as_deref(), Some(BUG));
}

#[test]
fn triage_opens_from_the_palette() {
    let f = my_tasks(true, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, None, (1440.0, 1100.0), false);
    p.harness
        .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    p.steps(2);
    p.harness.event(egui::Event::Text("Triage".into()));
    p.steps(2);
    p.harness.key_press(egui::Key::Enter);
    p.steps(3);
    assert!(tab(&p) == Tab::Triage);
}

#[test]
fn rows_are_quiet_until_hovered() {
    let f = my_tasks(true, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Triage, None, (1440.0, 1100.0), false);
    let accept = format!("Accept {BUG_TITLE}");
    assert!(!p.has(&accept) && !p.has("Accept"), "no buttons at rest");
    assert!(!p.has("Accept into a project"), "no split button");
    hover_row(&mut p, BUG_TITLE);
    assert!(p.has(&accept) && p.has(&format!("Dismiss {BUG_TITLE}")));
    assert!(
        !p.has(&format!("Accept {DM_TITLE}")),
        "only the row pointed at"
    );
    p.button(&accept);
    p.seed("tasks:action", json!({"id": BUG, "status": "open"}));
    p.steps(3);
    assert!(p.has("Accepted \u{2014} it is an open task now."));
    assert_eq!(p.open(), (None, None), "a decision is not a visit");
}

#[test]
fn a_and_d_decide_the_hovered_row() {
    let f = my_tasks(true, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Triage, None, (1440.0, 1100.0), false);
    hover_row(&mut p, DM_TITLE);
    p.harness.key_press(egui::Key::D);
    p.steps(3);
    assert!(p.has(&format!("Dismiss \u{201c}{DM_TITLE}\u{201d}?")));
    // Typing a reason with an A in it is typing, not a decision.
    p.harness
        .get_by(|n| {
            n.placeholder()
                .is_some_and(|h| h.starts_with("Already fixed"))
        })
        .focus();
    p.steps(1);
    p.harness.key_press(egui::Key::A);
    p.steps(2);
    assert!(
        p.has(&format!("Dismiss \u{201c}{DM_TITLE}\u{201d}?")),
        "still asking"
    );
    p.harness.key_press(egui::Key::Escape);
    p.steps(3);

    hover_row(&mut p, BUG_TITLE);
    p.harness.key_press(egui::Key::A);
    p.steps(2);
    p.seed("tasks:action", json!({"id": BUG, "status": "open"}));
    p.steps(3);
    assert!(p.has("Accepted \u{2014} it is an open task now."));
}

#[test]
fn accept_into_is_on_the_right_click_menu() {
    let f = my_tasks(true, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Triage, None, (1440.0, 1100.0), false);
    row_node(&p, BUG_TITLE).click_secondary();
    p.steps(3);
    for item in [
        "Open",
        "Accept",
        "Accept into",
        "Dismiss\u{2026}",
        "Copy title",
    ] {
        assert!(p.has(item), "{item}");
    }
    p.harness.get_by_label("Accept into").hover();
    p.steps(3);
    assert!(p.has("Checkout redesign"));
    assert!(!p.has("Old launch"), "archived projects are not offered");
    p.button("Checkout redesign");
    p.seed(
        "tasks:action",
        json!({"id": BUG, "status": "open", "projectId": CHECKOUT}),
    );
    p.steps(3);
    assert!(p.has("Accepted into Checkout redesign."));
}

#[test]
fn dismiss_asks_for_an_optional_reason() {
    let f = my_tasks(true, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Triage, None, (1440.0, 1100.0), false);
    let dismiss = format!("Dismiss {BUG_TITLE}");
    hover_row(&mut p, BUG_TITLE);
    p.button(&dismiss);
    assert!(p.has(&format!("Dismiss \u{201c}{BUG_TITLE}\u{201d}?")));
    p.harness.key_press(egui::Key::Escape);
    p.steps(3);
    assert!(
        !p.has(&format!("Dismiss \u{201c}{BUG_TITLE}\u{201d}?")),
        "Escape leaves it in triage"
    );

    hover_row(&mut p, BUG_TITLE);
    p.button(&dismiss);
    p.harness
        .get_by(|n| {
            n.placeholder()
                .is_some_and(|h| h.starts_with("Already fixed"))
        })
        .focus();
    p.steps(1);
    p.harness
        .get_by(|n| {
            n.placeholder()
                .is_some_and(|h| h.starts_with("Already fixed"))
        })
        .type_text("Fixed in #4821");
    p.steps(1);
    p.harness.key_press(egui::Key::Enter);
    p.steps(2);
    p.seed("tasks:action", json!({"id": BUG, "status": "dropped"}));
    p.steps(3);
    assert!(p.has("Dismissed."));
}

#[test]
fn an_empty_triage_tab_stays_put() {
    let mut f = my_tasks(false, true);
    f[1] = (
        "sidebar:counts",
        json!({"myOpen": 2, "activeProjects": 3, "triage": 0}),
    );
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::Triage, None, (1440.0, 1100.0), false);
    assert!(tab(&p) == Tab::Triage);
    assert!(p.has("Nothing to triage."));
    assert!(p.has("Tasks your intake agents file land here."));
    assert!(p.has("Triage"), "the sidebar row stays while you are on it");
    drop(p);

    // Elsewhere, with nothing waiting, neither the row nor the line shows.
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, None, (1440.0, 1100.0), false);
    assert!(!p.has("Triage") && !p.has_part("to triage"));
}

#[test]
fn source_card_quotes_the_message_for_the_owner() {
    let f = task_page_fixtures(BUG, true);
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(BUG), (1440.0, 1400.0), false);
    assert!(p.has("Source"));
    assert!(p.has(BUG_TEXT));
    assert!(p.has("Open in Slack"));
    assert!(p.has_part("Filed by Slack Agent \u{2014} looks like a bug: checkout fails for saved cards \u{00B7} 0.86"));
    assert!(p.has("Category: Bug"), "the owner can recategorise");
    assert!(p.has("Accept") && p.has("Dismiss"));
    assert!(!p.has("Accept into a project"), "no caret beside Accept");
    assert!(p.has("Triage"), "the rail's status");
}

#[test]
fn a_teammate_sees_a_dm_source_without_its_words() {
    let f = task_page_fixtures(DM, false);
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(DM), (1440.0, 1400.0), false);
    assert!(p.has("Source"));
    assert!(!p.has(DM_TEXT));
    assert!(!p.has_part("Rahul"));
    assert!(p.has("From a direct message \u{2014} only Anmol can read it."));
    assert!(!p.has("Open in Slack"));
    assert!(!p.has("Accept") && !p.has("Dismiss"));
    assert!(p.has_part("waiting on Anmol to accept or dismiss it"));
    drop(p);

    let f = task_page_fixtures(DM, true);
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(DM), (1440.0, 1400.0), false);
    assert!(p.has(DM_TEXT), "the owner reads their own DM");
    assert!(
        p.has("Rahul Mehta") && p.has_part("Direct message \u{00B7} "),
        "who, then where"
    );
}

#[test]
fn category_is_picked_from_its_chip() {
    let f = task_page_fixtures(BUG, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::MyTasks, Some(BUG), (1440.0, 1400.0), false);
    p.harness.get_by_label("Category: Bug").click();
    p.steps(3);
    assert!(p.has("Question"));
    p.harness.get_by_label("Feature").click();
    p.steps(2);
    p.seed("task:details", json!({"id": BUG, "category": "feature"}));
    p.steps(3);
    assert!(p.has("Saved."));
}

#[test]
fn accept_from_the_task_page() {
    let f = task_page_fixtures(BUG, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::MyTasks, Some(BUG), (1440.0, 1400.0), false);
    let accepts = p
        .harness
        .get_all_by(|n| {
            n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some("Accept")
        })
        .count();
    assert_eq!(accepts, 1, "one Accept");
    // Accept into another project is in the page's ⋯ menu.
    p.button("More actions");
    assert!(p.has("Accept into"));
    p.harness.key_press(egui::Key::Escape);
    p.steps(2);
    p.button("Accept");
    p.seed("tasks:action", json!({"id": BUG, "status": "open"}));
    p.steps(3);
    assert!(p.has("Accepted \u{2014} it is an open task now."));
}

#[test]
fn connect_offers_intake_as_a_role() {
    let f = base(me(ME));
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Agents, None, (1440.0, 1300.0), false);
    p.seed("agents:mine", agents(true));
    p.steps(2);
    p.button("Connect an agent");
    let label = "Creates tasks for me";
    let toggled = |p: &Page<'_>| p.harness.get_by_label(label).accesskit_node().toggled();
    assert_eq!(
        toggled(&p),
        Some(egui::accesskit::Toggled::False),
        "off unless asked for"
    );
    p.harness.get_by_label(label).click();
    p.steps(2);
    assert_eq!(toggled(&p), Some(egui::accesskit::Toggled::True));
}

#[test]
fn an_intake_agent_card_shows_what_it_filed() {
    let f = base(me(ME));
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Agents, None, (1440.0, 1300.0), false);
    p.seed("agents:mine", agents(true));
    p.steps(3);
    assert!(p.has("Files tasks"));
    assert!(p.has("in triage") && p.has("accepted") && p.has("dismissed"));
    assert!(p.has("12") && p.has("4"));
    // Its filings are standalone and labelled, not a project to open.
    assert!(p.has("Files into your Triage, each task labelled Slack."));
}

#[test]
fn home_lists_triage_as_needing_attention() {
    let f = home_fixtures();
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::Home, None, (1440.0, 1300.0), false);
    assert!(p.has("Needs attention"));
    assert!(p.has("Slack Agent filed a bug \u{00B7} accept or dismiss"));
    assert!(p.has("Slack Agent filed a feature request \u{00B7} accept or dismiss"));
    p.harness
        .get_all_by_label(BUG_TITLE)
        .next()
        .unwrap()
        .click();
    p.steps(2);
    assert_eq!(p.open().1.as_deref(), Some(BUG));
}

// -------------------------------------------------------------------- renders

const WIDTHS: [(f32, &str); 2] = [(1440.0, "1440"), (820.0, "820")];
const DIR: &str = "docs/design-mocks/render/intake";

const TRIAGE_DIR: &str = "docs/design-mocks/render/triage";

fn save(p: &mut Page<'_>, name: &str, label: &str) {
    save_in(p, DIR, name, label);
}

fn save_in(p: &mut Page<'_>, dir: &str, name: &str, label: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let path = format!("{dir}/{name}-{label}.png");
    p.harness
        .render()
        .expect("render")
        .save(&path)
        .expect("write png");
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs: cargo test --features app --test intake_ui -- --ignored"]
fn renders() {
    for (width, label) in WIDTHS {
        let f = base(me(ME));
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Agents, None, (width, 1100.0), true);
        p.seed("agents:mine", agents(true));
        p.steps(4);
        save(&mut p, "agent-card", label);
        p.button("Connect an agent");
        p.harness.get_by_label("Creates tasks for me").click();
        p.steps(20);
        save(&mut p, "connect-intake", label);
        // The page's dialog state is per thread, as the one window's is.
        p.button("Cancel");
        drop(p);

        let f = my_tasks(true, true);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::MyTasks, None, (width, 1000.0), true);
        save_in(&mut p, TRIAGE_DIR, "my-tasks-top", label);
        drop(p);

        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Triage, None, (width, 1000.0), true);
        save_in(&mut p, TRIAGE_DIR, "triage-tab", label);
        hover_row(&mut p, BUG_TITLE);
        p.steps(20);
        save_in(&mut p, TRIAGE_DIR, "triage-tab-hovered", label);
        row_node(&p, IDEA_TITLE).click_secondary();
        p.steps(3);
        p.harness.get_by_label("Accept into").hover();
        p.steps(6);
        save_in(&mut p, TRIAGE_DIR, "triage-row-menu", label);
        drop(p);

        let mut empty = my_tasks(false, true);
        empty[1] = (
            "sidebar:counts",
            json!({"myOpen": 2, "activeProjects": 3, "triage": 0}),
        );
        let app = RefCell::new(None);
        let mut p = page(&app, &empty, Tab::Triage, None, (width, 1000.0), true);
        save_in(&mut p, TRIAGE_DIR, "triage-empty", label);
        drop(p);

        let f = task_page_fixtures(BUG, true);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::MyTasks, Some(BUG), (width, 1300.0), true);
        save(&mut p, "task-source-owner", label);
        save_in(&mut p, TRIAGE_DIR, "task-page-header", label);
        p.button("More actions");
        p.steps(4);
        save_in(&mut p, TRIAGE_DIR, "task-page-more", label);
        drop(p);

        let f = task_page_fixtures(DM, false);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::MyTasks, Some(DM), (width, 1300.0), true);
        save(&mut p, "task-source-dm-teammate", label);
        drop(p);

        let f = task_page_fixtures(DM, true);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::MyTasks, Some(DM), (width, 1300.0), true);
        save(&mut p, "task-source-dm-owner", label);
        drop(p);

        let f = home_fixtures();
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Home, None, (width, 1300.0), true);
        save(&mut p, "home-triage", label);
    }
}

#[test]
#[ignore = "width sweep of the task page: cargo test --features app --test intake_ui task_page_width_sweep -- --ignored"]
fn task_page_width_sweep() {
    let mut f = task_page_fixtures(BUG, true);
    for (k, v) in f.iter_mut() {
        if *k == "task:one" {
            v["title"] =
                json!("Add Frontend to the Mock Interview skill options for every learner track");
            v["labels"] = json!([{"id": "l1", "name": "Slack", "colour": "purple"}]);
            v["source"]["reason"] = json!("A specific additional Mock Interview skill option was requested from the owner in the issues channel, which needs a product decision before it can be built.");
            v["projectId"] = json!("p1");
            v["projectName"] = json!("Give multiselect feature for the learner skill options picker");
        }
    }
    for (k, v) in f.iter_mut() {
        if *k == "task:artifacts" {
            *v = json!([
                {"id": "r1", "kind": "commit", "url": "https://github.com/airtribe-live/mycohort-api/commit/6f54d7dd1e2a9b", "title": "", "addedBy": {"name": "Anmol Srivastava"}, "canRemove": true},
                {"id": "r2", "kind": "pr", "url": "https://github.com/airtribe-live/mycohort-api/pull/4821", "title": "Mock interview: frontend skill option", "addedBy": {"name": "Anmol Srivastava"}, "addedByAgent": "Airtribe Agent", "canRemove": false},
            ]);
        }
    }
    // A long project name: the rail's Project picker must truncate.
    let long = json!("Give multiselect feature for the learner skill options picker");
    for (_, v) in f.iter_mut() {
        if let Some(rows) = v.as_array_mut() {
            for r in rows.iter_mut().filter(|r| r["id"] == "p1" && r.get("name").is_some()) {
                r["name"] = long.clone();
            }
        }
    }
    for w in [880.0, 960.0, 1040.0, 1120.0, 1200.0, 1400.0] {
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::Home, Some(BUG), (w, 900.0), true);
        let path = format!("/tmp/sweep-{}.png", w as i32);
        p.harness
            .render()
            .expect("render")
            .save(&path)
            .expect("png");
    }
}
