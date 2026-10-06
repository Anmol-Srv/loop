//! Whole pages, rendered offscreen from the real app against a real server.
//!
//! `tests/ui_render.rs` draws one table with hand-made rows; this draws what a
//! person sees — the shell, the fetches, the empty and loaded states — by
//! driving `App::frame` against a server seeded with
//! `tests/fixtures/render-seed.sql`. It is `#[ignore]`d because it needs that
//! server; `scripts/render-pages.sh` stands one up and runs it.
#![cfg(feature = "app")]

use std::cell::RefCell;
use std::time::{Duration, Instant};

use acp_server::desktop::design::theme;
use acp_server::desktop::net::Net;
use acp_server::desktop::{views, App, Tab};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

struct Shot {
    name: &'static str,
    tab: Tab,
    project: Option<&'static str>,
    task: Option<&'static str>,
    signed_in: bool,
}

const LEAD_RATING: &str = "11111111-0000-0000-0000-000000000001";
const CHECKOUT: &str = "11111111-0000-0000-0000-000000000002";
const TASK_IN_PROGRESS: &str = "22222222-0000-0000-0000-000000000001";
const TASK_SHIPPED: &str = "22222222-0000-0000-0000-000000000003";
const TASK_HANDOFF: &str = "22222222-0000-0000-0000-000000000005";
const TASK_AGENT: &str = "22222222-0000-0000-0000-000000000004";
const LONG_PROJECT: &str = "11111111-0000-0000-0000-000000000006";
const TASK_LONG: &str = "22222222-0000-0000-0000-000000000006";
const TASK_LONG_AGENT: &str = "22222222-0000-0000-0000-000000000007";

const SHOTS: &[Shot] = &[
    Shot { name: "home", tab: Tab::Home, project: None, task: None, signed_in: true },
    Shot { name: "mytasks", tab: Tab::MyTasks, project: None, task: None, signed_in: true },
    Shot { name: "mytasks-board", tab: Tab::MyTasks, project: None, task: None, signed_in: true },
    Shot { name: "settings", tab: Tab::Settings, project: None, task: None, signed_in: true },
    Shot { name: "account-menu", tab: Tab::MyTasks, project: None, task: None, signed_in: true },
    Shot { name: "projects", tab: Tab::Projects, project: None, task: None, signed_in: true },
    Shot { name: "project", tab: Tab::Projects, project: Some(LEAD_RATING), task: None, signed_in: true },
    Shot { name: "project-overdue", tab: Tab::Projects, project: Some(CHECKOUT), task: None, signed_in: true },
    Shot { name: "task", tab: Tab::Home, project: None, task: Some(TASK_IN_PROGRESS), signed_in: true },
    Shot { name: "task-shipped", tab: Tab::Home, project: None, task: Some(TASK_SHIPPED), signed_in: true },
    Shot { name: "task-agent", tab: Tab::Home, project: None, task: Some(TASK_AGENT), signed_in: true },
    Shot { name: "task-handoff", tab: Tab::Home, project: None, task: Some(TASK_HANDOFF), signed_in: true },
    Shot { name: "long-task", tab: Tab::Home, project: None, task: Some(TASK_LONG), signed_in: true },
    Shot { name: "long-agent", tab: Tab::Home, project: None, task: Some(TASK_LONG_AGENT), signed_in: true },
    Shot { name: "long-project", tab: Tab::Projects, project: Some(LONG_PROJECT), task: None, signed_in: true },
    Shot { name: "long-projects", tab: Tab::Projects, project: None, task: None, signed_in: true },
    Shot { name: "long-mytasks", tab: Tab::AllTasks, project: None, task: None, signed_in: true },
    Shot { name: "newtask", tab: Tab::MyTasks, project: None, task: None, signed_in: true },
    Shot { name: "newtask-held", tab: Tab::MyTasks, project: None, task: None, signed_in: true },
    Shot { name: "login", tab: Tab::Home, project: None, task: None, signed_in: false },
    Shot { name: "palette", tab: Tab::Home, project: None, task: None, signed_in: true },
    Shot { name: "login-error", tab: Tab::Home, project: None, task: None, signed_in: false },
    Shot { name: "create", tab: Tab::Projects, project: None, task: None, signed_in: true },
    Shot { name: "create-datepicker", tab: Tab::Projects, project: None, task: None, signed_in: true },
];

