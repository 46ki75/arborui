//! Backend-neutral terminal events, capabilities, state, and lifecycle.

mod backend;
mod capabilities;
mod event;
mod session;
mod state;

pub use backend::{TerminalBackend, WriteOutcome};
pub use capabilities::{Capabilities, ColorCapability, KeyboardCapability, MouseCapability};
pub use event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind, TerminalEvent,
};
pub use session::TerminalSession;
pub use state::{AutowrapMode, KeyboardMode, MouseMode, ScreenMode, TerminalState};
pub use yatui_render::FramePatch;
