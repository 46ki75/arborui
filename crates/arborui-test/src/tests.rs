use std::{num::NonZeroUsize, time::Duration};

use arborui_core::Size;
use arborui_runtime::{Application, Command, UpdateContext};
use arborui_ui::{
    Element, EventPhase, Invalidation, KeyModifiers as UiKeyModifiers, PointerEvent,
    PointerEventKind, ReconcileError,
};

use super::*;

struct Counter {
    count: usize,
    label: String,
}

enum Message {
    Increment,
    StartTimer,
}

impl Default for Counter {
    fn default() -> Self {
        Self {
            count: 0,
            label: "0".to_owned(),
        }
    }
}

impl Application for Counter {
    type Message = Message;

    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        match message {
            Message::Increment => {
                self.count += 1;
                self.label = self.count.to_string();
                context.invalidate(Invalidation::Paint);
                Command::none()
            }
            Message::StartTimer => Command::after(Duration::from_secs(2), Message::Increment),
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        arborui_widgets_for_test::view(&self.label)
    }
}

mod arborui_widgets_for_test {
    use arborui_ui::Element;

    use super::Message;

    pub(super) fn view(label: &str) -> Element<'_, Message> {
        Element::container([
            Element::text(label),
            Element::custom("button", [Element::text("add")])
                .key("add")
                .focusable(true)
                .on_event(arborui_ui::EventPhase::Target, |event, context| {
                    if matches!(
                        event,
                        arborui_ui::UiEvent::Key(arborui_ui::UiKeyEvent {
                            key: arborui_ui::UiKey::Enter,
                            action: arborui_ui::KeyAction::Press,
                            ..
                        })
                    ) {
                        context.emit(Message::Increment);
                    }
                }),
        ])
        .layout(arborui_layout::LayoutStyle {
            direction: arborui_layout::FlexDirection::Column,
            ..arborui_layout::LayoutStyle::default()
        })
    }
}

struct MissedInvalidationApp {
    expanded: bool,
    activations: usize,
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
            MissedInvalidationMessage::Expand => self.expanded = true,
            MissedInvalidationMessage::Activate => self.activations += 1,
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        if !self.expanded {
            return Element::text("old");
        }

        Element::custom("expanded", [Element::text("new")]).on_event(
            EventPhase::Bubble,
            |_event, context| {
                context.emit(MissedInvalidationMessage::Activate);
            },
        )
    }
}

struct DuplicateKeyApp {
    invalid: bool,
}

impl Application for DuplicateKeyApp {
    type Message = ();

    fn update(
        &mut self,
        _message: Self::Message,
        _context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        if self.invalid {
            Element::container([
                Element::text("one").key("duplicate"),
                Element::text("two").key("duplicate"),
            ])
        } else {
            Element::text("valid")
        }
    }
}

#[test]
fn events_render_and_expose_focus_and_patches() {
    let mut app = TestApp::new(Counter::default(), Size::new(4, 2));

    assert_eq!(app.frame().characters(), "0   \nadd ");
    assert!(
        app.last_frame_patch()
            .is_some_and(|patch| patch.full_repaint)
    );

    app.key(KeyCode::Tab);
    assert_eq!(app.focused_key(), Some(Key::from("add")));
    app.key(KeyCode::Enter);

    assert_eq!(app.application().count, 1);
    assert_eq!(app.frame().characters(), "1   \nadd ");
    assert!(app.hit_at(Point::new(0, 1)).is_some());
}

#[test]
fn configured_test_app_exercises_bounded_external_ingress() {
    let options = RuntimeOptions::new()
        .with_event_ingress_capacity(NonZeroUsize::new(1).unwrap_or(NonZeroUsize::MIN));
    let mut app = TestApp::with_runtime_options(Counter::default(), Size::new(4, 2), options);
    let proxy = app.event_proxy();

    assert!(proxy.send(Message::Increment).is_ok());
    let rejected = proxy
        .send(Message::Increment)
        .expect_err("second external message should exceed capacity");
    assert_eq!(rejected.kind(), EventProxySendErrorKind::Full);
    assert_eq!(proxy.metrics().depth, 1);
    assert_eq!(proxy.metrics().rejected, 1);

    app.settle();
    assert_eq!(app.application().count, 1);
    assert!(proxy.send(rejected.into_inner()).is_ok());
    app.settle();
    assert_eq!(app.application().count, 2);
}

