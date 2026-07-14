use std::{error::Error, time::Duration};

use yatui_core::Size;

use crate::{Capabilities, FramePatch, TerminalEvent, TerminalState};

/// Result of attempting to deliver a complete frame patch.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WriteOutcome {
    /// The backend accepted the complete patch in order.
    Applied,
    /// The backend applied no bytes and asks the caller to retry or discard.
    Deferred,
    /// Some output may have been applied; the renderer must force a repaint.
    StateUnknown,
}

/// Backend-neutral terminal operations required by the application runtime.
///
/// Implementations that read a process-global terminal must permit only one
/// active event reader and document that limitation.
pub trait TerminalBackend: Send {
    /// Backend-specific error type.
    type Error: Error + Send + Sync + 'static;

    /// Returns the current viewport size.
    fn size(&self) -> Result<Size, Self::Error>;

    /// Returns detected or configured terminal capabilities.
    fn capabilities(&self) -> &Capabilities;

    /// Waits for one normalized input event until `timeout` expires.
    fn poll_event(&mut self, timeout: Duration) -> Result<Option<TerminalEvent>, Self::Error>;

    /// Reconciles active terminal modes with `desired`.
    fn apply_state(&mut self, desired: &TerminalState) -> Result<(), Self::Error>;

    /// Delivers a complete frame patch.
    fn write_patch(&mut self, patch: &FramePatch) -> Result<WriteOutcome, Self::Error>;

    /// Restores terminal modes owned by this backend.
    fn restore(&mut self) -> Result<(), Self::Error>;
}
