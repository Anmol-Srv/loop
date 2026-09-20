//! The front door.
//!
//! Two modes on one screen: signing in with email and password, and the
//! first-time path that spends a setup code and chooses a password. Both post
//! through a short-lived unauthenticated `Net` — there is no token yet, and
//! `/api/auth/login` and `/api/auth/setup` are the two routes that do not want
//! one.
//!
//! Every failure here is rendered **verbatim** as the server wrote it. The
//! wording is deliberately vague about whether an address exists at all, and a
//! friendlier paraphrase on this side would undo that.

use serde_json::json;

use crate::desktop::{creds, net::Net, theme, App};

const CARD_WIDTH: f32 = 320.0;

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum Mode {
    #[default]
    SignIn,
    FirstTime,
}

#[derive(Default)]
pub struct State {
    pub mode: Mode,
    pub email: String,
    pub password: String,
    pub confirm: String,
    pub code: String,
    pub error: Option<String>,
    /// Unauthenticated bridge, alive only for the request in flight.
    net: Option<Net>,
}

/// Codes are read off Slack and typed by hand, so accept any casing and any
/// grouping: `k7qf m2xt 9pdr` becomes `K7QF-M2XT-9PDR`, which is what was
/// hashed.
fn tidy_code(raw: &str) -> String {
    let body: String = raw
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();

    body.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();

    // Settle last frame's reply before drawing anything.
    let mut token = None;
    if let Some(net) = app.login.net.as_mut() {
        net.pump();
        match net.results.remove("auth") {
            Some(Ok(v)) => {
                token = Some(
                    v.get("token").and_then(|t| t.as_str()).unwrap_or_default().to_string(),
                );
            }
            Some(Err(e)) => {
                app.login.error = Some(e);
                app.login.net = None;
            }
            None => {}
        }
    }
    if let Some(token) = token {
        match creds::store(&token) {
            Ok(()) => {
                app.login = State::default();
                app.connect(token, &ctx);
                return;
            }
            Err(e) => {
                app.login.error = Some(e);
                app.login.net = None;
            }
        }
    }

    let busy = app.login.net.as_ref().is_some_and(|n| n.is_loading("auth"));
    let first_time = app.login.mode == Mode::FirstTime;
    let mut submit = false;

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::BG))
        .show(ui, |ui| {
            ui.add_space(((ui.available_height() - 420.0) * 0.35).max(24.0));

            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Airtribe Control Plane").size(19.0).strong());
                ui.add_space(3.0);
                ui.label(
                    egui::RichText::new(creds::base_url())
                        .monospace()
                        .size(11.0)
                        .color(theme::MUTED),
                );
                ui.add_space(22.0);

                egui::Frame::new()
                    .fill(theme::PANEL)
                    .stroke(egui::Stroke::new(1.0, theme::LINE))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::same(26))
                    .show(ui, |ui| {
                        ui.set_width(CARD_WIDTH);
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(if first_time {
                                    "Set your password"
                                } else {
                                    "Sign in"
                                })
                                .size(14.0)
                                .strong(),
                            );
                            ui.add_space(16.0);

                            let s = &mut app.login;
                            submit |= field(ui, "Email", &mut s.email, false, "you@airtribe.live");

                            if first_time {
                                ui.add_space(12.0);
                                submit |= field(ui, "Setup code", &mut s.code, false, "K7QF-M2XT-9PDR");
                                ui.add_space(12.0);
                                submit |= field(ui, "New password", &mut s.password, true, "at least 12 characters");
                                ui.add_space(12.0);
                                submit |= field(ui, "Confirm password", &mut s.confirm, true, "");
                                if !s.confirm.is_empty() && s.confirm != s.password {
                                    ui.add_space(5.0);
                                    ui.label(
                                        egui::RichText::new("passwords do not match")
                                            .size(11.0)
                                            .color(theme::MUTED),
                                    );
                                }
                            } else {
                                ui.add_space(12.0);
                                submit |= field(ui, "Password", &mut s.password, true, "");
                            }

                            ui.add_space(20.0);

                            let ready = !s.email.trim().is_empty()
                                && !s.password.is_empty()
                                && (!first_time
                                    || (!s.code.trim().is_empty() && s.confirm == s.password));

                            let label = if first_time { "Set password and sign in" } else { "Sign in" };
                            let button = egui::Button::new(label)
                                .min_size(egui::vec2(CARD_WIDTH, 28.0))
                                .fill(theme::ACCENT)
                                .stroke(egui::Stroke::NONE);
                            let clicked = ui
                                .add_enabled(ready && !busy, button)
                                .on_disabled_hover_text(if busy { "working" } else { "fill in every field" })
                                .clicked();
                            submit = (submit || clicked) && ready && !busy;

                            if busy {
                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    ui.add(egui::Spinner::new().size(13.0));
                                    ui.label(
                                        egui::RichText::new("Contacting the server")
                                            .size(11.0)
                                            .color(theme::MUTED),
                                    );
                                });
                            }

                            if let Some(err) = &s.error {
                                ui.add_space(12.0);
                                ui.label(egui::RichText::new(err).size(12.0).color(theme::DANGER));
                            }

                            ui.add_space(16.0);
                            let toggle = if first_time {
                                "Already have a password? Sign in"
                            } else {
                                "First time here?"
                            };
                            if ui.link(egui::RichText::new(toggle).size(12.0)).clicked() {
                                s.mode = if first_time { Mode::SignIn } else { Mode::FirstTime };
                                s.password.clear();
                                s.confirm.clear();
                                s.code.clear();
                                s.error = None;
                            }
                        });
                    });

                if first_time {
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new("Ask an admin for a setup code. They expire after 48 hours.")
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                }
            });
        });

    if submit {
        let s = &app.login;
        let (path, body) = match s.mode {
            Mode::SignIn => (
                "/api/auth/login",
                json!({ "email": s.email.trim(), "password": s.password }),
            ),
            Mode::FirstTime => (
                "/api/auth/setup",
                json!({
                    "email": s.email.trim(),
                    "code": tidy_code(&s.code),
                    "password": s.password,
                }),
            ),
        };

        // A fresh bridge per attempt: no token to carry, and no stale reply
        // from a previous one can land on this key.
        let mut net = Net::spawn(creds::base_url(), String::new(), ctx.clone());
        net.post("auth", path, body);
        app.login.net = Some(net);
        app.login.error = None;
    }
}

/// One labelled field. Returns true when Enter was pressed in it.
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, secret: bool, hint: &str) -> bool {
    ui.label(egui::RichText::new(label).size(11.0).color(theme::MUTED));
    ui.add_space(3.0);
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .password(secret)
            .desired_width(CARD_WIDTH)
            .margin(egui::Margin::symmetric(8, 5))
            .hint_text(hint),
    );
    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}
