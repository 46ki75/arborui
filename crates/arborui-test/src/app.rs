use std::{error::Error, fmt, sync::Arc, time::Duration};

use arborui_core::{Point, Size};
use arborui_render::{FramePatch, Renderer};
use arborui_runtime::{
    AppRunner, Application, DispatchReport, EventProxy, RuntimeError, TerminalRenderOutcome,
};
use arborui_terminal::{
    Capabilities, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind, TerminalEvent, TerminalSession, TerminalState, WriteOutcome,
};
use arborui_text::WidthPolicy;
use arborui_ui::{Key, NodeId, ReconcileError, UiCommitError, UiError, UiEvent, UiTree};

use crate::{
    TestBackendError, TestFrame,
    backend::{MemoryBackend, ScriptedWrite},
    clock::ManualClock,
};

const MAX_SETTLE_TURNS: usize = 4_096;

/// Reason a deterministic settle operation stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettleOutcome {
    /// No immediately runnable or visual work remains.
    Settled,
    /// The application requested shutdown.
    Quitting,
    /// The next frame was rejected without applying output.
    Deferred,
    /// Output state became unknown and a full repaint is required.
    StateUnknown,
}

/// Aggregate work performed while settling a test application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettleReport {
    /// Number of scheduler/render turns.
    pub turns: usize,
    /// Number of serialized application updates.
    pub updates: usize,
    /// Number of command futures completed.
    pub completed_tasks: usize,
    /// Number of frames committed.
    pub committed_frames: usize,
    /// Reason settling stopped.
    pub outcome: SettleOutcome,
}

/// Failure while driving a headless application.
#[derive(Debug)]
pub enum TestError {
    /// A scripted terminal write failed.
    Backend(TestBackendError),
    /// UI preparation failed.
    Ui(UiError),
    /// Transactional UI commit failed.
    Commit(UiCommitError),
    /// Runtime execution and terminal restoration both failed.
    Restore {
        /// The failure that stopped runtime execution.
        error: Box<TestError>,
        /// The additional terminal restoration failure.
        restore_error: TestBackendError,
    },
    /// Event dispatch could not reconcile the current view.
    Reconcile(ReconcileError),
    /// Manual time exceeded [`Duration::MAX`].
    TimeOverflow,
    /// Immediate work did not settle within the safety limit.
    SettleLimit {
        /// Number of attempted scheduler/render turns.
        turns: usize,
    },
}

impl fmt::Display for TestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(error) => error.fmt(formatter),
            Self::Ui(error) => error.fmt(formatter),
            Self::Commit(error) => error.fmt(formatter),
            Self::Restore {
                error,
                restore_error,
            } => write!(
                formatter,
                "{error}; terminal restoration also failed: {restore_error}"
            ),
            Self::Reconcile(error) => error.fmt(formatter),
            Self::TimeOverflow => formatter.write_str("manual test clock overflowed"),
            Self::SettleLimit { turns } => {
                write!(formatter, "application did not settle within {turns} turns")
            }
        }
    }
}

impl Error for TestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Backend(error) => Some(error),
            Self::Ui(error) => Some(error),
            Self::Commit(error) => Some(error),
            Self::Restore { error, .. } => Some(error.as_ref()),
            Self::Reconcile(error) => Some(error),
            Self::TimeOverflow | Self::SettleLimit { .. } => None,
        }
    }
}

impl From<RuntimeError<TestBackendError>> for TestError {
    fn from(error: RuntimeError<TestBackendError>) -> Self {
        match error {
            RuntimeError::Backend(error) => Self::Backend(error),
            RuntimeError::Ui(error) => Self::Ui(error),
            RuntimeError::Commit(error) => Self::Commit(error),
            RuntimeError::Restore {
                error,
                restore_error,
            } => Self::Restore {
                error: Box::new(Self::from(*error)),
                restore_error,
            },
        }
    }
}

impl From<ReconcileError> for TestError {
    fn from(error: ReconcileError) -> Self {
        Self::Reconcile(error)
    }
}

/// Deterministic application-level harness with an in-memory terminal.
pub struct TestApp<A: Application> {
    runner: AppRunner<A>,
    terminal: TerminalSession<MemoryBackend>,
    clock: ManualClock,
}

impl<A: Application> TestApp<A> {
    /// Creates and initially renders an application using Unicode width rules.
    ///
    /// Panics with a test diagnostic if initial rendering fails. Use
    /// [`try_new`](Self::try_new) to inspect initialization errors.
    #[must_use]
    pub fn new(application: A, size: Size) -> Self {
        fail_test(Self::try_new(application, size))
    }

