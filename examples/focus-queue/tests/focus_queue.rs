//! Downstream pilot tests using only the public application and test facades.

use std::{
    num::NonZeroUsize,
    sync::{Arc, Barrier, Mutex, MutexGuard},
    thread,
    time::Duration,
};

use arborui::{CursorShape, CursorVisibility, EventProxy, Modifier, Point, TextBuffer};
use arborui_example_focus_queue::{ActivityCancellation, ActivityStatus, FocusQueue, Message};
use arborui_test::{
    EventProxySendErrorKind, Key, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    RuntimeOptions, Size, TestApp, TestCellContent, TestFrame,
};

fn focused_label(frame: &TestFrame) -> String {
    frame
        .cells()
        .iter()
        .filter(|cell| cell.style.modifiers.contains(Modifier::REVERSED))
        .filter_map(|cell| match &cell.content {
            TestCellContent::Grapheme { text, .. } => Some(text.as_ref()),
            TestCellContent::Empty | TestCellContent::Continuation { .. } => None,
        })
        .collect()
}

struct ActivityLaunch {
    generation: u64,
    cancellation: ActivityCancellation,
    proxy: EventProxy<Message>,
}

#[derive(Clone, Default)]
struct ControlledActivity {
    launches: Arc<Mutex<Vec<ActivityLaunch>>>,
}

impl ControlledActivity {
    fn queue(&self) -> FocusQueue {
        let launches = Arc::clone(&self.launches);
        FocusQueue::with_activity_launcher(60, move |generation, cancellation, proxy| {
            lock(&launches).push(ActivityLaunch {
                generation,
                cancellation,
                proxy,
            });
            Ok(())
        })
    }

