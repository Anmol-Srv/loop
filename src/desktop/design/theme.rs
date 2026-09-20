//! Applies the tokens to egui, and loads the system font.

use egui::{FontData, FontDefinitions, FontFamily, Stroke};

use super::tokens::{colour, radius, space, text};

/// Inter for the interface, JetBrains Mono for ids and the run log, Phosphor
/// for icons. All compiled in rather than read from the system: the app starts
/// offline and renders identically on every machine, which a system font cannot
/// promise across macOS versions.
///
/// Both faces are SIL Open Font License; see `assets/fonts/LICENSE.md`.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), FontData::from_static(bytes).into());
    };
    add("inter", include_bytes!("../../../assets/fonts/Inter-Regular.ttf"));
    add("inter-medium", include_bytes!("../../../assets/fonts/Inter-Medium.ttf"));
    add("inter-semibold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"));
    add("mono", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"));

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "mono".into());

    // Named families, so a widget can ask for weight without a second lookup.
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), vec!["inter-medium".into()]);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), vec!["inter-semibold".into()]);

    // Icons, so nothing ever reaches for an emoji.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Thin);

    ctx.set_fonts(fonts);
}

/// Weight families. `FontFamily::Name(MEDIUM.into())` in a `FontId`.
pub const MEDIUM: &str = "inter-medium";
pub const SEMIBOLD: &str = "inter-semibold";

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut v = egui::Visuals::dark();
    v.panel_fill = colour::CANVAS;
    v.window_fill = colour::SURFACE;
    v.extreme_bg_color = colour::SURFACE;
    v.faint_bg_color = colour::SURFACE_HOVER;
    v.code_bg_color = colour::INSET;
    v.window_stroke = Stroke::new(1.0, colour::LINE);
    v.override_text_color = Some(colour::TEXT);
    v.hyperlink_color = colour::ACCENT;
    v.selection.bg_fill = colour::ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, colour::ACCENT);

    // Hairlines everywhere. Nothing in this app needs a heavy border.
    let hairline = Stroke::new(1.0, colour::LINE);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, colour::LINE_SOFT);
    v.widgets.inactive.bg_stroke = hairline;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, colour::LINE);
    v.widgets.active.bg_stroke = Stroke::new(1.0, colour::ACCENT);

    v.widgets.noninteractive.bg_fill = colour::SURFACE;
    v.widgets.noninteractive.weak_bg_fill = colour::SURFACE;
    v.widgets.inactive.bg_fill = colour::SURFACE;
    v.widgets.inactive.weak_bg_fill = colour::SURFACE;
    v.widgets.hovered.bg_fill = colour::SURFACE_HOVER;
    v.widgets.hovered.weak_bg_fill = colour::SURFACE_HOVER;
    v.widgets.active.bg_fill = colour::SURFACE_ACTIVE;
    v.widgets.active.weak_bg_fill = colour::SURFACE_ACTIVE;

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        w.fg_stroke = Stroke::new(1.0, colour::TEXT);
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colour::TEXT_MUTED);

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
        s.spacing.menu_margin = egui::Margin::same(space::XS as i8);
        s.spacing.button_padding = egui::vec2(super::tokens::pad::BUTTON.0, super::tokens::pad::BUTTON.1);
        s.spacing.interact_size.y = super::tokens::size::CONTROL;
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
