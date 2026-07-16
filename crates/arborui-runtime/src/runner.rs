use std::{
    collections::VecDeque,
    fmt,
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};

use arborui_core::Size;
use arborui_render::Renderer;
use arborui_terminal::{
    TerminalBackend, TerminalEvent, TerminalSession, TerminalState, WriteOutcome,
};
use arborui_ui::{
    Invalidation, ReconcileError, UiCommitError, UiError, UiEvent, UiPreparationTimings, UiTree,
};

use crate::{
    Application, Clock, Command, SystemClock, UpdateContext,
    command::CommandAction,
    proxy::{EventProxy, EventReceiver, event_channel},
    scheduler::{Scheduler, WakeSignal},
    translate_terminal_event,
};

const MAX_WORK_PER_TURN: usize = 1_024;
const MAX_MESSAGES_PER_TURN: usize = 768;
const DEFAULT_EVENT_INGRESS_CAPACITY: usize = 1_024;

/// Configuration for application runtime construction.
///
/// External proxy ingress defaults to a capacity of 1,024 messages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeOptions {
    event_ingress_capacity: NonZeroUsize,
}

impl RuntimeOptions {
    /// Creates the default bounded runtime configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of external proxy messages waiting for updates.
    #[must_use]
    pub const fn with_event_ingress_capacity(mut self, capacity: NonZeroUsize) -> Self {
        self.event_ingress_capacity = capacity;
        self
    }

    /// Returns the external proxy ingress capacity.
    #[must_use]
    pub const fn event_ingress_capacity(&self) -> NonZeroUsize {
        self.event_ingress_capacity
    }
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            event_ingress_capacity: NonZeroUsize::new(DEFAULT_EVENT_INGRESS_CAPACITY)
                .unwrap_or(NonZeroUsize::MIN),
        }
    }
}

/// Summary of messages and tasks processed in one drain operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessReport {
    /// Number of serialized application updates performed.
    pub updates: usize,
    /// Number of command futures completed.
    pub completed_tasks: usize,
    /// Coalesced visual work still requested.
    pub invalidation: Invalidation,
    /// Whether shutdown was requested.
    pub quitting: bool,
    /// Whether the fairness budget was exhausted and immediate work may remain.
    pub budget_exhausted: bool,
}

/// Non-message details from one UI event dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchReport {
    /// Number of application messages emitted by handlers.
    pub messages: usize,
    /// Whether a handler marked the event handled.
    pub handled: bool,
    /// Whether a handler prevented default UI behavior.
    pub default_prevented: bool,
    /// Whether event propagation was stopped.
    pub propagation_stopped: bool,
}

/// Result of a requested headless render.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeadlessRenderOutcome {
    /// No invalidation was pending, so no view was built.
    Idle,
    /// A prepared UI frame was committed.
    Committed,
}

/// Failure while preparing or committing a headless frame.
#[derive(Debug)]
pub enum HeadlessRenderError {
    /// UI preparation failed.
    Ui(UiError),
    /// Transactional UI and renderer commit failed.
    Commit(UiCommitError),
}

impl fmt::Display for HeadlessRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui(error) => error.fmt(formatter),
            Self::Commit(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for HeadlessRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Ui(error) => Some(error),
            Self::Commit(error) => Some(error),
        }
    }
}

/// Result of attempting to render through a terminal session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalRenderOutcome {
    /// No invalidation was pending, so no frame was prepared.
    Idle,
    /// The frame was accepted and committed.
    Applied,
    /// The backend accepted no bytes; the prepared frame was discarded.
    Deferred,
    /// Output may be partial; the frame was discarded and full repaint was forced.
    StateUnknown,
}

/// Durations recorded for one opt-in application render attempt.
///
/// The selected phases do not necessarily sum exactly to [`Self::total`],
/// which also includes orchestration and timing overhead.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderTimings {
    /// Complete non-idle render attempt.
    pub total: Duration,
    /// Application view construction before preparation and after commit.
    pub view_construction: Duration,
    /// Retained-state staging and reconciliation.
    pub staging_reconciliation: Duration,
    /// Layout construction, computation, geometry assignment, and cursor resolution.
    pub layout: Duration,
    /// Logical frame allocation and painting.
    pub paint: Duration,
    /// Terminal-independent frame comparison and patch construction.
    pub diff: Duration,
    /// Backend validation, serialization, writer calls, and flush.
    ///
    /// This is `None` for headless rendering and empty patches.
    pub terminal_serialization_and_write: Option<Duration>,
    /// Transactional retained-tree and renderer commit.
    ///
    /// This is `None` when a prepared frame was discarded.
    pub commit: Option<Duration>,
    /// Focus and hover refresh performed after a successful commit.
    pub post_commit: Option<Duration>,
}

/// An ordinary render outcome accompanied by opt-in phase timings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimedRender<Outcome> {
    /// Ordinary render outcome.
    pub outcome: Outcome,
    /// Recorded phases, or `None` when the render was visually idle.
    pub timings: Option<RenderTimings>,
}

/// Failure from the UI pipeline or terminal backend.
#[derive(Debug)]
pub enum RuntimeError<BackendError> {
    /// A terminal operation failed.
    Backend(BackendError),
    /// UI preparation failed.
    Ui(UiError),
    /// Transactional UI and renderer commit failed.
    Commit(UiCommitError),
    /// Application execution and subsequent terminal restoration both failed.
    Restore {
        /// The failure that stopped application execution.
        error: Box<RuntimeError<BackendError>>,
        /// The additional failure while restoring terminal state.
        restore_error: BackendError,
    },
}

impl<BackendError: fmt::Display> fmt::Display for RuntimeError<BackendError> {
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
        }
    }
}

impl<BackendError: std::error::Error + 'static> std::error::Error for RuntimeError<BackendError> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(error) => Some(error),
            Self::Ui(error) => Some(error),
            Self::Commit(error) => Some(error),
            Self::Restore { error, .. } => Some(error.as_ref()),
        }
    }
}

/// Deterministic owner of an application, message scheduler, UI tree, and renderer.
pub struct AppRunner<A: Application> {
    application: A,
    messages: VecDeque<A::Message>,
    terminal_events: VecDeque<TerminalEvent>,
    receiver: EventReceiver<A::Message>,
    proxy: EventProxy<A::Message>,
    wake: Arc<WakeSignal>,
    scheduler: Scheduler<A::Message>,
    ui: UiTree,
    renderer: Renderer,
    viewport: Size,
    invalidation: Invalidation,
    quitting: bool,
    prefer_external: bool,
}

impl<A: Application> AppRunner<A> {
    /// Creates a headless runner with an explicitly supplied renderer.
    #[must_use]
    pub fn new(application: A, viewport: Size, renderer: Renderer) -> Self {
        Self::new_with_options(application, viewport, renderer, RuntimeOptions::default())
    }

    /// Creates a headless runner with explicit runtime configuration.
    #[must_use]
    pub fn new_with_options(
        application: A,
        viewport: Size,
        renderer: Renderer,
        options: RuntimeOptions,
    ) -> Self {
        Self::new_with_clock_and_options(
            application,
            viewport,
            renderer,
            Arc::new(SystemClock::new()),
            options,
        )
    }

