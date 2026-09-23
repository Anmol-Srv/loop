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

use crate::desktop::design::{avatar, colour, size, space, text, widgets as w};
use crate::desktop::{creds, net::Net, App};

/// The measure of the card's contents. Not a spacing token: it is a line
/// length, chosen so an email address fits on one line without the card
/// sprawling across a wide window.
const CARD_WIDTH: f32 = 320.0;
/// Roughly how tall the composed card runs, used only to bias it above the
/// optical centre. An estimate, not a layout constraint.
const CARD_HEIGHT_GUESS: f32 = 420.0;

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
    /// Set by a submit that had something missing, so the form says what —
    /// including on a pristine form, where nothing is flagged until then.
    tried: bool,
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

/// What the form is still waiting on. `None` on a pristine form until a
/// submit is tried: nothing is missing before anyone has typed.
///
/// The button stays enabled and this line explains instead. A disabled
/// primary draws as bare grey text, which on a fresh form read as a label
/// rather than as the way in.
fn missing(s: &State, first_time: bool) -> Option<&'static str> {
    if !s.tried && s.email.is_empty() && s.password.is_empty() && s.code.is_empty() {
        return None;
    }
    if s.email.trim().is_empty() {
        return Some("Enter your email address.");
    }
    if first_time && s.code.trim().is_empty() {
        return Some("Enter the setup code an admin gave you.");
    }
    if s.password.is_empty() {
        return Some(if first_time { "Choose a password." } else { "Enter your password." });
    }
    if first_time && s.confirm != s.password {
        // The mismatch already has its own line under the field.
        return s.confirm.is_empty().then_some("Confirm the new password.");
    }
    None
}

/// Enter in a field submits the form, the way every other sign-in does.
fn entered(response: egui::Response) -> bool {
    response.lost_focus() && response.ctx.input(|i| i.key_pressed(egui::Key::Enter))
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
    // A face for the address once it looks like one: a quiet confirmation that
    // what was typed is what was meant.
    let face = app.login.email.contains('@').then(|| app.login.email.clone());
    let mut submit = false;

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(colour::CANVAS))
        .show(ui, |ui| {
            let slack = (ui.available_height() - CARD_HEIGHT_GUESS) * 0.35;
            ui.add_space(slack.max(space::XL));

            ui.vertical_centered(|ui| {
                w::title(ui, "Airtribe Control Plane");
                ui.add_space(space::XS);
                // No widget for a monospaced caption: the URL is a machine
                // string and must read as one, so localhost and production are
                // told apart at a glance.
                ui.label(
                    egui::RichText::new(creds::base_url())
                        .monospace()
                        .size(text::CAPTION)
                        .color(colour::TEXT_FAINT),
                );
                ui.add_space(space::XL);

                w::card(ui, |ui| {
                    ui.set_width(CARD_WIDTH);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            if let Some(seed) = &face {
                                avatar::small(ui, seed, size::CONTROL);
                                ui.add_space(space::SM);
                            }
                            w::heading(
                                ui,
                                if first_time { "Set your password" } else { "Sign in" },
                            );
                        });
                        ui.add_space(space::LG);

                        let s = &mut app.login;
                        submit |= entered(w::field(ui, "Email", &mut s.email, false, "you@airtribe.live"));

                        if first_time {
                            ui.add_space(space::MD);
                            submit |= entered(w::field(ui, "Setup code", &mut s.code, false, "K7QF-M2XT-9PDR"));
                            ui.add_space(space::MD);
                            submit |=
                                entered(w::field(ui, "New password", &mut s.password, true, "At least 12 characters\u{2026}"));
                            ui.add_space(space::MD);
                            submit |=
                                entered(w::field(ui, "Confirm password", &mut s.confirm, true, ""));
                            if !s.confirm.is_empty() && s.confirm != s.password {
                                ui.add_space(space::XS);
                                w::caption(ui, "Passwords do not match \u{2014} retype the confirmation.");
                            }
                        } else {
                            ui.add_space(space::MD);
                            submit |= entered(w::field(ui, "Password", &mut s.password, true, ""));
                        }

                        ui.add_space(space::XL);

                        let ready = !s.email.trim().is_empty()
                            && !s.password.is_empty()
                            && (!first_time
                                || (!s.code.trim().is_empty() && s.confirm == s.password));

                        let label =
                            if first_time { "Set password and sign in" } else { "Sign in" };
                        // Left, on the same edge as the fields, the error and the
                        // link: `w::primary` sizes to its label, and a centred
                        // label-width button floated off that edge.
                        let clicked = w::primary(ui, label, !busy)
                            .on_disabled_hover_text("Signing you in\u{2026}")
                            .clicked();
                        if (submit || clicked) && !ready {
                            s.tried = true;
                        }
                        submit = (submit || clicked) && ready && !busy;

                        if let Some(next) = (!busy).then(|| missing(s, first_time)).flatten() {
                            ui.add_space(space::XS);
                            w::caption(ui, next);
                        }

                        if busy {
                            ui.add_space(space::MD);
                            w::loading(ui, "Contacting the server");
                        }

                        if let Some(err) = &s.error {
                            ui.add_space(space::MD);
                            // Verbatim, always — only the first letter is raised,
                            // so it reads as a sentence. The next step is ours.
                            let mut chars = err.chars();
                            let err: String = chars
                                .next()
                                .map(|f| f.to_uppercase().chain(chars).collect())
                                .unwrap_or_default();
                            w::error(ui, &err);
                            // Only for a wrong password: a lockout or a spent
                            // code already says what to do in its own words.
                            if !first_time && err.contains("incorrect") {
                                ui.add_space(space::XS);
                                w::caption(ui, "Check both and try again. New here? Use a setup code below.");
                            }
                        }

                        ui.add_space(space::LG);
                        let toggle = if first_time {
                            "Already have a password? Sign in"
                        } else {
                            "First time here?"
                        };
                        if w::link(ui, toggle).clicked() {
                            s.mode = if first_time { Mode::SignIn } else { Mode::FirstTime };
                            s.password.clear();
                            s.confirm.clear();
                            s.code.clear();
                            s.error = None;
                            s.tried = false;
                        }
                    });
                });

                if first_time {
                    ui.add_space(space::LG);
                    w::caption(ui, "Ask an admin for a setup code. They expire after 48 hours.");
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