#[test]
fn manual_time_completes_due_commands_only() {
    let mut app = TestApp::new(Counter::default(), Size::new(4, 2));
    app.send(Message::StartTimer);

    assert_eq!(app.elapsed(), Duration::ZERO);
    assert_eq!(app.application().count, 0);
    app.advance(Duration::from_secs(1));
    assert_eq!(app.application().count, 0);
    app.advance(Duration::from_secs(1));
    assert_eq!(app.application().count, 1);
}

#[test]
fn output_outcomes_preserve_committed_frame_and_force_repaint() {
    let mut app = TestApp::new(Counter::default(), Size::new(4, 2));
    let initial = app.frame().clone();

    app.defer_next_output();
    let deferred = app.send(Message::Increment);
    assert_eq!(deferred.outcome, SettleOutcome::Deferred);
    assert_eq!(app.frame(), &initial);

    let applied = app.settle();
    assert_eq!(applied.outcome, SettleOutcome::Settled);
    assert_eq!(app.frame().characters(), "1   \nadd ");

    app.make_next_output_unknown();
    let unknown = app.send(Message::Increment);
    assert_eq!(unknown.outcome, SettleOutcome::StateUnknown);
    assert_eq!(app.frame().characters(), "1   \nadd ");

    app.settle();
    assert_eq!(app.frame().characters(), "2   \nadd ");
    assert!(
        app.last_frame_patch()
            .is_some_and(|patch| patch.full_repaint)
    );
}

#[test]
fn output_errors_preserve_frame_and_recover_with_a_full_repaint() {
    let mut app = TestApp::new(Counter::default(), Size::new(4, 2));
    let initial = app.frame().clone();

    app.fail_next_output();
    let error = app.try_send(Message::Increment);
    assert!(matches!(error, Err(TestError::Backend(TestBackendError))));
    assert_eq!(app.frame(), &initial);

    app.settle();
    assert_eq!(app.frame().characters(), "1   \nadd ");
    assert!(
        app.last_frame_patch()
            .is_some_and(|patch| patch.full_repaint)
    );
}

#[test]
fn missed_invalidation_recovery_commits_before_exactly_once_dispatch() {
    let mut app = TestApp::new(
        MissedInvalidationApp {
            expanded: false,
            activations: 0,
        },
        Size::new(3, 1),
    );
    app.send(MissedInvalidationMessage::Expand);
    app.defer_next_output();

    let (dispatch, settle) = app.event(UiEvent::Pointer(PointerEvent {
        kind: PointerEventKind::Moved,
        position: Point::ORIGIN,
        modifiers: UiKeyModifiers::NONE,
    }));

    assert_eq!(dispatch.messages, 1);
    assert_eq!(app.application().activations, 1);
    assert_eq!(app.frame().characters(), "new");
    assert_eq!(settle.outcome, SettleOutcome::Settled);
    assert_eq!(settle.committed_frames, 1);
    assert_eq!(app.frame_patches().len(), 3);
}

#[test]
fn missed_invalidation_recovery_retains_event_after_output_error() {
    let mut app = TestApp::new(
        MissedInvalidationApp {
            expanded: false,
            activations: 0,
        },
        Size::new(3, 1),
    );
    app.send(MissedInvalidationMessage::Expand);
    app.fail_next_output();
    let event = UiEvent::Pointer(PointerEvent {
        kind: PointerEventKind::Moved,
        position: Point::ORIGIN,
        modifiers: UiKeyModifiers::NONE,
    });

    let error = app.try_event(event.clone());

    assert!(matches!(error, Err(TestError::Backend(TestBackendError))));
    assert_eq!(app.application().activations, 0);
    assert_eq!(app.frame().characters(), "old");

    let recovery = app.settle();
    assert_eq!(recovery.committed_frames, 1);
    assert_eq!(app.application().activations, 0);
    assert_eq!(app.frame().characters(), "new");

    let retry = app.try_event(event);
    let (dispatch, settle) = match retry {
        Ok(reports) => reports,
        Err(error) => panic!("event recovery failed: {error}"),
    };

    assert_eq!(dispatch.messages, 1);
    assert_eq!(app.application().activations, 1);
    assert_eq!(app.frame().characters(), "new");
    assert_eq!(settle.outcome, SettleOutcome::Settled);
    assert_eq!(settle.turns, 3);
    assert_eq!(settle.updates, 1);
    assert_eq!(settle.committed_frames, 0);
}