    /// Creates a headless runner with an explicit renderer and monotonic clock.
    #[must_use]
    pub fn new_with_clock(
        application: A,
        viewport: Size,
        renderer: Renderer,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self::new_with_clock_and_options(
            application,
            viewport,
            renderer,
            clock,
            RuntimeOptions::default(),
        )
    }

    /// Creates a headless runner with explicit clock and runtime configuration.
    #[must_use]
    pub fn new_with_clock_and_options(
        application: A,
        viewport: Size,
        renderer: Renderer,
        clock: Arc<dyn Clock>,
        options: RuntimeOptions,
    ) -> Self {
        let wake = Arc::new(WakeSignal::new());
        let (proxy, receiver) =
            event_channel(options.event_ingress_capacity().get(), Arc::clone(&wake));
        let scheduler = Scheduler::new(Arc::clone(&wake), clock);
        Self {
            application,
            messages: VecDeque::new(),
            terminal_events: VecDeque::new(),
            receiver,
            proxy,
            wake,
            scheduler,
            ui: UiTree::new(),
            renderer,
            viewport,
            invalidation: Invalidation::Recompose,
            quitting: false,
            prefer_external: false,
        }
    }

    /// Creates a runner using a terminal session's size and width policy.
    pub fn from_terminal<B: TerminalBackend>(
        application: A,
        session: &TerminalSession<B>,
    ) -> Result<Self, B::Error> {
        Self::from_terminal_with_options(application, session, RuntimeOptions::default())
    }

    /// Creates a configured runner using a terminal session's rendering policy.
    pub fn from_terminal_with_options<B: TerminalBackend>(
        application: A,
        session: &TerminalSession<B>,
        options: RuntimeOptions,
    ) -> Result<Self, B::Error> {
        let viewport = session.size()?;
        let renderer = Renderer::new(viewport, session.capabilities().width_policy);
        Ok(Self::new_with_options(
            application,
            viewport,
            renderer,
            options,
        ))
    }

    /// Returns a sender suitable for external threads and executors.
    #[must_use]
    pub fn event_proxy(&self) -> EventProxy<A::Message> {
        self.proxy.clone()
    }

    /// Enqueues a message for the next serialized drain.
    pub fn enqueue(&mut self, message: A::Message) {
        self.messages.push_back(message);
        self.wake.notify();
    }

    /// Requests visual work without applying a message.
    pub fn invalidate(&mut self, invalidation: Invalidation) {
        self.invalidation.request(invalidation);
    }

    /// Returns the currently coalesced visual request.
    #[must_use]
    pub const fn pending_invalidation(&self) -> Invalidation {
        self.invalidation
    }

    /// Returns whether orderly shutdown has been requested.
    #[must_use]
    pub const fn is_quitting(&self) -> bool {
        self.quitting
    }

    /// Returns whether no messages, tasks, or visual work are pending.
    #[must_use]
    pub fn is_idle(&mut self) -> bool {
        self.messages.is_empty()
            && self.receiver.is_empty()
            && !self.scheduler.has_tasks()
            && self.invalidation == Invalidation::None
    }

    /// Returns whether no immediately runnable or visual work remains.
    ///
    /// Unlike [`is_idle`](Self::is_idle), this treats dormant futures and
    /// timers with future deadlines as visually idle.
    #[must_use]
    pub fn is_visually_idle(&mut self) -> bool {
        self.messages.is_empty()
            && self.receiver.is_empty()
            && !self.scheduler.has_ready_work()
            && self.invalidation == Invalidation::None
    }

    /// Returns the application model.
    #[must_use]
    pub const fn application(&self) -> &A {
        &self.application
    }

    /// Returns mutable access to the model without implicitly invalidating it.
    pub const fn application_mut(&mut self) -> &mut A {
        &mut self.application
    }

    /// Consumes the runner and returns its application model.
    #[must_use]
    pub fn into_application(self) -> A {
        self.application
    }

    /// Returns the retained UI tree.
    #[must_use]
    pub const fn ui_tree(&self) -> &UiTree {
        &self.ui
    }

    /// Returns the committed renderer.
    #[must_use]
    pub const fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// Processes immediately ready work up to one fairness budget.
    pub fn process_pending(&mut self) -> ProcessReport {
        let mut updates = 0;
        let mut completed_tasks = 0;
        let mut work = 0;
        loop {
            let mut progressed = false;

            while !self.quitting && work < MAX_WORK_PER_TURN && updates < MAX_MESSAGES_PER_TURN {
                let Some(message) = self.next_message() else {
                    break;
                };
                progressed = true;
                updates += 1;
                work += 1;
                let mut context = UpdateContext::new(self.proxy.clone());
                let command = self.application.update(message, &mut context);
                self.invalidation.request(context.requested_invalidation());
                self.execute(command);
            }
            if self.quitting || work >= MAX_WORK_PER_TURN {
                break;
            }

            let mut completed = Vec::new();
            let poll_limit = MAX_WORK_PER_TURN.saturating_sub(work).min(256);
            let polled = self.scheduler.poll_ready(&mut completed, poll_limit);
            completed_tasks += polled.completed;
            work = work.saturating_add(polled.polled);
            progressed |= polled.polled != 0;
            self.messages.extend(completed);
            if !progressed
                || self.quitting
                || work >= MAX_WORK_PER_TURN
                || updates >= MAX_MESSAGES_PER_TURN
            {
                break;
            }
        }
        ProcessReport {
            updates,
            completed_tasks,
            invalidation: self.invalidation,
            quitting: self.quitting,
            budget_exhausted: work >= MAX_WORK_PER_TURN || updates >= MAX_MESSAGES_PER_TURN,
        }
    }

    /// Waits up to `timeout` for proxy or future activity, then drains ready work.
    pub fn wait_for_work(&mut self, timeout: Duration) -> ProcessReport {
        let report = self.process_pending();
        if report.updates != 0
            || report.completed_tasks != 0
            || report.invalidation != Invalidation::None
            || report.quitting
        {
            return report;
        }
        self.wake.wait(self.scheduler.wait_timeout(timeout));
        self.process_pending()
    }

    /// Routes one UI event and enqueues messages emitted by its handlers.
    pub fn dispatch_ui_event(&mut self, event: UiEvent) -> Result<DispatchReport, ReconcileError> {
        if let UiEvent::Resize(size) = &event {
            self.viewport = *size;
            self.invalidation.request(Invalidation::Layout);
        }
        let view = self.application.view();
        let outcome = self.ui.dispatch(&view, &event, &self.renderer)?;
        self.invalidation.request(self.ui.pending_invalidation());
        let report = DispatchReport {
            messages: outcome.messages.len(),
            handled: outcome.handled,
            default_prevented: outcome.default_prevented,
            propagation_stopped: outcome.propagation_stopped,
        };
        self.messages.extend(outcome.messages);
        Ok(report)
    }

