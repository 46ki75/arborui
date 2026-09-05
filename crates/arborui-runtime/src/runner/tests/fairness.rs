use super::*;

#[derive(Default)]
struct TimerStreamsApp {
    timer_updates: usize,
    future_updates: usize,
}

enum TimerStreamsMessage {
    Start(usize),
    Timer,
    Future,
}

impl Application for TimerStreamsApp {
    type Message = TimerStreamsMessage;

    fn update(
        &mut self,
        message: Self::Message,
        _context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        match message {
            TimerStreamsMessage::Start(streams) => Command::batch(
                (0..streams)
                    .map(|_| Command::after(Duration::ZERO, TimerStreamsMessage::Timer))
                    .chain(std::iter::once(Command::perform(
                        std::future::ready(()),
                        |_| TimerStreamsMessage::Future,
                    ))),
            ),
            TimerStreamsMessage::Timer => {
                self.timer_updates += 1;
                Command::after(Duration::ZERO, TimerStreamsMessage::Timer)
            }
            TimerStreamsMessage::Future => {
                self.future_updates += 1;
                Command::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        Element::text("")
    }
}

#[test]
fn due_timer_streams_do_not_starve_ready_future() {
    for timer_streams in [1, 256] {
        let size = Size::new(8, 2);
        let mut runner = AppRunner::new_with_clock(
            TimerStreamsApp::default(),
            size,
            Renderer::new(size, Capabilities::default().width_policy),
            Arc::new(ManualClock::default()),
        );
        runner.enqueue(TimerStreamsMessage::Start(timer_streams));
        let mut completed_tasks = 0;
        let mut first_delivery = None;

        for turn in 1..=32 {
            let previous_timers = runner.application().timer_updates;
            let report = runner.process_pending();
            completed_tasks += report.completed_tasks;
            assert!(report.budget_exhausted);
            assert!(report.updates <= MAX_MESSAGES_PER_TURN);
            assert!(runner.application().timer_updates > previous_timers);
            if runner.application().future_updates != 0 {
                first_delivery.get_or_insert(turn);
            }
        }

        assert_eq!(
            completed_tasks,
            1,
            "{timer_streams} timer streams starved the ready future after 32 turns and {} timer updates",
            runner.application().timer_updates,
        );
        assert_eq!(runner.application().future_updates, 1);
        assert!(first_delivery.is_some_and(|turn| turn <= 2));
    }
}

#[test]
fn scheduler_alternates_sources_across_small_and_full_allowances() {
    for allowance in [1, 2, 3, 256] {
        let mut scheduler = Scheduler::new(
            Arc::new(WakeSignal::new()),
            Arc::new(ManualClock::default()),
        );
        let polls = Arc::new(AtomicUsize::new(0));
        for value in 0..512 {
            scheduler.schedule_after(Duration::ZERO, value);
            let polls = Arc::clone(&polls);
            scheduler.spawn(Box::pin(std::future::poll_fn(move |_| {
                polls.fetch_add(1, Ordering::Relaxed);
                Poll::Ready(1_000 + value)
            })));
        }
        let mut output = Vec::new();
        for call in 1..=4 {
            let before = output.clone();
            let previous_polls = polls.load(Ordering::Relaxed);
            let zero = scheduler.poll_ready(&mut output, 0);
            assert_eq!((zero.polled, zero.completed), (0, 0));
            assert_eq!(output, before);
            assert_eq!(polls.load(Ordering::Relaxed), previous_polls);

            let report = scheduler.poll_ready(&mut output, allowance);
            assert_eq!(report.polled, allowance);
            assert_eq!(output.len() - before.len(), allowance);
            assert_eq!(
                polls.load(Ordering::Relaxed) - previous_polls,
                report.completed
            );
            if call % 2 == 0 {
                let expected = allowance * call / 2;
                assert_eq!(polls.load(Ordering::Relaxed), expected);
                assert_eq!(
                    output
                        .iter()
                        .copied()
                        .filter(|value| *value < 1_000)
                        .collect::<Vec<_>>(),
                    (0..expected).collect::<Vec<_>>(),
                );
                assert_eq!(
                    output
                        .iter()
                        .copied()
                        .filter(|value| *value >= 1_000)
                        .collect::<Vec<_>>(),
                    (1_000..1_000 + expected).collect::<Vec<_>>(),
                );
            }
        }
    }
}

#[test]
fn scheduler_single_source_uses_unused_capacity() {
    for futures in [false, true] {
        for allowance in [0, 1, 256] {
            let mut scheduler = Scheduler::new(
                Arc::new(WakeSignal::new()),
                Arc::new(ManualClock::default()),
            );
            for value in 0..512 {
                if futures {
                    scheduler.spawn(Box::pin(std::future::ready(value)));
                } else {
                    scheduler.schedule_after(Duration::ZERO, value);
                }
            }
            if futures {
                scheduler.schedule_after(Duration::from_secs(1), 512);
            }
            let mut output = Vec::new();
            for call in 1..=2 {
                let report = scheduler.poll_ready(&mut output, allowance);
                assert_eq!(report.polled, allowance);
                assert_eq!(report.completed, if futures { allowance } else { 0 });
                assert_eq!(output, (0..allowance * call).collect::<Vec<_>>());
            }
        }
    }
}

#[test]
fn self_waking_pending_futures_do_not_starve_due_timers() {
    for allowance in [1, 256] {
        let mut scheduler = Scheduler::new(
            Arc::new(WakeSignal::new()),
            Arc::new(ManualClock::default()),
        );
        let polls = Arc::new(AtomicUsize::new(0));
        for _ in 0..256 {
            let polls = Arc::clone(&polls);
            scheduler.spawn(Box::pin(std::future::poll_fn(move |context| {
                polls.fetch_add(1, Ordering::Relaxed);
                context.waker().wake_by_ref();
                Poll::Pending
            })));
        }
        scheduler.schedule_after(Duration::ZERO, ());
        let mut timers = 0;
        for _ in 0..4 {
            let previous_polls = polls.load(Ordering::Relaxed);
            let previous_timers = timers;
            for _ in 0..2 {
                let mut output = Vec::new();
                let report = scheduler.poll_ready(&mut output, allowance);
                assert_eq!((report.polled, report.completed), (allowance, 0));
                timers += output.len();
                for message in output {
                    scheduler.schedule_after(Duration::ZERO, message);
                }
            }
            assert!(timers > previous_timers);
            assert!(polls.load(Ordering::Relaxed) > previous_polls);
            assert_eq!(
                timers + polls.load(Ordering::Relaxed),
                (previous_timers + previous_polls) + 2 * allowance
            );
        }
    }
}

#[test]
fn timer_deadline_and_declaration_order_survive_multiple_mixed_batches() {
    let clock = Arc::new(ManualClock::default());
    let mut scheduler = Scheduler::new(Arc::new(WakeSignal::new()), clock.clone());
    for (deadline, values) in [(2, 600..1_200), (1, 0..600)] {
        for value in values {
            scheduler.schedule_after(Duration::from_secs(deadline), value);
        }
    }
    scheduler.schedule_after(Duration::from_secs(3), 1_200);
    clock.advance(Duration::from_secs(2));
    let mut timers = Vec::new();
    for _ in 0..5 {
        scheduler.spawn(Box::pin(std::future::ready(usize::MAX)));
        let mut output = Vec::new();
        let report = scheduler.poll_ready(&mut output, 256);
        assert!(report.polled <= 256);
        assert_eq!(report.completed, 1);
        assert_eq!(
            output.iter().filter(|value| **value == usize::MAX).count(),
            1
        );
        timers.extend(output.into_iter().filter(|value| *value != usize::MAX));
    }
    assert_eq!(timers, (0..1_200).collect::<Vec<_>>());
    assert!(!scheduler.has_ready_work());
    assert_eq!(
        scheduler.wait_timeout(Duration::from_secs(5)),
        Duration::from_secs(1)
    );
    clock.advance(Duration::from_secs(1));
    let mut output = Vec::new();
    let report = scheduler.poll_ready(&mut output, 256);
    assert_eq!((report.polled, report.completed), (1, 0));
    assert_eq!(output, [1_200]);
}

#[test]
fn scheduler_defers_and_deduplicates_self_wakes() {
    let mut scheduler = Scheduler::<()>::new(
        Arc::new(WakeSignal::new()),
        Arc::new(ManualClock::default()),
    );
    let polls = Arc::new(AtomicUsize::new(0));
    let future_polls = Arc::clone(&polls);
    scheduler.spawn(Box::pin(std::future::poll_fn(move |context| {
        future_polls.fetch_add(1, Ordering::Relaxed);
        context.waker().wake_by_ref();
        context.waker().wake_by_ref();
        Poll::Pending
    })));
    let mut output = Vec::new();
    for call in 1..=4 {
        let report = scheduler.poll_ready(&mut output, 256);
        assert_eq!((report.polled, report.completed), (1, 0));
        assert_eq!(polls.load(Ordering::Relaxed), call);
        assert!(output.is_empty());
        assert!(scheduler.has_ready_work());
    }
}

#[test]
fn scheduler_preserves_ready_fifo_and_charges_stale_ids_to_budget() {
    let mut scheduler = Scheduler::new(
        Arc::new(WakeSignal::new()),
        Arc::new(ManualClock::default()),
    );
    scheduler.spawn(Box::pin(std::future::poll_fn(|context| {
        context.waker().wake_by_ref();
        context.waker().wake_by_ref();
        Poll::Ready(10)
    })));
    scheduler.spawn(Box::pin(std::future::ready(11)));
    let mut output = Vec::new();
    for value in [10, 11] {
        let report = scheduler.poll_ready(&mut output, 1);
        assert_eq!((report.polled, report.completed), (1, 1));
        assert_eq!(output.pop(), Some(value));
    }
    // The completed task's self-wake precedes the next live task in the FIFO.
    scheduler.spawn(Box::pin(std::future::ready(12)));
    scheduler.schedule_after(Duration::ZERO, 0);
    scheduler.schedule_after(Duration::ZERO, 1);
    let stale = scheduler.poll_ready(&mut output, 2);
    assert_eq!((stale.polled, stale.completed), (2, 0));
    assert_eq!(output, [0]);
    output.clear();
    let live = scheduler.poll_ready(&mut output, 2);
    assert_eq!((live.polled, live.completed), (2, 1));
    assert_eq!(output, [1, 12]);
    assert!(!scheduler.has_tasks());
    let empty = scheduler.poll_ready(&mut output, 256);
    assert_eq!((empty.polled, empty.completed), (0, 0));
}
