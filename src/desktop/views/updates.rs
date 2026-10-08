//! Keeping this Mac's Loop setup current: the app itself, the private
//! workspace's server, and the Claude Code skills installed from Agents.
//!
//! The team server names its latest release in a small uncached file
//! (`/download/latest`, written by `scripts/release-mac.sh`). Loop reads it in
//! the background about once an hour and compares it with its own stamped
//! version; a newer one lights a dot on the account button and the Updates
//! entry in Settings. Updating runs the very same `install.sh` a teammate
//! pastes by hand, detached, so it survives Loop quitting to be replaced.

use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::desktop::creds;
use crate::desktop::design::{cards as c, colour, space, text, widgets as w};

use super::chrome::VERSION;

/// How often the latest release is asked for.
const EVERY: Duration = Duration::from_secs(60 * 60);

struct Check {
    /// The latest release the team server names, once known.
    latest: Option<String>,
    /// When it was last asked, so a frame never starts a second check.
    asked: Option<Instant>,
    /// A check is out.
    running: bool,
}

static STATE: Mutex<Check> = Mutex::new(Check { latest: None, asked: None, running: false });

/// The update in flight: the installer process, so the section can tell a
/// failure from a run still going. Loop is quit and reopened by the
/// installer itself when it succeeds, so success is never seen here.
static INSTALLING: Mutex<Option<std::process::Child>> = Mutex::new(None);

const UPDATE_LOG: &str = "update.log";

fn log_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Logs/Loop").join(name))
}

/// Where the installer is, read from its own output: (words, fraction done).
/// curl's progress bar writes "#### 42.0%" with carriage returns, so the last
/// percentage in the log is how far the download is.
fn install_phase() -> (String, Option<f32>) {
    let raw = log_path(UPDATE_LOG).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    if raw.contains("installed. Opening") {
        return ("Installed \u{2014} restarting Loop\u{2026}".into(), Some(1.0));
    }
    let pct = raw
        .split(|c: char| c == '\r' || c == '\n' || c == ' ')
        .filter_map(|w| w.strip_suffix('%'))
        .filter_map(|n| n.parse::<f32>().ok())
        .next_back();
    match pct {
        Some(p) if p >= 100.0 => ("Installing\u{2026}".into(), Some(1.0)),
        Some(p) => (format!("Downloading\u{2026} {p:.0}%"), Some(p / 100.0)),
        None => ("Starting the update\u{2026}".into(), None),
    }
}

/// The installer's state: still running, failed (with its last lines), or
/// not started.
enum Install {
    Idle,
    Running,
    Failed(String),
}

fn install_state() -> Install {
    let mut slot = INSTALLING.lock().unwrap();
    let Some(child) = slot.as_mut() else { return Install::Idle };
    match child.try_wait() {
        Ok(None) => Install::Running,
        Ok(Some(status)) if status.success() => Install::Running, // Loop is about to be quit
        _ => {
            *slot = None;
            let raw = log_path(UPDATE_LOG).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
            let tail: Vec<&str> = raw.lines().rev().filter(|l| !l.trim().is_empty() && !l.contains('#')).take(2).collect();
            Install::Failed(tail.into_iter().rev().collect::<Vec<_>>().join(" "))
        }
    }
}

fn start_install(ctx: &egui::Context, team: &str) {
    let script = format!("curl -fsSL {team}/install.sh | bash");
    match run_detached(&script, UPDATE_LOG) {
        Ok(child) => {
            *INSTALLING.lock().unwrap() = Some(child);
            ctx.request_repaint();
        }
        Err(e) => w::toast(ctx, format!("Couldn\u{2019}t start the update: {e}"), true),
    }
}

/// The team server: the first non-private workspace, else whatever this Mac
/// talks to. Releases are published there.
fn team_server() -> String {
    creds::workspaces()
        .into_iter()
        .find(|w| !w.private)
        .map(|w| w.server)
        .unwrap_or_else(creds::base_url)
}

/// Ask for the latest release when the last answer is an hour old. Cheap to
/// call every frame. `force` is the "Check now" button.
pub fn check(ctx: &egui::Context, force: bool) {
    {
        let mut s = STATE.lock().unwrap();
        let due = s.asked.is_none_or(|t| t.elapsed() >= EVERY);
        if s.running || !(due || force) {
            return;
        }
        s.running = true;
        s.asked = Some(Instant::now());
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let url = format!("{}/download/latest", team_server());
        let got = Command::new("curl")
            .args(["-fsS", "-m", "15", &url])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .filter(|v| !v.is_empty() && v.len() < 64);
        let mut s = STATE.lock().unwrap();
        s.running = false;
        // A failed check keeps the last answer rather than forgetting it.
        if got.is_some() {
            s.latest = got;
        }
        drop(s);
        ctx.request_repaint();
    });
}