    /// Translates and routes one terminal event.
    pub fn dispatch_terminal_event(
        &mut self,
        event: TerminalEvent,
    ) -> Result<Option<DispatchReport>, ReconcileError> {
        translate_terminal_event(event)
            .map(|event| self.dispatch_ui_event(event))
            .transpose()
    }

    /// Prepares and commits one frame without terminal output.
    pub fn render_headless(&mut self) -> Result<HeadlessRenderOutcome, HeadlessRenderError> {
        if self.invalidation == Invalidation::None {
            return Ok(HeadlessRenderOutcome::Idle);
        }
        let prepared = {
            let view = self.application.view();
            self.ui
                .prepare(&view, self.viewport, &mut self.renderer)
                .map_err(HeadlessRenderError::Ui)?
        };
        self.ui
            .commit(prepared, &mut self.renderer)
            .map_err(HeadlessRenderError::Commit)?;
        self.invalidation = Invalidation::None;
        self.refresh_after_commit()
            .map_err(HeadlessRenderError::Ui)?;
        Ok(HeadlessRenderOutcome::Committed)
    }

    /// Prepares and commits one headless frame while recording phase durations.
    pub fn render_headless_timed(
        &mut self,
    ) -> Result<TimedRender<HeadlessRenderOutcome>, HeadlessRenderError> {
        if self.invalidation == Invalidation::None {
            return Ok(TimedRender {
                outcome: HeadlessRenderOutcome::Idle,
                timings: None,
            });
        }
        let total_started = Instant::now();
        let (prepared, preparation, mut view_construction) = {
            let view_started = Instant::now();
            let view = self.application.view();
            let view_construction = view_started.elapsed();
            let (prepared, preparation) = self
                .ui
                .prepare_timed(&view, self.viewport, &mut self.renderer)
                .map_err(HeadlessRenderError::Ui)?;
            (prepared, preparation, view_construction)
        };
        let commit_started = Instant::now();
        self.ui
            .commit(prepared, &mut self.renderer)
            .map_err(HeadlessRenderError::Commit)?;
        let commit = commit_started.elapsed();
        self.invalidation = Invalidation::None;
        let (refresh_view, post_commit) = self
            .refresh_after_commit_timed()
            .map_err(HeadlessRenderError::Ui)?;
        view_construction = view_construction.saturating_add(refresh_view);
        let timings = render_timings(
            total_started.elapsed(),
            view_construction,
            preparation,
            None,
            Some(commit),
            Some(post_commit),
        );
        Ok(TimedRender {
            outcome: HeadlessRenderOutcome::Committed,
            timings: Some(timings),
        })
    }

    /// Attempts one transactional frame write through a terminal session.
    pub fn render_terminal<B: TerminalBackend>(
        &mut self,
        session: &mut TerminalSession<B>,
    ) -> Result<TerminalRenderOutcome, RuntimeError<B::Error>> {
        let width_policy = session.capabilities().width_policy;
        if self.renderer.width_policy() != width_policy {
            self.renderer.set_width_policy(width_policy);
            self.invalidation.request(Invalidation::Layout);
        }
        if session.take_full_repaint_required() {
            self.renderer.invalidate();
            self.invalidation.request(Invalidation::Paint);
        }
        let viewport = session.size().map_err(RuntimeError::Backend)?;
        if viewport != self.viewport {
            self.viewport = viewport;
            self.invalidation.request(Invalidation::Layout);
        }
        if self.invalidation == Invalidation::None {
            return Ok(TerminalRenderOutcome::Idle);
        }

        let prepared = {
            let view = self.application.view();
            self.ui
                .prepare(&view, self.viewport, &mut self.renderer)
                .map_err(RuntimeError::Ui)?
        };
        let outcome = if prepared.patch().is_empty() {
            WriteOutcome::Applied
        } else {
            match session.write_patch(prepared.patch()) {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.ui.discard(prepared, &mut self.renderer);
                    self.renderer.invalidate();
                    return Err(RuntimeError::Backend(error));
                }
            }
        };

