//! The design system: tokens, the egui theme built from them, and the widget
//! layer every view is written against.

pub mod avatar;
pub mod cards;
pub mod motion;
pub mod shell;
pub mod theme;
pub mod viz;
pub mod tokens;
pub mod widgets;

pub use tokens::{colour, pad, radius, size, space, status_colour, status_label, text};
pub use cards as c;
pub use widgets as w;
