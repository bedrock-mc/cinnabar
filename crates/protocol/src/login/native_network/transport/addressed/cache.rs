//! Probe reuse policy takes caller time and contains no account or transport owner.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const TTL: Duration = Duration::from_secs(10 * 60);

struct Entry {
    endpoint: Option<String>,
    expires: Instant,
}

#[derive(Default)]
pub(super) struct ProbeCache {
    entries: HashMap<String, Entry>,
}

impl ProbeCache {
    /// Distinguishes an absent/expired entry from a retained negative RakNet choice.
    pub(super) fn lookup(&self, key: &str, now: Instant) -> Option<Option<String>> {
        self.entries
            .get(key)
            .filter(|entry| now < entry.expires)
            .map(|entry| entry.endpoint.clone())
    }

    /// Keeps the original choice lifetime and prunes expired addresses during actual insertion.
    pub(super) fn store(&mut self, key: String, endpoint: Option<String>, now: Instant) {
        self.entries.retain(|_, entry| now < entry.expires);
        self.entries.insert(
            key,
            Entry {
                endpoint,
                expires: now + TTL,
            },
        );
    }

    /// Invalidates a failed dial's exact addressed choice; explicit future cancellation never calls it.
    pub(super) fn remove(&mut self, key: &str) {
        self.entries.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_and_negative_choices_expire_at_the_original_cutoff_and_failure_forgets_only_its_address()
     {
        let now = Instant::now();
        let mut cache = ProbeCache::default();
        cache.store(
            "first".into(),
            Some("https://fixture.invalid:19132".into()),
            now,
        );
        cache.store("second".into(), None, now);
        assert!(
            cache
                .lookup("first", now + TTL - Duration::from_nanos(1))
                .unwrap()
                .is_some()
        );
        assert_eq!(cache.lookup("second", now), Some(None));
        assert_eq!(cache.lookup("first", now + TTL), None);
        cache.remove("first");
        assert_eq!(cache.lookup("first", now), None);
        assert_eq!(cache.lookup("second", now), Some(None));
    }
}
