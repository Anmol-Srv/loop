//! Design tokens.
//!
//! Every colour, size and gap in the app comes from here. A view that wants
//! padding says `space::MD`, not `12.0` — so the scale can change in one place
//! instead of forty.
//!
//! The direction is quiet and dense: light surfaces, hairline rules, one accent
//! used sparingly. Colour carries meaning; it does not decorate. That is what
//! keeps a board with two hundred rows readable.

use egui::Color32;

/// Surfaces and text. Deliberately few — a palette you can hold in your head.
pub mod colour {
    use super::Color32;

    /// The window behind everything.
    pub const CANVAS: Color32 = Color32::from_rgb(0xFA, 0xFA, 0xF9);
    /// Cards, headers, anything raised off the canvas.
    pub const SURFACE: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    /// A surface that is hovered or selected.
    pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF2);
    /// Hairline rules and card borders. Never heavier than 1px.
    pub const LINE: Color32 = Color32::from_rgb(0xE6, 0xE4, 0xDF);
    /// A rule that should barely register.
    pub const LINE_SOFT: Color32 = Color32::from_rgb(0xF0, 0xEE, 0xEA);

    pub const TEXT: Color32 = Color32::from_rgb(0x1A, 0x19, 0x17);
    /// Secondary text: labels, metadata, timestamps.
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x74, 0x71, 0x6B);
    /// Tertiary: placeholders, disabled.
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(0xA3, 0xA0, 0x99);

    /// The one accent. If two things on screen are accented, one of them is
    /// probably wrong.
    pub const ACCENT: Color32 = Color32::from_rgb(0x30, 0x51, 0xD3);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x26, 0x42, 0xB0);
    pub const ON_ACCENT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);

    /// Semantic, for state only — a dot or a pill, never a filled block.
    pub const DONE: Color32 = Color32::from_rgb(0x1D, 0x7A, 0x4A);
    pub const REVIEW: Color32 = Color32::from_rgb(0xA8, 0x6A, 0x00);
    pub const BLOCKED: Color32 = Color32::from_rgb(0xB0, 0x2E, 0x2E);
    pub const AGENT: Color32 = Color32::from_rgb(0x6B, 0x3F, 0xC0);
    pub const IDLE: Color32 = Color32::from_rgb(0x9B, 0x98, 0x92);

    /// The run log, the one dark surface in the app.
    pub const LOG_BG: Color32 = Color32::from_rgb(0x1A, 0x19, 0x17);
    pub const LOG_TEXT: Color32 = Color32::from_rgb(0xDE, 0xDC, 0xD6);
    pub const LOG_SEQ: Color32 = Color32::from_rgb(0x6A, 0x67, 0x62);
}

/// A 4pt rhythm. Anything not on the scale is a mistake or a deliberate,
/// commented exception.
pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 10.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 36.0;
}

/// Type scale. Dense, but never below 11 — this is a tool people read all day.
pub mod text {
    pub const TITLE: f32 = 19.0;
    pub const HEADING: f32 = 14.5;
    pub const BODY: f32 = 12.5;
    pub const SMALL: f32 = 11.5;
    pub const CAPTION: f32 = 10.5;
}

pub mod radius {
    pub const SM: u8 = 4;
    pub const MD: u8 = 6;
    pub const LG: u8 = 10;
}

/// One row in a list. Fixed so columns line up across phases and screens.
pub const ROW_HEIGHT: f32 = 26.0;
/// Content never stretches wider than this; long lines are hard to read.
pub const CONTENT_MAX: f32 = 980.0;

/// Colour for a task or phase status. The single place that mapping lives.
pub fn status_colour(status: &str) -> egui::Color32 {
    match status {
        "done" => colour::DONE,
        "in_review" => colour::REVIEW,
        "in_progress" | "active" => colour::ACCENT,
        "blocked" => colour::BLOCKED,
        "dropped" => colour::TEXT_FAINT,
        _ => colour::IDLE,
    }
}

/// Human-facing label for a status. Avoids `in_review` leaking to screen.
pub fn status_label(status: &str) -> &str {
    match status {
        "in_review" => "in review",
        "in_progress" => "in progress",
        other => other,
    }
}
