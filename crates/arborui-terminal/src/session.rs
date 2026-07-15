use std::time::Duration;

use arborui_core::Size;

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
    cleanup_required: bool,
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
            cleanup_required: false,
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
            self.full_repaint_required = true;
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
        let outcome = self.backend.write_patch(patch);
        if matches!(outcome, Err(_) | Ok(WriteOutcome::StateUnknown)) {
            self.full_repaint_required = true;
        }
        outcome
    }

    /// Restores terminal modes temporarily for a child process or shell.
    pub fn suspend(&mut self) -> Result<(), B::Error> {
        if self.suspended {
            if self.cleanup_required {
                self.full_repaint_required = true;
                self.backend.restore()?;
                self.cleanup_required = false;
            }
            return Ok(());
        }
        self.full_repaint_required = true;
        self.backend.restore()?;
        self.suspended = true;
        self.cleanup_required = false;
        self.full_repaint_required = true;
        Ok(())
    }

    /// Reapplies desired state after suspension.
    pub fn resume(&mut self) -> Result<(), B::Error> {
        if !self.suspended {
            return Ok(());
        }
        if let Err(error) = self.backend.apply_state(&self.desired) {
            self.cleanup_required = self.backend.restore().is_err();
            self.full_repaint_required = true;
            return Err(error);
        }
        self.suspended = false;
        self.cleanup_required = false;
        self.full_repaint_required = true;
        Ok(())
    }

    /// Returns and clears the full-repaint requirement.
    pub fn take_full_repaint_required(&mut self) -> bool {
        std::mem::take(&mut self.full_repaint_required)
    }

    /// Explicitly restores the terminal.
    pub fn restore(&mut self) -> Result<(), B::Error> {
        if self.suspended && !self.cleanup_required {
            return Ok(());
        }
        self.full_repaint_required = true;
        self.backend.restore()?;
        self.suspended = true;
        self.cleanup_required = false;
        self.full_repaint_required = true;
        Ok(())
    }

    /// Returns whether terminal modes are currently restored.
    #[must_use]
    pub const fn is_suspended(&self) -> bool {
        self.suspended && !self.cleanup_required
    }

    /// Returns whether terminal I/O is currently available.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        !self.suspended
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
        if !self.suspended || self.cleanup_required {
            let _ = self.backend.restore();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        io,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

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
                cursor: arborui_core::CursorState::default(),
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

    #[derive(Clone, Default)]
    struct FailurePlan {
        applied: Arc<std::sync::atomic::AtomicUsize>,
        restored: Arc<std::sync::atomic::AtomicUsize>,
        writes: Arc<std::sync::atomic::AtomicUsize>,
        changed_screen_before_apply_failure: Arc<AtomicBool>,
        fail_apply_on: usize,
        fail_restore_on: usize,
        fail_write_on: usize,
        unknown_write_on: usize,
    }

    struct FailingBackend {
        capabilities: Capabilities,
        plan: FailurePlan,
    }

    impl TerminalBackend for FailingBackend {
        type Error = io::Error;

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
            let call = self
                .plan
                .applied
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            if call == self.plan.fail_apply_on {
                self.plan
                    .changed_screen_before_apply_failure
                    .store(true, Ordering::Relaxed);
                return Err(io::Error::other("injected apply failure"));
            }
            Ok(())
        }

        fn write_patch(&mut self, _patch: &FramePatch) -> Result<WriteOutcome, Self::Error> {
            let call = self
                .plan
                .writes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            if call == self.plan.fail_write_on {
                return Err(io::Error::other("injected write failure"));
            }
            if call == self.plan.unknown_write_on {
                return Ok(WriteOutcome::StateUnknown);
            }
            Ok(WriteOutcome::Applied)
        }

        fn restore(&mut self) -> Result<(), Self::Error> {
            let call = self
                .plan
                .restored
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            if call == self.plan.fail_restore_on {
                return Err(io::Error::other("injected restore failure"));
            }
            Ok(())
        }
    }

    fn failing_backend(plan: FailurePlan) -> FailingBackend {
        FailingBackend {
            capabilities: Capabilities::default(),
            plan,
        }
    }

    #[test]
    fn successful_active_apply_state_requires_full_repaint() -> io::Result<()> {
        let mut session = TerminalSession::open(
            failing_backend(FailurePlan::default()),
            TerminalState::fullscreen(),
        )?;
        assert!(session.take_full_repaint_required());

        session.apply_state(TerminalState::default())?;

        assert!(
            session.take_full_repaint_required(),
            "changing active terminal state can change the physical screen, so the next frame \
             must be a full repaint"
        );
        Ok(())
    }

    #[test]
    fn partially_failed_active_apply_state_requires_full_repaint() -> io::Result<()> {
        let plan = FailurePlan {
            fail_apply_on: 2,
            ..FailurePlan::default()
        };
        let mut session =
            TerminalSession::open(failing_backend(plan.clone()), TerminalState::fullscreen())?;
        assert!(session.take_full_repaint_required());

        assert!(session.apply_state(TerminalState::default()).is_err());
        assert!(
            plan.changed_screen_before_apply_failure
                .load(Ordering::Relaxed)
        );
        assert!(
            session.take_full_repaint_required(),
            "an apply-state error after a partial physical transition leaves screen state \
             unknown, so the next frame must be a full repaint"
        );
        Ok(())
    }

    #[test]
    fn failed_suspend_requires_full_repaint() -> io::Result<()> {
        let plan = FailurePlan {
            fail_restore_on: 1,
            ..FailurePlan::default()
        };
        let mut session =
            TerminalSession::open(failing_backend(plan), TerminalState::fullscreen())?;
        assert!(session.take_full_repaint_required());

        assert!(session.suspend().is_err());
        assert!(session.take_full_repaint_required());
        Ok(())
    }

    #[test]
    fn failed_explicit_restore_requires_full_repaint() -> io::Result<()> {
        let plan = FailurePlan {
            fail_restore_on: 1,
            ..FailurePlan::default()
        };
        let mut session =
            TerminalSession::open(failing_backend(plan), TerminalState::fullscreen())?;
        assert!(session.take_full_repaint_required());

        assert!(session.restore().is_err());
        assert!(session.take_full_repaint_required());
        Ok(())
    }

    #[test]
    fn failed_write_requires_full_repaint() -> io::Result<()> {
        let plan = FailurePlan {
            fail_write_on: 1,
            ..FailurePlan::default()
        };
        let mut session =
            TerminalSession::open(failing_backend(plan), TerminalState::fullscreen())?;
        assert!(session.take_full_repaint_required());

        let patch = FramePatch {
            size: Size::new(80, 24),
            runs: Vec::new(),
            cursor: arborui_core::CursorState::default(),
            cursor_changed: true,
            full_repaint: false,
        };
        assert!(session.write_patch(&patch).is_err());
        assert!(
            session.take_full_repaint_required(),
            "a failed patch write leaves physical screen state unknown, so the session \
             must require a full repaint before the next diff"
        );
        Ok(())
    }

    #[test]
    fn unknown_write_state_requires_full_repaint() -> io::Result<()> {
        let plan = FailurePlan {
            unknown_write_on: 1,
            ..FailurePlan::default()
        };
        let mut session =
            TerminalSession::open(failing_backend(plan), TerminalState::fullscreen())?;
        assert!(session.take_full_repaint_required());

        let patch = FramePatch {
            size: Size::new(80, 24),
            runs: Vec::new(),
            cursor: arborui_core::CursorState::default(),
            cursor_changed: true,
            full_repaint: false,
        };
        assert_eq!(session.write_patch(&patch)?, WriteOutcome::StateUnknown);
        assert!(session.take_full_repaint_required());
        Ok(())
    }

    #[test]
    fn open_failure_attempts_restoration() {
        let plan = FailurePlan {
            fail_apply_on: 1,
            ..FailurePlan::default()
        };

        assert!(
            TerminalSession::open(failing_backend(plan.clone()), TerminalState::fullscreen())
                .is_err()
        );
        assert_eq!(plan.restored.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn failed_resume_restores_partial_state() -> io::Result<()> {
        let plan = FailurePlan {
            fail_apply_on: 2,
            ..FailurePlan::default()
        };
        let mut session =
            TerminalSession::open(failing_backend(plan.clone()), TerminalState::fullscreen())?;
        session.suspend()?;

        assert!(session.resume().is_err());
        assert!(session.is_suspended());
        assert!(session.take_full_repaint_required());
        drop(session);

        assert_eq!(plan.restored.load(std::sync::atomic::Ordering::Relaxed), 2);
        Ok(())
    }

    #[test]
    fn drop_retries_cleanup_after_failed_resume_and_restore() -> io::Result<()> {
        let plan = FailurePlan {
            fail_apply_on: 2,
            fail_restore_on: 2,
            ..FailurePlan::default()
        };
        let mut session =
            TerminalSession::open(failing_backend(plan.clone()), TerminalState::fullscreen())?;
        session.suspend()?;

        assert!(session.resume().is_err());
        assert!(!session.is_suspended());
        drop(session);

        assert_eq!(plan.restored.load(std::sync::atomic::Ordering::Relaxed), 3);
        Ok(())
    }

    #[test]
    fn resume_retries_activation_after_failed_cleanup() -> io::Result<()> {
        let plan = FailurePlan {
            fail_apply_on: 2,
            fail_restore_on: 2,
            ..FailurePlan::default()
        };
        let mut session =
            TerminalSession::open(failing_backend(plan.clone()), TerminalState::fullscreen())?;
        session.suspend()?;

        assert!(session.resume().is_err());
        assert!(!session.is_suspended());
        session.resume()?;
        drop(session);

        assert_eq!(plan.applied.load(std::sync::atomic::Ordering::Relaxed), 3);
        assert_eq!(plan.restored.load(std::sync::atomic::Ordering::Relaxed), 3);
        Ok(())
    }

    #[test]
    fn drop_retries_failed_explicit_restore() -> io::Result<()> {
        let plan = FailurePlan {
            fail_restore_on: 1,
            ..FailurePlan::default()
        };
        let mut session =
            TerminalSession::open(failing_backend(plan.clone()), TerminalState::fullscreen())?;

        assert!(session.restore().is_err());
        assert!(!session.is_suspended());
        drop(session);

        assert_eq!(plan.restored.load(std::sync::atomic::Ordering::Relaxed), 2);
        Ok(())
    }
}
