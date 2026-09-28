//! Applies the tokens to egui, and loads the system font.

use egui::{FontData, FontDefinitions, FontFamily, Stroke};

use super::tokens::{colour, radius, space, text};


/// Nunito for the interface, JetBrains Mono for ids and the run log, Phosphor
/// for icons. All compiled in rather than read from the system: the app starts
/// offline and renders identically on every machine, which a system font cannot
/// promise across macOS versions.
///
/// Nunito is a rounded humanist sans. It softens the dusk palette in a way a
/// neutral grotesque does not, and its generous x-height survives being set at
/// 11pt in a dense table.
///
/// Both faces are SIL Open Font License; see `assets/fonts/LICENSE.md`.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), FontData::from_static(bytes).into());
    };
    add("ui", include_bytes!("../../../assets/fonts/Nunito-Regular.ttf"));
    add("ui-medium", include_bytes!("../../../assets/fonts/Nunito-Medium.ttf"));
    add("ui-semibold", include_bytes!("../../../assets/fonts/Nunito-SemiBold.ttf"));
    add("ui-bold", include_bytes!("../../../assets/fonts/Nunito-Bold.ttf"));
    add("mono", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"));

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "ui".into());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "mono".into());

    // Named families, so a widget can ask for weight without a second lookup.
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), vec!["ui-medium".into()]);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), vec!["ui-semibold".into()]);
    fonts
        .families
        .insert(FontFamily::Name(BOLD.into()), vec!["ui-bold".into()]);

    // Icons, so nothing ever reaches for an emoji.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    ctx.set_fonts(fonts);
}

/// Weight families. `FontFamily::Name(MEDIUM.into())` in a `FontId`.
pub const MEDIUM: &str = "ui-medium";
pub const SEMIBOLD: &str = "ui-semibold";
pub const BOLD: &str = "ui-bold";

/// Which palette the app wears. `System` follows macOS as it changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    System,
    Dark,
    Light,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::System, Mode::Dark, Mode::Light];

    pub fn label(self) -> &'static str {
        match self {
            Mode::System => "System",
            Mode::Dark => "Dark",
            Mode::Light => "Light",
        }
    }
}

/// The appearance setting: a mode and an index into `colour::ACCENTS`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Appearance {
    pub mode: Mode,
    pub accent: usize,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { mode: Mode::Dark, accent: 0 }
    }
}

const PREF: &str = "appearance";

impl Appearance {
    /// "light teal" and the like; anything unreadable falls back to default.
    fn parse(raw: &str) -> Self {
        let mut out = Self::default();
        for word in raw.split_whitespace() {
            if let Some(m) = Mode::ALL.iter().find(|m| m.label().eq_ignore_ascii_case(word)) {
                out.mode = *m;
            } else if let Some(i) = colour::ACCENTS.iter().position(|a| a.name.eq_ignore_ascii_case(word)) {
                out.accent = i;
            }
        }
        out
    }

    fn encode(self) -> String {
        format!("{} {}", self.mode.label(), colour::ACCENTS[self.accent].name).to_lowercase()
    }
}

fn store_id() -> egui::Id {
    egui::Id::new("theme:appearance")
}

/// The current setting.
pub fn appearance(ctx: &egui::Context) -> Appearance {
    ctx.data(|d| d.get_temp(store_id())).unwrap_or_default()
}

/// Change the setting: remembered on disk, painted from the next frame.
pub fn set_appearance(ctx: &egui::Context, a: Appearance) {
    ctx.data_mut(|d| d.insert_temp(store_id(), a));
    if let Err(e) = crate::desktop::creds::store_pref(PREF, &a.encode()) {
        tracing::warn!("appearance not saved: {e}");
    }
    follow(ctx);
}

/// Resolve the setting against the system's and swap palettes if the answer
/// changed. Once a frame: cheap when nothing moved, and it is what makes
/// `System` track macOS switching at sunset.
pub fn follow(ctx: &egui::Context) {
    let a = appearance(ctx);
    let light = match a.mode {
        Mode::Light => true,
        Mode::Dark => false,
        Mode::System => ctx.system_theme() == Some(egui::Theme::Light),
    };
    let applied: Option<(bool, usize)> = ctx.data(|d| d.get_temp(store_id().with("applied")));
    if applied == Some((light, a.accent)) {
        return;
    }
    colour::set(light, a.accent);
    apply_visuals(ctx, light);
    ctx.data_mut(|d| d.insert_temp(store_id().with("applied"), (light, a.accent)));
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    // An env var wins, like `ACP_TOKEN`: a render or a demo can pick a palette
    // without touching the person's saved one.
    let saved = std::env::var("ACP_APPEARANCE")
        .ok()
        .or_else(|| crate::desktop::creds::load_pref(PREF))
        .map(|raw| Appearance::parse(&raw));
    ctx.data_mut(|d| d.insert_temp(store_id(), saved.unwrap_or_default()));
    follow(ctx);
    install_style(ctx);
}

