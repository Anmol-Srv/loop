//! Where the token lives on a Mac.
//!
//! The Keychain, not a dotfile: this is a credential that grants write access
//! to shared team state, and macOS already has the right place for those.

const SERVICE: &str = "airtribe-control-plane";
const ACCOUNT: &str = "acp-token";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("keychain unavailable: {e}"))
}

pub fn load() -> Option<String> {
    // An env var wins, so a developer can point the app at a scratch server
    // without disturbing the stored credential.
    if let Ok(t) = std::env::var("ACP_TOKEN") {
        if !t.trim().is_empty() {
            return Some(t);
        }
    }
    entry().ok()?.get_password().ok()
}

pub fn store(token: &str) -> Result<(), String> {
    entry()?
        .set_password(token)
        .map_err(|e| format!("could not save to the keychain: {e}"))
}

pub fn clear() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        // Signing out when nothing was stored is not an error.
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("could not clear the keychain: {e}")),
    }
}

pub fn base_url() -> String {
    std::env::var("ACP_URL").unwrap_or_else(|_| "http://localhost:8080".into())
}