    /// Creates and initially renders an application with an explicit width policy.
    ///
    /// Panics with a test diagnostic if initial rendering fails. Use
    /// [`try_with_width_policy`](Self::try_with_width_policy) to inspect errors.
    #[must_use]
    pub fn with_width_policy(application: A, size: Size, width_policy: WidthPolicy) -> Self {
        fail_test(Self::try_with_width_policy(application, size, width_policy))
    }

    /// Fallible variant of [`new`](Self::new).
    pub fn try_new(application: A, size: Size) -> Result<Self, TestError> {
        Self::try_with_width_policy(application, size, WidthPolicy::Unicode)
    }

    /// Fallible variant of [`with_width_policy`](Self::with_width_policy).
    pub fn try_with_width_policy(
        application: A,
        size: Size,
        width_policy: WidthPolicy,
    ) -> Result<Self, TestError> {
        let clock = ManualClock::default();
        let clock_source: Arc<dyn arborui_runtime::Clock> = Arc::new(clock.clone());
        let runner = AppRunner::new_with_clock(
            application,
            size,
            Renderer::new(size, width_policy),
            clock_source,
        );
        let capabilities = Capabilities {
            width_policy,
            ..Capabilities::default()
        };
        let terminal = TerminalSession::open(
            MemoryBackend::new(size, capabilities),
            TerminalState::default(),
        )
        .map_err(TestError::Backend)?;
        let mut app = Self {
            runner,
            terminal,
            clock,
        };
        app.try_settle()?;
        Ok(app)
    }

    /// Returns the application model.
    #[must_use]
    pub const fn application(&self) -> &A {
        self.runner.application()
    }

    /// Returns mutable model access without implicitly invalidating the view.
    pub const fn application_mut(&mut self) -> &mut A {
        self.runner.application_mut()
    }

    /// Returns a sender for controlled external command completion.
    #[must_use]
    pub fn event_proxy(&self) -> EventProxy<A::Message> {
        self.runner.event_proxy()
    }

    /// Returns whether the application requested shutdown.
    #[must_use]
    pub const fn is_quitting(&self) -> bool {
        self.runner.is_quitting()
    }

