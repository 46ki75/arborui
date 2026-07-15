//! Deterministic headless application testing for `arborui`.
//!
//! [`TestApp`] drives the real runtime, retained UI tree, renderer, and
//! terminal transaction contract without selecting a concrete terminal backend.

mod app;
mod backend;
mod clock;
mod frame;

pub use app::{SettleOutcome, SettleReport, TestApp, TestError};
pub use backend::TestBackendError;
pub use frame::{TestCell, TestCellContent, TestFrame};

pub use arborui_core::{CursorState, Point, Size, Style};
pub use arborui_render::{FramePatch, HyperlinkId};
pub use arborui_runtime::{DispatchReport, EventProxy};
pub use arborui_terminal::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind, TerminalEvent,
};
pub use arborui_text::WidthPolicy;
pub use arborui_ui::{Key, NodeId, UiEvent};

#[cfg(test)]
mod tests;
