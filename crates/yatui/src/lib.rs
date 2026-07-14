//! Rust-native terminal user interface primitives.
//!
//! The facade currently exposes foundational [`core`], Unicode [`text`],
//! cell-based [`render`], and backend-neutral [`terminal`] APIs.
//!
//! The default `crossterm` feature exports [`CrosstermBackend`]. Disable
//! default features when providing another terminal backend.

/// Foundational geometry, color, style, and cursor types.
pub use yatui_core as core;
/// Grapheme-aware cell buffers, composition, and transactional frame diffing.
pub use yatui_render as render;
/// Backend-neutral terminal events, capabilities, state, and lifecycle.
pub use yatui_terminal as terminal;
/// Unicode grapheme segmentation and terminal width measurement.
pub use yatui_text as text;

#[cfg(feature = "crossterm")]
pub use yatui_backend_crossterm::CrosstermBackend;

pub use yatui_core::{
    Color, CursorShape, CursorState, CursorVisibility, Insets, Modifier, Point, Rect, Size, Style,
};
pub use yatui_render::{Buffer, Canvas, FramePatch, PreparedFrame, Renderer};
pub use yatui_terminal::{
    Capabilities, TerminalBackend, TerminalEvent, TerminalSession, TerminalState, WriteOutcome,
};
pub use yatui_text::{TextMetrics, WidthPolicy, grapheme_width, graphemes, measure};
