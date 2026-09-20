//! Design tokens.
//!
//! Every colour, gap, radius and type size in the app comes from here. A view
//! that wants padding says `space::MD`, never `10.0`, so the scale changes in
//! one place instead of forty.
//!
//! Dark, in a twilight register: a blue-slate canvas rather than neutral
//! black, surfaces separated by a step of lightness rather than by shadow,
//! hairline borders, one soft accent. Colour carries state and nothing else —
//! that is what keeps a board with two hundred rows readable.
//!
//! The palette is pulled from a dusk photograph: deep blue shadow, periwinkle
//! light, a warm gold where the light catches. Nothing is fully saturated,
//! which is what stops a dark interface looking like a neon sign.

use egui::Color32;

pub mod colour {
    use super::Color32;

    // ---- surfaces, each a step up in lightness. No shadows anywhere; depth
    // ---- is expressed by elevation of tone and a hairline.
    /// The window. Blue-slate, never pure black — the blue is what makes the
    /// whole surface read as dusk rather than switched off.
    pub const CANVAS: Color32 = Color32::from_rgb(0x0D, 0x11, 0x1A);
    /// The sidebar and other chrome that frames content.
    pub const CHROME: Color32 = Color32::from_rgb(0x11, 0x16, 0x21);
    /// Cards and panels.
    pub const SURFACE: Color32 = Color32::from_rgb(0x17, 0x1D, 0x2A);
    /// A raised or hovered surface.
    pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x1E, 0x25, 0x34);
    /// Selected rows and pressed controls.
    pub const SURFACE_ACTIVE: Color32 = Color32::from_rgb(0x26, 0x2E, 0x3F);
    /// Inputs, which sit *below* the surface they are on.
    pub const INSET: Color32 = Color32::from_rgb(0x0A, 0x0E, 0x16);

    // ---- lines
    pub const LINE: Color32 = Color32::from_rgb(0x28, 0x31, 0x43);
    pub const LINE_SOFT: Color32 = Color32::from_rgb(0x1C, 0x23, 0x31);
    pub const LINE_STRONG: Color32 = Color32::from_rgb(0x38, 0x44, 0x5A);

    // ---- text. Five levels, the way bencho.dev layers its ink scale: one
    // ---- step is rarely the right amount of de-emphasis.
    pub const TEXT: Color32 = Color32::from_rgb(0xE9, 0xEC, 0xF4);
    /// Secondary: a value next to its label.
    pub const TEXT_2: Color32 = Color32::from_rgb(0xBC, 0xC4, 0xD8);
    /// Muted: labels, metadata.
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8B, 0x96, 0xB0);
    /// Faint: timestamps, ids.
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x66, 0x71, 0x8A);
    /// Disabled.
    pub const TEXT_DISABLED: Color32 = Color32::from_rgb(0x48, 0x52, 0x68);

    /// The one accent: a pale sky blue. Light enough that a filled control
    /// takes dark text, which is why `ON_ACCENT` is near-black rather than
    /// white — white on this would be unreadable.
    pub const ACCENT: Color32 = Color32::from_rgb(0x8A, 0xCF, 0xF8);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xA6, 0xDC, 0xFA);
    /// An accent-tinted surface, for selected rows and quiet emphasis.
    pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x14, 0x24, 0x31);
    pub const ON_ACCENT: Color32 = Color32::from_rgb(0x06, 0x14, 0x1E);

    /// The warm counterpoint: gold where the light catches. Used sparingly —
    /// a highlight, never a surface. Cool everywhere and it reads as cold.
    pub const GLOW: Color32 = Color32::from_rgb(0xEA, 0xD6, 0xAE);

    /// The sidebar wash: a lift of periwinkle at the top fading into the
    /// chrome. Alpha-free pair, since a mesh interpolates the two directly.
    pub const WASH_TOP: Color32 = Color32::from_rgba_premultiplied(0x24, 0x2E, 0x4A, 0xFF);
    pub const WASH_BOTTOM: Color32 = Color32::from_rgba_premultiplied(0x11, 0x16, 0x21, 0xFF);

    // ---- glass. Cards are a translucent lift off the canvas rather than an
    // ---- opaque block, so the wash behind them shows through.
    /// Card fill. Deliberately weak: at 5% white the surface reads as lifted
    /// without becoming a grey slab.
    pub const GLASS: Color32 = Color32::from_rgba_premultiplied(0x0D, 0x0D, 0x0D, 0x0D);
    /// Hover lifts the fill. bencho.dev caps its equivalent (`--lift-max`) at
    /// .06; the same restraint applies — hover should be felt, not announced.
    pub const GLASS_HOVER: Color32 = Color32::from_rgba_premultiplied(0x1A, 0x1A, 0x1A, 0x1A);
    pub const GLASS_ACTIVE: Color32 = Color32::from_rgba_premultiplied(0x24, 0x24, 0x24, 0x24);

    // ---- the glass edge, as a gradient rather than one flat stroke.
    //
    // Real glass catches light on its top edge and loses it toward the bottom.
    // bencho.dev encodes exactly this as --edge-hi .75 / --edge-far .42 /
    // --edge-lo .18, and it is the difference between "glass" and "translucent
    // rectangle". Three tones, brightest on top.
    /// Top edge, catching the light.
    pub const EDGE_HI: Color32 = Color32::from_rgba_premultiplied(0x40, 0x40, 0x40, 0x40);
    /// The sides.
    pub const EDGE_MID: Color32 = Color32::from_rgba_premultiplied(0x1C, 0x1C, 0x1C, 0x1C);
    /// Bottom edge, in shadow.
    pub const EDGE_LO: Color32 = Color32::from_rgba_premultiplied(0x0E, 0x0E, 0x0E, 0x0E);
    /// Every edge brightens on hover — the lift is in the rim, not a shadow.
    pub const EDGE_HI_HOVER: Color32 = Color32::from_rgba_premultiplied(0x5E, 0x5E, 0x5E, 0x5E);
    pub const EDGE_MID_HOVER: Color32 = Color32::from_rgba_premultiplied(0x2E, 0x2E, 0x2E, 0x2E);

    // ---- state. Used as a dot, a pill or a thin rule; never a filled block.
    /// Desaturated to sit inside the dusk palette; a pure green would leap
    /// off this ground.
    pub const OK: Color32 = Color32::from_rgb(0x7F, 0xD4, 0xB0);
    pub const WARN: Color32 = Color32::from_rgb(0xEA, 0xC1, 0x7E);
    pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x8A, 0x8A);
    pub const AGENT: Color32 = Color32::from_rgb(0xC0, 0xAE, 0xE8);
    pub const IDLE: Color32 = Color32::from_rgb(0x6A, 0x74, 0x8C);

    /// The run log. Slightly darker than a card so it reads as a well.
    pub const LOG_BG: Color32 = Color32::from_rgb(0x08, 0x0B, 0x12);
    pub const LOG_TEXT: Color32 = Color32::from_rgb(0xC3, 0xCB, 0xDC);
    pub const LOG_SEQ: Color32 = Color32::from_rgb(0x4A, 0x54, 0x6B);
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