    /// Returns elapsed deterministic test time.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.clock.elapsed()
    }

    /// Enqueues one application message and settles immediate work.
    pub fn send(&mut self, message: A::Message) -> SettleReport {
        fail_test(self.try_send(message))
    }

    /// Fallible variant of [`send`](Self::send).
    pub fn try_send(&mut self, message: A::Message) -> Result<SettleReport, TestError> {
        self.runner.enqueue(message);
        self.try_settle()
    }

    /// Dispatches one backend-neutral UI event and settles immediate work.
    pub fn event(&mut self, event: UiEvent) -> (DispatchReport, SettleReport) {
        fail_test(self.try_event(event))
    }

    /// Fallible variant of [`event`](Self::event).
    pub fn try_event(
        &mut self,
        event: UiEvent,
    ) -> Result<(DispatchReport, SettleReport), TestError> {
        if let UiEvent::Resize(size) = &event {
            self.terminal.backend_mut().set_size(*size);
        }
        let dispatch = self.runner.dispatch_ui_event(event)?;
        let settle = self.try_settle()?;
        Ok((dispatch, settle))
    }

    /// Dispatches one normalized terminal event and settles immediate work.
    pub fn terminal_event(&mut self, event: TerminalEvent) -> SettleReport {
        fail_test(self.try_terminal_event(event))
    }

    /// Fallible variant of [`terminal_event`](Self::terminal_event).
    pub fn try_terminal_event(&mut self, event: TerminalEvent) -> Result<SettleReport, TestError> {
        if let TerminalEvent::Resize(size) = &event {
            self.terminal.backend_mut().set_size(*size);
        }
        self.runner.dispatch_terminal_event(event)?;
        self.try_settle()
    }

    /// Presses one unmodified key and settles immediate work.
    pub fn key(&mut self, code: KeyCode) -> SettleReport {
        self.key_with(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    /// Injects one key phase with explicit modifiers and settles immediate work.
    pub fn key_with(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        kind: KeyEventKind,
    ) -> SettleReport {
        self.terminal_event(TerminalEvent::Key(KeyEvent {
            code,
            modifiers,
            kind,
            state: KeyEventState::default(),
        }))
    }

    /// Injects one normalized mouse event and settles immediate work.
    pub fn mouse(&mut self, event: MouseEvent) -> SettleReport {
        self.terminal_event(TerminalEvent::Mouse(event))
    }

    /// Presses and releases the primary pointer button at `point`.
    pub fn click(&mut self, point: Point) -> SettleReport {
        self.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: point,
            modifiers: KeyModifiers::NONE,
        });
        self.mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            position: point,
            modifiers: KeyModifiers::NONE,
        })
    }

    /// Injects a complete bracketed-paste payload.
    pub fn paste(&mut self, text: impl Into<String>) -> SettleReport {
        self.terminal_event(TerminalEvent::Paste(text.into()))
    }

    /// Changes the viewport and dispatches the matching resize event.
    pub fn resize(&mut self, size: Size) -> SettleReport {
        self.terminal_event(TerminalEvent::Resize(size))
    }

    /// Advances deterministic time and settles newly due timers.
    pub fn advance(&mut self, duration: Duration) -> SettleReport {
        fail_test(self.try_advance(duration))
    }

    /// Fallible variant of [`advance`](Self::advance).
    pub fn try_advance(&mut self, duration: Duration) -> Result<SettleReport, TestError> {
        if !self.clock.advance(duration) {
            return Err(TestError::TimeOverflow);
        }
        self.try_settle()
    }

    /// Drains immediate messages, ready commands, invalidation, and rendering.
    pub fn settle(&mut self) -> SettleReport {
        fail_test(self.try_settle())
    }

    /// Fallible variant of [`settle`](Self::settle).
    pub fn try_settle(&mut self) -> Result<SettleReport, TestError> {
        let mut report = SettleReport {
            turns: 0,
            updates: 0,
            completed_tasks: 0,
            committed_frames: 0,
            outcome: SettleOutcome::Settled,
        };

        for turn in 1..=MAX_SETTLE_TURNS {
            report.turns = turn;
            let process = self.runner.process_pending();
            report.updates = report.updates.saturating_add(process.updates);
            report.completed_tasks = report
                .completed_tasks
                .saturating_add(process.completed_tasks);
            if process.quitting {
                report.outcome = SettleOutcome::Quitting;
                return Ok(report);
            }

            match self.runner.render_terminal(&mut self.terminal)? {
                TerminalRenderOutcome::Idle => {}
                TerminalRenderOutcome::Applied => {
                    self.terminal.backend_mut().sync_committed_size();
                    report.committed_frames = report.committed_frames.saturating_add(1);
                }
                TerminalRenderOutcome::Deferred => {
                    report.outcome = SettleOutcome::Deferred;
                    return Ok(report);
                }
                TerminalRenderOutcome::StateUnknown => {
                    report.outcome = SettleOutcome::StateUnknown;
                    return Ok(report);
                }
            }

            if !process.budget_exhausted && self.runner.is_visually_idle() {
                return Ok(report);
            }
        }

        Err(TestError::SettleLimit {
            turns: MAX_SETTLE_TURNS,
        })
    }

    /// Makes the next non-empty frame write apply no bytes.
    pub fn defer_next_output(&mut self) {
        self.terminal
            .backend_mut()
            .script(ScriptedWrite::Outcome(WriteOutcome::Deferred));
    }

    /// Makes the next non-empty frame write report unknown physical state.
    pub fn make_next_output_unknown(&mut self) {
        self.terminal
            .backend_mut()
            .script(ScriptedWrite::Outcome(WriteOutcome::StateUnknown));
    }

    /// Makes the next non-empty frame write return a backend error.
    pub fn fail_next_output(&mut self) {
        self.terminal.backend_mut().script(ScriptedWrite::Fail);
    }

    /// Returns the currently committed resolved frame.
    #[must_use]
    pub fn frame(&self) -> &TestFrame {
        self.terminal.backend().frame()
    }

    /// Returns every non-empty patch submitted to the in-memory terminal.
    #[must_use]
    pub fn frame_patches(&self) -> &[FramePatch] {
        self.terminal.backend().patches()
    }

    /// Returns the most recently submitted non-empty patch.
    #[must_use]
    pub fn last_frame_patch(&self) -> Option<&FramePatch> {
        self.frame_patches().last()
    }

    /// Returns the retained UI tree.
    #[must_use]
    pub const fn ui_tree(&self) -> &UiTree {
        self.runner.ui_tree()
    }

    /// Returns the focused retained node.
    #[must_use]
    pub fn focused_node(&self) -> Option<NodeId> {
        self.runner.ui_tree().focused()
    }

    /// Returns the explicit key of the focused node.
    #[must_use]
    pub fn focused_key(&self) -> Option<Key> {
        let tree = self.runner.ui_tree();
        tree.focused()
            .and_then(|node| tree.node(node))
            .and_then(|node| node.key())
            .cloned()
    }

    /// Returns the interactive node at `point` in the committed hit map.
    #[must_use]
    pub fn hit_at(&self, point: Point) -> Option<NodeId> {
        self.runner
            .ui_tree()
            .hit_test(self.runner.renderer().hit_map(), point)
    }
}

fn fail_test<T>(result: Result<T, TestError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("arborui test application failed: {error}"),
    }
}
