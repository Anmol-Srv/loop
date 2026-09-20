//! Design tokens.
//!
//! Every colour, gap, radius and type size in the app comes from here. A view
//! that wants padding says `space::MD`, never `10.0`, so the scale changes in
//! one place instead of forty.
//!
//! Dark, in the Linear register: near-black canvas, surfaces separated by a
//! step of lightness rather than by shadow, hairline borders, one accent.
//! Colour carries state and nothing else — that is what keeps a board with two
//! hundred rows readable.

use egui::Color32;

pub mod colour {
    use super::Color32;

    // ---- surfaces, each a step up in lightness. No shadows anywhere; depth
    // ---- is expressed by elevation of tone and a hairline.
    /// The window. Near-black, not pure black — pure black flattens everything
    /// stacked on it.
    pub const CANVAS: Color32 = Color32::from_rgb(0x0B, 0x0B, 0x0C);
    /// The sidebar and other chrome that frames content.
    pub const CHROME: Color32 = Color32::from_rgb(0x0F, 0x0F, 0x11);
    /// Cards and panels.
    pub const SURFACE: Color32 = Color32::from_rgb(0x15, 0x15, 0x17);
    /// A raised or hovered surface.
    pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1F);
    /// Selected rows and pressed controls.
    pub const SURFACE_ACTIVE: Color32 = Color32::from_rgb(0x23, 0x23, 0x27);
    /// Inputs, which sit *below* the surface they are on.
    pub const INSET: Color32 = Color32::from_rgb(0x0D, 0x0D, 0x0F);

    // ---- lines
    pub const LINE: Color32 = Color32::from_rgb(0x27, 0x27, 0x2B);
    pub const LINE_SOFT: Color32 = Color32::from_rgb(0x1D, 0x1D, 0x20);
    pub const LINE_STRONG: Color32 = Color32::from_rgb(0x35, 0x35, 0x3A);

    // ---- text
    pub const TEXT: Color32 = Color32::from_rgb(0xED, 0xED, 0xEF);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8C, 0x8C, 0x94);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5C, 0x5C, 0x63);

    /// The one accent.
    pub const ACCENT: Color32 = Color32::from_rgb(0x5E, 0x6A, 0xD2);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x6E, 0x7A, 0xE0);
    pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x1B, 0x1E, 0x33);
    pub const ON_ACCENT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);

    // ---- state. Used as a dot, a pill or a thin rule; never a filled block.
    pub const OK: Color32 = Color32::from_rgb(0x3E, 0xCF, 0x8E);
    pub const WARN: Color32 = Color32::from_rgb(0xF5, 0xA6, 0x23);
    pub const DANGER: Color32 = Color32::from_rgb(0xEF, 0x56, 0x4F);
    pub const AGENT: Color32 = Color32::from_rgb(0xA7, 0x8B, 0xFA);
    pub const IDLE: Color32 = Color32::from_rgb(0x6B, 0x6B, 0x73);

    /// The run log. Slightly darker than a card so it reads as a well.
    pub const LOG_BG: Color32 = Color32::from_rgb(0x08, 0x08, 0x09);
    pub const LOG_TEXT: Color32 = Color32::from_rgb(0xC9, 0xC9, 0xCF);
    pub const LOG_SEQ: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x52);
}

/// A 4pt rhythm. Anything off the scale is a bug or a commented exception.
pub mod space {
    /// 2 — hairline gaps inside a control.
    pub const XXS: f32 = 2.0;
    /// 4 — between a label and its field.
    pub const XS: f32 = 4.0;
    /// 8 — the default gap between items in a row.
    pub const SM: f32 = 8.0;
    /// 12 — padding inside a compact control; between related blocks.
    pub const MD: f32 = 12.0;
    /// 16 — card padding.
    pub const LG: f32 = 16.0;
    /// 24 — between sections.
    pub const XL: f32 = 24.0;
    /// 32 — page margin.
    pub const XXL: f32 = 32.0;
}

/// Padding presets, so components agree without each one inventing a number.
pub mod pad {
    use super::space;
    /// Inside a card or panel.
    pub const CARD: (f32, f32) = (space::LG, space::MD);
    /// Inside a button.
    pub const BUTTON: (f32, f32) = (space::MD, space::SM);
    /// Inside a text input.
    pub const INPUT: (f32, f32) = (space::MD, space::SM);
    /// Inside a list row.
    pub const ROW: (f32, f32) = (space::MD, 0.0);
    /// The page gutter.
    pub const PAGE: (f32, f32) = (space::XL, space::XL);
    /// Inside the sidebar.
    pub const SIDEBAR: (f32, f32) = (space::MD, space::MD);
}

/// Type scale. Dense, never below 11 — this is read all day.
pub mod text {
    pub const DISPLAY: f32 = 26.0;
    pub const TITLE: f32 = 18.0;
    pub const HEADING: f32 = 14.0;
    pub const BODY: f32 = 13.0;
    pub const SMALL: f32 = 11.5;
    pub const CAPTION: f32 = 10.5;
}

/// Slight curve, never round.
pub mod radius {
    pub const SM: u8 = 5;
    pub const MD: u8 = 7;
    pub const LG: u8 = 10;
    pub const PILL: u8 = 99;
}

/// Fixed heights, so columns align down a page and across screens.
pub mod size {
    pub const ROW: f32 = 30.0;
    pub const CONTROL: f32 = 28.0;
    pub const SIDEBAR_W: f32 = 212.0;
    pub const TOPBAR_H: f32 = 48.0;
    pub const CONTENT_MAX: f32 = 1100.0;
}

pub fn status_colour(status: &str) -> Color32 {
    match status {
        "done" => colour::OK,
        "in_review" => colour::WARN,
        "in_progress" | "active" => colour::ACCENT,
        "blocked" => colour::DANGER,
        "dropped" => colour::TEXT_FAINT,
        _ => colour::IDLE,
    }
}

/// Human-facing label, so `in_review` never reaches the screen.
pub fn status_label(status: &str) -> &str {
    match status {
        "in_review" => "in review",
        "in_progress" => "in progress",
        other => other,
    }
}
