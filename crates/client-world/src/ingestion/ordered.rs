use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

use protocol::{BlockUpdateEvent, WorldEvent};

use super::{
    MAX_ADMITTED_HEAVY_EVENTS, MAX_ADMITTED_WORLD_EVENTS, PreparedSubChunk, PreparedWorldEvent,
    WorldStreamError,
};

/// One synchronous commit operation whose admission remains owned by this stream.
// A synchronous handoff keeps the existing event allocation and ownership intact.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum CommitStep {
    /// Recheck the shared time budget after installing a partially applied batch.
    BatchStarted,
    Apply {
        sequence: u64,
        event: PreparedWorldEvent,
    },
    /// Snapshot these updates now; the sequence stays fenced until decode completes.
    BlockUpdates {
        sequence: u64,
        events: Vec<BlockUpdateEvent>,
    },
}

/// The ordered action caused by one completed decode.
// This returned event applies immediately; boxing would allocate on every block batch.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum DecodeCommit {
    /// The completion waits for its ordinary FIFO turn.
    Queued,
    /// The previously popped block batch must apply before deferred predictions.
    BlockUpdates(PreparedWorldEvent),
}

/// A partially applied network batch retains its original FIFO sequence.
#[derive(Debug)]
struct PendingSubChunkCommit {
    sequence: u64,
    dimension: i32,
    entries: std::vec::IntoIter<PreparedSubChunk>,
    duration: Duration,
}

/// Owns world admission, FIFO progression, partial batches and block decode fences.
#[derive(Debug)]
pub struct OrderedCommitState {
    next: u64,
    ready: BTreeMap<u64, PreparedWorldEvent>,
    submitted: HashSet<u64>,
    heavy_sequences: HashSet<u64>,
    pending_sub_chunks: Option<PendingSubChunkCommit>,
    blocking_block_updates: Option<u64>,
    applying: Option<u64>,
}

impl OrderedCommitState {
    /// Starts one session's ordered world frontier.
    #[must_use]
    pub fn new(first_sequence: u64) -> Self {
        Self {
            next: first_sequence,
            ready: BTreeMap::new(),
            submitted: HashSet::new(),
            heavy_sequences: HashSet::new(),
            pending_sub_chunks: None,
            blocking_block_updates: None,
            applying: None,
        }
    }

    /// Returns the next sequence that has not yet been popped from the ready FIFO.
    #[must_use]
    pub const fn next_sequence(&self) -> u64 {
        self.next
    }

    /// Returns the last sequence whose entire mutation has synchronously finished.
    /// Read this between commit steps. During the active callback it preserves the
    /// original popped-sequence view; finishing a partial batch restores its fence.
    #[must_use]
    pub fn committed_sequence(&self) -> u64 {
        let mut committed = self.next.saturating_sub(1);
        for sequence in [
            self.pending_sub_chunks.as_ref().and_then(|pending| {
                (self.applying != Some(pending.sequence)).then_some(pending.sequence)
            }),
            self.blocking_block_updates,
        ]
        .into_iter()
        .flatten()
        {
            committed = committed.min(sequence.saturating_sub(1));
        }
        committed
    }

    /// Rejects replayed sequences before any admission or request reservation changes.
    pub fn validate_sequence(&self, sequence: u64) -> Result<(), WorldStreamError> {
        if sequence < self.next || self.submitted.contains(&sequence) {
            return Err(WorldStreamError::DuplicateOrPast {
                sequence,
                next: self.next,
            });
        }
        Ok(())
    }

    /// Reserves a bounded event slot after accounting for retained committed consumers.
    pub fn admit(
        &mut self,
        sequence: u64,
        heavy: bool,
        retained_commits: usize,
    ) -> Result<(), WorldStreamError> {
        self.validate_sequence(sequence)?;
        if self.submitted.len() >= MAX_ADMITTED_WORLD_EVENTS.saturating_sub(retained_commits)
            || (heavy && self.heavy_sequences.len() >= MAX_ADMITTED_HEAVY_EVENTS)
        {
            return Err(WorldStreamError::AdmissionFull {
                sequence,
                admitted: self.submitted.len(),
                capacity: MAX_ADMITTED_WORLD_EVENTS,
                heavy_admitted: self.heavy_sequences.len(),
                heavy_capacity: MAX_ADMITTED_HEAVY_EVENTS,
            });
        }
        self.submitted.insert(sequence);
        if heavy {
            self.heavy_sequences.insert(sequence);
        }
        Ok(())
    }

    /// Queues an admitted prepared event without advancing the mutation frontier.
    pub fn insert_ready(
        &mut self,
        sequence: u64,
        event: PreparedWorldEvent,
    ) -> Result<(), WorldStreamError> {
        if sequence < self.next || self.ready.contains_key(&sequence) {
            return Err(WorldStreamError::DuplicateOrPast {
                sequence,
                next: self.next,
            });
        }
        self.ready.insert(sequence, event);
        Ok(())
    }

    /// Releases only heavy admission when normalization replaces an expensive event.
    pub fn release_heavy(&mut self, sequence: u64) {
        self.heavy_sequences.remove(&sequence);
    }

