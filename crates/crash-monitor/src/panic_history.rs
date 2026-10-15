//! Keeps the panic that started the client's final failure cascade.

/// The first report in the latest cascade and the latest observed uptime.
pub(crate) struct PanicHistory<T> {
    first: Option<T>,
    last_uptime_ms: u64,
}

impl<T> Default for PanicHistory<T> {
    /// Starts without retaining a report snapshot.
    fn default() -> Self {
        Self {
            first: None,
            last_uptime_ms: 0,
        }
    }
}

impl<T> PanicHistory<T> {
    /// Replaces recovered failures, retaining one snapshot through arbitrarily long cascades.
    pub(crate) fn record(&mut self, uptime_ms: u64, report: T) {
        if self.first.is_none()
            || uptime_ms.saturating_sub(self.last_uptime_ms) > crate::outcome::CASCADE_MS
        {
            self.first = Some(report);
        }
        self.last_uptime_ms = uptime_ms;
    }

    /// Whether any panic was reported.
    pub(crate) fn is_empty(&self) -> bool {
        self.first.is_none()
    }

    /// Takes the first report of the final cascade.
    pub(crate) fn take(self) -> Option<T> {
        self.first
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Snapshot(usize, Arc<AtomicUsize>);

    impl Drop for Snapshot {
        /// Tracks report ownership without allocating large log copies in the fixture.
        fn drop(&mut self) {
            self.1.fetch_sub(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn recovered_panics_keep_constant_snapshot_ownership() {
        let live = Arc::new(AtomicUsize::new(0));
        let mut history = PanicHistory::default();
        for index in 0..128 {
            live.fetch_add(1, Ordering::Relaxed);
            history.record(index * 2_000, Snapshot(index as usize, Arc::clone(&live)));
        }
        assert!(
            live.load(Ordering::Relaxed) <= 2,
            "recovered panic snapshots accumulate"
        );
        assert_eq!(history.take().unwrap().0, 127);
        assert_eq!(live.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_long_cascade_keeps_its_first_report() {
        let mut history = PanicHistory::default();
        assert!(history.is_empty());
        for index in 0..128 {
            history.record(index * 900, index);
        }
        assert_eq!(history.take(), Some(0));
    }
}
