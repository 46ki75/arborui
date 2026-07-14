use std::time::Duration;

use yatui_core::Size;

use crate::{
    Capabilities, FramePatch, TerminalBackend, TerminalEvent, TerminalState, WriteOutcome,
};

/// RAII owner for a configured terminal backend.
///
/// Explicit [`restore`](Self::restore) reports errors. `Drop` performs the same
/// operation on a best-effort basis.
pub struct TerminalSession<B: TerminalBackend> {
    backend: B,
    desired: TerminalState,
    suspended: bool,
    full_repaint_required: bool,
}

impl<B: TerminalBackend> TerminalSession<B> {
    /// Applies `desired` and takes ownership of the backend.
    pub fn open(mut backend: B, desired: TerminalState) -> Result<Self, B::Error> {
        if let Err(error) = backend.apply_state(&desired) {
            let _ = backend.restore();
            return Err(error);
        }
        Ok(Self {
            backend,
            desired,
            suspended: false,
            full_repaint_required: true,
        })
    }

    /// Returns terminal capabilities.
    #[must_use]
    pub fn capabilities(&self) -> &Capabilities {
        self.backend.capabilities()
    }

    /// Returns the current viewport size.
    pub fn size(&self) -> Result<Size, B::Error> {
        self.backend.size()
    }

    /// Returns the desired state restored after resume.
    #[must_use]
    pub const fn desired_state(&self) -> &TerminalState {
        &self.desired
    }

    /// Applies and records new desired terminal state.
    pub fn apply_state(&mut self, desired: TerminalState) -> Result<(), B::Error> {
        if !self.suspended {
            self.backend.apply_state(&desired)?;
        }
        self.desired = desired;
        Ok(())
    }

    /// Polls one normalized terminal event.
    pub fn poll_event(&mut self, timeout: Duration) -> Result<Option<TerminalEvent>, B::Error> {
        if self.suspended {
            return Ok(None);
        }
        self.backend.poll_event(timeout)
    }

    /// Writes a frame patch through the backend.
    pub fn write_patch(&mut self, patch: &FramePatch) -> Result<WriteOutcome, B::Error> {
        if self.suspended {
            return Ok(WriteOutcome::Deferred);
        }
        self.backend.write_patch(patch)
    }

    /// Restores terminal modes temporarily for a child process or shell.
    pub fn suspend(&mut self) -> Result<(), B::Error> {
        if self.suspended {
            return Ok(());
        }
        self.suspended = true;
        self.full_repaint_required = true;
        self.backend.restore()
    }

    /// Reapplies desired state after suspension.
    pub fn resume(&mut self) -> Result<(), B::Error> {
        if !self.suspended {
            return Ok(());
        }
        self.backend.apply_state(&self.desired)?;
        self.suspended = false;
        self.full_repaint_required = true;
        Ok(())
    }

    /// Returns and clears the full-repaint requirement.
    pub fn take_full_repaint_required(&mut self) -> bool {
        std::mem::take(&mut self.full_repaint_required)
    }

    /// Explicitly restores the terminal.
    pub fn restore(&mut self) -> Result<(), B::Error> {
        self.suspended = true;
        self.full_repaint_required = true;
        self.backend.restore()
    }

    /// Returns a shared reference to the backend.
    #[must_use]
    pub const fn backend(&self) -> &B {
        &self.backend
    }

    /// Returns a mutable reference to the backend.
    pub const fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
}

impl<B: TerminalBackend> Drop for TerminalSession<B> {
    fn drop(&mut self) {
        let _ = self.backend.restore();
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, sync::Arc};

    use super::*;
    use crate::Capabilities;

    #[derive(Clone, Default)]
    struct Counts {
        applied: Arc<std::sync::atomic::AtomicUsize>,
        restored: Arc<std::sync::atomic::AtomicUsize>,
        writes: Arc<std::sync::atomic::AtomicUsize>,
    }

    struct MockBackend {
        capabilities: Capabilities,
        counts: Counts,
    }

    impl TerminalBackend for MockBackend {
        type Error = Infallible;

        fn size(&self) -> Result<Size, Self::Error> {
            Ok(Size::new(80, 24))
        }

        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }

        fn poll_event(&mut self, _timeout: Duration) -> Result<Option<TerminalEvent>, Self::Error> {
            Ok(None)
        }

        fn apply_state(&mut self, _desired: &TerminalState) -> Result<(), Self::Error> {
            self.counts
                .applied
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }

        fn write_patch(&mut self, _patch: &FramePatch) -> Result<WriteOutcome, Self::Error> {
            self.counts
                .writes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(WriteOutcome::Applied)
        }

        fn restore(&mut self) -> Result<(), Self::Error> {
            self.counts
                .restored
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn session_restores_suspend_resume_and_drop() -> Result<(), Infallible> {
        let counts = Counts::default();
        let backend = MockBackend {
            capabilities: Capabilities::default(),
            counts: counts.clone(),
        };
        {
            let mut session = TerminalSession::open(backend, TerminalState::fullscreen())?;
            assert!(session.take_full_repaint_required());
            session.suspend()?;
            session.apply_state(TerminalState::default())?;
            let patch = FramePatch {
                size: Size::ZERO,
                runs: Vec::new(),
                cursor: yatui_core::CursorState::default(),
                cursor_changed: false,
                full_repaint: false,
            };
            assert_eq!(session.write_patch(&patch)?, WriteOutcome::Deferred);
            session.resume()?;
            assert!(session.take_full_repaint_required());
        }

        assert_eq!(counts.applied.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert_eq!(
            counts.restored.load(std::sync::atomic::Ordering::Relaxed),
            2
        );
        assert_eq!(counts.writes.load(std::sync::atomic::Ordering::Relaxed), 0);
        Ok(())
    }
}
