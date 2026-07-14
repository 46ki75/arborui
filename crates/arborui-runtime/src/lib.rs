//! Backend-neutral application updates, commands, scheduling, and terminal orchestration.

mod app;
mod clock;
mod command;
mod event;
mod proxy;
mod runner;
mod scheduler;

pub use app::{Application, UpdateContext};
pub use clock::{Clock, SystemClock};
pub use command::Command;
pub use event::translate_terminal_event;
pub use proxy::{EventProxy, EventProxySendError};
pub use runner::{
    AppRunner, DispatchReport, HeadlessRenderError, HeadlessRenderOutcome, ProcessReport,
    RuntimeError, TerminalRenderOutcome, run,
};
