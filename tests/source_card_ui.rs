//! The task page's Source card as a Slack message: mrkdwn rendered, the
//! images that came with it as thumbnails (click for the full image), other
//! files as chips, DM privacy; and label badges on task rows and the rail.
//!
//! `renders` draws each surface at 1440 and 820 into
//! docs/design-mocks/render/slack-source/:
//!   cargo test --features app --test source_card_ui -- --ignored --nocapture
#![cfg(feature = "app")]

use std::cell::RefCell;
use std::io::Cursor;

use acp_server::desktop::design::theme;
use acp_server::desktop::net::Net;
use acp_server::desktop::{views, App, Tab};
use chrono::Utc;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value};

const ME: &str = "aaaaaaaa-0000-0000-0000-000000000001";
const PRIYA: &str = "aaaaaaaa-0000-0000-0000-000000000002";
const BUG: &str = "33333333-0000-0000-0000-000000000001";
const DM: &str = "33333333-0000-0000-0000-000000000002";
const SHOT_1: &str = "44444444-0000-0000-0000-000000000001";
const SHOT_2: &str = "44444444-0000-0000-0000-000000000002";
const PDF: &str = "44444444-0000-0000-0000-000000000003";

const MESSAGE: &str = "*Checkout is failing* for anyone paying with a saved card \u{2014} it spins, then says `payment method invalid`.\n\
Started this morning, see <https://status.airtribe.live/incidents/42|the incident> and ask <@U04PRIYA>.\n\
\u{2022} new cards go through\n\
\u{2022} _only_ saved cards fail\n\
```\nPOST /api/checkout/confirm 422\n{\"error\":\"payment_method_invalid\"}\n```\n\
cc <#C05PAY|payments> ~maybe a Stripe thing~";

fn ago(minutes: i64) -> String {
    (Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339()
}

fn me(person: &str) -> Value {
    let (name, email) = if person == ME { ("Anmol Srivastava", "anmol@airtribe.live") } else { ("Priya Nair", "priya@airtribe.live") };
    json!({"personId": person, "email": email, "name": name, "role": "member", "scopes": ["read", "write"]})
}

fn slack_label() -> Value {
    json!({"id": "l-slack", "name": "Slack", "colour": "purple"})
}

fn files() -> Value {
    json!([
        {"id": SHOT_1, "name": "checkout-error.png", "mime": "image/png", "size": 212_000, "width": 1280, "height": 800},
        {"id": SHOT_2, "name": "network-tab.png", "mime": "image/png", "size": 96_000, "width": 900, "height": 1200},
        {"id": PDF, "name": "stripe-receipt.pdf", "mime": "application/pdf", "size": 48_000, "width": null, "height": null},
    ])
}

fn task(id: &str, owner_view: bool) -> Value {
    let (src, title) = if id == DM {
        let (text, author, files) = if owner_view {
            (json!("Could we get a CSV of the roster? I copy it by hand every Monday."), json!("Rahul Mehta"), files())
        } else {
            (json!("From a direct message"), Value::Null, json!([]))
        };
        (json!({"kind": "slack", "url": "https://airtribe.slack.com/archives/D04/p1", "channel": "D04AB12CD", "private": true,
                "author": author, "text": text, "receivedAt": ago(95), "agentName": "Slack Agent",
                "reason": "a feature ask sent to you directly", "confidence": 0.71, "files": files}),
         "Export cohort roster as CSV")
    } else {
        (json!({"kind": "slack", "url": "https://airtribe.slack.com/archives/C04/p2", "channel": "C04AB12CD",
                "channelName": "issues-and-feedback", "author": "Priya Nair", "text": MESSAGE, "receivedAt": ago(12),
                "agentName": "Slack Agent", "reason": "looks like a bug: saved-card checkout fails", "confidence": 0.86,
                "files": files()}),
         "Checkout fails for saved cards")
    };
    json!({
        "id": id, "title": title, "status": "triage", "category": "bug", "discipline": "backend", "priority": 1,
        "body": "Saved-card payments fail at the confirm step.", "projectId": null, "projectName": null,
        "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "assigneeEmail": "anmol@airtribe.live",
        "createdAt": ago(12), "updatedAt": ago(12), "doneAt": null, "delegate": null, "source": src,
        "labels": [slack_label()], "canArchive": true, "canDelete": true,
    })
}

/// A real PNG: a soft two-tone screenshot stand-in.
fn png(w: u32, h: u32, tint: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let band = if y < h / 8 { 40 } else if (x / 40 + y / 40) % 2 == 0 { 30 } else { 24 };
        image::Rgb([band + tint[0] / 6, band + tint[1] / 6, band + tint[2] / 6])
    });
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

