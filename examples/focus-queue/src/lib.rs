//! Pilot task queue and focus timer using only the `arborui` facade.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use arborui::{EventProxy, EventProxySendErrorKind, Modifier, prelude::*};

const DEFAULT_FOCUS_SECONDS: u32 = 25 * 60;
const TICK_INTERVAL: Duration = Duration::from_secs(1);
const ACTIVITY_ITEM_INTERVAL: Duration = Duration::from_millis(250);
const ACTIVITY_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const MAX_ACTIVITY_ITEMS: usize = 32;

/// Messages accepted by [`FocusQueue`].
pub enum Message {
    /// Replace the controlled new-task input.
    DraftChanged(TextBuffer),
    /// Add the current non-empty draft to the queue.
    AddTask,
    /// Toggle a task's completion state.
    ToggleTask(u64),
    /// Delete a task from the queue.
    DeleteTask(u64),
    /// Open the controlled edit dialog for one task.
    OpenEdit(u64),
    /// Replace the controlled edit title.
    EditTitleChanged(TextBuffer),
    /// Replace the controlled edit completion state.
    EditCompletedChanged(bool),
    /// Save the non-empty edit draft to its task.
    SaveEdit,
    /// Discard the edit draft and close the dialog.
    CancelEdit,
    /// Move the controlled queue viewport.
    Scrolled(Point),
    /// Start or resume the focus timer.
    StartTimer,
    /// Pause the focus timer.
    PauseTimer,
    /// Restore the timer to its configured duration.
    ResetTimer,
    /// Process a scheduled timer tick if it is still current.
    TimerTick(u64),
    /// Show the task queue screen.
    ShowQueue,
    /// Show the external activity screen.
    ShowActivity,
    /// Start or restart external activity work.
    StartActivity,
    /// Cooperatively cancel the current external activity work.
    CancelActivity,
    /// Append one externally produced item if its generation is current.
    ActivityItem {
        /// Generation of the work that produced the item.
        generation: u64,
        /// Display text owned by the application message.
        text: String,
    },
    /// Settle current external activity work successfully.
    ActivityFinished {
        /// Generation of the work that completed.
        generation: u64,
    },
    /// Settle current external activity work with a recoverable error.
    ActivityFailed {
        /// Generation of the work that failed.
        generation: u64,
        /// User-visible failure description.
        error: String,
    },
    /// Request orderly application shutdown.
    Quit,
}

/// Observable settlement state for the external activity screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivityStatus {
    /// No activity work has started.
    Idle,
    /// The current generation may still produce messages.
    Running,
    /// The user cooperatively cancelled the preceding generation.
    Cancelled,
    /// The current generation completed successfully.
    Completed,
    /// The current generation failed with a recoverable error.
    Failed,
}

/// Cooperative cancellation signal passed to an external activity launcher.
#[derive(Clone, Debug)]
pub struct ActivityCancellation {
    cancelled: Arc<AtomicBool>,
}

