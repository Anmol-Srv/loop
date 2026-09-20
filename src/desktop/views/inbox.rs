use crate::desktop::App;

#[derive(Default)]
pub struct State;

// Filled in by P5.
pub fn ui(_app: &mut App, ui: &mut egui::Ui) {
    ui.label("inbox");
}
