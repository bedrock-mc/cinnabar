//! Caller-timed admission budgets, without a timer or background owner.

use std::time::{Duration, Instant};

pub(super) struct Budget {
    tokens: u32,
    burst: u32,
    refill: Duration,
    fraction: u128,
    observed: Instant,
}

impl Budget {
    /// Starts with the configured burst; all callers use positive generated policy values.
    pub(super) fn new(burst: u32, refill: Duration, now: Instant) -> Self {
        Self {
            tokens: burst,
            burst,
            refill,
            fraction: 0,
            observed: now,
        }
    }

    /// Spends one action, preserving partial refill time without advancing on clock rollback.
    pub(super) fn spend(&mut self, now: Instant) -> bool {
        let Some(elapsed) = now.checked_duration_since(self.observed) else {
            return false;
        };
        let elapsed = elapsed.as_nanos() + self.fraction;
        let interval = self.refill.as_nanos();
        let added =
            u32::try_from((elapsed / interval).min(u128::from(self.burst))).unwrap_or(self.burst);
        self.tokens = self.tokens.saturating_add(added).min(self.burst);
        self.fraction = if self.tokens == self.burst {
            0
        } else {
            elapsed % interval
        };
        self.observed = now;
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }
}