/// The newer release this build could update to, if any. A dev build never
/// asks: it is not on the release line.
pub fn available() -> Option<String> {
    if VERSION == "dev" {
        return None;
    }
    // Release versions are zero-padded dates (2026.10.07-1358), so they sort
    // as strings.
    STATE.lock().unwrap().latest.clone().filter(|l| l.as_str() > VERSION)
}

/// Run a shell line detached from Loop — its own process group, output to
/// a log — so it keeps going after Loop quits.
fn run_detached(script: &str, log: &str) -> Result<std::process::Child, String> {
    use std::os::unix::process::CommandExt;
    let home = std::env::var("HOME").map_err(|_| "no home folder".to_owned())?;
    let dir = std::path::PathBuf::from(home).join("Library/Logs/Loop");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = std::fs::File::create(dir.join(log)).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    Command::new("/bin/bash")
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())
}

/// Settings > Updates.
pub(super) fn section(ui: &mut egui::Ui, email: &str) {
    let team = team_server();
    let (latest, running) = {
        let s = STATE.lock().unwrap();
        (s.latest.clone(), s.running)
    };
    let newer = available();

    let install = install_state();
    super::settings::group(ui, "Loop", |ui| {
        // An update in flight: what it is doing, live, until the installer
        // quits Loop and opens the new one.
        if let Install::Running = install {
            let (words, fraction) = install_phase();
            super::settings::row(ui, &format!("Updating Loop {VERSION}"), &words, false, |ui| {
                crate::desktop::design::agent::spinner(ui, text::BODY);
            });
            if let Some(f) = fraction {
                ui.add_space(space::XS);
                w::progress(ui, f, ui.available_width(), colour::ACCENT());
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        if let Install::Failed(why) = &install {
            ui.add_space(space::XS);
            w::error(ui, &format!("The update didn\u{2019}t finish: {}", if why.is_empty() { "see ~/Library/Logs/Loop/update.log" } else { why }));
            ui.add_space(space::SM);
        }
        let detail = match (&latest, VERSION) {
            (Some(l), "dev") => format!("A development build. Loop {l} is the latest release; installing it puts this Mac back on releases (and on Updates)."),
            (None, "dev") => "A development build \u{2014} it isn\u{2019}t on the release line.".to_owned(),
            (Some(l), _) if newer.is_some() => format!("Loop {l} is out. Updating quits Loop, installs it and opens it again; your sign-in and settings stay."),
            (Some(_), _) => "This is the latest release.".to_owned(),
            (None, _) if running => "Checking for a newer release\u{2026}".to_owned(),
            (None, _) => format!("Couldn\u{2019}t reach {team} to check."),
        };
        super::settings::row(ui, &format!("Loop {VERSION}"), &detail, false, |ui| {
            let offer = if VERSION == "dev" {
                latest.is_some().then_some("Install latest release")
            } else {
                newer.is_some().then_some("Update Loop")
            };
            if let Some(label) = offer {
                if w::primary(ui, label, true).clicked() {
                    start_install(ui.ctx(), &team);
                }
            } else if VERSION != "dev" {
                if latest.is_some() {
                    c::chip(ui, "Up to date", c::Tone::Ok, true);
                }
                if w::ghost(ui, if running { "Checking\u{2026}" } else { "Check now" }).clicked() && !running {
                    check(ui.ctx(), true);
                }
            }
        });
    });

    ui.add_space(space::LG);
    super::settings::group(ui, "This Mac\u{2019}s setup", |ui| {
        let private = creds::workspaces().into_iter().any(|w| w.private);
        if private {
            super::settings::row(
                ui,
                "Private workspace server",
                "Rebuilds its server from the latest release and restarts it. Its tasks and projects stay.",
                false,
                |ui| {
                    if w::secondary(ui, "Update", true).clicked() {
                        let script = format!("curl -fsSL {team}/private-workspace.sh | bash -s -- '{email}'");
                        match run_detached(&script, "private-update.log") {
                            Ok(_) => w::toast(ui.ctx(), "Updating the private server \u{2014} about a minute.", false),
                            Err(e) => w::toast(ui.ctx(), format!("Couldn\u{2019}t start it: {e}"), true),
                        }
                    }
                },
            );
        }
        let stale = super::agents::stale_skills();
        let detail = if stale.is_empty() {
            "The skills you installed from Agents match this version of Loop.".to_owned()
        } else {
            format!("Out of date: {}. Updating rewrites them in ~/.claude/skills.", stale.join(", "))
        };
        super::settings::row(ui, "Claude Code skills", &detail, private, |ui| {
            if !stale.is_empty() && w::secondary(ui, "Update", true).clicked() {
                match super::agents::reinstall_skills(&stale) {
                    Ok(()) => w::toast(ui.ctx(), "Skills updated.", false),
                    Err(e) => w::toast(ui.ctx(), format!("Couldn\u{2019}t update them: {e}"), true),
                }
            }
        });
    });
    ui.add_space(space::SM);
    w::caption(ui, "Connected agents fetch their own skill updates from the server on their next check.");
}