    fn launch(&self, index: usize) -> (u64, ActivityCancellation, EventProxy<Message>) {
        let launches = lock(&self.launches);
        let launch = launches
            .get(index)
            .unwrap_or_else(|| panic!("missing controlled activity launch {index}"));
        (
            launch.generation,
            launch.cancellation.clone(),
            launch.proxy.clone(),
        )
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn representative_frames_match_snapshots() {
    let empty = TestApp::new(FocusQueue::default(), Size::new(60, 16));
    assert_eq!(empty.application().task_count(), 0);
    insta::assert_snapshot!("focus_queue_empty", empty.frame());

    let mut task = TestApp::new(FocusQueue::default(), Size::new(60, 16));
    task.paste("Write pilot notes");
    task.key(KeyCode::Enter);
    assert_eq!(task.application().task_count(), 1);
    insta::assert_snapshot!("focus_queue_task_added", task.frame());

    task.key(KeyCode::Tab);
    task.key(KeyCode::Tab);
    task.key(KeyCode::Enter);
    assert_eq!(task.application().task_completed(0), Some(true));
    insta::assert_snapshot!("focus_queue_task_completed", task.frame());

    task.key(KeyCode::Tab);
    task.key(KeyCode::Enter);
    assert_eq!(task.application().editing_task_id(), Some(1));
    assert_eq!(task.focused_key(), Some(Key::from("edit-title")));
    insta::assert_snapshot!("focus_queue_edit_dialog", task.frame());

    let mut scrolled = TestApp::new(FocusQueue::default(), Size::new(60, 16));
    for index in 1..=8 {
        scrolled.send(Message::DraftChanged(TextBuffer::new(format!(
            "Task {index}"
        ))));
        scrolled.send(Message::AddTask);
    }
    let scroll_down = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        position: Point::new(2, 6),
        modifiers: KeyModifiers::NONE,
    };
    scrolled.mouse(scroll_down);
    scrolled.mouse(scroll_down);
    assert_eq!(scrolled.application().scroll_y(), 2);
    insta::assert_snapshot!("focus_queue_scrolled", scrolled.frame());

    let activity_source = ControlledActivity::default();
    let mut activity = TestApp::new(activity_source.queue(), Size::new(60, 16));
    activity.send(Message::ShowActivity);
    assert_eq!(
        activity.application().activity_status(),
        ActivityStatus::Idle
    );
    assert!(activity.application().showing_activity());
    insta::assert_snapshot!("focus_queue_activity_idle", activity.frame());
}

#[test]
fn first_typed_character_is_visible_in_the_input() {
    let mut app = TestApp::new(FocusQueue::default(), Size::new(60, 16));

    app.key(KeyCode::Character('m'));

    assert_eq!(app.application().draft(), "m");
    assert!(matches!(
        app.frame().cell(Point::new(1, 1)).map(|cell| &cell.content),
        Some(TestCellContent::Grapheme { text, .. }) if text.as_ref() == "m"
    ));
}

#[test]
fn tab_focus_is_visible_on_buttons() {
    let mut app = TestApp::new(FocusQueue::default(), Size::new(60, 16));

    let input_cursor = app.frame().cursor();
    assert_eq!(input_cursor.visibility, CursorVisibility::Visible);
    assert_eq!(input_cursor.shape, CursorShape::Bar);

    app.key(KeyCode::Tab);

    assert_eq!(app.focused_key(), Some(Key::from("add-task")));
    assert_eq!(app.frame().cursor().visibility, CursorVisibility::Hidden);
    assert_eq!(focused_label(app.frame()), "Add");

    app.key(KeyCode::Tab);

    assert_eq!(app.focused_key(), Some(Key::from("timer-toggle")));
    assert_eq!(focused_label(app.frame()), "Start");
}

#[test]
fn tasks_are_added_and_completed_through_widgets() {
    let mut app = TestApp::new(FocusQueue::with_focus_seconds(60), Size::new(72, 18));

    assert_eq!(app.focused_key(), Some(Key::from("new-task")));
    app.paste("Write pilot notes");
    app.key(KeyCode::Enter);

    assert_eq!(app.application().task_count(), 1);
    assert_eq!(app.application().task_title(0), Some("Write pilot notes"));
    assert_eq!(app.application().draft(), "");
    assert!(app.frame().characters().contains("Write pilot notes"));

    app.key(KeyCode::Tab);
    app.key(KeyCode::Tab);
    assert_eq!(
        app.focused_key(),
        Some(Key::from("task-1-toggle")),
        "the task toggle follows the input and Add button"
    );
    app.key(KeyCode::Enter);

    assert_eq!(app.application().task_completed(0), Some(true));
    assert!(app.frame().characters().contains("[x] Write pilot notes"));
    assert!(app.frame().characters().contains("0 open / 1 complete"));

    app.key(KeyCode::Tab);
    assert_eq!(app.focused_key(), Some(Key::from("task-1-edit")));
    app.key(KeyCode::Tab);
    assert_eq!(app.focused_key(), Some(Key::from("task-1-delete")));
    app.key(KeyCode::Enter);
    assert_eq!(app.application().task_count(), 0);
    assert!(app.frame().characters().contains("No tasks yet"));
}

#[test]
fn edit_dialog_traps_focus_cancels_and_restores_the_origin() {
    let mut app = TestApp::new(FocusQueue::default(), Size::new(72, 18));
    app.paste("Original task");
    app.key(KeyCode::Enter);
    app.key(KeyCode::Tab);
    app.key(KeyCode::Tab);
    app.key(KeyCode::Tab);
    assert_eq!(app.focused_key(), Some(Key::from("task-1-edit")));

    app.key(KeyCode::Enter);
    assert_eq!(app.application().editing_task_id(), Some(1));
    assert_eq!(app.focused_key(), Some(Key::from("edit-title")));

    app.key(KeyCode::Tab);
    assert_eq!(app.focused_key(), Some(Key::from("edit-completed")));
    app.key(KeyCode::Tab);
    assert_eq!(app.focused_key(), Some(Key::from("edit-save")));
    app.key(KeyCode::Tab);
    assert_eq!(app.focused_key(), Some(Key::from("edit-cancel")));
    app.key(KeyCode::Tab);
    assert_eq!(app.focused_key(), Some(Key::from("edit-title")));
    app.key_with(KeyCode::Tab, KeyModifiers::SHIFT, KeyEventKind::Press);
    assert_eq!(app.focused_key(), Some(Key::from("edit-cancel")));

    app.key(KeyCode::Escape);
    assert_eq!(app.application().editing_task_id(), None);
    assert_eq!(app.application().task_title(0), Some("Original task"));
    assert_eq!(app.focused_key(), Some(Key::from("task-1-edit")));
}

#[test]
fn edit_dialog_saves_unicode_and_checkbox_state() {
    let mut app = TestApp::new(FocusQueue::default(), Size::new(72, 18));
    app.paste("Original task");
    app.key(KeyCode::Enter);
    app.send(Message::OpenEdit(1));

    app.key_with(
        KeyCode::Character('a'),
        KeyModifiers::CONTROL,
        KeyEventKind::Press,
    );
    app.paste("a👩‍💻界");
    app.key(KeyCode::Home);
    app.key(KeyCode::Right);
    app.key(KeyCode::Delete);
    assert_eq!(app.application().edit_title(), Some("a界"));

    app.key(KeyCode::Tab);
    app.key(KeyCode::Enter);
    assert_eq!(app.application().edit_completed(), Some(true));
    app.key(KeyCode::Tab);
    app.key(KeyCode::Enter);

    assert_eq!(app.application().editing_task_id(), None);
    assert_eq!(app.application().task_title(0), Some("a界"));
    assert_eq!(app.application().task_completed(0), Some(true));
    insta::assert_snapshot!("focus_queue_unicode_saved", app.frame());
}

#[test]
fn edit_dialog_scrim_blocks_the_background() {
    let mut app = TestApp::new(FocusQueue::default(), Size::new(72, 18));
    app.paste("Keep this task");
    app.key(KeyCode::Enter);
    app.send(Message::OpenEdit(1));

    app.click(Point::new(70, 6));

    assert_eq!(app.application().editing_task_id(), Some(1));
    assert_eq!(app.application().task_count(), 1);
    assert_eq!(app.application().task_title(0), Some("Keep this task"));
}

#[test]
fn timer_pause_invalidates_an_already_scheduled_tick() {
    let mut app = TestApp::new(FocusQueue::with_focus_seconds(3), Size::new(60, 16));

    app.send(Message::StartTimer);
    app.advance(Duration::from_secs(1));
    assert_eq!(app.application().remaining_seconds(), 2);

    app.send(Message::PauseTimer);
    app.advance(Duration::from_secs(5));
    assert_eq!(app.application().remaining_seconds(), 2);

    app.send(Message::StartTimer);
    app.advance(Duration::from_secs(2));
    assert_eq!(app.application().remaining_seconds(), 1);
    assert!(app.application().timer_running());

    app.advance(Duration::from_secs(1));
    assert_eq!(app.application().remaining_seconds(), 0);
    assert!(!app.application().timer_running());
    assert!(app.frame().characters().contains("00:00"));
}

#[test]
fn wheel_scrolling_is_controlled_and_clamped_by_the_model() {
    let mut app = TestApp::new(FocusQueue::default(), Size::new(60, 16));
    for index in 1..=8 {
        app.send(Message::DraftChanged(TextBuffer::new(format!(
            "Task {index}"
        ))));
        app.send(Message::AddTask);
    }

    let scroll_down = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        position: Point::new(2, 6),
        modifiers: KeyModifiers::NONE,
    };
    app.mouse(scroll_down);
    app.mouse(scroll_down);
    assert_eq!(app.application().scroll_y(), 2);