#[test]
fn missed_invalidation_recovery_rejects_a_different_event() {
    let mut app = TestApp::new(
        MissedInvalidationApp {
            expanded: false,
            activations: 0,
        },
        Size::new(3, 1),
    );
    app.send(MissedInvalidationMessage::Expand);
    app.fail_next_output();
    let pending = UiEvent::Pointer(PointerEvent {
        kind: PointerEventKind::Moved,
        position: Point::ORIGIN,
        modifiers: UiKeyModifiers::NONE,
    });
    assert!(matches!(
        app.try_event(pending.clone()),
        Err(TestError::Backend(TestBackendError))
    ));

    let different = app.try_event(UiEvent::TerminalFocusGained);

    assert!(matches!(
        different,
        Err(TestError::RecoveryEventMismatch {
            pending: retained,
            received: UiEvent::TerminalFocusGained,
        }) if retained == pending
    ));
    assert_eq!(app.application().activations, 0);

    let retry = app.try_event(pending);
    assert!(retry.is_ok());
    assert_eq!(app.application().activations, 1);
    assert_eq!(app.frame().characters(), "new");
}

#[test]
fn retained_event_recomposes_again_when_view_changes_before_retry() {
    let mut app = TestApp::new(
        MissedInvalidationApp {
            expanded: false,
            activations: 0,
        },
        Size::new(3, 1),
    );
    app.send(MissedInvalidationMessage::Expand);
    app.fail_next_output();
    let event = UiEvent::Pointer(PointerEvent {
        kind: PointerEventKind::Moved,
        position: Point::ORIGIN,
        modifiers: UiKeyModifiers::NONE,
    });
    assert!(matches!(
        app.try_event(event.clone()),
        Err(TestError::Backend(TestBackendError))
    ));
    assert_eq!(app.settle().committed_frames, 1);
    app.application_mut().expanded = false;

    let retry = app.try_event(event);
    let (dispatch, settle) = match retry {
        Ok(reports) => reports,
        Err(error) => panic!("event recovery failed: {error}"),
    };

    assert_eq!(dispatch.messages, 0);
    assert_eq!(app.application().activations, 0);
    assert_eq!(app.frame().characters(), "old");
    assert_eq!(settle.committed_frames, 1);
}

#[test]
fn missed_invalidation_recovery_does_not_swallow_duplicate_keys() {
    let mut app = TestApp::new(DuplicateKeyApp { invalid: false }, Size::new(5, 1));
    app.application_mut().invalid = true;

    let error = app.try_event(UiEvent::TerminalFocusGained);

    assert!(matches!(
        error,
        Err(TestError::Reconcile(ReconcileError::DuplicateSiblingKey(
            Key::String(key)
        ))) if key.as_ref() == "duplicate"
    ));
}

#[test]
fn resize_repaints_at_the_new_size() {
    let mut app = TestApp::new(Counter::default(), Size::new(4, 2));

    app.resize(Size::new(6, 2));

    assert_eq!(app.frame().size(), Size::new(6, 2));
    assert_eq!(app.frame().characters(), "0     \nadd   ");
}

#[test]
fn generic_resize_events_and_zero_area_frames_update_the_snapshot() {
    let mut app = TestApp::new(Counter::default(), Size::new(4, 2));

    app.terminal_event(TerminalEvent::Resize(Size::new(5, 2)));
    assert_eq!(app.frame().size(), Size::new(5, 2));
    assert_eq!(app.frame().characters(), "0    \nadd  ");

    app.event(UiEvent::Resize(Size::new(6, 2)));
    assert_eq!(app.frame().size(), Size::new(6, 2));
    assert_eq!(app.frame().characters(), "0     \nadd   ");

    app.resize(Size::new(0, 3));
    assert_eq!(app.frame().size(), Size::new(0, 3));
    assert_eq!(app.frame().characters(), "\n\n");

    app.terminal_event(TerminalEvent::Resize(Size::new(4, 0)));
    assert_eq!(app.frame().size(), Size::new(4, 0));
    assert_eq!(app.frame().characters(), "");
}
