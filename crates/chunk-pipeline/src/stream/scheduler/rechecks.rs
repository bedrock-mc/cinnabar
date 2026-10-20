use std::collections::{BTreeMap, HashMap};

use world::SubChunkKey;

use super::MAX_RECHECK_BACKOFF_POLLS;

/// One revision waiting for a bounded ordinary readiness check.
pub(in crate::stream) struct DeferredRetry {
    pub(in crate::stream) key: SubChunkKey,
    pub(in crate::stream) revision: u64,
    attempts: u32,
    not_before: u64,
}

impl DeferredRetry {
    /// Advances backoff for one readiness or admission check that actually failed.
    fn block(&mut self, polls: u64) {
        let delay = (1_u64 << self.attempts.min(5)).min(MAX_RECHECK_BACKOFF_POLLS);
        self.attempts = self.attempts.saturating_add(1);
        self.not_before = polls + delay;
    }
}

/// A FIFO retry rotation with one removable record per key, independent of camera heaps.
#[derive(Default)]
pub(in crate::stream) struct RecheckQueue {
    entries: BTreeMap<u64, DeferredRetry>,
    positions: HashMap<SubChunkKey, u64>,
    next_sequence: u64,
    polls: u64,
}

impl RecheckQueue {
    /// Registers a revision without letting repeated arrivals move an existing waiter back.
    pub(in crate::stream) fn enqueue(&mut self, key: SubChunkKey, revision: u64) {
        if let Some(sequence) = self.positions.get(&key) {
            let retry = self.entries.get_mut(sequence).expect("indexed retry");
            if retry.revision != revision {
                retry.revision = revision;
                retry.attempts = 0;
                retry.not_before = self.polls;
            }
            return;
        }
        self.requeue(DeferredRetry {
            key,
            revision,
            attempts: 0,
            not_before: self.polls,
        });
    }

    /// Removes completed work from both indexes without scanning other waiters.
    pub(in crate::stream) fn remove(&mut self, key: &SubChunkKey) {
        if let Some(sequence) = self.positions.remove(key) {
            let removed = self.entries.remove(&sequence);
            debug_assert!(removed.is_some());
        }
    }

    /// Drops queued revisions and their poll-based retry history.
    pub(in crate::stream) fn clear(&mut self) {
        self.entries.clear();
        self.positions.clear();
        self.next_sequence = 0;
        self.polls = 0;
    }

    /// Starts one readiness slice and returns its initial waiter count.
    pub(in crate::stream) fn begin_poll(&mut self) -> usize {
        self.polls += 1;
        self.entries.len()
    }

    /// Removes the oldest waiter so the caller can check or rotate it.
    pub(in crate::stream) fn pop(&mut self) -> Option<DeferredRetry> {
        let (sequence, retry) = self.entries.pop_first()?;
        let indexed = self.positions.remove(&retry.key);
        debug_assert_eq!(indexed, Some(sequence));
        Some(retry)
    }

    /// Reports whether this revision's existing backoff has elapsed.
    pub(in crate::stream) fn is_due(&self, retry: &DeferredRetry) -> bool {
        self.polls >= retry.not_before
    }

    /// Rotates an unattempted or cooling revision without changing its retry history.
    pub(in crate::stream) fn requeue(&mut self, retry: DeferredRetry) {
        if self.entries.is_empty() {
            self.next_sequence = 0;
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        let old_position = self.positions.insert(retry.key, sequence);
        let old_entry = self.entries.insert(sequence, retry);
        debug_assert!(old_position.is_none() && old_entry.is_none());
    }

    /// Records an actual blocked readiness check before rotating the revision to the back.
    pub(in crate::stream) fn defer(&mut self, mut retry: DeferredRetry) {
        retry.block(self.polls);
        self.requeue(retry);
    }

    /// Applies an actual admission failure only to the matching queued revision.
    pub(in crate::stream) fn block(&mut self, key: SubChunkKey, revision: u64) {
        if let Some(sequence) = self.positions.get(&key) {
            let retry = self.entries.get_mut(sequence).expect("indexed retry");
            if retry.revision == revision {
                retry.block(self.polls);
            }
        }
    }
}