    for _ in 0..20 {
        app.mouse(scroll_down);
    }
    assert_eq!(app.application().scroll_y(), 7);
    assert!(
        app.frame().characters().contains("Task 8"),
        "{}",
        app.frame().characters()
    );
}

#[test]
fn screen_navigation_preserves_queue_owned_state() {
    let activity_source = ControlledActivity::default();
    let mut app = TestApp::new(activity_source.queue(), Size::new(72, 18));
    app.paste("Keep queue state");
    app.key(KeyCode::Enter);
    app.send(Message::StartTimer);

    for _ in 0..8 {
        app.key(KeyCode::Tab);
    }
    assert_eq!(app.focused_key(), Some(Key::from("screen-activity")));
    app.key(KeyCode::Enter);

    assert!(app.application().showing_activity());
    assert_eq!(app.focused_key(), Some(Key::from("screen-activity")));
    assert!(app.frame().characters().contains("External work"));

    app.key_with(KeyCode::Tab, KeyModifiers::SHIFT, KeyEventKind::Press);
    assert_eq!(app.focused_key(), Some(Key::from("screen-queue")));
    app.key(KeyCode::Enter);

    assert!(!app.application().showing_activity());
    assert_eq!(app.application().task_title(0), Some("Keep queue state"));
    assert!(app.application().timer_running());
    assert!(app.frame().characters().contains("Keep queue state"));
}

