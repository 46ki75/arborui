use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, mpsc},
    time::Duration,
};

use arborui_core::Size;
use arborui_render::Renderer;
use arborui_terminal::{
    TerminalBackend, TerminalEvent, TerminalSession, TerminalState, WriteOutcome,
};
use arborui_ui::{Invalidation, ReconcileError, UiCommitError, UiError, UiEvent, UiTree};

use crate::{
    Application, Clock, Command, SystemClock, UpdateContext,
    command::CommandAction,
    proxy::EventProxy,
    scheduler::{Scheduler, WakeSignal},
    translate_terminal_event,
};

const MAX_WORK_PER_TURN: usize = 1_024;
const MAX_MESSAGES_PER_TURN: usize = 768;

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
    receiver: mpsc::Receiver<A::Message>,
    proxy: EventProxy<A::Message>,
    wake: Arc<WakeSignal>,
    scheduler: Scheduler<A::Message>,
    ui: UiTree,
    renderer: Renderer,
    viewport: Size,
    invalidation: Invalidation,
    quitting: bool,
}

impl<A: Application> AppRunner<A> {
    /// Creates a headless runner with an explicitly supplied renderer.
    #[must_use]
    pub fn new(application: A, viewport: Size, renderer: Renderer) -> Self {
        Self::new_with_clock(
            application,
            viewport,
            renderer,
            Arc::new(SystemClock::new()),
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
        let wake = Arc::new(WakeSignal::new());
        let (sender, receiver) = mpsc::channel();
        let proxy = EventProxy::new(sender, Arc::clone(&wake));
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
        }
    }

    /// Creates a runner using a terminal session's size and width policy.
    pub fn from_terminal<B: TerminalBackend>(
        application: A,
        session: &TerminalSession<B>,
    ) -> Result<Self, B::Error> {
        let viewport = session.size()?;
        let renderer = Renderer::new(viewport, session.capabilities().width_policy);
        Ok(Self::new(application, viewport, renderer))
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
        self.receive_external(MAX_WORK_PER_TURN);
        self.messages.is_empty()
            && !self.scheduler.has_tasks()
            && self.invalidation == Invalidation::None
    }

    /// Returns whether no immediately runnable or visual work remains.
    ///
    /// Unlike [`is_idle`](Self::is_idle), this treats dormant futures and
    /// timers with future deadlines as visually idle.
    #[must_use]
    pub fn is_visually_idle(&mut self) -> bool {
        self.receive_external(MAX_WORK_PER_TURN);
        self.messages.is_empty()
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
            let mut progressed = self.receive_external(MAX_WORK_PER_TURN.saturating_sub(work));

            while !self.quitting && work < MAX_WORK_PER_TURN && updates < MAX_MESSAGES_PER_TURN {
                let Some(message) = self.messages.pop_front() else {
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
                self.dispatch_terminal_event(event)
                    .map_err(|error| RuntimeError::Ui(UiError::Reconcile(error)))?;
                continue;
            }
            if process.budget_exhausted {
                if let Some(event) = session
                    .poll_event(Duration::ZERO)
                    .map_err(RuntimeError::Backend)?
                {
                    self.dispatch_terminal_event(event)
                        .map_err(|error| RuntimeError::Ui(UiError::Reconcile(error)))?;
                }
                continue;
            }
            if let Some(event) = session
                .poll_event(self.scheduler.wait_timeout(poll_interval))
                .map_err(RuntimeError::Backend)?
            {
                self.dispatch_terminal_event(event)
                    .map_err(|error| RuntimeError::Ui(UiError::Reconcile(error)))?;
            }
        }
        Ok(())
    }

    fn receive_external(&mut self, limit: usize) -> bool {
        let received = (0..limit)
            .map_while(|_| self.receiver.try_recv().ok())
            .collect::<Vec<_>>();
        let progressed = !received.is_empty();
        self.messages.extend(received);
        progressed
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
                    break;
                }
            }
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
}

/// Opens a terminal, runs an application, restores the terminal, and returns the model.
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
    let mut session = TerminalSession::open(backend, desired).map_err(RuntimeError::Backend)?;
    let mut runner =
        AppRunner::from_terminal(application, &session).map_err(RuntimeError::Backend)?;
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
        convert::Infallible,
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
    use arborui_terminal::{Capabilities, TerminalBackend};
    use arborui_ui::Element;

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

    #[derive(Default)]
    struct BackendState {
        outcomes: VecDeque<WriteOutcome>,
        patches: Vec<FramePatch>,
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
                patches: Vec::new(),
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
        type Error = Infallible;

        fn size(&self) -> Result<Size, Self::Error> {
            Ok(Size::new(8, 2))
        }

        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }

        fn poll_event(&mut self, _timeout: Duration) -> Result<Option<TerminalEvent>, Self::Error> {
            Ok(None)
        }

        fn apply_state(&mut self, _desired: &TerminalState) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_patch(&mut self, patch: &FramePatch) -> Result<WriteOutcome, Self::Error> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.patches.push(patch.clone());
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
    ) -> Result<(TerminalSession<FakeBackend>, Arc<Mutex<BackendState>>), Infallible> {
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