type Fixtures = Vec<(&'static str, Value)>;

fn base(viewer: Value) -> Fixtures {
    vec![
        ("__me", viewer),
        ("sidebar:counts", json!({"myOpen": 2, "activeProjects": 3, "triage": 1})),
        ("__tracks", json!({})),
        ("board:people", json!([])),
        ("board:projects", json!([{"id": "p1", "name": "Lead Rating", "status": "active"}])),
        ("projects:labels", json!([slack_label(), {"id": "l-bug", "name": "bug", "colour": "red"},
                                   {"id": "l-infra", "name": "infra", "colour": "blue"}])),
    ]
}

fn task_page(id: &str, owner: bool) -> Fixtures {
    let mut f = base(me(if owner { ME } else { PRIYA }));
    f.push(("agents:mine", json!([])));
    f.push(("task:one", task(id, owner)));
    f.push(("task:notes", json!([])));
    f.push(("task:artifacts", json!([])));
    f
}

fn rows() -> Vec<Value> {
    let mut t = task(BUG, true);
    t["status"] = json!("open");
    vec![
        t,
        json!({"id": "t1", "title": "Rate leads by role bucket", "status": "in_progress", "priority": 1, "discipline": "backend",
               "body": "Expose the bucket on the lead payload.", "projectId": "p1", "projectName": "Lead Rating",
               "assigneePersonId": ME, "assigneeName": "Anmol Srivastava", "createdAt": ago(4000), "updatedAt": ago(10), "doneAt": null,
               "labels": [{"id": "l-bug", "name": "bug", "colour": "red"}, {"id": "l-infra", "name": "infra", "colour": "blue"},
                          {"id": "l-q4", "name": "Q4", "colour": "amber"}]}),
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
    fn has(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }
    fn has_part(&self, label: &str) -> bool {
        self.harness.query_all_by_label_contains(label).next().is_some()
    }
    fn button(&mut self, label: &str) {
        self.harness
            .get_all_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(label))
            .next()
            .unwrap_or_else(|| panic!("no button {label}"))
            .click();
        self.steps(3);
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

fn page<'a>(app: &'a RefCell<Option<App>>, fixtures: &'a Fixtures, tab: Tab, task: Option<&'a str>, size: (f32, f32), gpu: bool) -> Page<'a> {
    let builder = Harness::builder().with_size(egui::vec2(size.0, size.1)).with_pixels_per_point(2.0);
    let builder = if gpu { builder.wgpu() } else { builder };
    let mut harness = builder.build_ui(move |ui| {
        let mut slot = app.borrow_mut();
        if slot.is_none() {
            theme::install(ui.ctx());
            ui.ctx().all_styles_mut(|s| s.animation_time = egui::Style::default().animation_time);
            ui.ctx().set_zoom_factor(1.15);
            let mut net = Net::spawn(silent_server().into(), "test".into(), ui.ctx().clone());
            net.seed_bytes(&format!("file:{SHOT_1}"), png(1280, 800, [120, 160, 240]));
            net.seed_bytes(&format!("file:{SHOT_2}"), png(900, 1200, [180, 150, 240]));
            *slot = Some(App {
                net: Some(net),
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
fn the_message_is_rendered_from_mrkdwn() {
    let f = task_page(BUG, true);
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(BUG), (1440.0, 1600.0), false);
    // One label per paragraph, its accessible name the text without markup.
    assert!(p.has(
        "Checkout is failing for anyone paying with a saved card \u{2014} it spins, then says payment method invalid.\n\
         Started this morning, see the incident and ask @someone."
    ), "bold, code, a labelled link and an unresolved mention");
    assert!(p.has("new cards go through") && p.has("only saved cards fail"), "bullets");
    assert!(p.has("POST /api/checkout/confirm 422\n{\"error\":\"payment_method_invalid\"}"), "the fence, verbatim");
    assert!(p.has("cc #payments maybe a Stripe thing"));
    assert!(!p.has_part("<@U04PRIYA>") && !p.has_part("*Checkout"), "no raw markup");
    // Who, where, when.
    assert!(p.has("Priya Nair"));
    assert!(p.has_part("#issues-and-feedback \u{00B7} 12 minutes ago"));
    assert!(p.has("Open in Slack"));
}

#[test]
fn images_are_thumbnails_that_open_full_size() {
    let f = task_page(BUG, true);
    let app = RefCell::new(None);
    let mut p = page(&app, &f, Tab::MyTasks, Some(BUG), (1440.0, 1600.0), false);
    assert!(p.has("Open image checkout-error.png") && p.has("Open image network-tab.png"));
    assert!(p.has("Open stripe-receipt.pdf"), "a PDF is a file chip, not a thumbnail");

    p.button("Open image checkout-error.png");
    assert!(p.has("checkout-error.png"), "the lightbox names the image");
    assert!(p.harness.query_all_by(|n| n.role() == egui::accesskit::Role::Image && n.label().as_deref() == Some("checkout-error.png")).next().is_some());
    p.button("Close");
    assert!(!p.has("Close"), "closed");
    let open = p.app.borrow().as_ref().unwrap().task.clone();
    assert_eq!(open.as_deref(), Some(BUG), "still on the task");
}

#[test]
fn a_teammate_sees_no_words_and_no_files_from_a_dm() {
    let f = task_page(DM, false);
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(DM), (1440.0, 1400.0), false);
    assert!(p.has("From a direct message \u{2014} only Anmol can read it."));
    assert!(!p.has("From a direct message"), "the server's stand-in is not quoted as if it were the message");
    assert!(!p.has_part("Open image") && !p.has_part("Open stripe"));
    assert!(!p.has("Open in Slack"));
    drop(p);

    let f = task_page(DM, true);
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(DM), (1440.0, 1400.0), false);
    assert!(p.has("Rahul Mehta"));
    assert!(p.has("Could we get a CSV of the roster? I copy it by hand every Monday."));
    assert!(p.has("Open image checkout-error.png"), "the owner sees their DM's files");
}

#[test]
fn rows_wear_label_badges_and_the_rail_edits_them() {
    let mut f = base(me(ME));
    f.push(("agents:mine", json!([])));
    f.push(("mytasks:mine", Value::Array(rows())));
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, None, (1440.0, 1000.0), false);
    assert!(p.has("Slack"), "the source's label on the filed task");
    assert!(p.has("bug") && p.has("infra") && p.has("+1"), "two badges, then +N");
    assert!(!p.has("Q4"));
    drop(p);

    let f = task_page(BUG, true);
    let app = RefCell::new(None);
    let p = page(&app, &f, Tab::MyTasks, Some(BUG), (1440.0, 1600.0), false);
    assert!(p.has("Remove Slack"), "a writer's rail: removable badges");
    assert!(p.has_part("Add labels"));
}

// -------------------------------------------------------------------- renders

const WIDTHS: [(f32, &str); 2] = [(1440.0, "1440"), (820.0, "820")];
const DIR: &str = "docs/design-mocks/render/slack-source";

fn save(p: &mut Page<'_>, name: &str, label: &str) {
    std::fs::create_dir_all(DIR).unwrap();
    let path = format!("{DIR}/{name}-{label}.png");
    p.harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs: cargo test --features app --test source_card_ui -- --ignored"]
fn renders() {
    for (width, label) in WIDTHS {
        let f = task_page(BUG, true);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::MyTasks, Some(BUG), (width, 1700.0), true);
        p.steps(4);
        save(&mut p, "task-source", label);
        p.button("Open image checkout-error.png");
        p.steps(20);
        save(&mut p, "lightbox", label);
        p.button("Close");
        drop(p);

        let f = task_page(DM, false);
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::MyTasks, Some(DM), (width, 1200.0), true);
        save(&mut p, "dm-teammate", label);
        drop(p);

        let mut f = base(me(ME));
        f.push(("agents:mine", json!([])));
        f.push(("mytasks:mine", Value::Array(rows())));
        let app = RefCell::new(None);
        let mut p = page(&app, &f, Tab::MyTasks, None, (width, 800.0), true);
        save(&mut p, "my-tasks-labels", label);
    }
}