impl ActivityCancellation {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Returns whether the application has requested cancellation.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Screen {
    Queue,
    Activity,
}

struct ActivityItem {
    id: u64,
    text: String,
}

type ActivityLauncher =
    dyn Fn(u64, ActivityCancellation, EventProxy<Message>) -> Result<(), String> + Send + Sync;

fn launch_demo_activity(
    generation: u64,
    cancellation: ActivityCancellation,
    proxy: EventProxy<Message>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("focus-queue-activity".to_owned())
        .spawn(move || {
            for text in [
                "Connected to task service",
                "Fetched remote queue",
                "Validated incoming records",
                "Applied remote updates",
            ] {
                thread::sleep(ACTIVITY_ITEM_INTERVAL);
                if cancellation.is_cancelled() {
                    return;
                }
                if !send_activity_message(
                    &proxy,
                    &cancellation,
                    Message::ActivityItem {
                        generation,
                        text: text.to_owned(),
                    },
                ) {
                    return;
                }
            }
            if !cancellation.is_cancelled() {
                send_activity_message(
                    &proxy,
                    &cancellation,
                    Message::ActivityFinished { generation },
                );
            }
        })
        .map(|_worker| ())
        .map_err(|error| format!("could not start activity worker: {error}"))
}

fn send_activity_message(
    proxy: &EventProxy<Message>,
    cancellation: &ActivityCancellation,
    message: Message,
) -> bool {
    send_activity_message_with_retry(proxy, cancellation, message, || {
        thread::sleep(ACTIVITY_RETRY_INTERVAL);
    })
}

fn send_activity_message_with_retry<Wait>(
    proxy: &EventProxy<Message>,
    cancellation: &ActivityCancellation,
    mut message: Message,
    mut wait: Wait,
) -> bool
where
    Wait: FnMut(),
{
    loop {
        match proxy.send(message) {
            Ok(()) => return true,
            Err(error) if error.kind() == EventProxySendErrorKind::Full => {
                message = error.into_inner();
                wait();
                if cancellation.is_cancelled() {
                    return false;
                }
            }
            Err(_closed) => return false,
        }
    }
}

struct Task {
    id: u64,
    title: String,
    completed: bool,
}

struct EditDraft {
    task_id: u64,
    title: TextBuffer,
    completed: bool,
}

/// A small task queue paired with a deterministic focus timer.
pub struct FocusQueue {
    screen: Screen,
    draft: TextBuffer,
    tasks: Vec<Task>,
    edit: Option<EditDraft>,
    next_task_id: u64,
    scroll_y: i32,
    focus_seconds: u32,
    remaining_seconds: u32,
    timer_running: bool,
    timer_generation: u64,
    timer_label: String,
    summary_label: String,
    activity_status: ActivityStatus,
    activity_generation: u64,
    activity_items: Vec<ActivityItem>,
    next_activity_item_id: u64,
    activity_error: Option<String>,
    activity_status_label: String,
    activity_cancellation: Option<ActivityCancellation>,
    activity_launcher: Arc<ActivityLauncher>,
}

impl Default for FocusQueue {
    fn default() -> Self {
        Self::with_focus_seconds(DEFAULT_FOCUS_SECONDS)
    }
}

impl FocusQueue {
    /// Creates an empty queue with a focus timer of at least one second.
    #[must_use]
    pub fn with_focus_seconds(focus_seconds: u32) -> Self {
        Self::with_activity_launcher(focus_seconds, launch_demo_activity)
    }

    /// Creates a queue with an injected external activity launcher.
    ///
    /// The launcher may send any number of generation-tagged activity messages
    /// through the proxy. Returning an error settles the generation as failed.
    #[must_use]
    pub fn with_activity_launcher<Launcher>(focus_seconds: u32, launcher: Launcher) -> Self
    where
        Launcher: Fn(u64, ActivityCancellation, EventProxy<Message>) -> Result<(), String>
            + Send
            + Sync
            + 'static,
    {
        let focus_seconds = focus_seconds.max(1);
        let mut queue = Self {
            screen: Screen::Queue,
            draft: TextBuffer::default(),
            tasks: Vec::new(),
            edit: None,
            next_task_id: 1,
            scroll_y: 0,
            focus_seconds,
            remaining_seconds: focus_seconds,
            timer_running: false,
            timer_generation: 0,
            timer_label: String::new(),
            summary_label: String::new(),
            activity_status: ActivityStatus::Idle,
            activity_generation: 0,
            activity_items: Vec::new(),
            next_activity_item_id: 1,
            activity_error: None,
            activity_status_label: String::new(),
            activity_cancellation: None,
            activity_launcher: Arc::new(launcher),
        };
        queue.refresh_labels();
        queue.refresh_activity_status_label();
        queue
    }

    /// Returns the controlled draft text.
    #[must_use]
    pub fn draft(&self) -> &str {
        self.draft.text()
    }

    /// Returns the number of queued tasks.
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Returns a task title by queue position.
    #[must_use]
    pub fn task_title(&self, index: usize) -> Option<&str> {
        self.tasks.get(index).map(|task| task.title.as_str())
    }

    /// Returns whether a task is complete by queue position.
    #[must_use]
    pub fn task_completed(&self, index: usize) -> Option<bool> {
        self.tasks.get(index).map(|task| task.completed)
    }

    /// Returns the task currently open in the edit dialog.
    #[must_use]
    pub fn editing_task_id(&self) -> Option<u64> {
        self.edit.as_ref().map(|edit| edit.task_id)
    }

    /// Returns the controlled edit title while the dialog is open.
    #[must_use]
    pub fn edit_title(&self) -> Option<&str> {
        self.edit.as_ref().map(|edit| edit.title.text())
    }

