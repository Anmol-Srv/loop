//! Design tokens.
//!
//! Every colour, gap, radius and type size in the app comes from here. A view
//! that wants padding says `space::MD`, never `10.0`, so the scale changes in
//! one place instead of forty.
//!
//! Two palettes (see `colour`), dark by default, in a twilight register: a blue-slate canvas rather than neutral
//! black, surfaces separated by a step of lightness rather than by shadow,
//! hairline borders, one soft accent. Colour carries state and nothing else —
//! that is what keeps a board with two hundred rows readable.
//!
//! The palette is pulled from a dusk photograph: deep blue shadow, periwinkle
//! light, a warm gold where the light catches. Nothing is fully saturated,
//! which is what stops a dark interface looking like a neon sign.

use egui::Color32;

pub mod colour {
    //! Colour is read when it is painted, not baked in at compile time, so the
    //! whole app follows the appearance setting on the next frame. Every view
    //! still says `colour::TEXT()` — one vocabulary, two palettes behind it.
    #![allow(non_snake_case)]

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

    use super::Color32;

    const fn rgb(hex: u32) -> Color32 {
        Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }
    /// A premultiplied white at `a` — a lift that lets what is behind show.
    const fn white(a: u8) -> Color32 {
        Color32::from_rgba_premultiplied(a, a, a, a)
    }
    /// A premultiplied black at `a` — the light palette's version of a lift.
    const fn black(a: u8) -> Color32 {
        Color32::from_rgba_premultiplied(0, 0, 0, a)
    }

    /// Everything but the accent, which is picked separately.
    pub struct Palette {
        // ---- surfaces, each a step in lightness. No shadows; depth is tone
        // ---- plus a hairline.
        /// The window.
        pub CANVAS: Color32,
        /// The sidebar and other chrome that frames content.
        pub CHROME: Color32,
        /// Cards and panels.
        pub SURFACE: Color32,
        pub SURFACE_HOVER: Color32,
        /// Selected rows and pressed controls.
        pub SURFACE_ACTIVE: Color32,
        /// Inputs, which sit *below* the surface they are on.
        pub INSET: Color32,
        pub LINE: Color32,
        pub LINE_SOFT: Color32,
        pub LINE_STRONG: Color32,
        // ---- ink, five levels. Every level clears 4.5:1 on SURFACE.
        pub TEXT: Color32,
        /// A value next to its label.
        pub TEXT_2: Color32,
        /// Labels, metadata.
        pub TEXT_MUTED: Color32,
        /// Timestamps, ids.
        pub TEXT_FAINT: Color32,
        /// Disabled and placeholders — still hint text people read.
        pub TEXT_DISABLED: Color32,
        // ---- state. A dot, a pill or a thin rule; never a filled block.
        pub OK: Color32,
        pub WARN: Color32,
        pub DANGER: Color32,
        pub AGENT: Color32,
        pub INFO: Color32,
        pub IDLE: Color32,
        pub OK_BG: Color32,
        pub WARN_BG: Color32,
        pub DANGER_BG: Color32,
        pub AGENT_BG: Color32,
        pub INFO_BG: Color32,
        /// The warm counterpoint. A highlight, never a surface.
        pub GLOW: Color32,
        pub WASH_TOP: Color32,
        pub WASH_BOTTOM: Color32,
        // ---- glass: a translucent lift off the canvas, one even hairline.
        pub GLASS: Color32,
        pub GLASS_HOVER: Color32,
        pub GLASS_ACTIVE: Color32,
        pub EDGE_MID: Color32,
        pub EDGE_MID_HOVER: Color32,
        /// The secondary button's brighter rim on hover.
        pub EDGE_HI_HOVER: Color32,
        /// The run log: a well, a step darker than a card.
        pub LOG_BG: Color32,
        pub LOG_TEXT: Color32,
        pub LOG_SEQ: Color32,
        // ---- disciplines: close together and quiet, since one is on every
        // ---- row. The loud colours are reserved for status.
        pub DESIGN: Color32,
        pub FRONTEND: Color32,
        pub BACKEND: Color32,
    }

