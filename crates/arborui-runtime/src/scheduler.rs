use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

use crate::{Clock, command::CommandFuture};

pub(crate) struct WakeSignal {
    notified: Mutex<bool>,
    condition: Condvar,
}

impl WakeSignal {
    pub(crate) const fn new() -> Self {
        Self {
            notified: Mutex::new(false),
            condition: Condvar::new(),
        }
    }

    pub(crate) fn notify(&self) {
        let mut notified = self
            .notified
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *notified = true;
        self.condition.notify_one();
    }

    pub(crate) fn wait(&self, timeout: Duration) {
        let mut notified = self
            .notified
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !*notified {
            notified = match self
                .condition
                .wait_timeout_while(notified, timeout, |notified| !*notified)
            {
                Ok((notified, _timeout)) => notified,
                Err(error) => error.into_inner().0,
            };
        }
        *notified = false;
    }
}

struct TaskWake {
    id: u64,
    queued: AtomicBool,
    ready: mpsc::Sender<u64>,
    signal: Arc<WakeSignal>,
}

impl TaskWake {
    fn schedule(&self) {
        if self.queued.swap(true, Ordering::AcqRel) {
            return;
        }
        if self.ready.send(self.id).is_ok() {
            self.signal.notify();
        }
    }
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.schedule();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.schedule();
    }
}

struct Task<Message> {
    future: CommandFuture<Message>,
    wake: Arc<TaskWake>,
}

struct Timer<Message> {
    deadline: Duration,
    order: u64,
    message: Message,
}

pub(crate) struct Scheduler<Message> {
    tasks: HashMap<u64, Task<Message>>,
    next_id: u64,
    ready_sender: mpsc::Sender<u64>,
    ready_receiver: mpsc::Receiver<u64>,
    signal: Arc<WakeSignal>,
    clock: Arc<dyn Clock>,
    timers: Vec<Timer<Message>>,
    next_timer_order: u64,
}

pub(crate) struct PollReport {
    pub(crate) polled: usize,
    pub(crate) completed: usize,
}

impl<Message> Scheduler<Message> {
    pub(crate) fn new(signal: Arc<WakeSignal>, clock: Arc<dyn Clock>) -> Self {
        let (ready_sender, ready_receiver) = mpsc::channel();
        Self {
            tasks: HashMap::new(),
            next_id: 0,
            ready_sender,
            ready_receiver,
            signal,
            clock,
            timers: Vec::new(),
            next_timer_order: 0,
        }
    }

    pub(crate) fn spawn(&mut self, future: CommandFuture<Message>) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let wake = Arc::new(TaskWake {
            id,
            queued: AtomicBool::new(false),
            ready: self.ready_sender.clone(),
            signal: Arc::clone(&self.signal),
        });
        self.tasks.insert(
            id,
            Task {
                future,
                wake: Arc::clone(&wake),
            },
        );
        wake.schedule();
    }

    pub(crate) fn schedule_after(&mut self, delay: Duration, message: Message) {
        let deadline = self.clock.now().saturating_add(delay);
        let order = self.next_timer_order;
        self.next_timer_order = self.next_timer_order.wrapping_add(1);
        self.timers.push(Timer {
            deadline,
            order,
            message,
        });
        self.signal.notify();
    }

    pub(crate) fn poll_ready(&mut self, output: &mut Vec<Message>, limit: usize) -> PollReport {
        let now = self.clock.now();
        let mut due = 0;
        self.timers
            .sort_by_key(|timer| (timer.deadline, timer.order));
        while due < limit {
            if self
                .timers
                .first()
                .is_some_and(|timer| timer.deadline <= now)
            {
                output.push(self.timers.remove(0).message);
                due += 1;
            } else {
                break;
            }
        }
        let remaining = limit.saturating_sub(due);
        let ready = (0..limit)
            .take(remaining)
            .map_while(|_| self.ready_receiver.try_recv().ok())
            .collect::<Vec<_>>();
        let polled = ready.len();
        let mut completed = 0;
        for id in ready {
            let result = {
                let Some(task) = self.tasks.get_mut(&id) else {
                    continue;
                };
                task.wake.queued.store(false, Ordering::Release);
                let waker = Waker::from(Arc::clone(&task.wake));
                let mut context = Context::from_waker(&waker);
                task.future.as_mut().poll(&mut context)
            };
            if let Poll::Ready(message) = result {
                self.tasks.remove(&id);
                output.push(message);
                completed += 1;
            }
        }
        PollReport {
            polled: polled.saturating_add(due),
            completed,
        }
    }

    pub(crate) fn has_tasks(&self) -> bool {
        !self.tasks.is_empty() || !self.timers.is_empty()
    }

    pub(crate) fn has_ready_work(&self) -> bool {
        let now = self.clock.now();
        self.tasks
            .values()
            .any(|task| task.wake.queued.load(Ordering::Acquire))
            || self.timers.iter().any(|timer| timer.deadline <= now)
    }

    pub(crate) fn wait_timeout(&self, maximum: Duration) -> Duration {
        let now = self.clock.now();
        self.timers
            .iter()
            .map(|timer| timer.deadline.saturating_sub(now))
            .min()
            .map_or(maximum, |timer| timer.min(maximum))
    }
}
