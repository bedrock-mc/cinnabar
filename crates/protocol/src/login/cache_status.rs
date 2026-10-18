use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use bedrock_protocol_1_26_51::{packets::ClientCacheBlobStatus, wire};
use futures::task::AtomicWaker;
use simulation_clock::Moment;

use crate::{BlobCacheStatus, MAX_CLIENT_BLOB_HASHES_PER_PACKET, MAX_CLIENT_BLOB_PENDING_BYTES};

/// Minimum exclusion between eligible status selections, independent of simulation cadence.
const STATUS_EXCLUSION: Duration = Duration::from_millis(5);

/// Session-owned classifications shared with the detached writer, without sharing the resolver.
pub(super) type SharedCacheStatus = Arc<Mutex<CacheStatusOutbox>>;

/// Sealed values in class order; one admitted candidate may cross the intake byte budget.
#[derive(Default)]
pub(super) struct CacheStatusOutbox {
    missing: VecDeque<u64>,
    have: VecDeque<u64>,
    deadline: Option<Moment>,
    pub(super) epoch: u64,
    progress: AtomicWaker,
}

impl CacheStatusOutbox {
    /// Moves a classification exactly once, preserving repeats across classifications.
    pub(super) fn push(&mut self, status: BlobCacheStatus) {
        let (missing, have) = status.into_hashes();
        self.missing.extend(missing);
        self.have.extend(have);
    }

    /// Stops further classification at the byte bound until selection or reset frees space.
    pub(super) fn poll_capacity(&self, cx: &mut Context<'_>) -> Poll<()> {
        if (self.missing.len() + self.have.len()) * size_of::<u64>() < MAX_CLIENT_BLOB_PENDING_BYTES
        {
            Poll::Ready(())
        } else {
            self.progress.register(cx.waker());
            Poll::Pending
        }
    }

    /// Selects at most one miss-first packet, advancing even an empty eligible opportunity.
    pub(super) fn select(&mut self, now: Moment) -> Option<ClientCacheBlobStatus> {
        if self.deadline.is_some_and(|deadline| now <= deadline) {
            return None;
        }
        self.deadline = Some(
            now.checked_add(STATUS_EXCLUSION)
                .unwrap_or(Moment::from_elapsed(Duration::MAX)),
        );
        let mut packet = ClientCacheBlobStatus::default();
        while packet.missing_ids.len() + packet.found_ids.len() < MAX_CLIENT_BLOB_HASHES_PER_PACKET
        {
            if let Some(hash) = self.missing.pop_front() {
                packet.missing_ids.push(wire::U64LE(hash));
            } else if let Some(hash) = self.have.pop_front() {
                packet.found_ids.push(wire::U64LE(hash));
            } else {
                break;
            }
        }
        self.release_empty_storage();
        self.progress.wake();
        (!packet.missing_ids.is_empty() || !packet.found_ids.is_empty()).then_some(packet)
    }

    /// Retires pre-admission statuses and the deadline when the actual session epoch changes.
    pub(super) fn reset(&mut self) {
        self.epoch = self
            .epoch
            .checked_add(1)
            .expect("cache status epoch overflow");
        self.missing = VecDeque::new();
        self.have = VecDeque::new();
        self.deadline = None;
        self.progress.wake();
    }

    /// Releases drained allocations so a single large candidate does not stay resident forever.
    fn release_empty_storage(&mut self) {
        if self.missing.is_empty() {
            self.missing = VecDeque::new();
        }
        if self.have.is_empty() {
            self.have = VecDeque::new();
        }
    }
}

#[cfg(test)]
mod tests;