    /// Returns the controlled edit completion state while the dialog is open.
    #[must_use]
    pub fn edit_completed(&self) -> Option<bool> {
        self.edit.as_ref().map(|edit| edit.completed)
    }

    /// Returns the timer's remaining whole seconds.
    #[must_use]
    pub const fn remaining_seconds(&self) -> u32 {
        self.remaining_seconds
    }

    /// Returns whether the focus timer is running.
    #[must_use]
    pub const fn timer_running(&self) -> bool {
        self.timer_running
    }

    /// Returns the vertical queue offset.
    #[must_use]
    pub const fn scroll_y(&self) -> i32 {
        self.scroll_y
    }

    /// Returns whether the external activity screen is visible.
    #[must_use]
    pub const fn showing_activity(&self) -> bool {
        matches!(self.screen, Screen::Activity)
    }

    /// Returns the current external activity settlement state.
    #[must_use]
    pub const fn activity_status(&self) -> ActivityStatus {
        self.activity_status
    }

    /// Returns the generation accepted by the activity state machine.
    #[must_use]
    pub const fn activity_generation(&self) -> u64 {
        self.activity_generation
    }

    /// Returns the number of retained external activity items.
    #[must_use]
    pub fn activity_item_count(&self) -> usize {
        self.activity_items.len()
    }

    /// Returns a retained external activity item by display position.
    #[must_use]
    pub fn activity_item(&self, index: usize) -> Option<&str> {
        self.activity_items
            .get(index)
            .map(|item| item.text.as_str())
    }

    /// Returns the current recoverable activity failure, if any.
    #[must_use]
    pub fn activity_error(&self) -> Option<&str> {
        self.activity_error.as_deref()
    }

    fn add_task(&mut self) -> bool {
        let title = self.draft.text().trim().to_owned();
        if title.is_empty() {
            return false;
        }
        let Some(next_task_id) = self.next_task_id.checked_add(1) else {
            return false;
        };
        self.tasks.push(Task {
            id: self.next_task_id,
            title,
            completed: false,
        });
        self.next_task_id = next_task_id;
        self.draft = TextBuffer::default();
        self.refresh_labels();
        true
    }

    fn remove_task(&mut self, id: u64) -> bool {
        let previous_len = self.tasks.len();
        self.tasks.retain(|task| task.id != id);
        if self.tasks.len() == previous_len {
            return false;
        }
        self.scroll_y = self.scroll_y.min(self.max_scroll_y());
        self.refresh_labels();
        true
    }

    fn toggle_task(&mut self, id: u64) -> bool {
        let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) else {
            return false;
        };
        task.completed = !task.completed;
        self.refresh_labels();
        true
    }

    fn max_scroll_y(&self) -> i32 {
        i32::try_from(self.tasks.len().saturating_sub(1)).unwrap_or(i32::MAX)
    }

    fn refresh_labels(&mut self) {
        let minutes = self.remaining_seconds / 60;
        let seconds = self.remaining_seconds % 60;
        self.timer_label = format!("{minutes:02}:{seconds:02}");

        let completed = self.tasks.iter().filter(|task| task.completed).count();
        let open = self.tasks.len().saturating_sub(completed);
        self.summary_label = format!("{open} open / {completed} complete");
    }

    fn next_timer_generation(&mut self) -> u64 {
        self.timer_generation = self.timer_generation.wrapping_add(1);
        self.timer_generation
    }

    fn next_activity_generation(&mut self) -> u64 {
        self.activity_generation = self.activity_generation.wrapping_add(1);
        self.activity_generation
    }

    fn cancel_activity_worker(&mut self) {
        if let Some(cancellation) = self.activity_cancellation.take() {
            cancellation.cancel();
        }
    }

    fn refresh_activity_status_label(&mut self) {
        self.activity_status_label = match self.activity_status {
            ActivityStatus::Idle => "Status: idle".to_owned(),
            ActivityStatus::Running => {
                format!("Status: streaming | {} received", self.activity_items.len())
            }
            ActivityStatus::Cancelled => {
                format!("Status: cancelled | {} retained", self.activity_items.len())
            }
            ActivityStatus::Completed => {
                format!("Status: complete | {} received", self.activity_items.len())
            }
            ActivityStatus::Failed => format!(
                "Error: {}",
                self.activity_error.as_deref().unwrap_or("activity failed")
            ),
        };
    }
}

