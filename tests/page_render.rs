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

const SHOTS: &[Shot] = &[
    Shot { name: "home", tab: Tab::Home, project: None, task: None, signed_in: true },
    Shot { name: "mytasks", tab: Tab::MyTasks, project: None, task: None, signed_in: true },
    Shot { name: "projects", tab: Tab::Projects, project: None, task: None, signed_in: true },
    Shot { name: "project", tab: Tab::Projects, project: Some(LEAD_RATING), task: None, signed_in: true },
    Shot { name: "project-overdue", tab: Tab::Projects, project: Some(CHECKOUT), task: None, signed_in: true },
    Shot { name: "task", tab: Tab::Home, project: None, task: Some(TASK_IN_PROGRESS), signed_in: true },
    Shot { name: "task-shipped", tab: Tab::Home, project: None, task: Some(TASK_SHIPPED), signed_in: true },
    Shot { name: "task-handoff", tab: Tab::Home, project: None, task: Some(TASK_HANDOFF), signed_in: true },
    Shot { name: "login", tab: Tab::Home, project: None, task: None, signed_in: false },
    Shot { name: "palette", tab: Tab::Home, project: None, task: None, signed_in: true },
    Shot { name: "login-error", tab: Tab::Home, project: None, task: None, signed_in: false },
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
        _ => {}
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
        .with_size(egui::vec2(width, HEIGHT))
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

    let path = format!("docs/design-mocks/render/pages/{}-{label}.png", shot.name);
    harness.render().expect("render").save(&path).expect("write png");
    eprintln!("wrote {path}");
}
