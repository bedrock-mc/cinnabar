use std::{collections::BTreeMap, time::Duration};

use protocol::{BlockUpdateEvent, SyncedBlockUpdateEvent, WorldEvent};

use super::{
    MAX_ADMITTED_HEAVY_EVENTS, MAX_ADMITTED_WORLD_EVENTS, MAX_DEFERRED_WORLD_EVENTS,
    PreparedSubChunk, PreparedWorldEvent, WorldStreamError,
    lanes::{EarlierEvents, Footprint},
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
    SyncedBlockUpdates {
        sequence: u64,
        events: Vec<SyncedBlockUpdateEvent>,
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

/// Which admission bound an unfinished event holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    Light,
    Heavy,
    /// Ordered behind earlier terrain without decode admission yet.
    Deferred,
}

#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "Keeping prepared events inline avoids an allocation for each ordered packet"
)]
enum Slot {
    /// Admitted; its prepared event has not arrived.
    Waiting,
    Ready(PreparedWorldEvent),
    /// Popped: applying, a partial batch, or a block decode fence.
    Active,
}

#[derive(Debug)]
struct Entry {
    footprint: Footprint,
    held: Held,
    slot: Slot,
}

/// What one commit pass may spend.
#[derive(Debug, Clone, Copy)]
pub struct CommitBudget {
    /// Heavy terrain steps are allowed; light steps always are.
    pub heavy: bool,
    /// Heavy steps may include pure chunk data. When false, heavy chunk data commits only
    /// while a ready later event waits behind it alone, so mutations, retention changes and
    /// barriers never wait for a pass that skips terrain.
    pub chunk_data: bool,
    /// The server position scopes retention, so positional events order with terrain.
    pub couple_position: bool,
}

impl CommitBudget {
    pub const UNLIMITED: Self = Self {
        heavy: true,
        chunk_data: true,
        couple_position: true,
    };
}

/// Owns world admission, conflict-ordered progression, partial batches and block decode fences.
#[derive(Debug)]
pub struct OrderedCommitState {
    /// Lowest unfinished sequence; everything below it has finished.
    frontier: u64,
    /// Admitted, unfinished events only, so scans never touch completed work.
    entries: BTreeMap<u64, Entry>,
    /// Finished runs above the frontier as `start -> end`, merged when adjacent. Runs are
    /// separated by unfinished or unadmitted sequences, so they never outnumber `entries`
    /// by more than one.
    finished: BTreeMap<u64, u64>,
    unfinished: usize,
    light: usize,
    heavy: usize,
    deferred: usize,
    pending_sub_chunks: Option<PendingSubChunkCommit>,
    /// The block batch awaiting decode, with the last sequence it merged.
    blocking_block_updates: Option<(u64, u64)>,
    block_update_range_end: Option<u64>,
    applying: Option<u64>,
    last_step_heavy: bool,
    earlier: EarlierEvents,
    /// Chunk data a pass without the chunk-data allowance skips, for its lookahead.
    skipped_chunk_data: EarlierEvents,
}

impl OrderedCommitState {
    /// Starts one session's ordered world frontier.
    #[must_use]
    pub fn new(first_sequence: u64) -> Self {
        Self {
            frontier: first_sequence,
            entries: BTreeMap::new(),
            finished: BTreeMap::new(),
            unfinished: 0,
            light: 0,
            heavy: 0,
            deferred: 0,
            pending_sub_chunks: None,
            blocking_block_updates: None,
            block_update_range_end: None,
            applying: None,
            last_step_heavy: false,
            earlier: EarlierEvents::default(),
            skipped_chunk_data: EarlierEvents::default(),
        }
    }

    /// Returns the lowest sequence that has not yet been popped for commit.
    #[must_use]
    pub fn next_sequence(&self) -> u64 {
        let mut expected = self.frontier;
        for (&sequence, entry) in self.entries.range(self.frontier..) {
            expected = self.finished_through(expected).saturating_add(1);
            if sequence != expected || matches!(entry.slot, Slot::Waiting | Slot::Ready(_)) {
                return expected.min(sequence);
            }
            expected = sequence.saturating_add(1);
        }
        self.finished_through(expected).saturating_add(1)
    }