    /// Dusk: a blue-slate canvas, periwinkle light, gold where it catches.
    /// Nothing fully saturated, so it never reads as a neon sign.
    pub const DUSK: Palette = Palette {
        CANVAS: rgb(0x0B0C0E),
        CHROME: rgb(0x111214),
        SURFACE: rgb(0x151719),
        SURFACE_HOVER: rgb(0x1B1E21),
        SURFACE_ACTIVE: rgb(0x212428),
        INSET: rgb(0x0E0F11),
        LINE: rgb(0x232629),
        LINE_SOFT: rgb(0x1A1D20),
        LINE_STRONG: rgb(0x31353A),
        TEXT: rgb(0xF2F3F5),
        TEXT_2: rgb(0xC3C7CC),
        TEXT_MUTED: rgb(0x8A9099),
        TEXT_FAINT: rgb(0x7B828C),
        TEXT_DISABLED: rgb(0x767C85),
        OK: rgb(0x5FD39B),
        WARN: rgb(0xF0B354),
        DANGER: rgb(0xF27A7A),
        AGENT: rgb(0xB49BF0),
        INFO: rgb(0x7FB4F5),
        IDLE: rgb(0x6A7079),
        OK_BG: rgb(0x122A21),
        WARN_BG: rgb(0x2B2113),
        DANGER_BG: rgb(0x2C1718),
        AGENT_BG: rgb(0x221D33),
        INFO_BG: rgb(0x14202E),
        GLOW: rgb(0xEAD6AE),
        WASH_TOP: rgb(0x242E4A),
        WASH_BOTTOM: rgb(0x111621),
        GLASS: white(0x0D),
        GLASS_HOVER: white(0x1A),
        GLASS_ACTIVE: white(0x24),
        EDGE_MID: white(0x1C),
        EDGE_MID_HOVER: white(0x2E),
        EDGE_HI_HOVER: white(0x5E),
        LOG_BG: rgb(0x08090B),
        LOG_TEXT: rgb(0xC3C7CC),
        LOG_SEQ: rgb(0x797F88),
        DESIGN: rgb(0xC9A8D8),
        FRONTEND: rgb(0x8AC4D8),
        BACKEND: rgb(0x9AC0A8),
    };

    /// Daylight: the same system in daylight. A cool off-white canvas tinted
    /// a hair toward the slate, white cards, ink that is near-black rather
    /// than black. State hues deepen so a chip still clears 4.5:1 on its tint.
    pub const DAYLIGHT: Palette = Palette {
        CANVAS: rgb(0xF4F5F7),
        CHROME: rgb(0xECEEF1),
        SURFACE: rgb(0xFFFFFF),
        SURFACE_HOVER: rgb(0xF1F3F6),
        SURFACE_ACTIVE: rgb(0xE5E8ED),
        INSET: rgb(0xF6F7F9),
        LINE: rgb(0xDCE0E5),
        LINE_SOFT: rgb(0xE7EAEE),
        LINE_STRONG: rgb(0xC3C9D1),
        TEXT: rgb(0x15171B),
        TEXT_2: rgb(0x383D45),
        TEXT_MUTED: rgb(0x565D67),
        TEXT_FAINT: rgb(0x656C76),
        TEXT_DISABLED: rgb(0x6F757E),
        OK: rgb(0x16794D),
        WARN: rgb(0x9A5A00),
        DANGER: rgb(0xBF3036),
        AGENT: rgb(0x6A48C4),
        INFO: rgb(0x2463AD),
        IDLE: rgb(0x8A9099),
        OK_BG: rgb(0xE1F3EA),
        WARN_BG: rgb(0xFBEEDB),
        DANGER_BG: rgb(0xFCE7E7),
        AGENT_BG: rgb(0xEEE9FB),
        INFO_BG: rgb(0xE3EDFA),
        GLOW: rgb(0x9A6B12),
        WASH_TOP: rgb(0xE3E8F3),
        WASH_BOTTOM: rgb(0xECEEF1),
        GLASS: white(0xD0),
        GLASS_HOVER: black(0x08),
        GLASS_ACTIVE: black(0x10),
        EDGE_MID: black(0x1A),
        EDGE_MID_HOVER: black(0x26),
        EDGE_HI_HOVER: black(0x38),
        LOG_BG: rgb(0xF1F3F6),
        LOG_TEXT: rgb(0x383D45),
        LOG_SEQ: rgb(0x656C76),
        DESIGN: rgb(0x8A4AA6),
        FRONTEND: rgb(0x1C6F8C),
        BACKEND: rgb(0x2F7449),
    };

    /// One accent, in both modes. Dark mode's is pale enough to take dark
    /// ink; light mode's is deep enough to take white. `[accent, hover,
    /// soft tint, ink on it]`.
    pub struct Accent {
        pub name: &'static str,
        pub dark: [Color32; 4],
        pub light: [Color32; 4],
    }

