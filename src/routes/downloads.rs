//! The Mac app's installer and builds, served by the same server the app talks
//! to. GitHub's download hosts are blocked on some teammates' networks; this
//! server is reachable for anyone who can use Loop at all.
//!
//!   curl -fsSL <server>/install.sh | bash
//!
//! `scripts/release-mac.sh` uploads `install.sh` and `Loop.zip` into
//! `LOOP_DIST_DIR`. No auth: the build is useless without an account, and a
//! teammate installs before they have one.

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

use crate::db::AppState;

pub fn routes() -> Router<AppState> {
    let dir = std::env::var("LOOP_DIST_DIR").unwrap_or_else(|_| "/srv/loop-dist".into());
    Router::new()
        .route_service("/install.sh", ServeFile::new(format!("{dir}/install.sh")))
        .nest_service("/download", ServeDir::new(dir))
}