    /// The last sequence of the finished run starting at `from`, or `from - 1` when `from`
    /// has not finished.
    fn finished_through(&self, from: u64) -> u64 {
        match self.finished.range(..=from).next_back() {
            Some((_, &end)) if end >= from => end,
            _ => from.saturating_sub(1),
        }
    }

    /// Whether this sequence's mutation reads as complete to frontier observers.
    /// The active callback keeps the popped-sequence view; block-update steps and
    /// partial batches between steps stay fenced.
    fn reads_committed(&self, sequence: u64, entry: &Entry) -> bool {
        match entry.slot {
            Slot::Active => {
                self.applying == Some(sequence) && self.block_update_range_end.is_none()
            }
            Slot::Waiting | Slot::Ready(_) => false,
        }
    }

    /// Returns the last sequence through which every mutation has synchronously finished.
    #[must_use]
    pub fn committed_sequence(&self) -> u64 {
        self.frontier_where(|_| false)
    }

    /// Like [`Self::committed_sequence`], but passes unfinished chunk data, which no
    /// inventory or local-authority consumer waits for.
    #[must_use]
    pub fn committed_past_chunk_data(&self) -> u64 {
        self.frontier_where(Footprint::is_chunk_data)
    }

    fn frontier_where(&self, passes: impl Fn(&Footprint) -> bool) -> u64 {
        let mut committed = self.frontier.saturating_sub(1);
        for (&sequence, entry) in self.entries.range(self.frontier..) {
            committed = self.finished_through(committed.saturating_add(1));
            if sequence != committed.saturating_add(1)
                || !(self.reads_committed(sequence, entry) || passes(&entry.footprint))
            {
                return committed;
            }
            committed = sequence;
        }
        self.finished_through(committed.saturating_add(1))
    }

    /// Rejects replayed sequences before any admission or request reservation changes.
    pub fn validate_sequence(&self, sequence: u64) -> Result<(), WorldStreamError> {
        if self.is_finished(sequence) || self.entries.contains_key(&sequence) {
            return Err(WorldStreamError::DuplicateOrPast {
                sequence,
                next: self.frontier,
            });
        }
        Ok(())
    }

    fn full(&self, sequence: u64) -> WorldStreamError {
        WorldStreamError::AdmissionFull {
            sequence,
            admitted: self.unfinished,
            capacity: MAX_ADMITTED_WORLD_EVENTS,
            heavy_admitted: self.heavy,
            heavy_capacity: MAX_ADMITTED_HEAVY_EVENTS,
        }
    }

    /// Reserves one slot in the event's lane: heavy terrain against its own bound, every
    /// other event against the light bound shared with retained committed consumers.
    pub fn admit(
        &mut self,
        sequence: u64,
        footprint: Footprint,
        retained_commits: usize,
    ) -> Result<(), WorldStreamError> {
        self.validate_sequence(sequence)?;
        let held = if footprint.heavy {
            if self.heavy_capacity() == 0 {
                return Err(self.full(sequence));
            }
            self.heavy += 1;
            Held::Heavy
        } else {
            if self.light_capacity(retained_commits) == 0 {
                return Err(self.full(sequence));
            }
            self.light += 1;
            Held::Light
        };
        self.insert_entry(sequence, footprint, held);
        Ok(())
    }

    /// Orders an event whose preparation must wait behind earlier deferred terrain. A light
    /// event takes its light slot now, so promoting it never needs credit that later light
    /// events, which cannot pass it, may hold.
    pub fn admit_deferred(
        &mut self,
        sequence: u64,
        footprint: Footprint,
        retained_commits: usize,
    ) -> Result<(), WorldStreamError> {
        self.validate_sequence(sequence)?;
        let held = if footprint.heavy {
            if self.deferred_capacity() == 0 {
                return Err(self.full(sequence));
            }
            self.deferred += 1;
            Held::Deferred
        } else {
            if self.light_capacity(retained_commits) == 0 {
                return Err(self.full(sequence));
            }
            self.light += 1;
            Held::Light
        };
        self.insert_entry(sequence, footprint, held);
        Ok(())
    }

