//! DEPRECATED — superseded by `desktop::design`.
//!
//! Kept as a thin shim while the views are ported. Every name here forwards to
//! a token so there is only one definition of each colour, never two that
//! drift. Delete once no view imports it.

pub use crate::desktop::design::tokens::colour::{
    ACCENT, AGENT, CANVAS as BG, LINE, SURFACE as PANEL, TEXT, TEXT_MUTED as MUTED,
    BLOCKED as DANGER, DONE as OK, REVIEW as WARN,
};
pub use crate::desktop::design::tokens::status_colour as status;
pub use crate::desktop::design::widgets::{id as id_label, pill};

pub fn install(ctx: &egui::Context) {
    crate::desktop::design::theme::install(ctx);
}