#[test]
fn cancellation_signals_the_worker_and_rejects_racing_messages() {
    let activity_source = ControlledActivity::default();
    let mut app = TestApp::new(activity_source.queue(), Size::new(72, 18));
    app.send(Message::ShowActivity);
    app.send(Message::StartActivity);
    let (generation, cancellation, proxy) = activity_source.launch(0);

    assert_eq!(app.application().activity_status(), ActivityStatus::Running);
    assert!(!cancellation.is_cancelled());
    let item_sent = Arc::new(Barrier::new(2));
    let cancellation_sent = Arc::new(Barrier::new(2));
    let worker_item_sent = Arc::clone(&item_sent);
    let worker_cancellation_sent = Arc::clone(&cancellation_sent);
    let worker_cancellation = cancellation.clone();
    let worker = thread::spawn(move || {
        let first_sent = proxy
            .send(Message::ActivityItem {
                generation,
                text: "accepted before cancellation".to_owned(),
            })
            .is_ok();
        worker_item_sent.wait();
        worker_cancellation_sent.wait();
        let observed_cancellation = worker_cancellation.is_cancelled();
        let raced_item_sent = proxy
            .send(Message::ActivityItem {
                generation,
                text: "raced after cancellation".to_owned(),
            })
            .is_ok();
        let finish_sent = proxy.send(Message::ActivityFinished { generation }).is_ok();
        (
            first_sent,
            observed_cancellation,
            raced_item_sent,
            finish_sent,
        )
    });
    item_sent.wait();
    app.settle();
    assert_eq!(app.application().activity_item_count(), 1);

    app.send(Message::CancelActivity);
    assert!(cancellation.is_cancelled());
    assert_eq!(
        app.application().activity_status(),
        ActivityStatus::Cancelled
    );
    assert_ne!(app.application().activity_generation(), generation);

    cancellation_sent.wait();
    assert!(matches!(worker.join(), Ok((true, true, true, true))));
    app.settle();

    assert_eq!(
        app.application().activity_status(),
        ActivityStatus::Cancelled
    );
    assert_eq!(app.application().activity_item_count(), 1);
    assert_eq!(
        app.application().activity_item(0),
        Some("accepted before cancellation")
    );
}

#[test]
fn restarting_running_activity_cancels_and_replaces_its_generation() {
    let activity_source = ControlledActivity::default();
    let mut app = TestApp::new(activity_source.queue(), Size::new(72, 18));
    app.send(Message::StartActivity);
    let (first_generation, first_cancellation, first_proxy) = activity_source.launch(0);

    app.send(Message::StartActivity);
    let (second_generation, second_cancellation, second_proxy) = activity_source.launch(1);
    assert!(first_cancellation.is_cancelled());
    assert!(!second_cancellation.is_cancelled());
    assert_ne!(first_generation, second_generation);

    assert!(
        first_proxy
            .send(Message::ActivityItem {
                generation: first_generation,
                text: "stale replacement".to_owned(),
            })
            .is_ok()
    );
    assert!(
        second_proxy
            .send(Message::ActivityItem {
                generation: second_generation,
                text: "current replacement".to_owned(),
            })
            .is_ok()
    );
    app.settle();

    assert_eq!(app.application().activity_item_count(), 1);
    assert_eq!(
        app.application().activity_item(0),
        Some("current replacement")
    );
}

#[test]
fn synchronous_activity_launch_failure_settles_as_recoverable() {
    let cancellation = Arc::new(Mutex::new(None));
    let recorded_cancellation = Arc::clone(&cancellation);
    let queue =
        FocusQueue::with_activity_launcher(60, move |_generation, launch_cancellation, _proxy| {
            *lock(&recorded_cancellation) = Some(launch_cancellation);
            Err("worker capacity exhausted".to_owned())
        });
    let mut app = TestApp::new(queue, Size::new(72, 18));

    app.send(Message::ShowActivity);
    app.send(Message::StartActivity);

    assert_eq!(app.application().activity_status(), ActivityStatus::Failed);
    assert_eq!(
        app.application().activity_error(),
        Some("worker capacity exhausted")
    );
    assert!(app.frame().characters().contains("Retry"));
    assert!(
        lock(&cancellation)
            .as_ref()
            .is_some_and(ActivityCancellation::is_cancelled)
    );
}