        match outcome {
            WriteOutcome::Applied => {
                if let Err(error) = self.ui.commit(prepared, &mut self.renderer) {
                    self.renderer.invalidate();
                    self.invalidation.request(Invalidation::Paint);
                    return Err(RuntimeError::Commit(error));
                }
                self.invalidation = Invalidation::None;
                self.refresh_after_commit().map_err(RuntimeError::Ui)?;
                Ok(TerminalRenderOutcome::Applied)
            }
            WriteOutcome::Deferred => {
                self.ui.discard(prepared, &mut self.renderer);
                Ok(TerminalRenderOutcome::Deferred)
            }
            WriteOutcome::StateUnknown => {
                self.ui.discard(prepared, &mut self.renderer);
                self.renderer.invalidate();
                self.invalidation.request(Invalidation::Paint);
                Ok(TerminalRenderOutcome::StateUnknown)
            }
        }
    }

    /// Attempts one transactional terminal write while recording phase durations.
    pub fn render_terminal_timed<B: TerminalBackend>(
        &mut self,
        session: &mut TerminalSession<B>,
    ) -> Result<TimedRender<TerminalRenderOutcome>, RuntimeError<B::Error>> {
        let total_started = Instant::now();
        let width_policy = session.capabilities().width_policy;
        if self.renderer.width_policy() != width_policy {
            self.renderer.set_width_policy(width_policy);
            self.invalidation.request(Invalidation::Layout);
        }
        if session.take_full_repaint_required() {
            self.renderer.invalidate();
            self.invalidation.request(Invalidation::Paint);
        }
        let viewport = session.size().map_err(RuntimeError::Backend)?;
        if viewport != self.viewport {
            self.viewport = viewport;
            self.invalidation.request(Invalidation::Layout);
        }
        if self.invalidation == Invalidation::None {
            return Ok(TimedRender {
                outcome: TerminalRenderOutcome::Idle,
                timings: None,
            });
        }

        let (prepared, preparation, mut view_construction) = {
            let view_started = Instant::now();
            let view = self.application.view();
            let view_construction = view_started.elapsed();
            let (prepared, preparation) = self
                .ui
                .prepare_timed(&view, self.viewport, &mut self.renderer)
                .map_err(RuntimeError::Ui)?;
            (prepared, preparation, view_construction)
        };
        let (outcome, terminal_serialization_and_write) = if prepared.patch().is_empty() {
            (WriteOutcome::Applied, None)
        } else {
            let write_started = Instant::now();
            match session.write_patch(prepared.patch()) {
                Ok(outcome) => (outcome, Some(write_started.elapsed())),
                Err(error) => {
                    self.ui.discard(prepared, &mut self.renderer);
                    self.renderer.invalidate();
                    return Err(RuntimeError::Backend(error));
                }
            }
        };

        let (outcome, commit, post_commit) = match outcome {
            WriteOutcome::Applied => {
                let commit_started = Instant::now();
                if let Err(error) = self.ui.commit(prepared, &mut self.renderer) {
                    self.renderer.invalidate();
                    self.invalidation.request(Invalidation::Paint);
                    return Err(RuntimeError::Commit(error));
                }
                let commit = commit_started.elapsed();
                self.invalidation = Invalidation::None;
                let (refresh_view, post_commit) = self
                    .refresh_after_commit_timed()
                    .map_err(RuntimeError::Ui)?;
                view_construction = view_construction.saturating_add(refresh_view);
                (
                    TerminalRenderOutcome::Applied,
                    Some(commit),
                    Some(post_commit),
                )
            }
            WriteOutcome::Deferred => {
                self.ui.discard(prepared, &mut self.renderer);
                (TerminalRenderOutcome::Deferred, None, None)
            }
            WriteOutcome::StateUnknown => {
                self.ui.discard(prepared, &mut self.renderer);
                self.renderer.invalidate();
                self.invalidation.request(Invalidation::Paint);
                (TerminalRenderOutcome::StateUnknown, None, None)
            }
        };
        let timings = render_timings(
            total_started.elapsed(),
            view_construction,
            preparation,
            terminal_serialization_and_write,
            commit,
            post_commit,
        );
        Ok(TimedRender {
            outcome,
            timings: Some(timings),
        })
    }

    /// Runs terminal polling, serialized updates, and rendering until quit.
    pub fn run_terminal<B: TerminalBackend>(
        &mut self,
        session: &mut TerminalSession<B>,
        poll_interval: Duration,
    ) -> Result<(), RuntimeError<B::Error>> {
        while !self.quitting {
            let process = self.process_pending();
            if self.quitting {
                break;
            }
            match self.render_terminal(session)? {
                TerminalRenderOutcome::Deferred | TerminalRenderOutcome::StateUnknown => {
                    if !session.is_active() {
                        self.wake.wait(self.scheduler.wait_timeout(poll_interval));
                        continue;
                    }
                    // Buffer one event until a matching UI tree can be committed.
                    if self.terminal_events.is_empty() {
                        if let Some(event) = session
                            .poll_event(self.scheduler.wait_timeout(poll_interval))
                            .map_err(RuntimeError::Backend)?
                        {
                            self.terminal_events.push_back(event);
                        }
                    } else {
                        self.wake.wait(self.scheduler.wait_timeout(poll_interval));
                    }
                    continue;
                }
                TerminalRenderOutcome::Idle | TerminalRenderOutcome::Applied => {}
            }
            if let Some(event) = self.terminal_events.pop_front() {
                self.dispatch_terminal_event_with_recovery(event)
                    .map_err(|error| RuntimeError::Ui(UiError::Reconcile(error)))?;
                continue;
            }
            if process.budget_exhausted {
                if let Some(event) = session
                    .poll_event(Duration::ZERO)
                    .map_err(RuntimeError::Backend)?
                {
                    self.dispatch_terminal_event_with_recovery(event)
                        .map_err(|error| RuntimeError::Ui(UiError::Reconcile(error)))?;
                }
                continue;
            }
            if let Some(event) = session
                .poll_event(self.scheduler.wait_timeout(poll_interval))
                .map_err(RuntimeError::Backend)?
            {
                self.dispatch_terminal_event_with_recovery(event)
                    .map_err(|error| RuntimeError::Ui(UiError::Reconcile(error)))?;
            }
        }
        Ok(())
    }

    fn next_message(&mut self) -> Option<A::Message> {
        if self.prefer_external {
            if let Some(message) = self.receiver.receive() {
                self.prefer_external = false;
                return Some(message);
            }
            let message = self.messages.pop_front()?;
            self.prefer_external = true;
            return Some(message);
        }

        if let Some(message) = self.messages.pop_front() {
            self.prefer_external = true;
            return Some(message);
        }
        let message = self.receiver.receive()?;
        self.prefer_external = false;
        Some(message)
    }

    fn execute(&mut self, command: Command<A::Message>) {
        for action in command.actions {
            match action {
                CommandAction::Message(message) => self.messages.push_back(message),
                CommandAction::Perform(future) => self.scheduler.spawn(future),
                CommandAction::After(delay, message) => {
                    self.scheduler.schedule_after(delay, message);
                }
                CommandAction::Quit => {
                    self.quitting = true;
                    self.receiver.close();
                    break;
                }
            }
        }
    }

    fn dispatch_terminal_event_with_recovery(
        &mut self,
        event: TerminalEvent,
    ) -> Result<(), ReconcileError> {
        match self.dispatch_terminal_event(event.clone()) {
            Ok(_) => Ok(()),
            Err(ReconcileError::ViewDoesNotMatchCommittedTree) => {
                self.terminal_events.push_front(event);
                self.invalidation.request(Invalidation::Recompose);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn refresh_after_commit(&mut self) -> Result<(), UiError> {
        let view = self.application.view();
        let outcome = self
            .ui
            .refresh_hover(&view, &self.renderer)
            .map_err(UiError::Reconcile)?;
        self.invalidation.request(self.ui.pending_invalidation());
        self.messages.extend(outcome.messages);
        Ok(())
    }

    fn refresh_after_commit_timed(&mut self) -> Result<(Duration, Duration), UiError> {
        let view_started = Instant::now();
        let view = self.application.view();
        let view_construction = view_started.elapsed();
        let refresh_started = Instant::now();
        let outcome = self
            .ui
            .refresh_hover(&view, &self.renderer)
            .map_err(UiError::Reconcile)?;
        self.invalidation.request(self.ui.pending_invalidation());
        self.messages.extend(outcome.messages);
        Ok((view_construction, refresh_started.elapsed()))
    }
}

fn render_timings(
    total: Duration,
    view_construction: Duration,
    preparation: UiPreparationTimings,
    terminal_serialization_and_write: Option<Duration>,
    commit: Option<Duration>,
    post_commit: Option<Duration>,
) -> RenderTimings {
    RenderTimings {
        total,
        view_construction,
        staging_reconciliation: preparation.staging_reconciliation,
        layout: preparation.layout,
        paint: preparation.paint,
        diff: preparation.diff,
        terminal_serialization_and_write,
        commit,
        post_commit,
    }
}

/// Opens a terminal, runs a fullscreen application, restores the terminal, and
/// returns the model.
///
/// The renderer owns and repaints the complete viewport. Use
/// [`ScreenMode::Alternate`](arborui_terminal::ScreenMode::Alternate); main-screen
/// inline regions and native scrollback require a different rendering contract.
pub fn run<A, B>(
    application: A,
    backend: B,
    desired: TerminalState,
    poll_interval: Duration,
) -> Result<A, RuntimeError<B::Error>>
where
    A: Application,
    B: TerminalBackend,
{
    run_with_options(
        application,
        backend,
        desired,
        poll_interval,
        RuntimeOptions::default(),
    )
}

/// Configured variant of [`run`].
pub fn run_with_options<A, B>(
    application: A,
    backend: B,
    desired: TerminalState,
    poll_interval: Duration,
    options: RuntimeOptions,
) -> Result<A, RuntimeError<B::Error>>
where
    A: Application,
    B: TerminalBackend,
{
    let mut session = TerminalSession::open(backend, desired).map_err(RuntimeError::Backend)?;
    let mut runner = AppRunner::from_terminal_with_options(application, &session, options)
        .map_err(RuntimeError::Backend)?;
    let result = runner.run_terminal(&mut session, poll_interval);
    match result {
        Ok(()) => session.restore().map_err(RuntimeError::Backend)?,
        Err(error) => {
            if let Err(restore_error) = session.restore() {
                return Err(RuntimeError::Restore {
                    error: Box::new(error),
                    restore_error,
                });
            }
            return Err(error);
        }
    }
    Ok(runner.into_application())
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        io,
        pin::Pin,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        task::{Context, Poll, Waker},
    };

    use arborui_render::FramePatch;
    use arborui_terminal::{
        Capabilities, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, TerminalBackend,
    };
    use arborui_ui::{Element, EventPhase};

    use super::*;

    #[derive(Default)]
    struct OrderedApp {
        values: Vec<u8>,
    }

    enum OrderedMessage {
        Start,
        Value(u8),
    }

    impl Application for OrderedApp {
        type Message = OrderedMessage;

        fn update(
            &mut self,
            message: Self::Message,
            _context: &mut UpdateContext<Self::Message>,
        ) -> Command<Self::Message> {
            match message {
                OrderedMessage::Start => Command::batch([
                    Command::message(OrderedMessage::Value(1)),
                    Command::batch([
                        Command::message(OrderedMessage::Value(2)),
                        Command::message(OrderedMessage::Value(3)),
                    ]),
                ]),
                OrderedMessage::Value(value) => {
                    self.values.push(value);
                    Command::none()
                }
            }
        }

        fn view(&self) -> Element<'_, Self::Message> {
            Element::text("")
        }
    }

    #[derive(Default)]
    struct FutureState {
        output: Option<u8>,
        waker: Option<Waker>,
    }

    #[derive(Clone, Default)]
    struct ControlledFuture {
        state: Arc<Mutex<FutureState>>,
    }

    impl ControlledFuture {
        fn complete(&self, output: u8) {
            let waker = {
                let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                state.output = Some(output);
                state.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }

    impl Future for ControlledFuture {
        type Output = u8;

        fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            match state.output.take() {
                Some(output) => Poll::Ready(output),
                None => {
                    state.waker = Some(context.waker().clone());
                    Poll::Pending
                }
            }
        }
    }

    enum FutureMessage {
        Start(ControlledFuture),
        Yield,
        Timer,
        Timers,
        Finished(u8),
    }

    #[derive(Default)]
    struct FutureApp {
        values: Vec<u8>,
    }

    impl Application for FutureApp {
        type Message = FutureMessage;

        fn update(
            &mut self,
            message: Self::Message,
            _context: &mut UpdateContext<Self::Message>,
        ) -> Command<Self::Message> {
            match message {
                FutureMessage::Start(future) => Command::perform(future, FutureMessage::Finished),
                FutureMessage::Yield => Command::perform(YieldOnce(false), FutureMessage::Finished),
                FutureMessage::Timer => Command::after(Duration::ZERO, FutureMessage::Finished(4)),
                FutureMessage::Timers => Command::batch([
                    Command::after(Duration::ZERO, FutureMessage::Finished(1)),
                    Command::after(Duration::ZERO, FutureMessage::Finished(2)),
                    Command::after(Duration::ZERO, FutureMessage::Finished(3)),
                ]),
                FutureMessage::Finished(value) => {
                    self.values.push(value);
                    Command::none()
                }
            }
        }

        fn view(&self) -> Element<'_, Self::Message> {
            Element::text("")
        }
    }

    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = u8;

        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            if self.0 {
                Poll::Ready(6)
            } else {
                self.0 = true;
                context.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    #[derive(Default)]
    struct ManualClock {
        elapsed: Mutex<Duration>,
    }

    impl ManualClock {
        fn advance(&self, duration: Duration) {
            let mut elapsed = self
                .elapsed
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            *elapsed = elapsed.saturating_add(duration);
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Duration {
            *self
                .elapsed
                .lock()
                .unwrap_or_else(|error| error.into_inner())
        }
    }

    #[derive(Default)]
    struct ViewApp {
        value: usize,
        views: Arc<AtomicUsize>,
    }

    enum ViewMessage {
        Increment,
        Invalidate(Invalidation),
        Quit,
    }

    impl Application for ViewApp {
        type Message = ViewMessage;

        fn update(
            &mut self,
            message: Self::Message,
            context: &mut UpdateContext<Self::Message>,
        ) -> Command<Self::Message> {
            match message {
                ViewMessage::Increment => self.value += 1,
                ViewMessage::Invalidate(invalidation) => context.invalidate(invalidation),
                ViewMessage::Quit => return Command::quit(),
            }
            Command::none()
        }

        fn view(&self) -> Element<'_, Self::Message> {
            self.views.fetch_add(1, Ordering::Relaxed);
            Element::text("view")
        }
    }

    struct MissedInvalidationApp {
        expanded: bool,
        activations: usize,
        handler_calls: Arc<AtomicUsize>,
    }

    enum MissedInvalidationMessage {
        Expand,
        Activate,
    }

    impl Application for MissedInvalidationApp {
        type Message = MissedInvalidationMessage;

        fn update(
            &mut self,
            message: Self::Message,
            _context: &mut UpdateContext<Self::Message>,
        ) -> Command<Self::Message> {
            match message {
                MissedInvalidationMessage::Expand => {
                    self.expanded = true;
                    Command::none()
                }
                MissedInvalidationMessage::Activate => {
                    self.activations += 1;
                    Command::quit()
                }
            }
        }

        fn view(&self) -> Element<'_, Self::Message> {
            if !self.expanded {
                return Element::text("collapsed");
            }

            Element::custom("expanded", [Element::text("expanded")]).on_event(
                EventPhase::Bubble,
                |_event, context| {
                    self.handler_calls.fetch_add(1, Ordering::Relaxed);
                    context.emit(MissedInvalidationMessage::Activate);
                },
            )
        }
    }

    #[derive(Default)]
    struct BackendState {
        outcomes: VecDeque<WriteOutcome>,
        events: VecDeque<TerminalEvent>,
        patches: Vec<FramePatch>,
        fail_next_write: bool,
    }

    struct FakeBackend {
        capabilities: Capabilities,
        state: Arc<Mutex<BackendState>>,
    }

    impl FakeBackend {
        fn new(
            outcomes: impl IntoIterator<Item = WriteOutcome>,
        ) -> (Self, Arc<Mutex<BackendState>>) {
            let state = Arc::new(Mutex::new(BackendState {
                outcomes: outcomes.into_iter().collect(),
                events: VecDeque::new(),
                patches: Vec::new(),
                fail_next_write: false,
            }));
            (
                Self {
                    capabilities: Capabilities::default(),
                    state: Arc::clone(&state),
                },
                state,
            )
        }
    }

    impl TerminalBackend for FakeBackend {
        type Error = io::Error;

        fn size(&self) -> Result<Size, Self::Error> {
            Ok(Size::new(8, 2))
        }

        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }

        fn poll_event(&mut self, _timeout: Duration) -> Result<Option<TerminalEvent>, Self::Error> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            Ok(state.events.pop_front())
        }

        fn apply_state(&mut self, _desired: &TerminalState) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_patch(&mut self, patch: &FramePatch) -> Result<WriteOutcome, Self::Error> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.patches.push(patch.clone());
            if std::mem::take(&mut state.fail_next_write) {
                return Err(io::Error::other("injected write failure"));
            }
            Ok(state.outcomes.pop_front().unwrap_or(WriteOutcome::Applied))
        }

        fn restore(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct DualFailureBackend {
        capabilities: Capabilities,
    }

    struct ResumeCleanupFailureBackend {
        capabilities: Capabilities,
        sizes: Arc<AtomicUsize>,
        apply_calls: usize,
        restore_calls: usize,
    }

    impl TerminalBackend for ResumeCleanupFailureBackend {
        type Error = io::Error;

        fn size(&self) -> Result<Size, Self::Error> {
            self.sizes.fetch_add(1, Ordering::Relaxed);
            Ok(Size::new(8, 2))
        }

        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }

        fn poll_event(&mut self, _timeout: Duration) -> Result<Option<TerminalEvent>, Self::Error> {
            Ok(None)
        }

        fn apply_state(&mut self, _desired: &TerminalState) -> Result<(), Self::Error> {
            self.apply_calls += 1;
            if self.apply_calls == 2 {
                return Err(io::Error::other("injected resume failure"));
            }
            Ok(())
        }

        fn write_patch(&mut self, _patch: &FramePatch) -> Result<WriteOutcome, Self::Error> {
            Ok(WriteOutcome::Applied)
        }

        fn restore(&mut self) -> Result<(), Self::Error> {
            self.restore_calls += 1;
            if self.restore_calls == 2 {
                return Err(io::Error::other("injected cleanup failure"));
            }
            Ok(())
        }
    }

    impl TerminalBackend for DualFailureBackend {
        type Error = io::Error;

        fn size(&self) -> Result<Size, Self::Error> {
            Ok(Size::new(8, 2))
        }

        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }

        fn poll_event(&mut self, _timeout: Duration) -> Result<Option<TerminalEvent>, Self::Error> {
            Err(io::Error::other("poll failure"))
        }

        fn apply_state(&mut self, _desired: &TerminalState) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_patch(&mut self, _patch: &FramePatch) -> Result<WriteOutcome, Self::Error> {
            Ok(WriteOutcome::Applied)
        }

        fn restore(&mut self) -> Result<(), Self::Error> {
            Err(io::Error::other("restore failure"))
        }
    }

    fn session(
        outcomes: impl IntoIterator<Item = WriteOutcome>,
    ) -> Result<(TerminalSession<FakeBackend>, Arc<Mutex<BackendState>>), io::Error> {
        let (backend, state) = FakeBackend::new(outcomes);
        Ok((
            TerminalSession::open(backend, TerminalState::default())?,
            state,
        ))
    }

    #[test]
    fn batches_preserve_command_order() -> Result<(), Box<dyn std::error::Error>> {
        let (terminal, _state) = session([])?;
        let mut runner = AppRunner::from_terminal(OrderedApp::default(), &terminal)?;
        runner.enqueue(OrderedMessage::Start);

        let report = runner.process_pending();

        assert_eq!(report.updates, 4);
        assert_eq!(runner.application().values, [1, 2, 3]);
        Ok(())
    }

    #[test]
    fn cloned_external_proxy_delivers_from_another_thread() -> Result<(), Box<dyn std::error::Error>>
    {
        let (terminal, _state) = session([])?;
        let mut runner = AppRunner::from_terminal(OrderedApp::default(), &terminal)?;
        let proxy = runner.event_proxy();
        let sent = std::thread::spawn(move || proxy.send(OrderedMessage::Value(7)).is_ok())
            .join()
            .unwrap_or(false);

        assert!(sent);
        assert_eq!(runner.process_pending().updates, 1);
        assert_eq!(runner.application().values, [7]);
        Ok(())
    }

    #[test]
    fn bounded_external_ingress_rejects_new_and_accepts_after_processing()
    -> Result<(), Box<dyn std::error::Error>> {
        let (terminal, _state) = session([])?;
        let options = RuntimeOptions::new()
            .with_event_ingress_capacity(NonZeroUsize::new(2).unwrap_or(NonZeroUsize::MIN));
        let mut runner =
            AppRunner::from_terminal_with_options(OrderedApp::default(), &terminal, options)?;
        let proxy = runner.event_proxy();

        assert!(proxy.send(OrderedMessage::Value(1)).is_ok());
        assert!(proxy.send(OrderedMessage::Value(2)).is_ok());
        let rejected = proxy
            .send(OrderedMessage::Value(3))
            .expect_err("capacity should reject the new message");
        assert_eq!(rejected.kind(), crate::EventProxySendErrorKind::Full);
        assert_eq!(proxy.metrics().depth, 2);
        assert_eq!(proxy.metrics().high_water_mark, 2);
        assert_eq!(proxy.metrics().rejected, 1);

        assert_eq!(runner.process_pending().updates, 2);
        assert_eq!(runner.application().values, [1, 2]);
        assert_eq!(proxy.metrics().depth, 0);
        assert!(proxy.send(rejected.into_inner()).is_ok());
        assert_eq!(runner.process_pending().updates, 1);
        assert_eq!(runner.application().values, [1, 2, 3]);
        Ok(())
    }

    #[test]
    fn ready_internal_and_external_sources_alternate_without_reordering()
    -> Result<(), Box<dyn std::error::Error>> {
        let (terminal, _state) = session([])?;
        let mut runner = AppRunner::from_terminal(OrderedApp::default(), &terminal)?;
        let proxy = runner.event_proxy();
        runner.enqueue(OrderedMessage::Value(10));
        runner.enqueue(OrderedMessage::Value(11));
        assert!(proxy.send(OrderedMessage::Value(1)).is_ok());
        assert!(proxy.send(OrderedMessage::Value(2)).is_ok());

        assert_eq!(runner.process_pending().updates, 4);

        assert_eq!(runner.application().values, [10, 1, 11, 2]);
        Ok(())
    }

    #[test]
    fn dropping_or_quitting_runner_closes_external_ingress()
    -> Result<(), Box<dyn std::error::Error>> {
        let (terminal, _state) = session([])?;
        let runner = AppRunner::from_terminal(ViewApp::default(), &terminal)?;
        let dropped_proxy = runner.event_proxy();
        drop(runner);
        let dropped = dropped_proxy
            .send(ViewMessage::Increment)
            .expect_err("dropped runner should close ingress");
        assert_eq!(dropped.kind(), crate::EventProxySendErrorKind::Closed);

        let mut runner = AppRunner::from_terminal(ViewApp::default(), &terminal)?;
        let quitting_proxy = runner.event_proxy();
        assert!(quitting_proxy.send(ViewMessage::Quit).is_ok());
        assert!(quitting_proxy.send(ViewMessage::Increment).is_ok());
        let report = runner.process_pending();
        assert!(report.quitting);
        assert_eq!(runner.application().value, 0);
        let quitting = quitting_proxy
            .send(ViewMessage::Increment)
            .expect_err("quitting runner should close ingress");
        assert_eq!(quitting.kind(), crate::EventProxySendErrorKind::Closed);
        assert!(quitting_proxy.metrics().closed);
        assert_eq!(quitting_proxy.metrics().depth, 0);
        Ok(())
    }

    #[test]
    fn self_waking_future_completion_becomes_a_serialized_message()
    -> Result<(), Box<dyn std::error::Error>> {
        let (terminal, _state) = session([])?;
        let mut runner = AppRunner::from_terminal(FutureApp::default(), &terminal)?;
        let future = ControlledFuture::default();
        runner.enqueue(FutureMessage::Start(future.clone()));

        let pending = runner.process_pending();
        assert_eq!(pending.updates, 1);
        assert_eq!(pending.completed_tasks, 0);
        assert!(runner.application().values.is_empty());

        future.complete(9);
        let completed = runner.process_pending();
        assert_eq!(completed.completed_tasks, 1);
        assert_eq!(completed.updates, 1);
        assert_eq!(runner.application().values, [9]);
        Ok(())
    }

    #[test]
    fn immediate_rewake_and_zero_timer_drain_in_the_same_turn()
    -> Result<(), Box<dyn std::error::Error>> {
        let (terminal, _state) = session([])?;
        let mut runner = AppRunner::from_terminal(FutureApp::default(), &terminal)?;
        runner.enqueue(FutureMessage::Yield);
        runner.enqueue(FutureMessage::Timer);

        let report = runner.process_pending();

        assert_eq!(report.completed_tasks, 1);
        let mut values = runner.application().values.clone();
        values.sort_unstable();
        assert_eq!(values, [4, 6]);
        Ok(())
    }

    #[test]
    fn equal_deadline_timers_preserve_declaration_order() -> Result<(), Box<dyn std::error::Error>>
    {
        let (terminal, _state) = session([])?;
        let mut runner = AppRunner::from_terminal(FutureApp::default(), &terminal)?;
        runner.enqueue(FutureMessage::Timers);

        runner.process_pending();

        assert_eq!(runner.application().values, [1, 2, 3]);
        Ok(())
    }

    #[test]
    fn injected_clock_controls_timers_and_visual_idle() -> Result<(), Box<dyn std::error::Error>> {
        let clock = Arc::new(ManualClock::default());
        let clock_source: Arc<dyn Clock> = clock.clone();
        let size = Size::new(8, 2);
        let mut runner = AppRunner::new_with_clock(
            OrderedApp::default(),
            size,
            Renderer::new(size, Capabilities::default().width_policy),
            clock_source,
        );
        assert_eq!(runner.render_headless()?, HeadlessRenderOutcome::Committed);
        runner.execute(Command::after(
            Duration::from_secs(2),
            OrderedMessage::Value(8),
        ));

        assert!(runner.is_visually_idle());
        assert!(!runner.is_idle());
        clock.advance(Duration::from_secs(1));
        assert_eq!(runner.process_pending().updates, 0);
        clock.advance(Duration::from_secs(1));
        assert!(!runner.is_visually_idle());
        assert_eq!(runner.process_pending().updates, 1);
        assert_eq!(runner.application().values, [8]);
        assert!(runner.is_visually_idle());

        clock.advance(Duration::MAX);
        runner.execute(Command::after(
            Duration::from_secs(1),
            OrderedMessage::Value(9),
        ));
        assert_eq!(runner.process_pending().updates, 1);
        assert_eq!(runner.application().values, [8, 9]);
        Ok(())
    }

    #[test]
    fn idle_and_noninvalidating_updates_do_not_render() -> Result<(), Box<dyn std::error::Error>> {
        let (terminal, _state) = session([])?;
        let views = Arc::new(AtomicUsize::new(0));
        let app = ViewApp {
            value: 0,
            views: Arc::clone(&views),
        };
        let mut runner = AppRunner::from_terminal(app, &terminal)?;

        assert_eq!(runner.render_headless()?, HeadlessRenderOutcome::Committed);
        let after_initial = views.load(Ordering::Relaxed);
        assert_eq!(runner.render_headless()?, HeadlessRenderOutcome::Idle);
        assert_eq!(views.load(Ordering::Relaxed), after_initial);

        runner.enqueue(ViewMessage::Increment);
        assert_eq!(runner.process_pending().updates, 1);
        assert_eq!(runner.render_headless()?, HeadlessRenderOutcome::Idle);
        assert_eq!(views.load(Ordering::Relaxed), after_initial);
        Ok(())
    }

    #[test]
    fn timed_headless_render_reports_phases_and_skips_idle_work()
    -> Result<(), Box<dyn std::error::Error>> {
        let (terminal, _state) = session([])?;
        let mut runner = AppRunner::from_terminal(ViewApp::default(), &terminal)?;

        let rendered = runner.render_headless_timed()?;

        assert_eq!(rendered.outcome, HeadlessRenderOutcome::Committed);
        let timings = rendered.timings.expect("committed render must be timed");
        assert_eq!(timings.terminal_serialization_and_write, None);
        assert!(timings.commit.is_some());
        assert!(timings.post_commit.is_some());
        assert_timing_bounds(timings);
        assert_eq!(
            runner.render_headless_timed()?,
            TimedRender {
                outcome: HeadlessRenderOutcome::Idle,
                timings: None,
            }
        );
        Ok(())
    }

    #[test]
    fn invalidations_coalesce_into_one_frame() -> Result<(), Box<dyn std::error::Error>> {
        let (terminal, _state) = session([])?;
        let mut runner = AppRunner::from_terminal(ViewApp::default(), &terminal)?;
        assert_eq!(runner.render_headless()?, HeadlessRenderOutcome::Committed);

        runner.enqueue(ViewMessage::Invalidate(Invalidation::Paint));
        runner.enqueue(ViewMessage::Invalidate(Invalidation::Layout));
        let report = runner.process_pending();

        assert_eq!(report.updates, 2);
        assert_eq!(report.invalidation, Invalidation::Layout);
        assert_eq!(runner.render_headless()?, HeadlessRenderOutcome::Committed);
        assert_eq!(runner.render_headless()?, HeadlessRenderOutcome::Idle);
        Ok(())
    }

    #[test]
    fn terminal_write_outcomes_control_commit_and_full_repaint()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut terminal, state) = session([
            WriteOutcome::Deferred,
            WriteOutcome::StateUnknown,
            WriteOutcome::Applied,
        ])?;
        let mut runner = AppRunner::from_terminal(ViewApp::default(), &terminal)?;

        assert_eq!(
            runner.render_terminal(&mut terminal)?,
            TerminalRenderOutcome::Deferred
        );
        assert!(runner.ui_tree().is_empty());
        assert_eq!(
            runner.render_terminal(&mut terminal)?,
            TerminalRenderOutcome::StateUnknown
        );
        assert!(runner.ui_tree().is_empty());
        assert_eq!(
            runner.render_terminal(&mut terminal)?,
            TerminalRenderOutcome::Applied
        );
        assert!(!runner.ui_tree().is_empty());
        assert_eq!(
            runner.render_terminal(&mut terminal)?,
            TerminalRenderOutcome::Idle
        );

        let state = state.lock().unwrap_or_else(|error| error.into_inner());
        assert_eq!(state.patches.len(), 3);
        assert!(state.patches.iter().all(|patch| patch.full_repaint));
        Ok(())
    }

    #[test]
    fn timed_terminal_render_preserves_write_outcome_transactions()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut terminal, _state) = session([
            WriteOutcome::Deferred,
            WriteOutcome::StateUnknown,
            WriteOutcome::Applied,
        ])?;
        let mut runner = AppRunner::from_terminal(ViewApp::default(), &terminal)?;

        for expected in [
            TerminalRenderOutcome::Deferred,
            TerminalRenderOutcome::StateUnknown,
        ] {
            let rendered = runner.render_terminal_timed(&mut terminal)?;
            assert_eq!(rendered.outcome, expected);
            let timings = rendered.timings.expect("attempted write must be timed");
            assert!(timings.terminal_serialization_and_write.is_some());
            assert_eq!(timings.commit, None);
            assert_eq!(timings.post_commit, None);
            assert_timing_bounds(timings);
            assert!(runner.ui_tree().is_empty());
        }

        let applied = runner.render_terminal_timed(&mut terminal)?;
        assert_eq!(applied.outcome, TerminalRenderOutcome::Applied);
        let timings = applied.timings.expect("applied write must be timed");
        assert!(timings.terminal_serialization_and_write.is_some());
        assert!(timings.commit.is_some());
        assert!(timings.post_commit.is_some());
        assert_timing_bounds(timings);
        assert!(!runner.ui_tree().is_empty());
        assert_eq!(
            runner.render_terminal_timed(&mut terminal)?,
            TimedRender {
                outcome: TerminalRenderOutcome::Idle,
                timings: None,
            }
        );
        Ok(())
    }

    #[test]
    fn timed_empty_patch_commits_without_backend_output() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut terminal, state) = session([])?;
        let mut runner = AppRunner::from_terminal(ViewApp::default(), &terminal)?;
        assert_eq!(
            runner.render_terminal(&mut terminal)?,
            TerminalRenderOutcome::Applied
        );
        runner.enqueue(ViewMessage::Invalidate(Invalidation::Paint));
        assert_eq!(runner.process_pending().updates, 1);

        let rendered = runner.render_terminal_timed(&mut terminal)?;

        assert_eq!(rendered.outcome, TerminalRenderOutcome::Applied);
        let timings = rendered
            .timings
            .expect("empty prepared frame must be timed");
        assert_eq!(timings.terminal_serialization_and_write, None);
        assert!(timings.commit.is_some());
        assert!(timings.post_commit.is_some());
        assert_eq!(
            state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .patches
                .len(),
            1
        );
        Ok(())
    }

    fn assert_timing_bounds(timings: RenderTimings) {
        for phase in [
            timings.view_construction,
            timings.staging_reconciliation,
            timings.layout,
            timings.paint,
            timings.diff,
        ] {
            assert!(timings.total >= phase);
        }
        for phase in [
            timings.terminal_serialization_and_write,
            timings.commit,
            timings.post_commit,
        ]
        .into_iter()
        .flatten()
        {
            assert!(timings.total >= phase);
        }
    }

    #[test]
    fn terminal_runtime_recomposes_before_retrying_a_mismatched_event()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut terminal, state) = session([])?;
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let app = MissedInvalidationApp {
            expanded: false,
            activations: 0,
            handler_calls: Arc::clone(&handler_calls),
        };
        let mut runner = AppRunner::from_terminal(app, &terminal)?;
        assert_eq!(
            runner.render_terminal(&mut terminal)?,
            TerminalRenderOutcome::Applied
        );

        runner.enqueue(MissedInvalidationMessage::Expand);
        assert_eq!(runner.process_pending().updates, 1);
        assert_eq!(runner.pending_invalidation(), Invalidation::None);
        assert_eq!(
            runner.dispatch_ui_event(UiEvent::TerminalFocusGained),
            Err(ReconcileError::ViewDoesNotMatchCommittedTree)
        );
        {
            let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
            state.fail_next_write = true;
            state
                .outcomes
                .extend([WriteOutcome::Deferred, WriteOutcome::Applied]);
            state.events.push_back(TerminalEvent::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::default(),
            }));
        }

        assert!(matches!(
            runner.run_terminal(&mut terminal, Duration::ZERO),
            Err(RuntimeError::Backend(_))
        ));
        assert_eq!(handler_calls.load(Ordering::Relaxed), 0);
        assert_eq!(runner.application().activations, 0);

        runner.run_terminal(&mut terminal, Duration::ZERO)?;

        assert_eq!(handler_calls.load(Ordering::Relaxed), 1);
        assert_eq!(runner.application().activations, 1);
        let state = state.lock().unwrap_or_else(|error| error.into_inner());
        assert_eq!(state.patches.len(), 4);
        Ok(())
    }

    #[test]
    fn failed_resume_and_cleanup_does_not_busy_loop_run_terminal()
    -> Result<(), Box<dyn std::error::Error>> {
        let sizes = Arc::new(AtomicUsize::new(0));
        let backend = ResumeCleanupFailureBackend {
            capabilities: Capabilities::default(),
            sizes: Arc::clone(&sizes),
            apply_calls: 0,
            restore_calls: 0,
        };
        let mut terminal = TerminalSession::open(backend, TerminalState::fullscreen())?;
        terminal.suspend()?;
        assert!(terminal.resume().is_err());

        let mut runner = AppRunner::from_terminal(ViewApp::default(), &terminal)?;
        let proxy = runner.event_proxy();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            proxy.send(ViewMessage::Quit).is_ok()
        });

        runner.run_terminal(&mut terminal, Duration::from_secs(1))?;
        assert!(sender.join().unwrap_or(false));
        let size_calls = sizes.load(Ordering::Relaxed);
        assert!(
            size_calls <= 2,
            "a suspended session awaiting cleanup must block instead of repeatedly preparing \
             deferred frames; observed {size_calls} terminal size calls"
        );
        Ok(())
    }

    #[test]
    fn runtime_and_restore_failures_are_both_preserved() {
        let result = run(
            ViewApp::default(),
            DualFailureBackend {
                capabilities: Capabilities::default(),
            },
            TerminalState::default(),
            Duration::ZERO,
        );
        let Err(RuntimeError::Restore {
            error,
            restore_error,
        }) = result
        else {
            panic!("expected a combined runtime and restoration failure");
        };

        assert_eq!(restore_error.to_string(), "restore failure");
        let RuntimeError::Backend(runtime_error) = *error else {
            panic!("expected the original backend failure");
        };
        assert_eq!(runtime_error.to_string(), "poll failure");
    }
}
