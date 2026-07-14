//! Rust-native terminal user interface primitives.
//!
//! The facade currently exposes foundational [`core`], Unicode [`text`],
//! cell-based [`render`], layout, retained [`ui`], backend-neutral
//! [`terminal`], application [`runtime`], and controlled [`widgets`] APIs.
//!
//! The default `crossterm` feature exports `CrosstermBackend`. Disable
//! default features when providing another terminal backend.

/// Foundational geometry, color, style, and cursor types.
pub use arborui_core as core;
/// Backend-independent flex layout in terminal-cell coordinates.
pub use arborui_layout as layout;
/// Grapheme-aware cell buffers, composition, and transactional frame diffing.
pub use arborui_render as render;
/// Serialized application updates, commands, scheduling, and terminal orchestration.
pub use arborui_runtime as runtime;
/// Backend-neutral terminal events, capabilities, state, and lifecycle.
pub use arborui_terminal as terminal;
/// Unicode grapheme segmentation and terminal width measurement.
pub use arborui_text as text;
/// Borrowed declarative elements and retained UI identity.
pub use arborui_ui as ui;
/// Standard backend-independent controlled widgets.
pub use arborui_widgets as widgets;

#[cfg(feature = "crossterm")]
pub use arborui_backend_crossterm::CrosstermBackend;

pub use arborui_core::{
    Color, CursorShape, CursorState, CursorVisibility, Insets, Modifier, Point, Rect, Size, Style,
};
pub use arborui_layout::{Dimension, LayoutStyle, Position};
pub use arborui_render::{
    Buffer, Canvas, CommitError, FramePatch, HitId, HitMap, PreparedFrame, Renderer,
};
pub use arborui_runtime::{
    AppRunner, Application, Clock, Command, DispatchReport, EventProxy, EventProxySendError,
    HeadlessRenderError, HeadlessRenderOutcome, ProcessReport, RuntimeError, SystemClock,
    TerminalRenderOutcome, UpdateContext, run, translate_terminal_event,
};
pub use arborui_terminal::{
    Capabilities, TerminalBackend, TerminalEvent, TerminalSession, TerminalState, WriteOutcome,
};
pub use arborui_text::{
    ByteOffset, Selection, TextBuffer, TextEdit, TextMetrics, TextMovement, WidthPolicy,
    grapheme_width, graphemes, measure,
};
pub use arborui_ui::{
    DispatchOutcome, Element, EventContext, EventPhase, FocusChange, FocusError, Invalidation, Key,
    KeyAction, KeyModifiers, NodeId, PointerButton, PointerEvent, PointerEventKind,
    PreparedUiFrame, UiCommitError, UiEvent, UiKey, UiKeyEvent, UiTree, WidgetKind,
};
pub use arborui_widgets::{
    Block, BorderSet, Button, ScrollView, TextInput, button, column, column_with_gap,
    flexible_spacer, list, list_with_gap, row, row_with_gap, scroll_view, spacer,
    spacer_with_dimensions, stack, text_input,
};

/// Common application, layout, and widget APIs.
pub mod prelude {
    pub use crate::layout::{Align, FlexDirection, Justify, Position};
    pub use crate::widgets::text;
    pub use crate::{
        Application, Block, Button, Color, Command, Dimension, Element, Insets, Invalidation, Key,
        LayoutStyle, Point, ScrollView, Size, Style, TextBuffer, TextInput, UpdateContext, button,
        column, column_with_gap, flexible_spacer, list, list_with_gap, row, row_with_gap,
        scroll_view, spacer, spacer_with_dimensions, stack, text_input,
    };
}
