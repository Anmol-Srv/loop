//! Where the token lives on this machine.
//!
//! Shared by the CLI and the desktop app: one credential, one place. Two
//! stores would mean signing in twice and revoking twice.
//!
//! A 0600 file under Application Support, not the macOS Keychain. The Keychain
//! is the better home for a credential in principle, but it gates access on the
//! binary's code signature — and an ad-hoc-signed binary gets a fresh identity
//! on every rebuild. In practice that meant macOS prompting for the login
//! password on each launch and never actually persisting anything. This is what
//! `gh`, `aws` and `kubectl` do, and it survives a rebuild.
//!
//! Move back to the Keychain once there is a Developer ID signed build; the
//! interface here is the only thing that would change.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/airtribe-control-plane"))
}

fn file() -> Option<PathBuf> {
    Some(dir()?.join("credentials"))
}

pub fn load() -> Option<String> {
    // An env var wins, so a developer can point at a scratch server without
    // disturbing the stored credential.
    if let Ok(t) = std::env::var("ACP_TOKEN") {
        if !t.trim().is_empty() {
            return Some(t.trim().to_string());
        }
    }

    let raw = fs::read_to_string(file()?).ok()?;
    let token = raw.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

pub fn store(token: &str) -> Result<(), String> {
    write_private("credentials", token)
}

/// Write a file next to the credential, 0600 from the outset rather than
/// written then chmod-ed: between those two steps it would be world-readable.
fn write_private(name: &str, contents: &str) -> Result<(), String> {
    let dir = dir().ok_or("cannot find your home directory")?;
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    let path = dir.join(name);

    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }

    let mut f = opts
        .open(&path)
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    f.write_all(contents.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;

    Ok(())
}

/// A small preference beside the credential — the app's appearance setting.
pub fn load_pref(name: &str) -> Option<String> {
    fs::read_to_string(dir()?.join(name)).ok()
}

pub fn store_pref(name: &str, contents: &str) -> Result<(), String> {
    write_private(name, contents)
}

pub fn clear() -> Result<(), String> {
    let Some(path) = file() else { return Ok(()) };
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        // Signing out when nothing was stored is not an error.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("could not clear {}: {e}", path.display())),
    }
}

/// Where the credential lives, for telling the user.
pub fn location() -> String {
    file()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unknown>".into())
}

/// The server a fresh install talks to before anyone has signed in. Baked in
/// at build time: `scripts/bundle-mac.sh` sets `LOOP_DEFAULT_SERVER` to the
/// team's hosted server, so a teammate's first launch is already pointed at
/// it; a plain `cargo build` (a developer) still gets a local one.
const DEFAULT_SERVER: &str = match option_env!("LOOP_DEFAULT_SERVER") {
    Some(url) => url,
    None => "http://localhost:8080",
};

/// Which server to talk to: `ACP_URL`, then the one saved at sign-in, then
/// the built-in default. A double-clicked `.app` has no environment, so the saved value is
/// what points it at a hosted server; the env var still wins for a developer.
pub fn base_url() -> String {
    std::env::var("ACP_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .or_else(saved_server)
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_SERVER.into())
}

fn saved_server() -> Option<String> {
    let raw = fs::read_to_string(dir()?.join("server")).ok()?;
    Some(raw.trim().to_string()).filter(|u| !u.is_empty())
}

/// Remember the server the app signed in to. Kept when signing out: the
/// next sign-in is almost always to the same place.
pub fn store_server(url: &str) -> Result<(), String> {
    write_private("server", url.trim().trim_end_matches('/'))
}

// ------------------------------------------------------------------ workspaces
//
// A workspace is one server and your sign-in there: the team's hosted server,
// or a private one that only runs on this Mac. The active workspace's token and
// server stay in `credentials` and `server`, so the CLI and everything that
// reads them are unchanged; `workspaces.json` (0600, like the credential)
// remembers the rest, and switching copies one into the active slot.

/// One server you have signed in to.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Workspace {
    pub name: String,
    pub server: String,
    /// `None` after signing out: the workspace stays listed, ready to sign
    /// back in to.
    #[serde(default)]
    pub token: Option<String>,
    /// Runs on this Mac only; nothing in it reaches a shared server.
    #[serde(default)]
    pub private: bool,
}

const WORKSPACES: &str = "workspaces.json";

fn same_server(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/') == b.trim().trim_end_matches('/')
}

/// Every workspace, the active one included. On first use this is seeded from
/// the existing sign-in, so nobody signs in again to get a switcher.
pub fn workspaces() -> Vec<Workspace> {
    let saved = dir()
        .and_then(|d| fs::read_to_string(d.join(WORKSPACES)).ok())
        .and_then(|raw| serde_json::from_str::<Vec<Workspace>>(&raw).ok());
    if let Some(list) = saved {
        return list;
    }
    match (load(), saved_server()) {
        (Some(token), Some(server)) => {
            vec![Workspace { name: "Airtribe".into(), server, token: Some(token), private: false }]
        }
        _ => Vec::new(),
    }
}

/// When the workspace list last changed on disk, so a cached copy knows to
/// read it again.
pub fn workspaces_modified() -> Option<std::time::SystemTime> {
    fs::metadata(dir()?.join(WORKSPACES)).and_then(|m| m.modified()).ok()
}

pub fn save_workspaces(list: &[Workspace]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    write_private(WORKSPACES, &json)
}

/// Record a sign-in: the workspace for `server` gets this token (added if it
/// is new, named after its host until renamed).
pub fn remember(server: &str, token: &str) -> Result<(), String> {
    let mut list = workspaces();
    match list.iter_mut().find(|w| same_server(&w.server, server)) {
        Some(w) => w.token = Some(token.to_owned()),
        None => {
            let host = server.split("://").last().unwrap_or(server).split('/').next().unwrap_or(server);
            list.push(Workspace { name: host.to_owned(), server: server.trim_end_matches('/').to_owned(), token: Some(token.to_owned()), private: false });
        }
    }
    save_workspaces(&list)
}

/// Forget the token for `server`, keeping the workspace listed.
pub fn forget(server: &str) -> Result<(), String> {
    let mut list = workspaces();
    for w in list.iter_mut().filter(|w| same_server(&w.server, server)) {
        w.token = None;
    }
    save_workspaces(&list)
}

/// Make `w` the active workspace: its server and token go into the slots the
/// app and the CLI read.
pub fn activate(w: &Workspace) -> Result<(), String> {
    store_server(&w.server)?;
    match &w.token {
        Some(t) => store(t),
        None => clear(),
    }
}

/// The active workspace, by the server in the active slot.
pub fn active() -> Option<Workspace> {
    let server = base_url();
    workspaces().into_iter().find(|w| same_server(&w.server, &server))
}
