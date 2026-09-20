//! A restrained light theme. Shared vocabulary so the views agree on colour.

use egui::{Color32, Stroke};

pub const BG: Color32 = Color32::from_rgb(0xFA, 0xFA, 0xF8);
pub const PANEL: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const LINE: Color32 = Color32::from_rgb(0xE3, 0xE1, 0xDC);
pub const TEXT: Color32 = Color32::from_rgb(0x1C, 0x1B, 0x19);
pub const MUTED: Color32 = Color32::from_rgb(0x79, 0x76, 0x70);
pub const ACCENT: Color32 = Color32::from_rgb(0x2F, 0x5C, 0xE5);
pub const AGENT: Color32 = Color32::from_rgb(0x6A, 0x3D, 0xC4);
pub const WARN: Color32 = Color32::from_rgb(0xB4, 0x7A, 0x00);
pub const DANGER: Color32 = Color32::from_rgb(0xB3, 0x2B, 0x2B);
pub const OK: Color32 = Color32::from_rgb(0x1E, 0x7A, 0x46);

/// Colour for a task or phase status.
pub fn status(s: &str) -> Color32 {
    match s {
        "done" => OK,
        "in_review" => WARN,
        "in_progress" | "active" => ACCENT,
        "blocked" => DANGER,
        "dropped" => MUTED,
        _ => MUTED,
    }
}

pub fn install(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = PANEL;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    visuals.hyperlink_color = ACCENT;
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}

/// A small coloured pill, used for statuses and assignee kinds.
pub fn pill(ui: &mut egui::Ui, text: &str, colour: Color32) {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(11.0),
        colour,
    );
    let pad = egui::vec2(7.0, 3.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad * 2.0, egui::Sense::hover());
    ui.painter().rect_filled(rect, 4.0, colour.gamma_multiply(0.12));
    ui.painter().galley(rect.min + pad, galley, colour);
}

/// Monospace id, shortened. Full ids are noise on screen but needed to copy.
pub fn id_label(ui: &mut egui::Ui, id: &str) {
    let short = id.get(..8).unwrap_or(id);
    ui.add(egui::Label::new(
        egui::RichText::new(short).monospace().size(11.0).color(MUTED),
    ))
    .on_hover_text(id);
}