/// Type scale. Six sizes, each with one job — if a new size is tempting, the
/// answer is almost always one of these.
///
/// | size | where it is used |
/// |---|---|
/// | `DISPLAY` 26 | a single big number on a stat tile. Nowhere else. |
/// | `TITLE` 18 | the one heading that names the screen |
/// | `HEADING` 14 | a card or group heading, a project name in a list |
/// | `BODY` 13 | the default: task titles, buttons, nav items, table cells, inputs |
/// | `SMALL` 11.5 | metadata, section labels, secondary text, pills |
/// | `CAPTION` 10.5 | field labels, ids, timestamps |
///
/// Buttons, nav items and table cells are all `BODY` on purpose: they are the
/// same rank of thing and should not shift size between screens.
pub mod text {
    pub const DISPLAY: f32 = 26.0;
    pub const TITLE: f32 = 18.0;
    pub const HEADING: f32 = 14.0;
    pub const BODY: f32 = 13.0;
    pub const SMALL: f32 = 11.5;
    pub const CAPTION: f32 = 10.5;
}

/// Slight curve, never round.
/// Slight curve, never round — except pills, which are fully round.
/// bencho.dev leans harder on both ends (28px cards, 999px pills); we keep the
/// cards tighter because this is a dense board, not a gallery.
pub mod radius {
    pub const SM: u8 = 6;
    pub const MD: u8 = 10;
    pub const LG: u8 = 14;
    pub const PILL: u8 = 99;
}

/// Fixed heights, so columns align down a page and across screens.
pub mod size {
    pub const ROW: f32 = 30.0;
    pub const CONTROL: f32 = 28.0;
    pub const SIDEBAR_W: f32 = 212.0;
    /// Avatar diameters. Two callers independently reached for `24.0` during
    /// the port, which is exactly the literal this module exists to prevent.
    pub const AVATAR_SM: f32 = 20.0;
    pub const AVATAR_MD: f32 = 24.0;
    pub const AVATAR_LG: f32 = 34.0;
    // No CONTENT_MAX: the body runs to the window edge. A centred measure suits
    // prose; a board wants every pixel.
}

/// A discipline's colour. Deliberately close together and all quiet: the
/// discipline is on every row, so it must be scannable without shouting. The
/// loud colours are reserved for status, which is what you are actually
/// hunting for.
pub fn discipline_colour(discipline: &str) -> Color32 {
    match discipline {
        "design" => Color32::from_rgb(0xC9, 0xA8, 0xD8),
        "frontend" => Color32::from_rgb(0x8A, 0xC4, 0xD8),
        "backend" => Color32::from_rgb(0x9A, 0xC0, 0xA8),
        _ => colour::TEXT_FAINT,
    }
}

/// Width of the discipline column, so every row in every list agrees.
pub const DISCIPLINE_W: f32 = 62.0;

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
