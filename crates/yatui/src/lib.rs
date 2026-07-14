//! Rust-native terminal user interface primitives.
//!
//! The facade currently exposes foundational [`core`], Unicode [`text`],
//! cell-based [`render`], layout, retained [`ui`], and backend-neutral
//! [`terminal`] APIs.
//!
//! The default `crossterm` feature exports [`CrosstermBackend`]. Disable
//! default features when providing another terminal backend.

/// Foundational geometry, color, style, and cursor types.
pub use yatui_core as core;
/// Backend-independent flex layout in terminal-cell coordinates.
pub use yatui_layout as layout;
/// Grapheme-aware cell buffers, composition, and transactional frame diffing.
pub use yatui_render as render;
/// Backend-neutral terminal events, capabilities, state, and lifecycle.
pub use yatui_terminal as terminal;
/// Unicode grapheme segmentation and terminal width measurement.
pub use yatui_text as text;
/// Borrowed declarative elements and retained UI identity.
pub use yatui_ui as ui;

#[cfg(feature = "crossterm")]
pub use yatui_backend_crossterm::CrosstermBackend;

pub use yatui_core::{
    Color, CursorShape, CursorState, CursorVisibility, Insets, Modifier, Point, Rect, Size, Style,
};
pub use yatui_layout::{Dimension, LayoutStyle};
pub use yatui_render::{Buffer, Canvas, FramePatch, PreparedFrame, Renderer};
pub use yatui_terminal::{
    Capabilities, TerminalBackend, TerminalEvent, TerminalSession, TerminalState, WriteOutcome,
};
pub use yatui_text::{TextMetrics, WidthPolicy, grapheme_width, graphemes, measure};
pub use yatui_ui::{Element, Invalidation, Key, NodeId, UiTree, WidgetKind};
