//! Applies the tokens to egui, and loads the system font.

use egui::{FontData, FontDefinitions, FontFamily, Stroke};

use super::tokens::{colour, radius, space, text};

/// macOS ships SF Pro and SF Mono as plain .ttf, which egui can parse. Using
/// them is most of what makes the app look native rather than generic — the
/// stock egui font is the single biggest tell.
///
/// Best effort: if a face is missing or unparseable on some machine we keep the
/// default rather than refusing to start.
fn install_fonts(ctx: &egui::Context) {
    const SANS: &str = "/System/Library/Fonts/SFNS.ttf";
    const MONO: &str = "/System/Library/Fonts/SFNSMono.ttf";

    let mut fonts = FontDefinitions::default();
    let mut loaded_sans = false;

    if let Ok(bytes) = std::fs::read(SANS) {
        fonts.font_data.insert("sf".into(), FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "sf".into());
        loaded_sans = true;
    }

    if let Ok(bytes) = std::fs::read(MONO) {
        fonts.font_data.insert("sf-mono".into(), FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "sf-mono".into());
    }

    // Icons, so we never reach for an emoji.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Thin);

    if !loaded_sans {
        tracing::debug!("system font unavailable; using the egui default");
    }
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut v = egui::Visuals::light();
    v.panel_fill = colour::CANVAS;
    v.window_fill = colour::SURFACE;
    v.extreme_bg_color = colour::SURFACE;
    v.faint_bg_color = colour::SURFACE_HOVER;
    v.override_text_color = Some(colour::TEXT);
    v.hyperlink_color = colour::ACCENT;
    v.selection.bg_fill = colour::ACCENT.gamma_multiply(0.18);
    v.selection.stroke = Stroke::new(1.0, colour::ACCENT);

    // Hairlines everywhere. Nothing in this app needs a heavy border.
    let hairline = Stroke::new(1.0, colour::LINE);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, colour::LINE_SOFT);
    v.widgets.inactive.bg_stroke = hairline;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, colour::LINE);
    v.widgets.active.bg_stroke = Stroke::new(1.0, colour::ACCENT);

    v.widgets.inactive.bg_fill = colour::SURFACE;
    v.widgets.hovered.bg_fill = colour::SURFACE_HOVER;
    v.widgets.active.bg_fill = colour::SURFACE_HOVER;

    let r = egui::CornerRadius::same(radius::SM);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }

    // Flat: no drop shadows anywhere. They read as decoration here.
    v.popup_shadow = egui::epaint::Shadow::NONE;
    v.window_shadow = egui::epaint::Shadow::NONE;

    ctx.set_visuals(v);

    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(space::SM, space::SM);
        s.spacing.button_padding = egui::vec2(space::MD, space::SM);
        s.spacing.interact_size.y = 24.0;
        s.spacing.scroll.bar_width = 8.0;

        use egui::{FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Heading, FontId::proportional(text::TITLE)),
            (TextStyle::Body, FontId::proportional(text::BODY)),
            (TextStyle::Button, FontId::proportional(text::BODY)),
            (TextStyle::Small, FontId::proportional(text::CAPTION)),
            (TextStyle::Monospace, FontId::monospace(text::SMALL)),
        ]
        .into();
    });
}