    /// Moves deferred heavy terrain into decode admission; the caller checked capacity, or
    /// the event is the frontier and owes nothing to later events.
    pub fn promote_deferred(&mut self, sequence: u64) {
        let entry = self
            .entries
            .get_mut(&sequence)
            .expect("only an ordered deferred event is promoted");
        if entry.held == Held::Deferred {
            self.deferred -= 1;
            self.heavy += 1;
            entry.held = Held::Heavy;
        }
        self.debug_check_accounting();
    }

    /// Whether every earlier sequence has finished.
    #[must_use]
    pub const fn is_frontier(&self, sequence: u64) -> bool {
        sequence == self.frontier
    }

    fn insert_entry(&mut self, sequence: u64, footprint: Footprint, held: Held) {
        self.unfinished += 1;
        self.entries.insert(
            sequence,
            Entry {
                footprint,
                held,
                slot: Slot::Waiting,
            },
        );
        self.debug_check_accounting();
    }

    /// Every unfinished event holds exactly one bounded admission lane until it finishes.
    fn debug_check_accounting(&self) {
        debug_assert_eq!(self.unfinished, self.entries.len());
        debug_assert_eq!(
            self.unfinished,
            self.light + self.heavy + self.deferred,
            "an unfinished event is uncharged"
        );
    }

    /// Queues an admitted prepared event without advancing the mutation frontier.
    pub fn insert_ready(
        &mut self,
        sequence: u64,
        event: PreparedWorldEvent,
    ) -> Result<(), WorldStreamError> {
        let next = self.frontier;
        match self.entries.get_mut(&sequence) {
            Some(
                entry @ Entry {
                    slot: Slot::Waiting,
                    ..
                },
            ) => {
                entry.slot = Slot::Ready(event);
                Ok(())
            }
            _ => Err(WorldStreamError::DuplicateOrPast { sequence, next }),
        }
    }

    /// Commits terrain that normalization replaced as light work. It keeps its heavy
    /// admission until it finishes, so a flood of such events stays bounded.
    pub fn normalize(&mut self, sequence: u64) {
        if let Some(entry) = self.entries.get_mut(&sequence) {
            entry.footprint.heavy = false;
        }
    }

    fn release_held(&mut self, held: Held) {
        match held {
            Held::Light => self.light -= 1,
            Held::Heavy => self.heavy -= 1,
            Held::Deferred => self.deferred -= 1,
        }
        self.debug_check_accounting();
    }

    /// Rolls back admission when preparing an accepted event cannot be queued.
    pub fn cancel_admission(&mut self, sequence: u64) {
        if let Some(entry) = self.entries.remove(&sequence) {
            self.unfinished -= 1;
            self.release_held(entry.held);
        }
    }

    /// Applies one decode completion's ordered disposition, retaining partial-batch fences.
    pub fn complete_decode(
        &mut self,
        sequence: u64,
        event: PreparedWorldEvent,
    ) -> Result<DecodeCommit, WorldStreamError> {
        if let Some((start, end)) = self.blocking_block_updates
            && start == sequence
            && matches!(
                &event,
                PreparedWorldEvent::BlockUpdates { .. }
                    | PreparedWorldEvent::SyncedBlockUpdates { .. }
            )
        {
            self.blocking_block_updates = None;
            self.release_range(sequence, end);
            return Ok(DecodeCommit::BlockUpdates(event));
        }
        self.insert_ready(sequence, event)?;
        Ok(DecodeCommit::Queued)
    }

    /// Returns one synchronous mutation with no earlier unfinished conflict, never crossing
    /// an unfinished batch or block decode fence of its own kind.
    /// The caller applies and finishes this step before returning to external consumers.
    pub fn next_commit(&mut self) -> Option<CommitStep> {
        self.next_commit_within(CommitBudget::UNLIMITED)
    }

    /// Whether the last returned step spent the heavy terrain budget.
    #[must_use]
    pub const fn last_step_was_heavy(&self) -> bool {
        self.last_step_heavy
    }