/// States a `Shot` has no field for, set on the app before its first frame.
fn stage(shot: &Shot, app: &mut App) {
    match shot.name {
        // A query typed in, so the grouped results show rather than the
        // empty-query page list.
        "palette" => {
            app.palette.open = true;
            app.palette.query = "cart".into();
        }
        // The server's own wording for a bad password.
        "login-error" => app.login.error = Some("email or password is incorrect".into()),
        // The create form half filled in: a title, and one date set so both
        // the picker's set and unset states are on screen at once.
        "create" | "create-datepicker" => {
            app.board.creating = Some(views::projects::Draft {
                title: "Lead rating v3".into(),
                start: chrono::NaiveDate::from_ymd_opt(2026, 9, 28),
                ..Default::default()
            });
        }
        _ => {}
    }
}

/// Clicks a `Shot` needs once the page has loaded: popups only exist after
/// one, and a render that starts every page closed never shows them.
fn interact(shot: &Shot, harness: &mut Harness<'_>) {
    let click = match shot.name {
        "mytasks-board" => harness.query_by_label("Board view"),
        "account-menu" => harness.query_by_label_contains("Account:"),
        "newtask" | "newtask-held" => harness.query_by_label("New task"),
        _ => None,
    };
    if let Some(node) = click {
        node.click();
        for _ in 0..6 {
            harness.step();
        }
    }
    if shot.name == "newtask-held" {
        harness
            .get_by(|n| n.placeholder().is_some_and(|h| h.starts_with("e.g. Fix")))
            .type_text("Checkout totals look wrong on saved cards");
        let shot = std::path::PathBuf::from("docs/design-mocks/render/pages/task-wide.png");
        harness.input_mut().dropped_files.push(std::sync::Arc::new(Dropped(shot)));
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(25));
            harness.step();
        }
    }
    if shot.name == "create-datepicker" {
        // The target is the only picker still reading "Not set".
        harness.get_by_label("Not set").click();
        for _ in 0..4 {
            harness.step();
        }
    }
}

#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Laptop, a narrower laptop window, and past the sidebar's collapse point.
const WIDTHS: [(f32, &str); 3] = [(1440.0, "wide"), (1100.0, "mid"), (820.0, "narrow")];
const HEIGHT: f32 = 1300.0;

#[test]
#[ignore = "needs a seeded server: run scripts/render-pages.sh"]
fn pages() {
    let url = std::env::var("ACP_RENDER_URL").expect("ACP_RENDER_URL");
    let token = std::env::var("ACP_RENDER_TOKEN").expect("ACP_RENDER_TOKEN");
    let only: Vec<String> = std::env::var("RENDER_SHOTS")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect();

    for shot in SHOTS {
        if !only.is_empty() && !only.iter().any(|o| shot.name.starts_with(o.as_str())) {
            continue;
        }
        for (width, label) in WIDTHS {
            render(shot, width, label, &url, &token);
        }
    }
}

fn render(shot: &Shot, width: f32, label: &str, url: &str, token: &str) {
    let app: RefCell<Option<App>> = RefCell::new(None);

    let mut harness = Harness::builder()
        // The agent's task runs long: drawn tall so its activity is all seen.
        .with_size(egui::vec2(width, if shot.name == "task-agent" { HEIGHT * 2.2 } else { HEIGHT }))
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui(|ui| {
            let mut slot = app.borrow_mut();
            // Fonts bind on the frame after they are set, so the first frame
            // installs the theme and draws nothing.
            if slot.is_none() {
                theme::install(ui.ctx());
                ui.ctx().set_zoom_factor(1.15);
                let net = shot
                    .signed_in
                    .then(|| Net::spawn(url.to_owned(), token.to_owned(), ui.ctx().clone()));
                *slot = Some(App {
                    net,
                    tab: shot.tab,
                    project: shot.project.map(str::to_owned),
                    task: shot.task.map(str::to_owned),
                    scopes: Vec::new(),
                    login: views::login::State::default(),
                    board: views::board::State::default(),
                    palette: views::palette::State::default(),
                    settings: views::settings::State::default(),
                });
                stage(shot, slot.as_mut().unwrap());
                return;
            }
            slot.as_mut().unwrap().frame(ui);
        });

    // Step until every request has landed and a few quiet frames have passed,
    // so what is drawn is the loaded page rather than its spinner.
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut quiet = 0;
    while quiet < 6 && Instant::now() < deadline {
        harness.step();
        std::thread::sleep(Duration::from_millis(25));
        let busy = app
            .borrow()
            .as_ref()
            .and_then(|a| a.net.as_ref())
            .map(|n| n.inflight.values().any(|b| *b))
            .unwrap_or(false);
        quiet = if busy { 0 } else { quiet + 1 };
    }

    interact(shot, &mut harness);

    // RENDER_TAG keeps a second palette's shots beside the first, not over them.
    let tag = std::env::var("RENDER_TAG").map(|t| format!("-{t}")).unwrap_or_default();
    let path = format!("docs/design-mocks/render/pages/{}-{label}{tag}.png", shot.name);
    harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}