    pub const ACCENTS: [Accent; 4] = [
        Accent {
            name: "Sky",
            dark: [rgb(0x8ACFF8), rgb(0xA6DCFA), rgb(0x142431), rgb(0x06141E)],
            light: [rgb(0x1F6FB2), rgb(0x195E98), rgb(0xE2EEF9), rgb(0xFFFFFF)],
        },
        Accent {
            name: "Teal",
            dark: [rgb(0x6ED6C8), rgb(0x8EE2D6), rgb(0x12282A), rgb(0x04201C)],
            light: [rgb(0x0E7A70), rgb(0x0A675F), rgb(0xDDF2EF), rgb(0xFFFFFF)],
        },
        Accent {
            name: "Coral",
            dark: [rgb(0xF5A08A), rgb(0xF8B6A5), rgb(0x2E1B18), rgb(0x2A0D06)],
            light: [rgb(0xBC432B), rgb(0xA23823), rgb(0xFBE6E1), rgb(0xFFFFFF)],
        },
        Accent {
            name: "Periwinkle",
            dark: [rgb(0xA9B4FA), rgb(0xBFC7FB), rgb(0x1C2038), rgb(0x0E1230)],
            light: [rgb(0x4652C8), rgb(0x3A45AD), rgb(0xE7E9FB), rgb(0xFFFFFF)],
        },
    ];

    static LIGHT: AtomicBool = AtomicBool::new(false);
    static ACCENT_IX: AtomicUsize = AtomicUsize::new(0);

    /// Switch palettes. Takes effect on the next paint; `theme::apply` is the
    /// one caller, since egui's own visuals have to follow too.
    pub fn set(light: bool, accent: usize) {
        LIGHT.store(light, Relaxed);
        ACCENT_IX.store(accent.min(ACCENTS.len() - 1), Relaxed);
    }

    pub fn is_light() -> bool {
        LIGHT.load(Relaxed)
    }

    pub fn palette() -> &'static Palette {
        if is_light() { &DAYLIGHT } else { &DUSK }
    }

    fn accent() -> &'static [Color32; 4] {
        let a = &ACCENTS[ACCENT_IX.load(Relaxed)];
        if is_light() { &a.light } else { &a.dark }
    }

    macro_rules! read {
        ($($name:ident),* $(,)?) => {
            $( #[inline] pub fn $name() -> Color32 { palette().$name } )*
        };
    }
    read!(
        CANVAS, CHROME, SURFACE, SURFACE_HOVER, SURFACE_ACTIVE, INSET, LINE, LINE_SOFT,
        LINE_STRONG, TEXT, TEXT_2, TEXT_MUTED, TEXT_FAINT, TEXT_DISABLED, OK, WARN, DANGER, AGENT,
        INFO, IDLE, OK_BG, WARN_BG, DANGER_BG, AGENT_BG, INFO_BG, GLOW, WASH_TOP, WASH_BOTTOM,
        GLASS, GLASS_HOVER, GLASS_ACTIVE, EDGE_MID, EDGE_MID_HOVER, EDGE_HI_HOVER, LOG_BG,
        LOG_TEXT, LOG_SEQ,
    );

    /// The one accent: primary actions, the current selection, attention
    /// badges. Never decoration.
    pub fn ACCENT() -> Color32 {
        accent()[0]
    }
    pub fn ACCENT_HOVER() -> Color32 {
        accent()[1]
    }
    /// An accent-tinted surface, for selected rows and quiet emphasis.
    pub fn ACCENT_SOFT() -> Color32 {
        accent()[2]
    }
    /// Ink on a filled accent control.
    pub fn ON_ACCENT() -> Color32 {
        accent()[3]
    }
    /// Nothing at all, named so a component says "no fill" in the same
    /// vocabulary as every other colour.
    pub fn TRANSPARENT() -> Color32 {
        Color32::TRANSPARENT
    }
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
    /// Inside a card that holds rows rather than prose. Rows carry their own
    /// horizontal padding, so card padding on top of it double-indents.
    pub const LIST: (f32, f32) = (space::SM, space::SM);
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
    /// A stat tile's numeral. The largest thing on any screen.
    pub const HERO: f32 = 32.0;
    pub const DISPLAY: f32 = 24.0;
    pub const TITLE: f32 = 19.0;
    /// A card's own heading — a task title, a section name.
    pub const CARD: f32 = 15.0;
    pub const HEADING: f32 = 14.0;
    pub const BODY: f32 = 13.0;
    pub const SMALL: f32 = 11.5;
    pub const CAPTION: f32 = 10.5;
}