    /// [`Self::next_commit`] restricted to what this pass may spend.
    pub fn next_commit_within(&mut self, budget: CommitBudget) -> Option<CommitStep> {
        self.last_step_heavy = false;
        if self.applying.is_some() {
            return None;
        }
        if budget.heavy && budget.chunk_data && self.pending_sub_chunks.is_some() {
            return Some(self.continue_sub_chunks());
        }
        let sequence = self.find_unblocked(budget)?;
        if self
            .pending_sub_chunks
            .as_ref()
            .is_some_and(|pending| pending.sequence == sequence)
        {
            return Some(self.continue_sub_chunks());
        }
        let entry = self
            .entries
            .get_mut(&sequence)
            .expect("the unblocked event is ordered");
        let Slot::Ready(event) = std::mem::replace(&mut entry.slot, Slot::Active) else {
            unreachable!("only ready events are unblocked");
        };
        let heavy = entry.footprint.heavy || entry.footprint.barrier;
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
            PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(mut events)) => {
                self.last_step_heavy = heavy;
                // Apply a ready wire burst from one current block snapshot. Only adjacent,
                // unblocked updates merge: a missing sequence or any other event is a fence.
                // Keep a malformed mutation's existing packet-sized failure boundary.
                let mut end = sequence;
                if block_updates_can_coalesce(&events) {
                    while let Some(next) = self.take_adjacent_block_updates(end, budget) {
                        events.extend(next);
                        end += 1;
                    }
                }
                self.block_update_range_end = Some(end);
                self.applying = Some(sequence);
                CommitStep::BlockUpdates { sequence, events }
            }
            PreparedWorldEvent::Immediate(WorldEvent::SyncedBlockUpdates(events)) => {
                self.last_step_heavy = heavy;
                self.block_update_range_end = Some(sequence);
                self.applying = Some(sequence);
                CommitStep::SyncedBlockUpdates { sequence, events }
            }
            event => {
                self.last_step_heavy = heavy;
                self.applying = Some(sequence);
                CommitStep::Apply { sequence, event }
            }
        })
    }

    /// Applies the partial sub-chunk batch's next entry, or finishes the batch.
    fn continue_sub_chunks(&mut self) -> CommitStep {
        let pending = self
            .pending_sub_chunks
            .as_mut()
            .expect("a partial sub-chunk batch is pending");
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
        self.last_step_heavy = true;
        CommitStep::Apply {
            sequence: pending.sequence,
            event,
        }
    }

    /// The first ready event `budget` may commit that no earlier unfinished event blocks; a
    /// partial sub-chunk batch's sequence means continuing that batch. Leaves `earlier`
    /// summarising everything before a returned ready event.
    fn find_unblocked(&mut self, budget: CommitBudget) -> Option<u64> {
        if let Some(sequence) = self.scan_unblocked(budget) {
            return Some(sequence);
        }
        if !budget.heavy || budget.chunk_data {
            return None;
        }
        let skipped = self.chunk_data_holding_ready_work(budget.couple_position)?;
        if self
            .pending_sub_chunks
            .as_ref()
            .is_some_and(|pending| pending.sequence == skipped)
        {
            return Some(skipped);
        }
        // The earliest skipped chunk data is the first event the full allowance unblocks.
        self.scan_unblocked(CommitBudget {
            chunk_data: true,
            ..budget
        })
    }

    /// Scans in wire order for the first ready event no earlier unfinished event blocks.
    /// Leaves `earlier` summarising everything before the returned sequence.
    fn scan_unblocked(&mut self, budget: CommitBudget) -> Option<u64> {
        self.earlier.clear();
        let mut expected = self.frontier;
        for (&sequence, entry) in self.entries.range(self.frontier..) {
            if sequence != self.finished_through(expected).saturating_add(1) {
                self.earlier.add_unknown();
            }
            if self.earlier.is_barrier() {
                return None;
            }
            expected = sequence.saturating_add(1);
            match &entry.slot {
                Slot::Ready(event)
                    if (budget.heavy || !(entry.footprint.heavy || entry.footprint.barrier))
                        && (budget.chunk_data || !is_heavy_chunk_data(&entry.footprint))
                        && self.kind_is_free(event)
                        && !self
                            .earlier
                            .blocks(&entry.footprint, budget.couple_position) =>
                {
                    return Some(sequence);
                }
                _ => self.earlier.add(&entry.footprint, budget.couple_position),
            }
        }
        None
    }

    /// Finds chunk data that can progress when a ready later event waits only on chunk data.
    /// A queued sub-chunk batch first needs the active batch to release its shared slot.
    fn chunk_data_holding_ready_work(&mut self, couple_position: bool) -> Option<u64> {
        self.earlier.clear();
        self.skipped_chunk_data.clear();
        let mut first_skipped = None;
        let mut expected = self.frontier;
        for (&sequence, entry) in self.entries.range(self.frontier..) {
            if sequence != self.finished_through(expected).saturating_add(1) {
                self.earlier.add_unknown();
            }
            if self.earlier.is_barrier() {
                return None;
            }
            expected = sequence.saturating_add(1);
            let blocked = self.earlier.blocks(&entry.footprint, couple_position);
            let partial_batch = self
                .pending_sub_chunks
                .as_ref()
                .is_some_and(|pending| pending.sequence == sequence);
            let free = matches!(&entry.slot, Slot::Ready(event) if self.kind_is_free(event));
            let batch_waits_for = match &entry.slot {
                Slot::Ready(PreparedWorldEvent::SubChunks { .. }) => self
                    .pending_sub_chunks
                    .as_ref()
                    .map(|pending| pending.sequence),
                _ => None,
            };
            if !blocked
                && is_heavy_chunk_data(&entry.footprint)
                && (free || partial_batch || batch_waits_for.is_some())
            {
                first_skipped.get_or_insert(batch_waits_for.unwrap_or(sequence));
                self.skipped_chunk_data
                    .add(&entry.footprint, couple_position);
                continue;
            }
            if !blocked
                && free
                && self
                    .skipped_chunk_data
                    .blocks(&entry.footprint, couple_position)
            {
                return first_skipped;
            }
            self.earlier.add(&entry.footprint, couple_position);
        }
        None
    }

    /// One partial batch and one block decode fence exist at a time.
    fn kind_is_free(&self, event: &PreparedWorldEvent) -> bool {
        match event {
            PreparedWorldEvent::SubChunks { .. } => self.pending_sub_chunks.is_none(),
            PreparedWorldEvent::Immediate(
                WorldEvent::BlockUpdates(_) | WorldEvent::SyncedBlockUpdates(_),
            ) => self.blocking_block_updates.is_none(),
            _ => true,
        }
    }

    /// Takes the next sequence's ready updates when nothing before the burst blocks them.
    fn take_adjacent_block_updates(
        &mut self,
        end: u64,
        budget: CommitBudget,
    ) -> Option<Vec<BlockUpdateEvent>> {
        let next = end.checked_add(1)?;
        let entry = self.entries.get_mut(&next)?;
        let Slot::Ready(PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(updates))) =
            &entry.slot
        else {
            return None;
        };
        if !block_updates_can_coalesce(updates)
            || self
                .earlier
                .blocks(&entry.footprint, budget.couple_position)
        {
            return None;
        }
        let Slot::Ready(PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(updates))) =
            std::mem::replace(&mut entry.slot, Slot::Active)
        else {
            unreachable!("the adjacent block update was inspected");
        };
        Some(updates)
    }

    /// Installs the async fence only after the caller snapshots and enqueues a block batch.
    pub fn defer_block_updates(&mut self, sequence: u64) {
        assert_eq!(
            self.applying,
            Some(sequence),
            "block batch must be the active FIFO step"
        );
        self.applying = None;
        let end = self.block_update_range_end.take().unwrap_or(sequence);
        self.blocking_block_updates = Some((sequence, end));
    }

    /// Completes one applied step, releasing admission only after its entire event finishes.
    /// Returns true when the coordinator should release the event's request reservation.
    pub fn finish_commit(&mut self, sequence: u64) -> bool {
        if self.applying != Some(sequence) {
            return false;
        }
        self.applying = None;
        if let Some(pending) = &self.pending_sub_chunks
            && pending.sequence == sequence
        {
            if pending.entries.len() != 0 {
                return false;
            }
            self.pending_sub_chunks = None;
        }
        let end = self.block_update_range_end.take().unwrap_or(sequence);
        self.release_range(sequence, end);
        true
    }

    /// Finishes a whole merged event range, then advances the contiguous frontier.
    fn release_range(&mut self, start: u64, end: u64) {
        let mut released = false;
        for sequence in start..=end {
            let Some(entry) = self.entries.remove(&sequence) else {
                continue;
            };
            self.unfinished -= 1;
            self.release_held(entry.held);
            released = true;
        }
        if !released {
            return;
        }
        // Merge with the runs ending just before and starting just after this one.
        let mut run = (start, end);
        if let Some((&before, &before_end)) = self.finished.range(..start).next_back()
            && before_end.saturating_add(1) >= start
        {
            self.finished.remove(&before);
            run = (before, run.1.max(before_end));
        }
        if let Some(after_end) = self.finished.remove(&run.1.saturating_add(1)) {
            run.1 = after_end;
        }
        if run.0 <= self.frontier {
            self.frontier = run.1.saturating_add(1);
        } else {
            self.finished.insert(run.0, run.1);
        }
    }

    /// Whether this sequence's whole mutation has finished.
    #[must_use]
    pub fn is_finished(&self, sequence: u64) -> bool {
        sequence < self.frontier || self.finished_through(sequence) >= sequence
    }

    /// Unfinished entries plus finished runs: everything the state retains per sequence.
    #[cfg(test)]
    pub(crate) fn retained_sequence_records(&self) -> usize {
        self.entries.len() + self.finished.len()
    }

    /// Returns the currently admitted, unfinished event count.
    #[must_use]
    pub const fn admitted_count(&self) -> usize {
        self.unfinished
    }

    /// Returns the admitted terrain and block mutation count.
    #[must_use]
    pub const fn heavy_count(&self) -> usize {
        self.heavy
    }

    /// Returns ordered events still waiting for decode admission.
    #[must_use]
    pub const fn deferred_count(&self) -> usize {
        self.deferred
    }

    /// Returns whether a sequence still owns one heavy admission slot.
    #[must_use]
    pub fn is_heavy_admitted(&self, sequence: u64) -> bool {
        self.entries
            .get(&sequence)
            .is_some_and(|entry| entry.held == Held::Heavy)
    }

    /// Returns the partially applied batch's original sequence.
    #[must_use]
    pub fn pending_batch_sequence(&self) -> Option<u64> {
        self.pending_sub_chunks
            .as_ref()
            .map(|pending| pending.sequence)
    }

    /// Returns the block mutation whose worker completion is still outstanding.
    #[must_use]
    pub fn blocking_block_updates(&self) -> Option<u64> {
        self.blocking_block_updates.map(|(start, _)| start)
    }

    /// Whether a deferred event will take heavy admission when promoted.
    #[must_use]
    pub fn is_deferred_heavy(&self, sequence: u64) -> bool {
        self.entries
            .get(&sequence)
            .is_some_and(|entry| entry.held == Held::Deferred && entry.footprint.heavy)
    }

    /// Counts ready events without exposing mutable access to ordered state.
    #[must_use]
    pub fn ready_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| matches!(entry.slot, Slot::Ready(_)))
            .count()
    }

    /// Light-lane headroom left after retained committed consumers.
    #[must_use]
    pub const fn light_capacity(&self, retained_commits: usize) -> usize {
        MAX_ADMITTED_WORLD_EVENTS.saturating_sub(self.light.saturating_add(retained_commits))
    }

    /// Heavy decode admission headroom.
    #[must_use]
    pub const fn heavy_capacity(&self) -> usize {
        MAX_ADMITTED_HEAVY_EVENTS.saturating_sub(self.heavy)
    }

    /// Room to order terrain behind full decode admission.
    #[must_use]
    pub const fn deferred_capacity(&self) -> usize {
        MAX_DEFERRED_WORLD_EVENTS.saturating_sub(self.deferred)
    }

    /// Headroom for any next event, light or terrain, before the outbound-request bound.
    #[must_use]
    pub const fn remaining_admission_capacity(&self, retained_commits: usize) -> usize {
        let terrain = self
            .deferred_capacity()
            .saturating_add(if self.deferred == 0 {
                self.heavy_capacity()
            } else {
                0
            });
        let light = self.light_capacity(retained_commits);
        if light < terrain { light } else { terrain }
    }
}

fn block_updates_can_coalesce(events: &[BlockUpdateEvent]) -> bool {
    events
        .iter()
        .all(|event| event.layer < world::MAX_STORAGE_COUNT)
}

/// Terrain that spends the heavy allowance with nothing local waiting on it; a pass without
/// the chunk-data allowance skips it.
fn is_heavy_chunk_data(footprint: &Footprint) -> bool {
    footprint.heavy && footprint.is_chunk_data()
}