impl Drop for FocusQueue {
    fn drop(&mut self) {
        self.cancel_activity_worker();
    }
}

impl Application for FocusQueue {
    type Message = Message;

    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        match message {
            Message::DraftChanged(draft) => {
                self.draft = draft;
                context.invalidate(Invalidation::Layout);
                Command::none()
            }
            Message::AddTask => {
                if self.add_task() {
                    context.invalidate(Invalidation::Recompose);
                }
                Command::none()
            }
            Message::ToggleTask(id) => {
                if self.toggle_task(id) {
                    context.invalidate(Invalidation::Paint);
                }
                Command::none()
            }
            Message::DeleteTask(id) => {
                if self.remove_task(id) {
                    context.invalidate(Invalidation::Recompose);
                }
                Command::none()
            }
            Message::OpenEdit(id) => {
                let Some(task) = self.tasks.iter().find(|task| task.id == id) else {
                    return Command::none();
                };
                self.edit = Some(EditDraft {
                    task_id: task.id,
                    title: TextBuffer::new(task.title.clone()),
                    completed: task.completed,
                });
                context.invalidate(Invalidation::Recompose);
                Command::none()
            }
            Message::EditTitleChanged(title) => {
                let Some(edit) = self.edit.as_mut() else {
                    return Command::none();
                };
                edit.title = title;
                context.invalidate(Invalidation::Layout);
                Command::none()
            }
            Message::EditCompletedChanged(completed) => {
                let Some(edit) = self.edit.as_mut() else {
                    return Command::none();
                };
                edit.completed = completed;
                context.invalidate(Invalidation::Paint);
                Command::none()
            }
            Message::SaveEdit => {
                let Some(edit) = self.edit.as_ref() else {
                    return Command::none();
                };
                let title = edit.title.text().trim().to_owned();
                if title.is_empty() {
                    return Command::none();
                }
                let task_id = edit.task_id;
                let completed = edit.completed;
                let Some(task) = self.tasks.iter_mut().find(|task| task.id == task_id) else {
                    self.edit = None;
                    context.invalidate(Invalidation::Recompose);
                    return Command::none();
                };
                task.title = title;
                task.completed = completed;
                self.edit = None;
                self.refresh_labels();
                context.invalidate(Invalidation::Recompose);
                Command::none()
            }
            Message::CancelEdit => {
                if self.edit.take().is_some() {
                    context.invalidate(Invalidation::Recompose);
                }
                Command::none()
            }
            Message::Scrolled(delta) => {
                let next = self
                    .scroll_y
                    .saturating_add(delta.y)
                    .clamp(0, self.max_scroll_y());
                if next != self.scroll_y {
                    self.scroll_y = next;
                    context.invalidate(Invalidation::Layout);
                }
                Command::none()
            }
            Message::StartTimer => {
                if self.timer_running {
                    return Command::none();
                }
                if self.remaining_seconds == 0 {
                    self.remaining_seconds = self.focus_seconds;
                    self.refresh_labels();
                }
                self.timer_running = true;
                let generation = self.next_timer_generation();
                context.invalidate(Invalidation::Layout);
                Command::after(TICK_INTERVAL, Message::TimerTick(generation))
            }
            Message::PauseTimer => {
                if self.timer_running {
                    self.timer_running = false;
                    self.next_timer_generation();
                    context.invalidate(Invalidation::Layout);
                }
                Command::none()
            }
            Message::ResetTimer => {
                self.timer_running = false;
                self.next_timer_generation();
                self.remaining_seconds = self.focus_seconds;
                self.refresh_labels();
                context.invalidate(Invalidation::Layout);
                Command::none()
            }
            Message::TimerTick(generation) => {
                if !self.timer_running || generation != self.timer_generation {
                    return Command::none();
                }
                self.remaining_seconds = self.remaining_seconds.saturating_sub(1);
                self.refresh_labels();
                if self.remaining_seconds == 0 {
                    self.timer_running = false;
                    context.invalidate(Invalidation::Layout);
                    Command::none()
                } else {
                    context.invalidate(Invalidation::Paint);
                    Command::after(TICK_INTERVAL, Message::TimerTick(generation))
                }
            }
            Message::ShowQueue => {
                if self.screen != Screen::Queue {
                    self.screen = Screen::Queue;
                    context.invalidate(Invalidation::Recompose);
                }
                Command::none()
            }
            Message::ShowActivity => {
                if self.screen != Screen::Activity {
                    self.screen = Screen::Activity;
                    context.invalidate(Invalidation::Recompose);
                }
                Command::none()
            }
            Message::StartActivity => {
                self.cancel_activity_worker();
                let generation = self.next_activity_generation();
                self.activity_status = ActivityStatus::Running;
                self.activity_items.clear();
                self.activity_error = None;
                let cancellation = ActivityCancellation::new();
                self.activity_cancellation = Some(cancellation.clone());
                self.refresh_activity_status_label();
                context.invalidate(Invalidation::Recompose);

                let launcher = Arc::clone(&self.activity_launcher);
                if let Err(error) = launcher(generation, cancellation, context.event_proxy()) {
                    self.cancel_activity_worker();
                    self.activity_status = ActivityStatus::Failed;
                    self.activity_error = Some(error);
                    self.refresh_activity_status_label();
                }
                Command::none()
            }
            Message::CancelActivity => {
                if self.activity_status == ActivityStatus::Running {
                    self.cancel_activity_worker();
                    self.next_activity_generation();
                    self.activity_status = ActivityStatus::Cancelled;
                    self.activity_error = None;
                    self.refresh_activity_status_label();
                    context.invalidate(Invalidation::Recompose);
                }
                Command::none()
            }
            Message::ActivityItem { generation, text } => {
                if generation != self.activity_generation
                    || self.activity_status != ActivityStatus::Running
                {
                    return Command::none();
                }
                let Some(next_id) = self.next_activity_item_id.checked_add(1) else {
                    self.cancel_activity_worker();
                    self.activity_status = ActivityStatus::Failed;
                    self.activity_error = Some("activity item identity exhausted".to_owned());
                    self.refresh_activity_status_label();
                    context.invalidate(Invalidation::Recompose);
                    return Command::none();
                };
                self.activity_items.push(ActivityItem {
                    id: self.next_activity_item_id,
                    text,
                });
                self.next_activity_item_id = next_id;
                if self.activity_items.len() > MAX_ACTIVITY_ITEMS {
                    self.activity_items.remove(0);
                }
                self.refresh_activity_status_label();
                context.invalidate(Invalidation::Recompose);
                Command::none()
            }
            Message::ActivityFinished { generation } => {
                if generation == self.activity_generation
                    && self.activity_status == ActivityStatus::Running
                {
                    self.cancel_activity_worker();
                    self.activity_status = ActivityStatus::Completed;
                    self.refresh_activity_status_label();
                    context.invalidate(Invalidation::Recompose);
                }
                Command::none()
            }
            Message::ActivityFailed { generation, error } => {
                if generation == self.activity_generation
                    && self.activity_status == ActivityStatus::Running
                {
                    self.cancel_activity_worker();
                    self.activity_status = ActivityStatus::Failed;
                    self.activity_error = Some(error);
                    self.refresh_activity_status_label();
                    context.invalidate(Invalidation::Recompose);
                }
                Command::none()
            }
            Message::Quit => Command::quit(),
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let border_style = Style::new().foreground(Color::BrightCyan);
        let button_style = Style::new().foreground(Color::BrightYellow);
        let full_width = Dimension::percent(100);

        let input = text_input(&self.draft, Message::DraftChanged)
            .on_submit(|| Message::AddTask)
            .style(Style::new().foreground(Color::BrightWhite))
            .layout(LayoutStyle::new().flex(1, 1))
            .focus_order(0)
            .build()
            .key("new-task");
        let add = button("Add", || Message::AddTask)
            .label_style(button_style)
            .focus_order(1)
            .build()
            .key("add-task");
        let input_row = row_with_gap([input, add], 1).layout(LayoutStyle {
            width: full_width,
            direction: FlexDirection::Row,
            gap: 1,
            ..LayoutStyle::default()
        });
        let input_panel = Block::new(input_row)
            .title("New task")
            .border_style(border_style)
            .layout(LayoutStyle::new().size(full_width, Dimension::cells(3)))
            .build();

        let task_rows = self.tasks.iter().map(|task| {
            let id = task.id;
            let marker = if task.completed { "[x]" } else { "[ ]" };
            let title_style = if task.completed {
                Style::new()
                    .foreground(Color::BrightBlack)
                    .add_modifiers(Modifier::DIM | Modifier::CROSSED_OUT)
            } else {
                Style::new().foreground(Color::White)
            };
            let row_layout = LayoutStyle {
                width: full_width,
                height: Dimension::cells(1),
                direction: FlexDirection::Row,
                flex_shrink: 0,
                gap: 1,
                ..LayoutStyle::default()
            };
            let row = row_with_gap(
                [
                    button(marker, move || Message::ToggleTask(id))
                        .label_style(button_style)
                        .build()
                        .key(format!("task-{id}-toggle")),
                    text(&task.title)
                        .style(title_style)
                        .layout(LayoutStyle::new().flex(1, 1)),
                    button("Edit", move || Message::OpenEdit(id))
                        .label_style(button_style)
                        .build()
                        .key(format!("task-{id}-edit")),
                    button("Delete", move || Message::DeleteTask(id))
                        .label_style(Style::new().foreground(Color::BrightRed))
                        .build()
                        .key(format!("task-{id}-delete")),
                ],
                1,
            )
            .layout(row_layout);
            (id, row)
        });
        let queue_content = if self.tasks.is_empty() {
            text("No tasks yet. Type one above and press Enter.")
                .style(Style::new().foreground(Color::BrightBlack))
        } else {
            let content_height = u16::try_from(self.tasks.len()).unwrap_or(u16::MAX);
            list(task_rows).layout(LayoutStyle {
                width: full_width,
                height: Dimension::cells(content_height),
                direction: FlexDirection::Column,
                flex_shrink: 0,
                ..LayoutStyle::default()
            })
        };
        let queue = scroll_view(Point::new(0, self.scroll_y), queue_content)
            .on_scroll(Message::Scrolled)
            .layout(LayoutStyle::new().size(full_width, Dimension::percent(100)))
            .build();
        let queue_panel = Block::new(queue)
            .title("Focus queue")
            .border_style(border_style)
            .layout(LayoutStyle {
                width: full_width,
                min_height: Dimension::cells(5),
                flex_grow: 1,
                flex_shrink: 1,
                ..LayoutStyle::default()
            })
            .build();

        let timer_style = Style::new()
            .foreground(if self.timer_running {
                Color::BrightGreen
            } else {
                Color::BrightYellow
            })
            .add_modifiers(Modifier::BOLD);
        let timer_action = if self.timer_running {
            button("Pause", || Message::PauseTimer)
        } else {
            button("Start", || Message::StartTimer)
        }
        .label_style(button_style)
        .build()
        .key("timer-toggle");
        let timer_panel = Block::new(row_with_gap(
            [
                text(&self.timer_label).style(timer_style),
                text(&self.summary_label),
                flexible_spacer(),
                timer_action,
                button("Reset", || Message::ResetTimer)
                    .label_style(button_style)
                    .build()
                    .key("timer-reset"),
            ],
            2,
        ))
        .title("Focus timer")
        .border_style(border_style)
        .layout(LayoutStyle::new().size(full_width, Dimension::cells(3)))
        .build();

        let activity_action = match self.activity_status {
            ActivityStatus::Running => button("Cancel", || Message::CancelActivity),
            ActivityStatus::Failed => button("Retry", || Message::StartActivity),
            ActivityStatus::Idle | ActivityStatus::Cancelled | ActivityStatus::Completed => {
                button("Start", || Message::StartActivity)
            }
        }
        .label_style(button_style)
        .build()
        .key("activity-action");
        let activity_status_style = match self.activity_status {
            ActivityStatus::Running => Style::new().foreground(Color::BrightYellow),
            ActivityStatus::Completed => Style::new().foreground(Color::BrightGreen),
            ActivityStatus::Failed => Style::new().foreground(Color::BrightRed),
            ActivityStatus::Idle | ActivityStatus::Cancelled => {
                Style::new().foreground(Color::BrightBlack)
            }
        };
        let activity_controls = Block::new(row_with_gap(
            [
                text(&self.activity_status_label).style(activity_status_style),
                flexible_spacer(),
                activity_action,
            ],
            1,
        ))
        .title("External work")
        .border_style(border_style)
        .layout(LayoutStyle::new().size(full_width, Dimension::cells(3)))
        .build();
        let activity_rows = self.activity_items.iter().rev().map(|item| {
            (
                item.id,
                text(&item.text).layout(LayoutStyle {
                    width: full_width,
                    height: Dimension::cells(1),
                    flex_shrink: 0,
                    ..LayoutStyle::default()
                }),
            )
        });
        let activity_content = if self.activity_items.is_empty() {
            text("No external activity yet. Start a run to receive updates.")
                .style(Style::new().foreground(Color::BrightBlack))
        } else {
            let content_height = u16::try_from(self.activity_items.len()).unwrap_or(u16::MAX);
            list(activity_rows).layout(LayoutStyle {
                width: full_width,
                height: Dimension::cells(content_height),
                direction: FlexDirection::Column,
                flex_shrink: 0,
                ..LayoutStyle::default()
            })
        };
        let activity_log = scroll_view(Point::new(0, 0), activity_content)
            .layout(LayoutStyle::new().size(full_width, Dimension::percent(100)))
            .build();
        let activity_log_panel = Block::new(activity_log)
            .title("Activity log (newest first)")
            .border_style(border_style)
            .layout(LayoutStyle {
                width: full_width,
                min_height: Dimension::cells(5),
                flex_grow: 1,
                flex_shrink: 1,
                ..LayoutStyle::default()
            })
            .build();

        let queue_screen =
            column_with_gap([input_panel, queue_panel, timer_panel], 1).layout(LayoutStyle {
                width: full_width,
                height: Dimension::cells(0),
                min_height: Dimension::cells(0),
                direction: FlexDirection::Column,
                flex_grow: 1,
                flex_shrink: 1,
                gap: 1,
                ..LayoutStyle::default()
            });
        let activity_screen =
            column_with_gap([activity_controls, activity_log_panel], 1).layout(LayoutStyle {
                width: full_width,
                height: Dimension::cells(0),
                min_height: Dimension::cells(0),
                direction: FlexDirection::Column,
                flex_grow: 1,
                flex_shrink: 1,
                gap: 1,
                ..LayoutStyle::default()
            });
        let screen = match self.screen {
            Screen::Queue => queue_screen,
            Screen::Activity => activity_screen,
        };
        let queue_label = if self.screen == Screen::Queue {
            "[Queue]"
        } else {
            "Queue"
        };
        let activity_label = if self.screen == Screen::Activity {
            " [Activity]"
        } else {
            " Activity"
        };
        let footer = row_with_gap(
            [
                button(queue_label, || Message::ShowQueue)
                    .label_style(button_style)
                    .build()
                    .key("screen-queue"),
                button(activity_label, || Message::ShowActivity)
                    .label_style(button_style)
                    .build()
                    .key("screen-activity"),
                flexible_spacer(),
                text("Tab focus | Enter activate")
                    .style(Style::new().foreground(Color::BrightBlack)),
                button(" Quit", || Message::Quit)
                    .label_style(Style::new().foreground(Color::BrightMagenta))
                    .build()
                    .key("quit"),
            ],
            1,
        )
        .layout(LayoutStyle::new().size(full_width, Dimension::cells(1)));

        let application = column_with_gap([screen, footer], 1).layout(LayoutStyle {
            width: full_width,
            height: Dimension::percent(100),
            direction: FlexDirection::Column,
            gap: 1,
            ..LayoutStyle::default()
        });

        let overlay_layout =
            LayoutStyle::new().size(Dimension::percent(100), Dimension::percent(100));
        let Some(edit) = self.edit.as_ref() else {
            return stack([application]).layout(overlay_layout);
        };
        let edit_title = text_input(&edit.title, Message::EditTitleChanged)
            .on_submit(|| Message::SaveEdit)
            .style(Style::new().foreground(Color::BrightWhite))
            .layout(LayoutStyle::new().size(full_width, Dimension::cells(1)))
            .focus_order(0)
            .build()
            .key("edit-title");
        let completed = checkbox("Completed", edit.completed, Message::EditCompletedChanged)
            .label_style(Style::new().foreground(Color::BrightYellow))
            .focus_order(1)
            .build()
            .key("edit-completed");
        let actions = row_with_gap(
            [
                flexible_spacer(),
                button("Save", || Message::SaveEdit)
                    .label_style(Style::new().foreground(Color::BrightGreen))
                    .focus_order(2)
                    .build()
                    .key("edit-save"),
                button("Cancel", || Message::CancelEdit)
                    .label_style(Style::new().foreground(Color::BrightRed))
                    .focus_order(3)
                    .build()
                    .key("edit-cancel"),
            ],
            2,
        )
        .layout(LayoutStyle {
            width: full_width,
            height: Dimension::cells(1),
            direction: FlexDirection::Row,
            gap: 2,
            ..LayoutStyle::default()
        });
        let edit_form = column_with_gap([text("Title"), edit_title, completed, actions], 1).layout(
            LayoutStyle {
                width: full_width,
                height: Dimension::percent(100),
                direction: FlexDirection::Column,
                gap: 1,
                ..LayoutStyle::default()
            },
        );
        let edit_panel = Block::new(edit_form)
            .title("Edit task")
            .padding(Insets::all(1))
            .style(
                Style::new()
                    .foreground(Color::BrightWhite)
                    .background(Color::Black),
            )
            .border_style(Style::new().foreground(Color::BrightCyan))
            .layout(LayoutStyle {
                width: Dimension::percent(80),
                height: Dimension::cells(11),
                min_width: Dimension::cells(30),
                max_width: Dimension::cells(52),
                flex_shrink: 1,
                ..LayoutStyle::default()
            })
            .build();
        let edit_dialog = dialog(edit_panel, || Message::CancelEdit)
            .scrim_style(Style::new().background(Color::Black))
            .build()
            .key("edit-dialog");

        stack([application, edit_dialog]).layout(overlay_layout)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroUsize,
        sync::{Arc, Barrier},
    };