/// egui's own widgets (text edits, popups, selection) in the current palette.
fn apply_visuals(ctx: &egui::Context, light: bool) {
    let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.panel_fill = colour::CANVAS();
    v.window_fill = colour::SURFACE();
    v.extreme_bg_color = colour::SURFACE();
    v.faint_bg_color = colour::SURFACE_HOVER();
    v.code_bg_color = colour::INSET();
    v.window_stroke = Stroke::new(1.0, colour::LINE());
    v.override_text_color = Some(colour::TEXT());
    v.hyperlink_color = colour::ACCENT();
    v.selection.bg_fill = colour::ACCENT().gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, colour::ACCENT());

    // Hairlines everywhere. Nothing in this app needs a heavy border.
    let hairline = Stroke::new(1.0, colour::LINE());
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, colour::LINE_SOFT());
    v.widgets.inactive.bg_stroke = hairline;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, colour::LINE());
    v.widgets.active.bg_stroke = Stroke::new(1.0, colour::ACCENT());

    v.widgets.noninteractive.bg_fill = colour::SURFACE();
    v.widgets.noninteractive.weak_bg_fill = colour::SURFACE();
    v.widgets.inactive.bg_fill = colour::SURFACE();
    v.widgets.inactive.weak_bg_fill = colour::SURFACE();
    v.widgets.hovered.bg_fill = colour::SURFACE_HOVER();
    v.widgets.hovered.weak_bg_fill = colour::SURFACE_HOVER();
    v.widgets.active.bg_fill = colour::SURFACE_ACTIVE();
    v.widgets.active.weak_bg_fill = colour::SURFACE_ACTIVE();

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        w.fg_stroke = Stroke::new(1.0, colour::TEXT());
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colour::TEXT_MUTED());

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

    // Pinned rather than left to egui's own system-following, which would
    // swap to its stock light style underneath ours.
    let theme = if light { egui::Theme::Light } else { egui::Theme::Dark };
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, v);
}

fn install_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| {
        // egui surfaces no OS reduce-motion flag (see motion.rs), but its own
        // animation_time turns every animation off — including the hand-rolled
        // ones, which all read the knob. An env override makes that reachable
        // until eframe exposes the system setting.
        if std::env::var_os("AIRTRIBE_REDUCE_MOTION").is_some() {
            s.animation_time = 0.0;
        }
        s.spacing.item_spacing = egui::vec2(space::SM, space::SM);
        s.spacing.menu_margin = egui::Margin::same(space::XS as i8);
        s.spacing.button_padding = egui::vec2(super::tokens::pad::BUTTON.0, super::tokens::pad::BUTTON.1);
        s.spacing.interact_size.y = super::tokens::size::CONTROL;
        // Scrollbars overlay rather than reserve a gutter, and they are the
        // same greys as every other line in the app. The default is an opaque
        // near-white bar that laid itself across whatever it scrolled past.
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        s.spacing.scroll.floating_width = space::XS;
        s.spacing.scroll.floating_allocated_width = 0.0;
        s.spacing.scroll.bar_inner_margin = space::XXS;
        s.spacing.scroll.bar_outer_margin = 0.0;
        // No scrollbar at all: no track, no handle, in any state. Scrolling
        // still works — the trackpad is the affordance on a Mac — the bar
        // just never paints. Opacity is the only knob egui exposes here.
        s.spacing.scroll.foreground_color = true;
        s.spacing.scroll.dormant_background_opacity = 0.0;
        s.spacing.scroll.active_background_opacity = 0.0;
        s.spacing.scroll.interact_background_opacity = 0.0;
        s.spacing.scroll.dormant_handle_opacity = 0.0;
        s.spacing.scroll.active_handle_opacity = 0.0;
        s.spacing.scroll.interact_handle_opacity = 0.0;

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