#[test]
fn external_activity_settles_and_bounds_retained_history() {
    let activity_source = ControlledActivity::default();
    let mut app = TestApp::new(activity_source.queue(), Size::new(60, 16));
    app.send(Message::ShowActivity);
    app.send(Message::StartActivity);
    let (generation, cancellation, proxy) = activity_source.launch(0);

    for index in 0..35 {
        assert!(
            proxy
                .send(Message::ActivityItem {
                    generation,
                    text: format!("Remote update {index}"),
                })
                .is_ok()
        );
    }
    assert!(proxy.send(Message::ActivityFinished { generation }).is_ok());
    app.settle();

    assert!(cancellation.is_cancelled());
    assert_eq!(
        app.application().activity_status(),
        ActivityStatus::Completed
    );
    assert_eq!(app.application().activity_item_count(), 32);
    assert_eq!(app.application().activity_item(0), Some("Remote update 3"));
    assert_eq!(
        app.application().activity_item(31),
        Some("Remote update 34")
    );
    insta::assert_snapshot!("focus_queue_activity_complete", app.frame());
}

#[test]
fn bounded_activity_ingress_rejects_recovers_and_retries_new_item() {
    let activity_source = ControlledActivity::default();
    let options = RuntimeOptions::new()
        .with_event_ingress_capacity(NonZeroUsize::new(2).unwrap_or(NonZeroUsize::MIN));
    let mut app =
        TestApp::with_runtime_options(activity_source.queue(), Size::new(72, 18), options);
    app.send(Message::StartActivity);
    let (generation, cancellation, proxy) = activity_source.launch(0);

    for text in ["accepted one", "accepted two"] {
        assert!(
            proxy
                .send(Message::ActivityItem {
                    generation,
                    text: text.to_owned(),
                })
                .is_ok()
        );
    }
    let rejected = proxy
        .send(Message::ActivityItem {
            generation,
            text: "recovered three".to_owned(),
        })
        .expect_err("third item should exceed configured ingress capacity");
    assert_eq!(rejected.kind(), EventProxySendErrorKind::Full);
    assert_eq!(proxy.metrics().capacity, 2);
    assert_eq!(proxy.metrics().depth, 2);
    assert_eq!(proxy.metrics().high_water_mark, 2);
    assert_eq!(proxy.metrics().rejected, 1);

    app.settle();
    assert_eq!(app.application().activity_item_count(), 2);
    let recovered = rejected.into_inner();
    assert!(proxy.send(recovered).is_ok());
    assert!(proxy.send(Message::ActivityFinished { generation }).is_ok());
    app.settle();

    assert!(cancellation.is_cancelled());
    assert_eq!(
        app.application().activity_status(),
        ActivityStatus::Completed
    );
    assert_eq!(app.application().activity_item_count(), 3);
    assert_eq!(app.application().activity_item(2), Some("recovered three"));
    assert_eq!(proxy.metrics().depth, 0);
}

#[test]
fn external_activity_failure_is_recoverable_by_a_new_generation() {
    let activity_source = ControlledActivity::default();
    let mut app = TestApp::new(activity_source.queue(), Size::new(72, 18));
    app.send(Message::ShowActivity);
    app.send(Message::StartActivity);
    let (failed_generation, failed_cancellation, failed_proxy) = activity_source.launch(0);

    assert!(
        failed_proxy
            .send(Message::ActivityFailed {
                generation: failed_generation,
                error: "remote service unavailable".to_owned(),
            })
            .is_ok()
    );
    app.settle();
    assert!(failed_cancellation.is_cancelled());
    assert_eq!(app.application().activity_status(), ActivityStatus::Failed);
    assert_eq!(
        app.application().activity_error(),
        Some("remote service unavailable")
    );
    assert!(app.frame().characters().contains("Retry"));

    app.send(Message::StartActivity);
    let (retry_generation, _retry_cancellation, retry_proxy) = activity_source.launch(1);
    assert_ne!(retry_generation, failed_generation);
    assert_eq!(app.application().activity_status(), ActivityStatus::Running);
    assert_eq!(app.application().activity_error(), None);

    assert!(
        failed_proxy
            .send(Message::ActivityFinished {
                generation: failed_generation,
            })
            .is_ok()
    );
    assert!(
        retry_proxy
            .send(Message::ActivityFinished {
                generation: retry_generation,
            })
            .is_ok()
    );
    app.settle();
    assert_eq!(
        app.application().activity_status(),
        ActivityStatus::Completed
    );
}
