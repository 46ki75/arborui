use std::cell::Cell;

use arborui_core::Point;
use arborui_ui::{PointerEvent, PointerEventKind};

use super::*;

const POLL_INTERVAL: Duration = Duration::from_secs(60);

fn polling_session(
    events: impl IntoIterator<Item = TerminalEvent>,
) -> Result<(TerminalSession<FakeBackend>, Arc<Mutex<BackendState>>), io::Error> {
    let (terminal, state) = session([])?;
    {
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        state.events.extend(events);
        // Fail deterministically if a regression prevents the expected quit.
        state.poll_limit = Some(4);
    }
    Ok((terminal, state))
}

fn interrupt() -> TerminalEvent {
    key_event(
        KeyCode::Character('c'),
        KeyModifiers::CONTROL,
        KeyEventKind::Press,
    )
}

#[derive(Default)]
struct FocusApp {
    emit_quit: bool,
    focused: Cell<bool>,
    updates: usize,
}

impl Application for FocusApp {
    type Message = ();

    fn update(
        &mut self,
        (): Self::Message,
        _context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        self.updates += 1;
        Command::quit()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        Element::text(if self.focused.get() {
            "focused"
        } else {
            "blurred"
        })
        .focusable(true)
        .on_event(EventPhase::Target, |event, context| {
            if matches!(event, UiEvent::FocusGained) {
                self.focused.set(true);
                if self.emit_quit {
                    context.emit(());
                } else {
                    context.invalidate(Invalidation::Paint);
                }
            }
        })
    }
}

#[test]
fn post_commit_message_is_processed_before_blocking_poll() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut terminal, state) = polling_session([])?;
    let mut runner = AppRunner::from_terminal(
        FocusApp {
            emit_quit: true,
            ..FocusApp::default()
        },
        &terminal,
    )?;

    runner.run_terminal(&mut terminal, POLL_INTERVAL)?;

    assert_eq!(runner.application().updates, 1);
    assert!(runner.is_quitting());
    let state = state.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(state.poll_timeouts, [Duration::ZERO]);
    Ok(())
}

#[test]
fn post_commit_work_still_checks_input_before_another_turn()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut terminal, state) = polling_session([interrupt()])?;
    let mut runner = AppRunner::from_terminal(
        FocusApp {
            emit_quit: true,
            ..FocusApp::default()
        },
        &terminal,
    )?;

    runner.run_terminal(&mut terminal, POLL_INTERVAL)?;

    assert!(runner.is_quitting());
    assert_eq!(runner.application().updates, 0);
    let state = state.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(state.poll_timeouts, [Duration::ZERO]);
    Ok(())
}

#[test]
fn post_commit_invalidation_renders_before_blocking_poll() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut terminal, state) = polling_session([TerminalEvent::FocusGained, interrupt()])?;
    let mut runner = AppRunner::from_terminal(FocusApp::default(), &terminal)?;

    runner.run_terminal(&mut terminal, POLL_INTERVAL)?;

    assert!(runner.application().focused.get());
    assert_eq!(runner.application().updates, 0);
    assert_eq!(runner.pending_invalidation(), Invalidation::None);
    let state = state.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(state.patches.len(), 2);
    assert_eq!(state.poll_timeouts, [Duration::ZERO, POLL_INTERVAL]);
    Ok(())
}

#[derive(Default)]
struct HoverApp {
    show_target: bool,
    quits: usize,
}

enum HoverMessage {
    ShowTarget,
    Quit,
}

impl Application for HoverApp {
    type Message = HoverMessage;

    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        match message {
            HoverMessage::ShowTarget => {
                self.show_target = true;
                context.invalidate(Invalidation::Recompose);
                Command::none()
            }
            HoverMessage::Quit => {
                self.quits += 1;
                Command::quit()
            }
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        if self.show_target {
            Element::text("target")
                .key("target")
                .on_event(EventPhase::Target, |event, context| {
                    if matches!(event, UiEvent::PointerEntered) {
                        context.emit(HoverMessage::Quit);
                    }
                })
        } else {
            Element::text("initial").key("initial").interactive(true)
        }
    }
}

#[test]
fn post_commit_hover_message_is_processed_before_blocking_poll()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut terminal, state) = polling_session([])?;
    let mut runner = AppRunner::from_terminal(HoverApp::default(), &terminal)?;
    assert_eq!(
        runner.render_terminal(&mut terminal)?,
        TerminalRenderOutcome::Applied
    );
    let moved = runner.dispatch_ui_event(UiEvent::Pointer(PointerEvent {
        kind: PointerEventKind::Moved,
        position: Point::ORIGIN,
        modifiers: arborui_ui::KeyModifiers::NONE,
    }))?;
    assert_eq!(moved.messages, 0);
    let previous_hover = runner.ui_tree().hovered().expect("initial node is hovered");
    assert!(runner.is_visually_idle());
    runner.enqueue(HoverMessage::ShowTarget);

    // No further pointer input: the committed hit map alone changes hover.
    runner.run_terminal(&mut terminal, POLL_INTERVAL)?;

    assert_eq!(runner.application().quits, 1);
    let current_hover = runner.ui_tree().hovered().expect("new target is hovered");
    assert_ne!(current_hover, previous_hover);
    let state = state.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(state.poll_timeouts, [Duration::ZERO]);
    Ok(())
}

#[test]
fn idle_runner_preserves_configured_poll_interval() -> Result<(), Box<dyn std::error::Error>> {
    let (mut terminal, state) = polling_session([interrupt()])?;
    let mut runner = AppRunner::from_terminal(ViewApp::default(), &terminal)?;

    runner.run_terminal(&mut terminal, POLL_INTERVAL)?;

    let state = state.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(state.poll_timeouts, [POLL_INTERVAL]);
    Ok(())
}

#[test]
fn dormant_future_does_not_force_nonblocking_poll() -> Result<(), Box<dyn std::error::Error>> {
    let (mut terminal, state) = polling_session([interrupt()])?;
    let mut runner = AppRunner::from_terminal(FutureApp::default(), &terminal)?;
    runner.enqueue(FutureMessage::Start(ControlledFuture::default()));

    runner.run_terminal(&mut terminal, POLL_INTERVAL)?;

    assert!(runner.is_visually_idle());
    assert!(!runner.is_idle());
    assert!(runner.application().values.is_empty());
    let state = state.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(state.poll_timeouts, [POLL_INTERVAL]);
    Ok(())
}

#[test]
fn future_timer_caps_idle_poll_without_spinning() -> Result<(), Box<dyn std::error::Error>> {
    for (delay, expected) in [(5, 3), (90, 60)] {
        let (mut terminal, state) = polling_session([interrupt()])?;
        let clock = Arc::new(ManualClock::default());
        let size = terminal.size()?;
        let mut runner = AppRunner::new_with_clock(
            OrderedApp::default(),
            size,
            Renderer::new(size, terminal.capabilities().width_policy),
            clock.clone(),
        );
        runner.execute(Command::after(
            Duration::from_secs(delay),
            OrderedMessage::Value(8),
        ));
        clock.advance(Duration::from_secs(2));

        runner.run_terminal(&mut terminal, POLL_INTERVAL)?;

        assert!(runner.is_visually_idle());
        assert!(!runner.is_idle());
        assert!(runner.application().values.is_empty());
        let state = state.lock().unwrap_or_else(|error| error.into_inner());
        assert_eq!(state.poll_timeouts, [Duration::from_secs(expected)]);
    }
    Ok(())
}
