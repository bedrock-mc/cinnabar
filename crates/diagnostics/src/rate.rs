//! Bounded logging: interval budgets driven by caller-supplied time, and once-per-callsite gates.
//!
//! Nothing here reads a clock. Callers pass `now` from their own time source, either a
//! [`std::time::Instant`] or a [`Duration`] since a fixed origin such as Bevy's elapsed time.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use web_time::Instant;

/// Re-exports used by the logging macros, so callers need no direct `tracing` dependency.
#[doc(hidden)]
pub mod __private {
    pub use tracing;
}

/// A point on a caller's monotonic timeline.
pub trait Moment: Copy {
    /// Time from `earlier` to `self`; zero when `earlier` is not before `self`.
    fn since(self, earlier: Self) -> Duration;
}

impl Moment for Instant {
    fn since(self, earlier: Self) -> Duration {
        self.saturating_duration_since(earlier)
    }
}

impl Moment for Duration {
    fn since(self, earlier: Self) -> Duration {
        self.saturating_sub(earlier)
    }
}

/// Admits at most one emission per interval and counts the refused emissions in between.
///
/// The first emission is always admitted. An emission at `now` is admitted when at least
/// `interval` has passed since the last admitted one; each admission returns how many were
/// refused since the previous admission and starts that count again.
#[derive(Debug, Clone)]
pub struct LogBudget<T = Instant> {
    interval: Duration,
    last: Option<T>,
    suppressed: u64,
}

impl<T: Moment> LogBudget<T> {
    /// A budget admitting one emission per `interval`.
    #[must_use]
    pub const fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
            suppressed: 0,
        }
    }

    /// Whether an emission at `now` would be admitted. Records nothing.
    #[must_use]
    pub fn ready(&self, now: T) -> bool {
        self.last
            .is_none_or(|last| now.since(last) >= self.interval)
    }

    /// Admits an emission at `now`, returning the number refused since the previous admission,
    /// or refuses it, counts it and returns `None`.
    pub fn admit(&mut self, now: T) -> Option<u64> {
        if !self.ready(now) {
            self.suppressed = self.suppressed.saturating_add(1);
            return None;
        }
        self.last = Some(now);
        Some(std::mem::take(&mut self.suppressed))
    }

    /// Forgets the last admission so the next emission is admitted immediately. The refused
    /// count is kept and reported by that emission.
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// Emissions refused since the last admission.
    #[must_use]
    pub fn suppressed(&self) -> u64 {
        self.suppressed
    }
}

/// Returns `true` exactly once for `flag`: on the first call, from any thread.
pub fn first(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::Relaxed)
}

/// Emits one `tracing` event when `budget` admits `now`, adding a `suppressed` field with the
/// number of emissions the budget refused since the previous event.
///
/// `budget` is a place expression of type [`LogBudget`]; `LEVEL` is one of `TRACE`, `DEBUG`,
/// `INFO`, `WARN` or `ERROR`; the remaining arguments are `tracing` fields and message, as for
/// `tracing::event!`. An optional `target: "..."` may precede the level. The budget is consulted
/// whether or not the level is enabled, so its accounting does not depend on the filter.
///
/// ```
/// use std::time::{Duration, Instant};
/// let mut budget = diagnostics::rate::LogBudget::new(Duration::from_secs(5));
/// let count = 3;
/// diagnostics::log_every!(budget, Instant::now(), WARN, count, "unsorted water drew");
/// ```
#[macro_export]
macro_rules! log_every {
    ($budget:expr, $now:expr, target: $target:expr, $level:ident, $($rest:tt)+) => {
        if let ::core::option::Option::Some(suppressed) = $budget.admit($now) {
            $crate::rate::__private::tracing::event!(
                target: $target,
                $crate::rate::__private::tracing::Level::$level,
                suppressed,
                $($rest)+
            );
        }
    };
    ($budget:expr, $now:expr, $level:ident, $($rest:tt)+) => {
        if let ::core::option::Option::Some(suppressed) = $budget.admit($now) {
            $crate::rate::__private::tracing::event!(
                $crate::rate::__private::tracing::Level::$level,
                suppressed,
                $($rest)+
            );
        }
    };
}

/// Emits one `tracing` event at `LEVEL` the first time this callsite runs in the process, and
/// never again. Arguments follow [`log_every!`] without the budget and time.
#[macro_export]
macro_rules! log_once {
    (target: $target:expr, $level:ident, $($rest:tt)+) => {{
        static ONCE: ::std::sync::atomic::AtomicBool = ::std::sync::atomic::AtomicBool::new(false);
        if $crate::rate::first(&ONCE) {
            $crate::rate::__private::tracing::event!(
                target: $target,
                $crate::rate::__private::tracing::Level::$level,
                $($rest)+
            );
        }
    }};
    ($level:ident, $($rest:tt)+) => {{
        static ONCE: ::std::sync::atomic::AtomicBool = ::std::sync::atomic::AtomicBool::new(false);
        if $crate::rate::first(&ONCE) {
            $crate::rate::__private::tracing::event!(
                $crate::rate::__private::tracing::Level::$level,
                $($rest)+
            );
        }
    }};
}

/// [`log_once!`] at `WARN`.
#[macro_export]
macro_rules! warn_once {
    (target: $target:expr, $($rest:tt)+) => {
        $crate::log_once!(target: $target, WARN, $($rest)+)
    };
    ($($rest:tt)+) => {
        $crate::log_once!(WARN, $($rest)+)
    };
}

#[cfg(test)]
mod tests;