/// Slight curve, never round. Only count badges and avatars are circles.
/// bencho.dev leans much harder (28px cards, 999px pills); this is a dense
/// board, not a gallery.
pub mod radius {
    pub const SM: u8 = 4;
    pub const MD: u8 = 6;
    pub const LG: u8 = 8;
    pub const XL: u8 = 10;
    pub const PILL: u8 = 99;
}

/// Fixed heights, so columns align down a page and across screens.
pub mod size {
    pub const ROW: f32 = 30.0;
    pub const CONTROL: f32 = 28.0;
    pub const SIDEBAR_W: f32 = 232.0;
    /// Avatar diameters. Two callers independently reached for `24.0` during
    /// the port, which is exactly the literal this module exists to prevent.
    pub const AVATAR_SM: f32 = 20.0;
    pub const AVATAR_MD: f32 = 24.0;
    pub const AVATAR_LG: f32 = 34.0;
    /// Height of a count badge.
    pub const BADGE_H: f32 = 16.0;
    /// The icon gutter. One number, so a leading icon sits at the same offset
    /// in the sidebar as in a button — they were 24 and 18 before.
    pub const ICON_COL: f32 = 22.0;
    /// A status dot.
    pub const DOT: f32 = 8.0;
    /// Collapse the sidebar to icons below this window width. The only
    /// structural response in the app, and the right one: it hands 160px back
    /// to the column that runs out first.
    pub const SIDEBAR_COLLAPSE_AT: f32 = 960.0;
    pub const SIDEBAR_W_NARROW: f32 = 56.0;
    /// The work column stops widening here; beyond it, cards just stretch.
    pub const CONTENT_MAX: f32 = 1080.0;
    /// The properties rail beside a detail page. Wide enough for a status
    /// dropdown and an avatar with a name, narrow enough that the reading
    /// column keeps the majority of the page.
    pub const RAIL_W: f32 = 264.0;
    /// One width for every picker that sits in a set — a form's property row,
    /// a properties rail — so the set reads as equal slots instead of boxes
    /// sized by whatever each happens to say. Room for "30 Sep 2026" and a
    /// caret, and it fits the rail's value column.
    pub const PICKER_W: f32 = 140.0;
    /// A nav row, taller than a list row — it is a target, not data.
    pub const NAV_ROW: f32 = 32.0;
    // No CONTENT_MAX: the body runs to the window edge. A centred measure suits
    // prose; a board wants every pixel.
}

/// A discipline's colour. Deliberately close together and all quiet: the
/// discipline is on every row, so it must be scannable without shouting. The
/// loud colours are reserved for status, which is what you are actually
/// hunting for.
pub fn discipline_colour(discipline: &str) -> Color32 {
    match discipline {
        "design" => colour::palette().DESIGN,
        "frontend" => colour::palette().FRONTEND,
        "backend" => colour::palette().BACKEND,
        _ => colour::TEXT_FAINT(),
    }
}

/// Width of the discipline column, so every row in every list agrees.
pub const DISCIPLINE_W: f32 = 62.0;

/// The dot beside a status.
///
/// `shipped` and `completed` both read as finished, but only one of them is
/// the end of the engineering track, so shipped takes the confident green and
/// completed the cooler blue. `handoff` is design's "it is someone else's
/// turn", which is the same shape of fact as an agent holding something.
pub fn status_colour(status: &str) -> Color32 {
    match status {
        "shipped" => colour::OK(),
        "completed" | "done" => colour::INFO(),
        "handoff" => colour::AGENT(),
        "research" => colour::INFO(),
        // Amber, to match the chip. It was the accent blue, which put two
        // near-identical blues side by side in the donut (in progress and
        // completed) and made the row dot disagree with its own status chip.
        "in_progress" | "active" => colour::WARN(),
        "blocked" => colour::DANGER(),
        "dropped" => colour::TEXT_FAINT(),
        "triage" => colour::INFO(),
        _ => colour::IDLE(),
    }
}

/// Human-facing label, so no underscore ever reaches the screen.
pub fn status_label(status: &str) -> &str {
    // Sentence case here, once, so no view has to remember to capitalise —
    // the app shipped "active" on one page and "Active" on the next because
    // half the call sites did and half did not.
    match status {
        "open" => "Open",
        "triage" => "Triage",
        "in_progress" => "In progress",
        "research" => "Research",
        "handoff" => "Handoff",
        "completed" => "Completed",
        "shipped" => "Shipped",
        "blocked" => "Blocked",
        "dropped" => "Dropped",
        "active" => "Active",
        "paused" => "Paused",
        "done" => "Done",
        "archived" => "Archived",
        other => other,
    }
}
