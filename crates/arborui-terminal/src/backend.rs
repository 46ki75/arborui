use std::{error::Error, time::Duration};

use arborui_core::Size;

use crate::{Capabilities, FramePatch, TerminalEvent, TerminalState};

/// Pixel dimensions of a terminal's drawable cell grid.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TerminalPixelSize {
    /// Drawable width in pixels.
    pub width: u16,
    /// Drawable height in pixels.
    pub height: u16,
}

impl TerminalPixelSize {
    /// Creates terminal pixel dimensions.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// Current terminal dimensions in cells and, when available, pixels.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TerminalViewport {
    /// Drawable dimensions in terminal cells.
    pub cells: Size,
    /// Drawable pixel dimensions reported by the terminal or PTY.
    pub pixels: Option<TerminalPixelSize>,
}

impl TerminalViewport {
    /// Creates a viewport without reported pixel dimensions.
    #[must_use]
    pub const fn from_cells(cells: Size) -> Self {
        Self {
            cells,
            pixels: None,
        }
    }

    /// Creates a viewport with reported pixel dimensions.
    #[must_use]
    pub const fn with_pixels(cells: Size, pixels: TerminalPixelSize) -> Self {
        Self {
            cells,
            pixels: Some(pixels),
        }
    }
}

impl From<Size> for TerminalViewport {
    fn from(cells: Size) -> Self {
        Self::from_cells(cells)
    }
}

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

    /// Returns the current viewport with optional drawable pixel dimensions.
    ///
    /// Backends that cannot report pixels retain the cell dimensions from
    /// [`size`](Self::size).
    fn viewport(&self) -> Result<TerminalViewport, Self::Error> {
        self.size().map(TerminalViewport::from_cells)
    }

    /// Returns detected or configured terminal capabilities.
    fn capabilities(&self) -> &Capabilities;

    /// Waits for one normalized input event until `timeout` expires.
    fn poll_event(&mut self, timeout: Duration) -> Result<Option<TerminalEvent>, Self::Error>;

    /// Reconciles active terminal modes with `desired`.
    fn apply_state(&mut self, desired: &TerminalState) -> Result<(), Self::Error>;

    /// Delivers a complete frame patch.
    ///
    /// An error means output may have been applied partially, so callers must
    /// treat physical screen state as unknown and force a full repaint.
    fn write_patch(&mut self, patch: &FramePatch) -> Result<WriteOutcome, Self::Error>;

    /// Restores terminal modes owned by this backend.
    fn restore(&mut self) -> Result<(), Self::Error>;
}
