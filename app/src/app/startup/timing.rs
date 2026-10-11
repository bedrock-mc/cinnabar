//! Wall-clock instrumentation for startup carrier I/O.
use assets::startup::CarrierLoadObserver;

/// Wall time of each startup carrier load, logged as one line.
#[derive(Default)]
pub(crate) struct LoadTimes(std::sync::Mutex<Vec<(&'static str, std::time::Duration)>>);

impl LoadTimes {
    /// Times one load while preserving its result and panic behavior.
    pub(crate) fn time<T>(&self, carrier: &'static str, load: impl FnOnce() -> T) -> T {
        let start = std::time::Instant::now();
        let value = load();
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((carrier, start.elapsed()));
        value
    }

    /// The carriers loaded so far, in completion order.
    #[cfg(test)]
    pub(crate) fn carriers(&self) -> Vec<&'static str> {
        let times = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        times.iter().map(|(carrier, _)| *carrier).collect()
    }

    /// Reports total wall time and each carrier duration in descending order.
    pub(crate) fn summary(&self, wall: std::time::Duration) -> String {
        let mut times = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        times.sort_by_key(|time| std::cmp::Reverse(time.1));
        let each = times
            .iter()
            .map(|(carrier, time)| format!("{carrier} {:.1}", time.as_secs_f64() * 1e3))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "loaded startup carriers in {:.1} ms (ms each: {each})",
            wall.as_secs_f64() * 1e3
        )
    }
}

impl CarrierLoadObserver for LoadTimes {
    /// Records elapsed time while retaining the carrier loader's return value.
    fn observe<T>(&self, carrier: &'static str, load: impl FnOnce() -> T) -> T {
        self.time(carrier, load)
    }
}

/// Joins a scoped load, re-raising its panic on this thread.
pub(crate) fn join<T>(handle: std::thread::ScopedJoinHandle<'_, T>) -> T {
    handle
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}
