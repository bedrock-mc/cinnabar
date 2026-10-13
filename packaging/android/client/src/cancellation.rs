//! Per-attempt cancellation for setup, which outlives the activity that started it.

use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicBool, Ordering::SeqCst},
};

/// Attempts are numbered upward by the setup activity; cancelling one also cancels older ones.
#[derive(Debug, Default)]
pub(crate) struct Cancellation {
    /// The highest cancelled attempt and the running one, changed together so a cancellation
    /// cannot land between a start's check and its flag reset.
    attempts: Mutex<(u64, u64)>,
    flag: AtomicBool,
}

impl Cancellation {
    pub(crate) const fn new() -> Self {
        Self {
            attempts: Mutex::new((0, 0)),
            flag: AtomicBool::new(false),
        }
    }

    /// Starts `attempt`, returning false when it was cancelled before it began.
    pub(crate) fn begin(&self, attempt: u64) -> bool {
        let mut attempts = self.attempts.lock().unwrap_or_else(PoisonError::into_inner);
        attempts.1 = attempt;
        let cancelled = attempts.0 >= attempt;
        self.flag.store(cancelled, SeqCst);
        !cancelled
    }

    /// Cancels `attempt`, before or while it runs.
    pub(crate) fn cancel(&self, attempt: u64) {
        let mut attempts = self.attempts.lock().unwrap_or_else(PoisonError::into_inner);
        attempts.0 = attempts.0.max(attempt);
        if attempts.1 == attempt {
            self.flag.store(true, SeqCst);
        }
    }

    /// The running attempt's flag, which preparation polls.
    pub(crate) fn flag(&self) -> &AtomicBool {
        &self.flag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An attempt cancelled while it waited to start never runs, and stays cancelled.
    #[test]
    fn a_cancellation_before_the_attempt_starts_is_kept() {
        let cancellation = Cancellation::new();
        cancellation.cancel(1);
        assert!(!cancellation.begin(1));
        assert!(cancellation.flag().load(SeqCst));
        assert!(cancellation.begin(2));
        assert!(!cancellation.flag().load(SeqCst));
    }

    /// Cancelling the running attempt stops it; a stale cancellation does not stop a newer one.
    #[test]
    fn only_the_cancelled_attempt_stops() {
        let cancellation = Cancellation::new();
        assert!(cancellation.begin(3));
        cancellation.cancel(2);
        assert!(!cancellation.flag().load(SeqCst));
        cancellation.cancel(3);
        assert!(cancellation.flag().load(SeqCst));
    }
}
