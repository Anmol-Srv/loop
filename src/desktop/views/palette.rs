//! The command palette (⌘K). Stub: filled in by the shell pass.

#[derive(Default)]
pub struct State {
    /// Whether the palette is showing.
    pub open: bool,
}

pub fn ui(_app: &mut crate::desktop::App, _ui: &mut egui::Ui) {}
