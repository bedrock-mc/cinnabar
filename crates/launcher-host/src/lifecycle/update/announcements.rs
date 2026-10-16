//! Scheduling hints from release pushes; signed manifests remain the authority.

const MAX_JITTER_SECONDS: u64 = 5 * 60;
const RETRIES: u8 = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Announcement {
    pub version: String,
    pub build: u64,
    pub not_before: u64,
    attempts: u8,
}

impl Announcement {
    /// Schedules one announced identity after a bounded random delay.
    pub fn new(version: &str, build: u64, now: u64, jitter: u64) -> Self {
        Self {
            version: version.into(),
            build,
            not_before: now.saturating_add(jitter.min(MAX_JITTER_SECONDS)),
            attempts: 0,
        }
    }

    /// True while a verified manifest has not yet reached the announced version and build.
    pub fn ahead_of(&self, version: &str, build: u64) -> bool {
        update_manifest::select::newer_build(&self.version, self.build, version, build)
            .unwrap_or(false)
    }

    /// Allows a few fresh fetches when publication caches still serve the previous manifest.
    pub fn retry(&mut self, now: u64, jitter: u64) -> bool {
        if self.attempts >= RETRIES {
            return false;
        }
        self.attempts += 1;
        self.not_before = now.saturating_add(30 + jitter.min(MAX_JITTER_SECONDS));
        true
    }
}

/// Reads edge entropy for a delay between zero and five minutes, including the endpoints.
pub(super) fn jitter() -> u64 {
    let mut bytes = [0; 8];
    let _ = getrandom::fill(&mut bytes);
    u64::from_le_bytes(bytes) % (MAX_JITTER_SECONDS + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The caller's clock and entropy determine the deadline without any wall-clock waiting.
    #[test]
    fn release_checks_are_jittered_and_stale_manifest_retries_are_bounded() {
        let mut hint = Announcement::new("1.2.3", 42, 1000, 300);
        assert_eq!(hint.not_before, 1300);
        assert!(hint.ahead_of("1.2.3", 41));
        assert!(!hint.ahead_of("1.2.3", 42));
        assert!(!hint.ahead_of("1.2.4", 1));
        assert_eq!(Announcement::new("1.2.3", 42, 1000, 0).not_before, 1000);
        for _ in 0..RETRIES {
            assert!(hint.retry(2000, 300));
            assert_eq!(hint.not_before, 2330);
        }
        assert!(!hint.retry(3000, 0));
    }
}
