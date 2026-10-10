//! Test-only time helpers: bounded waits instead of sleeps, and a clock tests advance by hand.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Generous bound for background work in tests; a wait that reaches it fails the test.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

const MAX_BACKOFF: Duration = Duration::from_millis(20);

/// Polls `condition` with a short backoff until it holds; false if `timeout` passes first.
pub fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    let mut backoff = Duration::from_micros(100);
    loop {
        if condition() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        thread::sleep(backoff.min(deadline - now));
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Waits up to [`DEFAULT_TIMEOUT`] for `condition`, panicking with `what` if it never holds.
#[track_caller]
pub fn eventually(what: &str, condition: impl FnMut() -> bool) {
    eventually_within(DEFAULT_TIMEOUT, what, condition);
}

/// [`eventually`] with an explicit bound.
#[track_caller]
pub fn eventually_within(timeout: Duration, what: &str, condition: impl FnMut() -> bool) {
    assert!(
        wait_until(timeout, condition),
        "timed out after {timeout:?} waiting for {what}"
    );
}

/// Brief pause between iterations of a bounded loop that also drives work; prefer
/// [`eventually`] when a test only waits.
pub fn idle() {
    thread::sleep(Duration::from_millis(1));
}

/// A shareable clock that only moves when a test calls [`ManualClock::advance`].
#[derive(Clone, Debug)]
pub struct ManualClock {
    origin: Instant,
    offset_nanos: Arc<AtomicU64>,
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ManualClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            offset_nanos: Arc::default(),
        }
    }

    /// The fixed origin plus every advance so far, shared by all clones.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.origin + self.elapsed()
    }

    #[must_use]
    pub fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.offset_nanos.load(Ordering::Acquire))
    }

    pub fn advance(&self, by: Duration) {
        let nanos = u64::try_from(by.as_nanos()).expect("advance fits in u64 nanoseconds");
        self.offset_nanos.fetch_add(nanos, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn wait_until_sees_work_finished_on_another_thread() {
        let done = Arc::new(AtomicBool::new(false));
        let worker = {
            let done = Arc::clone(&done);
            thread::spawn(move || done.store(true, Ordering::Release))
        };
        assert!(wait_until(DEFAULT_TIMEOUT, || done.load(Ordering::Acquire)));
        worker.join().unwrap();
    }

    #[test]
    fn wait_until_gives_up_at_its_timeout() {
        let start = Instant::now();
        assert!(!wait_until(Duration::from_millis(30), || false));
        assert!(start.elapsed() >= Duration::from_millis(30));
    }

    #[test]
    #[should_panic(expected = "waiting for the impossible")]
    fn eventually_names_what_it_waited_for() {
        eventually_within(Duration::ZERO, "the impossible", || false);
    }

    #[test]
    fn manual_clock_moves_only_when_advanced_and_is_shared_by_clones() {
        let clock = ManualClock::new();
        let start = clock.now();
        let other = clock.clone();
        assert_eq!(clock.now(), start);
        other.advance(Duration::from_secs(5));
        assert_eq!(clock.now() - start, Duration::from_secs(5));
        assert_eq!(clock.elapsed(), Duration::from_secs(5));
    }
}
