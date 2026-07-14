use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use arborui_runtime::Clock;

#[derive(Clone, Default)]
pub(crate) struct ManualClock {
    elapsed: Arc<Mutex<Duration>>,
}

impl ManualClock {
    pub(crate) fn advance(&self, duration: Duration) -> bool {
        let mut elapsed = self
            .elapsed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(next) = elapsed.checked_add(duration) else {
            return false;
        };
        *elapsed = next;
        true
    }

    pub(crate) fn elapsed(&self) -> Duration {
        *self
            .elapsed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.elapsed()
    }
}
