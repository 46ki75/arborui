use std::{cell::Cell, num::NonZeroUsize, time::Duration};

use arborui_core::{Rect, Size};
use arborui_layout::{Dimension, LayoutStyle};
use arborui_render::RgbaImage;
use arborui_runtime::{Application, Command, UpdateContext};
use arborui_ui::{
    Element, EventPhase, Invalidation, KeyAction, KeyModifiers as UiKeyModifiers, PointerEvent,
    PointerEventKind, ReconcileError, UiKey, UiKeyEvent,
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

struct ImageApp {
    image: RgbaImage,
}

struct UnsafeTextApp;

impl Application for UnsafeTextApp {
    type Message = ();

    fn update(
        &mut self,
        _message: Self::Message,
        _context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        Element::text("a\u{1b}b\u{b}c\u{2028}d")
    }
}

#[test]
fn application_text_omits_controls_and_honors_mandatory_breaks() {
    for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
        let app = TestApp::with_width_policy(UnsafeTextApp, Size::new(2, 3), policy);
        assert_eq!(app.frame().characters(), "ab\nc \nd ");
        assert!(
            app.frame_patches()
                .iter()
                .all(|patch| patch.validate_for_width_policy(policy).is_ok())
        );
    }
}

impl Application for ImageApp {
    type Message = RgbaImage;

    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        self.image = message;
        context.invalidate(Invalidation::Paint);
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        Element::custom("image", [])
            .layout(LayoutStyle::new().size(Dimension::cells(2), Dimension::cells(1)))
            .paint(self.image.id().get(), |size, canvas| {
                canvas.draw_image(Rect::new(0, 0, size.width, size.height), &self.image)?;
                Ok(())
            })
    }
}

