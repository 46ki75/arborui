use std::time::Duration;

use arborui_core::Size;
use arborui_runtime::{Application, Command, UpdateContext};
use arborui_ui::{Element, Invalidation};

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
