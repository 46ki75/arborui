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

pub use arborui_core::{Point, Size};
pub use arborui_terminal::{
    KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, TerminalEvent,
};
pub use arborui_ui::{Key, NodeId, UiEvent};

#[cfg(test)]
mod tests;