#[test]
fn test_frame_retains_native_image_scene() -> Result<(), Box<dyn std::error::Error>> {
    let image = RgbaImage::new(2, 1, vec![255; 8])?;
    let id = image.id();
    let mut app = TestApp::new(ImageApp { image }, Size::new(2, 1));

    assert_eq!(app.frame().images().placements().len(), 1);
    assert_eq!(app.frame().images().placements()[0].image().id(), id);
    let replacement = RgbaImage::new(2, 1, vec![0; 8])?;
    let replacement_id = replacement.id();
    app.send(replacement);
    let patch = app.last_frame_patch().ok_or("missing image update patch")?;
    assert_eq!(patch.runs.len(), 1);
    assert_eq!(patch.runs[0].position, Point::new(0, 0));
    assert_eq!(patch.runs[0].cells.len(), 2);
    assert!(patch.images.is_some());
    assert_eq!(
        app.frame().images().placements()[0].image().id(),
        replacement_id
    );
    Ok(())
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

struct RecoveryOutputApp {
    rekeyed: bool,
    handler_calls: Cell<usize>,
    activations: usize,
    label: String,
}

impl Default for RecoveryOutputApp {
    fn default() -> Self {
        Self {
            rekeyed: false,
            handler_calls: Cell::new(0),
            activations: 0,
            label: "0".to_owned(),
        }
    }
}

impl Application for RecoveryOutputApp {
    type Message = ();

    fn update(
        &mut self,
        (): Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        self.activations += 1;
        self.label = self.activations.to_string();
        context.invalidate(Invalidation::Paint);
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        Element::custom(
            "button",
            [Element::text(&self.label).key(if self.rekeyed { "new" } else { "old" })],
        )
        .key("activate")
        .focusable(true)
        .on_event(EventPhase::Target, |event, context| {
            if matches!(
                event,
                UiEvent::Key(UiKeyEvent {
                    key: UiKey::Enter,
                    action: KeyAction::Press,
                    ..
                })
            ) {
                self.handler_calls.set(self.handler_calls.get() + 1);
                context.emit(());
                if self.activations == 0 {
                    context.mark_handled();
                    context.prevent_default();
                    context.stop_propagation();
                }
            }
        })
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
fn constructors_record_initial_patches_by_default() -> Result<(), TestError> {
    let size = Size::new(4, 2);
    let apps = [
        TestApp::new(Counter::default(), size),
        TestApp::try_new(Counter::default(), size)?,
        TestApp::with_width_policy(Counter::default(), size, WidthPolicy::Unicode),
        TestApp::try_with_width_policy(Counter::default(), size, WidthPolicy::Unicode)?,
        TestApp::with_runtime_options(Counter::default(), size, RuntimeOptions::default()),
        TestApp::try_with_runtime_options(Counter::default(), size, RuntimeOptions::default())?,
        TestApp::with_width_policy_and_runtime_options(
            Counter::default(),
            size,
            WidthPolicy::Unicode,
            RuntimeOptions::default(),
        ),
        TestApp::try_with_width_policy_and_runtime_options(
            Counter::default(),
            size,
            WidthPolicy::Unicode,
            RuntimeOptions::default(),
        )?,
        TestApp::with_options(Counter::default(), size, TestAppOptions::default()),
        TestApp::try_with_options(Counter::default(), size, TestAppOptions::default())?,
    ];
    for app in apps {
        assert_eq!(app.frame_patches().len(), 1);
        assert!(
            app.last_frame_patch()
                .is_some_and(|patch| patch.full_repaint)
        );
        assert_eq!(app.frame().characters(), "0   \nadd ");
    }
    Ok(())
}

#[test]
fn nonrecording_preserves_configuration_frames_and_settle_reports() {
    for width_policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
        let runtime_options = RuntimeOptions::new()
            .with_event_ingress_capacity(NonZeroUsize::MIN)
            .with_interrupt(InterruptPolicy::Ignore);
        let counter = || Counter {
            label: "\u{b7}".to_owned(),
            ..Counter::default()
        };
        let mut recording = TestApp::with_width_policy_and_runtime_options(
            counter(),
            Size::new(4, 2),
            width_policy,
            runtime_options,
        );
        let mut nonrecording = TestApp::try_with_options(
            counter(),
            Size::new(4, 2),
            TestAppOptions {
                width_policy,
                runtime_options,
                record_patches: false,
            },
        )
        .expect("nonrecording initialization must settle");
        assert_eq!(recording.frame(), nonrecording.frame());
        assert!(nonrecording.frame_patches().is_empty());
        assert!(nonrecording.last_frame_patch().is_none());
        for outcome in [
            SettleOutcome::Deferred,
            SettleOutcome::StateUnknown,
            SettleOutcome::Settled,
        ] {
            for app in [&mut recording, &mut nonrecording] {
                match outcome {
                    SettleOutcome::Deferred => app.defer_next_output(),
                    SettleOutcome::StateUnknown => app.make_next_output_unknown(),
                    _ => {}
                }
                let proxy = app.event_proxy();
                assert!(proxy.send(Message::Increment).is_ok());
                assert_eq!(
                    proxy
                        .send(Message::Increment)
                        .expect_err("capacity one")
                        .kind(),
                    EventProxySendErrorKind::Full
                );
            }
            let expected = recording.settle();
            assert_eq!(expected.outcome, outcome);
            assert_eq!(expected.updates, 1);
            assert_eq!(nonrecording.settle(), expected);
            assert_eq!(recording.frame(), nonrecording.frame());
            assert_eq!(recording.settle(), nonrecording.settle());
            assert_eq!(recording.frame(), nonrecording.frame());
        }
        for app in [&mut recording, &mut nonrecording] {
            app.fail_next_output();
            assert!(matches!(
                app.try_send(Message::Increment),
                Err(TestError::Backend(TestBackendError))
            ));
        }
        assert_eq!(recording.frame(), nonrecording.frame());
        assert_eq!(recording.settle(), nonrecording.settle());
        assert_eq!(
            recording.resize(Size::new(6, 3)),
            nonrecording.resize(Size::new(6, 3))
        );
        assert_eq!(
            recording.send(Message::StartTimer),
            nonrecording.send(Message::StartTimer)
        );
        assert_eq!(
            recording.advance(Duration::from_secs(2)),
            nonrecording.advance(Duration::from_secs(2))
        );
        assert_eq!(
            recording.key_with(
                KeyCode::Character('c'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press
            ),
            nonrecording.key_with(
                KeyCode::Character('c'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press
            )
        );
        assert!(!nonrecording.is_quitting());
        assert_eq!(recording.frame(), nonrecording.frame());
        assert_eq!(
            recording.application().count,
            nonrecording.application().count
        );
        for app in [&recording, &nonrecording] {
            let metrics = app.event_proxy().metrics();
            // Ingress latency uses wall time; only counters are deterministic.
            assert_eq!(
                (metrics.capacity, metrics.depth, metrics.high_water_mark),
                (1, 0, 1)
            );
            assert_eq!(
                (metrics.accepted, metrics.dequeued, metrics.rejected),
                (3, 3, 3)
            );
            assert!(!metrics.closed);
        }
        assert!(nonrecording.frame_patches().is_empty());
        assert!(nonrecording.last_frame_patch().is_none());
    }
}

#[test]
fn nonrecording_recovery_dispatches_exactly_once() {
    for external_settle in [false, true] {
        let mut recording = TestApp::new(RecoveryOutputApp::default(), Size::new(1, 1));
        let mut nonrecording = TestApp::with_options(
            RecoveryOutputApp::default(),
            Size::new(1, 1),
            TestAppOptions {
                record_patches: false,
                ..TestAppOptions::default()
            },
        );
        let event = UiEvent::Key(UiKeyEvent {
            key: UiKey::Enter,
            modifiers: UiKeyModifiers::NONE,
            action: KeyAction::Press,
        });
        for app in [&mut recording, &mut nonrecording] {
            app.key(KeyCode::Tab);
            app.application_mut().rekeyed = true;
            for _ in 0..3 {
                app.fail_next_output();
                assert!(matches!(
                    app.try_event(event.clone()),
                    Err(TestError::Backend(TestBackendError))
                ));
                assert_eq!(app.application().handler_calls.get(), 1);
                assert_eq!(app.application().activations, 1);
                assert_eq!(app.frame().characters(), "0");
                assert!(matches!(
                    app.try_event(UiEvent::Resize(Size::new(2, 1))),
                    Err(TestError::RecoveryEventMismatch { .. })
                ));
            }
        }
        if external_settle {
            assert_eq!(recording.settle(), nonrecording.settle());
        }
        let (dispatch, settle) = recording.event(event.clone());
        assert_eq!(nonrecording.event(event.clone()), (dispatch, settle));
        assert_eq!(dispatch.messages, 1);
        assert_eq!(settle.updates, 0);
        assert_eq!(recording.frame(), nonrecording.frame());
        assert_eq!(nonrecording.application().handler_calls.get(), 1);
        assert_eq!(nonrecording.application().activations, 1);
        assert_eq!(nonrecording.frame().characters(), "1");
        assert_eq!(recording.event(event.clone()), nonrecording.event(event));
        assert_eq!(nonrecording.application().handler_calls.get(), 2);
        assert_eq!(nonrecording.application().activations, 2);
        assert!(nonrecording.frame_patches().is_empty());
        assert!(nonrecording.last_frame_patch().is_none());
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
    app.defer_next_output();
    app.make_next_output_unknown();
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
    assert_eq!(settle.turns, 5);
    assert_eq!(settle.updates, 1);
    assert_eq!(settle.committed_frames, 0);
}

#[test]
fn recovery_output_error_retry_is_exactly_once() {
    let mut app = TestApp::new(RecoveryOutputApp::default(), Size::new(1, 1));
    app.key(KeyCode::Tab);
    assert_eq!(app.focused_key(), Some(Key::from("activate")));
    let initial = app.frame().clone();
    let patches = app.frame_patches().len();
    app.application_mut().rekeyed = true;
    let event = UiEvent::Key(UiKeyEvent {
        key: UiKey::Enter,
        modifiers: UiKeyModifiers::NONE,
        action: KeyAction::Press,
    });
    app.fail_next_output();

    assert!(matches!(
        app.try_event(event.clone()),
        Err(TestError::Backend(TestBackendError))
    ));
    // The pixel-identical recovery committed without consuming the scripted
    // failure. The handler and update ran before their visible output failed.
    assert_eq!(app.application().handler_calls.get(), 1);
    assert_eq!(app.application().activations, 1);
    assert_eq!(app.frame(), &initial);
    assert_eq!(app.frame_patches().len(), patches + 1);

    for _ in 0..2 {
        let patches = app.frame_patches().len();
        app.fail_next_output();
        let different = UiEvent::Resize(Size::new(2, 1));
        assert!(matches!(
            app.try_event(different.clone()),
            Err(TestError::RecoveryEventMismatch { pending, received })
                if pending == event && received == different
        ));
        assert_eq!(app.frame_patches().len(), patches);
        assert!(matches!(
            app.try_event(event.clone()),
            Err(TestError::Backend(TestBackendError))
        ));
        assert_eq!(app.application().handler_calls.get(), 1);
        assert_eq!(app.application().activations, 1);
        assert_eq!(app.frame(), &initial);
        assert_eq!(app.frame_patches().len(), patches + 1);
    }

    let (dispatch, settle) = app
        .try_event(event.clone())
        .expect("retry must finish settling");

    assert_eq!(app.application().handler_calls.get(), 1);
    assert_eq!(app.application().activations, 1);
    assert_eq!(app.frame().characters(), "1");
    assert_eq!(
        dispatch,
        DispatchReport {
            messages: 1,
            handled: true,
            default_prevented: true,
            propagation_stopped: true,
        }
    );
    assert_eq!(settle.outcome, SettleOutcome::Settled);
    assert_eq!(settle.turns, 2);
    assert_eq!(settle.updates, 0);
    assert_eq!(settle.committed_frames, 2);
    assert_eq!(app.frame().size(), Size::new(1, 1));
    assert!(
        app.last_frame_patch()
            .is_some_and(|patch| patch.full_repaint)
    );

    let (dispatch, settle) = app.event(event);
    assert_eq!(app.application().handler_calls.get(), 2);
    assert_eq!(app.application().activations, 2);
    assert_eq!(app.frame().characters(), "2");
    assert_eq!(settle.updates, 1);
    assert_eq!(dispatch.messages, 1);
    assert!(!dispatch.handled && !dispatch.default_prevented && !dispatch.propagation_stopped);
}

#[test]
fn recovery_output_error_external_settle_then_retry_is_exactly_once() {
    let mut app = TestApp::new(RecoveryOutputApp::default(), Size::new(1, 1));
    app.key(KeyCode::Tab);
    app.application_mut().rekeyed = true;
    let event = UiEvent::Key(UiKeyEvent {
        key: UiKey::Enter,
        modifiers: UiKeyModifiers::NONE,
        action: KeyAction::Press,
    });
    app.fail_next_output();

    assert!(matches!(
        app.try_event(event.clone()),
        Err(TestError::Backend(TestBackendError))
    ));
    assert_eq!(app.application().handler_calls.get(), 1);
    assert_eq!(app.application().activations, 1);
    assert_eq!(app.frame().characters(), "0");

    let recovery = app
        .try_settle()
        .expect("external settle must repair output");
    assert_eq!(recovery.outcome, SettleOutcome::Settled);
    assert_eq!(recovery.updates, 0);
    assert_eq!(recovery.committed_frames, 1);
    assert_eq!(app.frame().characters(), "1");

    let patches = app.frame_patches().len();
    let different = UiEvent::Resize(Size::new(2, 1));
    assert!(matches!(
        app.try_event(different.clone()),
        Err(TestError::RecoveryEventMismatch { pending, received })
            if pending == event && received == different
    ));
    assert_eq!(app.frame_patches().len(), patches);
    assert_eq!(app.frame().size(), Size::new(1, 1));

    let (dispatch, settle) = app
        .try_event(event.clone())
        .expect("retry must acknowledge dispatch");

    assert_eq!(app.application().handler_calls.get(), 1);
    assert_eq!(app.application().activations, 1);
    assert_eq!(app.frame().characters(), "1");
    assert_eq!(
        dispatch,
        DispatchReport {
            messages: 1,
            handled: true,
            default_prevented: true,
            propagation_stopped: true,
        }
    );
    assert_eq!(settle.outcome, SettleOutcome::Settled);
    assert_eq!(settle.turns, 2);
    assert_eq!(settle.updates, 0);
    assert_eq!(settle.committed_frames, 1);
    assert_eq!(app.frame_patches().len(), patches);

    app.event(event);
    assert_eq!(app.application().handler_calls.get(), 2);
    assert_eq!(app.application().activations, 2);
    assert_eq!(app.frame().characters(), "2");
    assert_eq!(app.frame().size(), Size::new(1, 1));
}

#[test]
fn recovery_post_dispatch_non_applied_output_acknowledges_event() {
    for outcome in [SettleOutcome::Deferred, SettleOutcome::StateUnknown] {
        for retry_after_error in [false, true] {
            let mut app = TestApp::new(RecoveryOutputApp::default(), Size::new(1, 1));
            app.key(KeyCode::Tab);
            app.application_mut().rekeyed = true;
            let event = UiEvent::Key(UiKeyEvent {
                key: UiKey::Enter,
                modifiers: UiKeyModifiers::NONE,
                action: KeyAction::Press,
            });
            if retry_after_error {
                app.fail_next_output();
                assert!(matches!(
                    app.try_event(event.clone()),
                    Err(TestError::Backend(TestBackendError))
                ));
                assert_eq!(app.application().handler_calls.get(), 1);
                assert_eq!(app.application().activations, 1);
            }
            if outcome == SettleOutcome::Deferred {
                app.defer_next_output();
            } else {
                app.make_next_output_unknown();
            }

            let (dispatch, settle) = app
                .try_event(event.clone())
                .expect("non-applied output must return successful reports");

            assert_eq!(dispatch.messages, 1);
            assert!(dispatch.handled && dispatch.default_prevented && dispatch.propagation_stopped);
            assert_eq!(app.application().handler_calls.get(), 1);
            assert_eq!(app.application().activations, 1);
            assert_eq!(app.frame().characters(), "0");
            assert_eq!(settle.outcome, outcome);
            assert_eq!(settle.turns, 2);
            assert_eq!(settle.updates, usize::from(!retry_after_error));
            assert_eq!(settle.committed_frames, 1);

            app.settle();
            assert_eq!(app.frame().characters(), "1");
            app.event(event);
            assert_eq!(app.application().handler_calls.get(), 2);
            assert_eq!(app.application().activations, 2);
            assert_eq!(app.frame().characters(), "2");
        }
    }
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

    let different = UiEvent::Resize(Size::new(6, 1));
    let patches = app.frame_patches().len();
    let rejected = app.try_event(different.clone());

    assert!(matches!(
        rejected,
        Err(TestError::RecoveryEventMismatch {
            pending: retained,
            received,
        }) if retained == pending && received == different
    ));
    assert_eq!(app.application().activations, 0);
    assert_eq!(app.frame_patches().len(), patches);
    assert_eq!(app.frame().characters(), "old");

    let retry = app.try_event(pending);
    assert!(retry.is_ok());
    assert_eq!(app.application().activations, 1);
    assert_eq!(app.frame().characters(), "new");
    assert_eq!(app.frame().size(), Size::new(3, 1));
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

#[test]
fn control_c_quits_through_the_public_harness() {
    // Raw mode suppresses SIGINT, so Ctrl+C must exit as a key event.
    let mut app = TestApp::new(Counter::default(), Size::new(4, 2));
    app.key_with(
        KeyCode::Character('c'),
        KeyModifiers::CONTROL,
        KeyEventKind::Press,
    );
    assert!(app.is_quitting());
}

#[test]
fn ignore_policy_leaves_control_c_to_the_application() {
    let mut app = TestApp::with_runtime_options(
        Counter::default(),
        Size::new(4, 2),
        RuntimeOptions::new().with_interrupt(InterruptPolicy::Ignore),
    );
    app.key_with(
        KeyCode::Character('c'),
        KeyModifiers::CONTROL,
        KeyEventKind::Press,
    );
    assert!(!app.is_quitting());
}
