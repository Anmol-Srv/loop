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

use crate::desktop::design::{avatar, colour, radius, size, space, widgets as w};
use crate::desktop::{creds, net::Net, App};

/// The measure of the card's contents. Not a spacing token: it is a line
/// length, chosen so an email address fits on one line without the card
/// sprawling across a wide window.
const CARD_WIDTH: f32 = 320.0;
/// Roughly how tall the composed card runs, used only to bias it above the
/// optical centre. An estimate, not a layout constraint.
const CARD_HEIGHT_GUESS: f32 = 480.0;

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum Mode {
    #[default]
    SignIn,
    FirstTime,
}

pub struct State {
    pub mode: Mode,
    /// The server to sign in to, prefilled with the one last used. Saved next
    /// to the credential once a sign-in succeeds.
    pub server: String,
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

impl Default for State {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            server: creds::base_url(),
            email: String::new(),
            password: String::new(),
            confirm: String::new(),
            code: String::new(),
            error: None,
            tried: false,
            net: None,
        }
    }
}

/// Only the scheme is checked here; whether anything answers is the
/// server's to say, and the request's error says it.
fn server_ok(server: &str) -> bool {
    let s = server.trim();
    s.starts_with("http://") || s.starts_with("https://")
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
    if !server_ok(&s.server) {
        return Some("Enter the server address, starting https://.");
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

/// The login backdrop: HeroBg, full-bleed behind the card. A 2048 px JPEG
/// copy of assets/HeroBg.png (the 4K original), since 2048 is the widest
/// texture egui can count on:
/// `sips -s format jpeg -s formatOptions 82 --resampleWidth 2048 assets/HeroBg.png --out assets/login/hero-bg.jpg`
/// How much of the canvas colour is laid over the backdrop. The card is glass
/// (a ~5% fill), so this is what keeps the form legible over HeroBg's glow.
const SCRIM_ALPHA: f32 = 0.6;
const HERO_BG: &[u8] = include_bytes!("../../../assets/login/hero-bg.jpg");

/// Decode the backdrop the first time it is shown, then reuse the handle.
///
/// Lookup and load are separate statements on purpose: `data_mut` holds the
/// context lock and `load_texture` takes it too (see `shell::mark_texture`).
fn hero_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("login:hero");
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return handle;
    }
    let rgb = image::load_from_memory(HERO_BG).expect("the backdrop is baked in").to_rgb8();
    let (w, h) = rgb.dimensions();
    let pixels = rgb.pixels().map(|p| egui::Color32::from_rgb(p[0], p[1], p[2])).collect();
    let handle = ctx.load_texture(
        "login:hero",
        egui::ColorImage { size: [w as usize, h as usize], pixels, source_size: egui::vec2(w as f32, h as f32) },
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

/// Cover-fit: the largest centred sub-rectangle of a `tex`-shaped image that
/// has `rect`'s aspect, as UVs. The image fills `rect`, unstretched, and the
/// overflow is cropped evenly off both sides.
fn cover_uv(tex: egui::Vec2, rect: egui::Rect) -> egui::Rect {
    let scale = (rect.width() / tex.x).max(rect.height() / tex.y);
    let seen = egui::vec2(rect.width() / (tex.x * scale), rect.height() / (tex.y * scale));
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), seen)
}

fn backdrop(ui: &egui::Ui, rect: egui::Rect) {
    let tex = hero_texture(ui.ctx());
    ui.painter().image(tex.id(), rect, cover_uv(tex.size_vec2(), rect), egui::Color32::WHITE);
    ui.painter().rect_filled(rect, 0.0, colour::CANVAS.gamma_multiply(SCRIM_ALPHA));
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
        match creds::store(&token).and_then(|()| creds::store_server(&app.login.server)) {
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
            backdrop(ui, ui.max_rect());
            let slack = (ui.available_height() - CARD_HEIGHT_GUESS) * 0.35;
            ui.add_space(slack.max(space::XL));

            ui.vertical_centered(|ui| {
                w::title(ui, "Loop");
                ui.add_space(space::XL);

                // The shared card is glass; over HeroBg the login card needs a
                // solid black backing so the form reads first.
                egui::Frame::new()
                    .fill(egui::Color32::BLACK)
                    .corner_radius(radius::MD)
                    .show(ui, |ui| {
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

                        ui.add_space(space::MD);
                        submit |= entered(w::field(ui, "Server", &mut s.server, false, "https://acp.airtribe.live"));

                        ui.add_space(space::XL);

                        let ready = server_ok(&s.server)
                            && !s.email.trim().is_empty()
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
        let server = s.server.trim().trim_end_matches('/').to_string();
        let mut net = Net::spawn(server, String::new(), ctx.clone());
        net.post("auth", path, body);
        app.login.net = Some(net);
        app.login.error = None;
    }
}

#[cfg(test)]
mod backdrop_tests {
    use super::*;

    #[test]
    fn cover_crops_without_stretching() {
        let tex = egui::vec2(2560.0, 1440.0);
        // Taller window than 16:9: full height, sides cropped evenly.
        let uv = cover_uv(tex, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(820.0, 1300.0)));
        assert!((uv.height() - 1.0).abs() < 1e-6 && uv.width() < 1.0);
        assert!((uv.center().x - 0.5).abs() < 1e-6);
        let px = egui::vec2(uv.width() * tex.x, uv.height() * tex.y);
        assert!((px.x / px.y - 820.0 / 1300.0).abs() < 1e-4);
        // Wider: full width, top and bottom cropped.
        let uv = cover_uv(tex, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(3000.0, 1000.0)));
        assert!((uv.width() - 1.0).abs() < 1e-6 && uv.height() < 1.0);
    }
}