    use arborui::{AppRunner, Renderer, RuntimeOptions, Size, WidthPolicy};

    use super::*;

    fn runner_with_capacity(capacity: usize) -> AppRunner<FocusQueue> {
        let size = Size::new(20, 8);
        let options = RuntimeOptions::new()
            .with_event_ingress_capacity(NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::MIN));
        AppRunner::new_with_options(
            FocusQueue::with_activity_launcher(60, |_generation, _cancellation, _proxy| Ok(())),
            size,
            Renderer::new(size, WidthPolicy::Unicode),
            options,
        )
    }

    #[test]
    fn activity_sender_retries_recovered_message_after_capacity_frees() {
        let mut runner = runner_with_capacity(1);
        let proxy = runner.event_proxy();
        assert!(proxy.send(Message::ShowActivity).is_ok());
        let cancellation = ActivityCancellation::new();
        let retrying = Arc::new(Barrier::new(2));
        let retry_allowed = Arc::new(Barrier::new(2));
        let worker_retrying = Arc::clone(&retrying);
        let worker_retry_allowed = Arc::clone(&retry_allowed);
        let worker_proxy = proxy.clone();
        let worker_cancellation = cancellation.clone();
        let worker = thread::spawn(move || {
            send_activity_message_with_retry(
                &worker_proxy,
                &worker_cancellation,
                Message::ShowQueue,
                || {
                    worker_retrying.wait();
                    worker_retry_allowed.wait();
                },
            )
        });

        retrying.wait();
        assert_eq!(proxy.metrics().rejected, 1);
        assert_eq!(runner.process_pending().updates, 1);
        retry_allowed.wait();
        assert!(worker.join().is_ok_and(|sent| sent));
        assert_eq!(runner.process_pending().updates, 1);
        assert!(!runner.application().showing_activity());
    }

    #[test]
    fn activity_sender_stops_retrying_after_cancellation_or_close() {
        let runner = runner_with_capacity(1);
        let proxy = runner.event_proxy();
        assert!(proxy.send(Message::ShowActivity).is_ok());
        let cancellation = ActivityCancellation::new();
        cancellation.cancel();

        assert!(!send_activity_message_with_retry(
            &proxy,
            &cancellation,
            Message::ShowQueue,
            || {},
        ));

        drop(runner);
        let open_cancellation = ActivityCancellation::new();
        assert!(!send_activity_message_with_retry(
            &proxy,
            &open_cancellation,
            Message::ShowQueue,
            || panic!("closed ingress must not retry"),
        ));
    }
}
