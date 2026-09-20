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
    let dir = dir().ok_or("cannot find your home directory")?;
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    let path = dir.join("credentials");

    // Create with 0600 from the outset rather than writing then chmod-ing:
    // between those two steps the token would be world-readable.
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
    f.write_all(token.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;

    Ok(())
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

pub fn base_url() -> String {
    std::env::var("ACP_URL").unwrap_or_else(|_| "http://localhost:8080".into())
}
