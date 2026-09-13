//! Headless terminal state and protocol handling.

pub mod modes;
pub mod screen;
mod terminal;
pub mod unicode;

pub use screen::{
    Cell, Color, Cursor, CursorShape, GridPoint, Row, Screen, Selection, SemanticContent, Style,
    TrackedPoint, Underline,
};
pub use terminal::{Effect, Margins, Terminal, default_palette, parse_color};
