use std::time::{Duration, Instant};

/// Monotonic time source used by runtime timers.
///
/// Applications normally use [`SystemClock`]. Headless harnesses can supply a
/// manually advanced implementation through [`crate::AppRunner::new_with_clock`].
pub trait Clock: Send + Sync + 'static {
    /// Returns monotonic elapsed time from an implementation-defined origin.
    fn now(&self) -> Duration;
}

/// Monotonic clock backed by [`Instant`].
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// Creates a clock whose elapsed time starts now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}
