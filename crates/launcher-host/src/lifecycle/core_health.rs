//! Restart pacing for a core that keeps exiting on its own.

use std::time::Duration;

const BASE_DELAY: Duration = Duration::from_millis(500);
const MAX_DELAY: Duration = Duration::from_secs(8);
const MAX_CONSECUTIVE_FAILURES: u32 = 5;
const STABLE_RUN: Duration = Duration::from_secs(60);

/// Exponential restart delay that resets once a core stays up for a stable interval.
#[derive(Debug, Default)]
pub struct RestartBackoff {
    failures: u32,
}

impl RestartBackoff {
    /// Delay before the next restart after a core that ran for `ran_for` died: 0.5 s doubling
    /// to 8 s; `None` after five quick failures in a row, meaning stop retrying. A run of a
    /// minute or more starts the count again.
    pub fn next_delay(&mut self, ran_for: Duration) -> Option<Duration> {
        if ran_for >= STABLE_RUN {
            self.failures = 0;
        }
        self.failures += 1;
        if self.failures > MAX_CONSECUTIVE_FAILURES {
            return None;
        }
        Some((BASE_DELAY * 2u32.pow(self.failures - 1)).min(MAX_DELAY))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_doubles_caps_and_gives_up() {
        let mut backoff = RestartBackoff::default();
        let quick = Duration::from_secs(1);
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_millis(500)));
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_secs(1)));
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_secs(2)));
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_secs(4)));
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_secs(8)));
        assert_eq!(backoff.next_delay(quick), None);
    }

    #[test]
    fn a_stable_run_resets_the_failure_count() {
        let mut backoff = RestartBackoff::default();
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            backoff.next_delay(Duration::from_secs(1));
        }
        assert_eq!(backoff.next_delay(STABLE_RUN), Some(BASE_DELAY));
    }
}