    /// Rolls back ordinary admission when preparing an accepted event cannot be queued.
    pub fn cancel_admission(&mut self, sequence: u64) {
        self.submitted.remove(&sequence);
    }

    /// Applies one decode completion's ordered disposition, retaining partial-batch fences.
    pub fn complete_decode(
        &mut self,
        sequence: u64,
        event: PreparedWorldEvent,
    ) -> Result<DecodeCommit, WorldStreamError> {
        if self.blocking_block_updates == Some(sequence)
            && matches!(&event, PreparedWorldEvent::BlockUpdates { .. })
        {
            self.blocking_block_updates = None;
            self.submitted.remove(&sequence);
            self.heavy_sequences.remove(&sequence);
            return Ok(DecodeCommit::BlockUpdates(event));
        }
        if let Err(error) = self.insert_ready(sequence, event) {
            self.heavy_sequences.remove(&sequence);
            return Err(error);
        }
        Ok(DecodeCommit::Queued)
    }

    /// Returns one synchronous mutation, never crossing an unfinished batch or decode fence.
    /// The caller applies and finishes this step before returning to external consumers.
    pub fn next_commit(&mut self) -> Option<CommitStep> {
        if self.blocking_block_updates.is_some() || self.applying.is_some() {
            return None;
        }
        if let Some(pending) = self.pending_sub_chunks.as_mut() {
            let event = pending
                .entries
                .next()
                .map_or(PreparedWorldEvent::CommitOnly, |entry| {
                    PreparedWorldEvent::SubChunks {
                        dimension: pending.dimension,
                        entries: vec![entry],
                        duration: pending.duration,
                    }
                });
            self.applying = Some(pending.sequence);
            return Some(CommitStep::Apply {
                sequence: pending.sequence,
                event,
            });
        }
        let event = self.ready.remove(&self.next)?;
        self.next = self.next.saturating_add(1);
        let sequence = self.next.saturating_sub(1);
        Some(match event {
            PreparedWorldEvent::SubChunks {
                dimension,
                entries,
                duration,
            } => {
                self.pending_sub_chunks = Some(PendingSubChunkCommit {
                    sequence,
                    dimension,
                    entries: entries.into_iter(),
                    duration,
                });
                CommitStep::BatchStarted
            }
            PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(events)) => {
                self.applying = Some(sequence);
                CommitStep::BlockUpdates { sequence, events }
            }
            event => {
                self.applying = Some(sequence);
                CommitStep::Apply { sequence, event }
            }
        })
    }

    /// Installs the async fence only after the caller snapshots and enqueues a block batch.
    pub fn defer_block_updates(&mut self, sequence: u64) {
        assert_eq!(
            self.applying,
            Some(sequence),
            "block batch must be the active FIFO step"
        );
        self.applying = None;
        self.blocking_block_updates = Some(sequence);
    }

    /// Completes one applied step, releasing admission only after its entire event finishes.
    /// Returns true when the coordinator should release the event's request reservation.
    pub fn finish_commit(&mut self, sequence: u64) -> bool {
        if self.applying == Some(sequence) {
            self.applying = None;
            if let Some(pending) = &self.pending_sub_chunks {
                if pending.sequence != sequence || pending.entries.len() != 0 {
                    return false;
                }
                self.pending_sub_chunks = None;
            }
        } else {
            return false;
        }
        self.submitted.remove(&sequence);
        self.heavy_sequences.remove(&sequence);
        true
    }

    /// Returns the currently admitted event count for backpressure and diagnostics.
    #[must_use]
    pub fn admitted_count(&self) -> usize {
        self.submitted.len()
    }

    /// Returns the admitted terrain and block mutation count.
    #[must_use]
    pub fn heavy_count(&self) -> usize {
        self.heavy_sequences.len()
    }

    /// Returns whether a sequence still owns one heavy admission slot.
    #[must_use]
    pub fn is_heavy_admitted(&self, sequence: u64) -> bool {
        self.heavy_sequences.contains(&sequence)
    }

    /// Returns the partially applied batch's original sequence.
    #[must_use]
    pub fn pending_batch_sequence(&self) -> Option<u64> {
        self.pending_sub_chunks
            .as_ref()
            .map(|pending| pending.sequence)
    }

    /// Returns the block mutation whose worker completion currently fences the FIFO.
    #[must_use]
    pub const fn blocking_block_updates(&self) -> Option<u64> {
        self.blocking_block_updates
    }

    /// Counts ready events without exposing mutable access to ordered state.
    #[must_use]
    pub fn ready_count(&self) -> usize {
        self.ready.len()
    }

    /// Returns admission headroom before the independent outbound-request bound is applied.
    #[must_use]
    pub fn remaining_admission_capacity(&self, retained_commits: usize) -> usize {
        MAX_ADMITTED_WORLD_EVENTS
            .saturating_sub(self.submitted.len().saturating_add(retained_commits))
            .min(MAX_ADMITTED_HEAVY_EVENTS.saturating_sub(self.heavy_sequences.len()))
    }
}
