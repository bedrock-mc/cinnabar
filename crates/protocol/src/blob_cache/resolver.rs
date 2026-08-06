use super::*;

mod accounting;
mod helpers;
mod ordering;
mod pressure;
mod reconstruction;
mod recovery;
mod status;
pub use self::status::BlobCacheStatus;
use self::{helpers::*, ordering::*, reconstruction::*, recovery::*};

#[derive(Debug, Clone, PartialEq)]
pub enum BlobCacheReady {
    Packet(Packet),
    WorldEvent(WorldEvent),
}

impl BlobCacheReady {
    pub fn into_packet(self) -> Option<Packet> {
        match self {
            Self::Packet(packet) => Some(packet),
            Self::WorldEvent(_) => None,
        }
    }

    pub fn into_world_event(self) -> Option<WorldEvent> {
        match self {
            Self::Packet(_) => None,
            Self::WorldEvent(event) => Some(event),
        }
    }
}

impl BlobCacheResolver {
    #[must_use]
    pub fn new(cache: ClientBlobCache) -> Self {
        Self {
            cache,
            pending: HashMap::with_capacity(MAX_CLIENT_BLOB_PENDING_TRANSACTIONS),
            pending_order: BTreeSet::new(),
            pending_by_hash: HashMap::with_capacity(MAX_CLIENT_BLOB_PENDING_TRANSACTIONS),
            resolved_pending: BTreeSet::new(),
            ready: BTreeMap::new(),
            immediate_ready: BTreeMap::new(),
            recovery_ready: BTreeMap::new(),
            fast_transfer_reset_armed: false,
            next_ready_sequence: 0,
            stats: BlobCacheStats::default(),
        }
    }

    #[must_use]
    pub fn cache(&self) -> &ClientBlobCache {
        &self.cache
    }

    #[must_use]
    pub const fn stats(&self) -> BlobCacheStats {
        self.stats
    }

    /// Arms one bounded, one-shot fast-transfer rotation. No transaction is
    /// changed until a later data-bearing chunk candidate is observed.
    pub fn arm_fast_transfer_reset(&mut self) {
        self.fast_transfer_reset_armed = true;
    }

    /// Clears stale cached work at a confirmed fast-transfer data boundary.
    ///
    /// A response from the prior backend is no longer relevant to this session. Exact recovery
    /// rolls back scheduler admissions before the replacement backend's chunk stream proceeds.
    pub fn reset_pending_for_fast_transfer_candidate(&mut self) -> Result<bool, BlobCacheError> {
        if !std::mem::take(&mut self.fast_transfer_reset_armed) {
            return Ok(false);
        }
        let removed = !self.pending.is_empty() || !self.ready.is_empty();
        if removed {
            self.stats.pending_resets = self.stats.pending_resets.saturating_add(1);
            self.recover_retained_cached_transactions()?;
        }
        Ok(removed)
    }

    pub fn accept_cached_packet(
        &mut self,
        packet: Packet,
    ) -> Result<BlobCacheStatus, BlobCacheError> {
        self.accept_cached_packet_with_raw_size(packet, None)
    }

    pub fn accept_cached_packet_with_size(
        &mut self,
        packet: Packet,
        raw_packet_bytes: usize,
    ) -> Result<BlobCacheStatus, BlobCacheError> {
        self.accept_cached_packet_with_raw_size(packet, Some(raw_packet_bytes))
    }

    fn accept_cached_packet_with_raw_size(
        &mut self,
        packet: Packet,
        raw_packet_bytes: Option<usize>,
    ) -> Result<BlobCacheStatus, BlobCacheError> {
        let skipped_packet = packet.clone();
        match self.accept_cached_packet_inner(packet, raw_packet_bytes) {
            Ok(status) => Ok(status),
            Err(
                BlobCacheError::InvalidLevelChunkCount(_)
                | BlobCacheError::InvalidLevelChunkHashCount { .. },
            ) => {
                self.stats.skipped_cached_packets =
                    self.stats.skipped_cached_packets.saturating_add(1);
                self.stats.cached_packet_semantic_shape =
                    self.stats.cached_packet_semantic_shape.saturating_add(1);
                let recovery = chunk_recovery(&skipped_packet);
                Ok(self.classify_status(&skipped_packet, recovery, false))
            }
            Err(error) => {
                self.reset_pending();
                Err(error)
            }
        }
    }

    /// Makes an ordinary packet ready independently of blob-cache pressure.
    pub fn accept_passthrough(
        &mut self,
        packet: Packet,
        accounted_bytes: usize,
    ) -> Result<(), BlobCacheError> {
        self.accept_immediate(BlobCacheReady::Packet(packet), accounted_bytes)
    }

    /// Makes a normalized hash-free event ready independently of blob-cache pressure.
    pub fn accept_world_event(
        &mut self,
        event: WorldEvent,
        accounted_bytes: usize,
    ) -> Result<(), BlobCacheError> {
        self.accept_immediate(BlobCacheReady::WorldEvent(event), accounted_bytes)
    }

    fn accept_immediate(
        &mut self,
        value: BlobCacheReady,
        accounted_bytes: usize,
    ) -> Result<(), BlobCacheError> {
        let retained_bytes = self
            .retained_ordinary_bytes()?
            .checked_add(accounted_bytes)
            .and_then(|bytes| bytes.checked_add(size_of::<(u64, ImmediateReady)>()))
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        if self.immediate_ready.len() >= MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS
            || retained_bytes > MAX_CLIENT_BLOB_ORDINARY_READY_BYTES
        {
            self.stats.ordinary_backpressure = self.stats.ordinary_backpressure.saturating_add(1);
            return Err(BlobCacheError::OrdinaryLaneFull {
                events: self.immediate_ready.len(),
                bytes: self.stats.ordinary_ready_bytes,
                max_events: MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS,
                max_bytes: MAX_CLIENT_BLOB_ORDINARY_READY_BYTES,
            });
        }
        let columns = ready_value_columns(&value);
        let sequence = self.take_ready_sequence()?;
        self.immediate_ready.insert(
            sequence,
            ImmediateReady {
                value,
                columns,
                accounted_bytes,
                sequence,
            },
        );
        self.refresh_pending_accounting()
    }

    fn accept_cached_packet_inner(
        &mut self,
        packet: Packet,
        raw_packet_bytes: Option<usize>,
    ) -> Result<BlobCacheStatus, BlobCacheError> {
        let (packet, hashes, packet_retained_bytes) = match packet.data {
            McpePacketData::LevelChunkPacket(packet) => {
                let Some(blobs) = packet.blobs.as_ref() else {
                    return Err(BlobCacheError::NotCachedPacket);
                };
                let hashes = blobs.hashes.clone();
                let expected = match packet.sub_chunk_count {
                    count if count >= 0 => usize::try_from(count)
                        .ok()
                        .and_then(|count| count.checked_add(1))
                        .ok_or(BlobCacheError::ByteCountOverflow)?,
                    -1 | -2 => 1,
                    count => return Err(BlobCacheError::InvalidLevelChunkCount(count)),
                };
                if hashes.len() != expected {
                    return Err(BlobCacheError::InvalidLevelChunkHashCount {
                        actual: hashes.len(),
                        expected,
                    });
                }
                let hash_bytes = blobs
                    .hashes
                    .capacity()
                    .checked_mul(8)
                    .ok_or(BlobCacheError::ByteCountOverflow)?;
                let bytes = size_of::<LevelChunkPacket>()
                    .checked_add(packet.payload.capacity())
                    .and_then(|bytes| bytes.checked_add(hash_bytes))
                    .ok_or(BlobCacheError::ByteCountOverflow)?;
                (PendingPacket::LevelChunk(packet), hashes, bytes)
            }
            McpePacketData::SubChunkPacket(packet) => {
                let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = &packet.entries
                else {
                    return Err(BlobCacheError::NotCachedPacket);
                };
                let mut hashes = Vec::new();
                let mut bytes = entries
                    .capacity()
                    .checked_mul(size_of::<
                        valentine::bedrock::version::v1_26_40::SubChunkEntryWithCachingItem,
                    >())
                    .and_then(|entries| entries.checked_add(size_of::<SubchunkPacket>()))
                    .ok_or(BlobCacheError::ByteCountOverflow)?;
                for entry in entries {
                    bytes = bytes
                        .checked_add(entry.payload.as_ref().map_or(0, Vec::capacity))
                        .ok_or(BlobCacheError::ByteCountOverflow)?;
                    if entry.result == SubChunkEntryWithCachingItemResult::Success {
                        hashes.push(entry.blob_id);
                    }
                }
                (PendingPacket::SubChunk(packet), hashes, bytes)
            }
            _ => return Err(BlobCacheError::NotCachedPacket),
        };
        let unique_hashes = stable_unique(&hashes);
        let packet_retained_bytes =
            raw_packet_bytes.map_or(packet_retained_bytes, |raw| raw.max(packet_retained_bytes));
        let accounted_bytes = packet_retained_bytes
            .checked_add(
                hashes
                    .capacity()
                    .checked_mul(size_of::<u64>())
                    .ok_or(BlobCacheError::ByteCountOverflow)?,
            )
            .and_then(|bytes| {
                unique_hashes
                    .capacity()
                    .checked_mul(size_of::<u64>())
                    .and_then(|hash_bytes| bytes.checked_add(hash_bytes))
            })
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        if projected_reconstruction_bytes(&self.cache, &packet, &hashes)?
            .is_some_and(|bytes| bytes > MAX_CLIENT_BLOB_RECONSTRUCTED_BYTES)
        {
            self.record_reconstruction_skip();
            self.stats.abandoned_cached_transactions =
                self.stats.abandoned_cached_transactions.saturating_add(1);
            let recovery = self.prepare_recovery(unadmitted_packet_recovery(&packet));
            return Ok(self.classify_status(&packet, recovery, false));
        }
        let columns = pending_packet_columns(&packet);
        let transaction_pressure =
            self.retained_cached_transaction_count() >= MAX_CLIENT_BLOB_PENDING_TRANSACTIONS;
        let recovery_slot_pressure = self
            .retained_recovery_slot_count()
            .saturating_add(columns.len())
            > MAX_CLIENT_BLOB_RECOVERY_READY_EVENTS;
        let pressure = transaction_pressure || recovery_slot_pressure;
        let mut pressure_recovery = None;
        if pressure {
            self.stats.cached_packet_transaction_pressure = self
                .stats
                .cached_packet_transaction_pressure
                .saturating_add(1);
            if self.recovery_ready.len() < MAX_CLIENT_BLOB_RECOVERY_READY_EVENTS {
                pressure_recovery = self.rotate_oldest_pending_transaction()?;
            }
        }
        let pressure_remains = self.retained_cached_transaction_count()
            >= MAX_CLIENT_BLOB_PENDING_TRANSACTIONS
            || self
                .retained_recovery_slot_count()
                .saturating_add(columns.len())
                > MAX_CLIENT_BLOB_RECOVERY_READY_EVENTS;
        let recovery_must_precede_admission = matches!(packet, PendingPacket::SubChunk(_));
        if pressure_recovery.is_some() && (pressure_remains || recovery_must_precede_admission) {
            self.stats.skipped_cached_packets = self.stats.skipped_cached_packets.saturating_add(1);
            self.stats.abandoned_cached_transactions =
                self.stats.abandoned_cached_transactions.saturating_add(1);
            self.enqueue_recoveries(unadmitted_packet_recovery(&packet));
            let recovery = pressure_recovery
                .take()
                .expect("pressure recovery was checked as present");
            self.refresh_pending_accounting()?;
            return Ok(self.classify_status(&packet, Some(recovery), false));
        }
        if self.retained_cached_transaction_count() >= MAX_CLIENT_BLOB_PENDING_TRANSACTIONS
            || self.recovery_ready.len() >= MAX_CLIENT_BLOB_RECOVERY_READY_EVENTS
            || self
                .retained_recovery_slot_count()
                .saturating_add(columns.len())
                > MAX_CLIENT_BLOB_RECOVERY_READY_EVENTS
        {
            self.stats.skipped_cached_packets = self.stats.skipped_cached_packets.saturating_add(1);
            if !pressure {
                self.stats.cached_packet_transaction_pressure = self
                    .stats
                    .cached_packet_transaction_pressure
                    .saturating_add(1);
            }
            self.stats.abandoned_cached_transactions =
                self.stats.abandoned_cached_transactions.saturating_add(1);
            let recovery = self.prepare_recovery(unadmitted_packet_recovery(&packet));
            return Ok(self.classify_status(&packet, recovery, false));
        }
        let mut status = self.classify_status(&packet, pressure_recovery, true);
        let staged_bytes = status.staged_bytes();
        if staged_bytes > MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION {
            self.cache.unpin_all(&unique_hashes);
            self.record_staged_skip();
            self.stats.abandoned_cached_transactions =
                self.stats.abandoned_cached_transactions.saturating_add(1);
            let recovery = self.prepare_recovery(unadmitted_packet_recovery(&packet));
            self.merge_status_recovery(&mut status, recovery);
            self.refresh_pending_accounting()?;
            return Ok(status);
        }
        let missing = status.missing().to_vec();
        let unresolved_hashes = missing.len().saturating_add(status.outstanding().len());
        let accounted_bytes = accounted_bytes
            .checked_add(
                missing
                    .capacity()
                    .checked_mul(size_of::<u64>())
                    .ok_or(BlobCacheError::ByteCountOverflow)?,
            )
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        debug_assert!(
            self.retained_recovery_slot_count()
                .saturating_add(columns.len())
                <= MAX_CLIENT_BLOB_RECOVERY_READY_EVENTS
        );
        let admission = cached_sub_chunk_admission(&packet);
        let sequence = self.take_ready_sequence()?;
        self.pending.insert(
            sequence,
            PendingTransaction {
                packet,
                hashes,
                unique_hashes,
                owned_hashes: missing,
                unresolved_hashes,
                staged_bytes,
                columns: columns.clone(),
                accounted_bytes,
            },
        );
        self.pending_order.insert(sequence);
        for hash in status.missing().iter().chain(status.outstanding().iter()) {
            self.pending_by_hash
                .entry(*hash)
                .or_default()
                .insert(sequence);
        }
        self.relieve_cached_ready_pressure()?;
        self.refresh_pending_accounting()?;
        if self.stats.pending_bytes > MAX_CLIENT_BLOB_PENDING_BYTES {
            self.record_pending_skip();
            let recovery = self.abandon_pending_transaction_with_recovery(sequence, true)?;
            self.merge_status_recovery(&mut status, recovery);
            self.refresh_pending_accounting()?;
            return Ok(status);
        }
        if unresolved_hashes == 0 {
            self.resolved_pending.insert(sequence);
            self.drain_ready()?;
        }
        if admission.is_some()
            && (self.pending.contains_key(&sequence) || self.ready.contains_key(&sequence))
        {
            status.set_admission(admission);
        }
        Ok(status)
    }

    pub fn accept_miss_response(
        &mut self,
        response: ClientCacheMissResponsePacket,
    ) -> Result<(), BlobCacheError> {
        if response.blobs.is_empty() {
            self.stats.empty_miss_responses = self.stats.empty_miss_responses.saturating_add(1);
            return Ok(());
        }
        let response_hashes = response
            .blobs
            .iter()
            .map(|blob| blob.hash)
            .collect::<Vec<_>>();
        let rejected = u64::try_from(response.blobs.len().max(1)).unwrap_or(u64::MAX);
        match self.accept_miss_response_inner(response) {
            Ok(()) => Ok(()),
            Err(BlobCacheError::UnsolicitedBlob(_)) => {
                self.stats.miss_response_unsolicited =
                    self.stats.miss_response_unsolicited.saturating_add(1);
                self.recover_skipped_miss_response(&response_hashes)
            }
            Err(BlobCacheError::HashMismatch { .. } | BlobCacheError::ConflictingDuplicate(_)) => {
                self.stats.miss_response_integrity_rejection = self
                    .stats
                    .miss_response_integrity_rejection
                    .saturating_add(1);
                self.stats.rejected_blobs = self.stats.rejected_blobs.saturating_add(rejected);
                self.recover_skipped_miss_response(&response_hashes)
            }
            Err(error) => {
                self.stats.rejected_blobs = self.stats.rejected_blobs.saturating_add(rejected);
                self.reset_pending();
                Err(error)
            }
        }
    }

    fn accept_miss_response_inner(
        &mut self,
        response: ClientCacheMissResponsePacket,
    ) -> Result<(), BlobCacheError> {
        let mut unique = Vec::<(u64, Vec<u8>)>::new();
        let mut positions = HashMap::<u64, usize>::new();
        for blob in response.blobs {
            if !self.pending_by_hash.contains_key(&blob.hash) {
                return Err(BlobCacheError::UnsolicitedBlob(blob.hash));
            }
            if let Some(&index) = positions.get(&blob.hash) {
                if unique[index].1 != blob.payload {
                    return Err(BlobCacheError::ConflictingDuplicate(blob.hash));
                }
                continue;
            }
            // Deliberate security divergence from current vanilla: the public cache-poisoning
            // disclosure at https://gist.github.com/JustTalDevelops/1abfdae7ab7618af2ec82f709ffa93bb
            // reports that vanilla stopped validating this hash. Cinnabar keeps validation.
            let actual = client_blob_hash(&blob.payload);
            if actual != blob.hash {
                return Err(BlobCacheError::HashMismatch {
                    claimed: blob.hash,
                    actual,
                });
            }
            positions.insert(blob.hash, unique.len());
            unique.push((blob.hash, blob.payload));
        }

        let mut staged_additions = HashMap::<u64, usize>::new();
        for (hash, payload) in &unique {
            let Some(transactions) = self.pending_by_hash.get(hash) else {
                continue;
            };
            for sequence in transactions {
                let addition = staged_additions.entry(*sequence).or_default();
                *addition = addition.saturating_add(payload.len());
            }
        }
        let staged_excess = staged_additions
            .iter()
            .filter_map(|(sequence, addition)| {
                self.pending
                    .get(sequence)
                    .filter(|transaction| {
                        transaction.staged_bytes.saturating_add(*addition)
                            > MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION
                    })
                    .map(|_| *sequence)
            })
            .collect::<Vec<_>>();
        for sequence in staged_excess {
            self.record_staged_skip();
            self.abandon_pending_transaction(sequence)?;
        }
        for (sequence, addition) in staged_additions {
            if let Some(transaction) = self.pending.get_mut(&sequence) {
                transaction.staged_bytes = transaction.staged_bytes.saturating_add(addition);
                debug_assert!(
                    transaction.staged_bytes <= MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION
                );
            }
        }

        let evictions = {
            let mut store = self.cache.lock();
            let mut candidate = store.clone();
            let before = candidate.entries.len();
            let newly_admitted = unique
                .iter()
                .filter(|(hash, _)| !candidate.entries.contains_key(hash))
                .count();
            for (hash, payload) in &unique {
                insert_verified(&mut candidate, self.cache.limits, *hash, payload)?;
            }
            let expected_without_eviction = before.saturating_add(newly_admitted);
            let evictions = expected_without_eviction.saturating_sub(candidate.entries.len());
            *store = candidate;
            evictions
        };
        self.stats.admitted_blobs = self
            .stats
            .admitted_blobs
            .saturating_add(u64::try_from(unique.len()).unwrap_or(u64::MAX));
        self.stats.evictions = self
            .stats
            .evictions
            .saturating_add(u64::try_from(evictions).unwrap_or(u64::MAX));
        for (hash, _) in &unique {
            self.resolve_hash(*hash)?;
        }
        self.refresh_pending_accounting()
    }

    pub fn pop_ready(&mut self) -> Option<BlobCacheReady> {
        if let Some(recovery) = self.pop_recovery_ready() {
            return Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
                recovery,
            )));
        }
        let cached_sequence = self
            .ready
            .iter()
            .find(|(_, ready)| self.sequence_is_unblocked(ready.sequence, &ready.columns))
            .map(|(sequence, _)| *sequence);
        let immediate_sequence = self
            .immediate_ready
            .iter()
            .find(|(_, ready)| self.sequence_is_unblocked(ready.sequence, &ready.columns))
            .map(|(sequence, _)| *sequence);
        let next_sequence = [cached_sequence, immediate_sequence]
            .into_iter()
            .flatten()
            .min();

        if let Some(sequence) = next_sequence
            && cached_sequence == Some(sequence)
        {
            let ready = self
                .ready
                .remove(&sequence)
                .expect("cached sequence was present");
            self.refresh_pending_accounting()
                .expect("retained ready accounting cannot overflow after a pop");
            return Some(ready.value);
        }
        if let Some(sequence) = next_sequence
            && immediate_sequence == Some(sequence)
        {
            let ready = self
                .immediate_ready
                .remove(&sequence)
                .expect("immediate sequence was present");
            self.refresh_pending_accounting()
                .expect("retained immediate accounting cannot overflow after a pop");
            return Some(ready.value);
        }
        None
    }

    fn drain_ready(&mut self) -> Result<(), BlobCacheError> {
        while let Some(sequence) = self.resolved_pending.first().copied() {
            self.move_pending_to_ready(sequence)?;
        }
        self.refresh_pending_accounting()?;
        Ok(())
    }

    fn move_pending_to_ready(&mut self, sequence: u64) -> Result<(), BlobCacheError> {
        let projected_bytes = {
            let transaction = self
                .pending
                .get(&sequence)
                .expect("resolved index references a pending transaction");
            projected_reconstruction_bytes(&self.cache, &transaction.packet, &transaction.hashes)?
                .expect("a resolved transaction has every referenced blob")
        };
        if projected_bytes > MAX_CLIENT_BLOB_RECONSTRUCTED_BYTES {
            self.record_reconstruction_skip();
            self.abandon_pending_transaction(sequence)?;
            return self.refresh_pending_accounting();
        }
        let (packet, ready_bytes) = {
            let transaction = self
                .pending
                .get(&sequence)
                .expect("resolved index references a pending transaction");
            let packet = reconstruct(&self.cache, transaction, &mut self.stats)?;
            let ready_bytes = ready_value_accounted_bytes(&packet)?;
            (packet, ready_bytes)
        };
        if self
            .retained_reconstructed_bytes()?
            .checked_add(ready_bytes)
            .is_none_or(|bytes| bytes > MAX_CLIENT_BLOB_READY_BYTES)
        {
            self.record_ready_skip();
            self.abandon_pending_transaction(sequence)?;
            return self.refresh_pending_accounting();
        }
        let projected_retained_bytes = self
            .retained_cached_bytes()?
            .checked_sub(
                self.pending
                    .get(&sequence)
                    .expect("resolved transaction remains pending")
                    .accounted_bytes,
            )
            .and_then(|bytes| bytes.checked_add(ready_bytes))
            .and_then(|bytes| bytes.checked_add(size_of::<(u64, ReadyTransaction)>()))
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        if projected_retained_bytes > MAX_CLIENT_BLOB_PENDING_BYTES {
            self.record_pending_skip();
            self.abandon_pending_transaction(sequence)?;
            return self.refresh_pending_accounting();
        }
        let transaction = self
            .remove_pending_transaction(sequence)
            .expect("resolved transaction remains pending");
        self.cache.unpin_all(&transaction.unique_hashes);
        self.ready.insert(
            sequence,
            ReadyTransaction {
                value: packet,
                columns: transaction.columns,
                accounted_bytes: ready_bytes,
                sequence,
            },
        );
        self.relieve_cached_ready_pressure()?;
        self.refresh_pending_accounting()
    }

    fn resolve_hash(&mut self, hash: u64) -> Result<(), BlobCacheError> {
        let Some(transactions) = self.pending_by_hash.remove(&hash) else {
            return Ok(());
        };
        for sequence in transactions {
            let Some(transaction) = self.pending.get_mut(&sequence) else {
                continue;
            };
            transaction.unresolved_hashes = transaction.unresolved_hashes.saturating_sub(1);
            if transaction.unresolved_hashes == 0 {
                self.resolved_pending.insert(sequence);
            }
        }
        self.drain_ready()
    }

    fn remove_pending_transaction(&mut self, sequence: u64) -> Option<PendingTransaction> {
        self.pending_order.remove(&sequence);
        self.resolved_pending.remove(&sequence);
        let transaction = self.pending.remove(&sequence)?;
        for hash in &transaction.unique_hashes {
            let remove_hash = if let Some(transactions) = self.pending_by_hash.get_mut(hash) {
                transactions.remove(&sequence);
                transactions.is_empty()
            } else {
                false
            };
            if remove_hash {
                self.pending_by_hash.remove(hash);
            }
        }
        Some(transaction)
    }

    fn abandon_pending_transaction(&mut self, sequence: u64) -> Result<(), BlobCacheError> {
        self.abandon_pending_transaction_with_recovery(sequence, false)
            .map(|_| ())
    }

    fn abandon_pending_transaction_with_recovery(
        &mut self,
        sequence: u64,
        return_first: bool,
    ) -> Result<Option<ChunkResyncEvent>, BlobCacheError> {
        self.promote_waiters(sequence)?;
        let Some(transaction) = self.remove_pending_transaction(sequence) else {
            return Ok(None);
        };
        self.cache.unpin_all(&transaction.unique_hashes);
        self.stats.abandoned_cached_transactions =
            self.stats.abandoned_cached_transactions.saturating_add(1);
        let recoveries = pending_packet_recovery(&transaction.packet);
        if return_first {
            Ok(self.prepare_recovery(recoveries))
        } else {
            self.enqueue_recoveries(recoveries);
            Ok(None)
        }
    }

    fn promote_waiters(&mut self, owner: u64) -> Result<(), BlobCacheError> {
        let promotions = self
            .pending
            .get(&owner)
            .into_iter()
            .flat_map(|transaction| transaction.owned_hashes.iter())
            .filter_map(|hash| {
                self.pending_by_hash.get(hash).and_then(|transactions| {
                    transactions
                        .iter()
                        .filter(|sequence| **sequence != owner)
                        .min()
                        .copied()
                        .map(|waiter| (*hash, waiter))
                })
            })
            .collect::<Vec<_>>();
        for (hash, waiter) in promotions {
            self.add_owned_hash(waiter, hash)?;
        }
        Ok(())
    }

    fn add_owned_hash(&mut self, sequence: u64, hash: u64) -> Result<(), BlobCacheError> {
        let Some(transaction) = self.pending.get_mut(&sequence) else {
            return Ok(());
        };
        if transaction.owned_hashes.contains(&hash) {
            return Ok(());
        }
        let previous_capacity = transaction.owned_hashes.capacity();
        transaction
            .owned_hashes
            .try_reserve(1)
            .map_err(|_| BlobCacheError::ByteCountOverflow)?;
        let added_capacity = transaction
            .owned_hashes
            .capacity()
            .saturating_sub(previous_capacity);
        let added_bytes = added_capacity
            .checked_mul(size_of::<u64>())
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        transaction.accounted_bytes = transaction
            .accounted_bytes
            .checked_add(added_bytes)
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        transaction.owned_hashes.push(hash);
        Ok(())
    }

    fn retained_cached_transaction_count(&self) -> usize {
        self.pending.len().saturating_add(self.ready.len())
    }

    fn retained_recovery_slot_count(&self) -> usize {
        self.recovery_ready
            .len()
            .saturating_add(
                self.pending
                    .values()
                    .map(|transaction| transaction.columns.len())
                    .sum::<usize>(),
            )
            .saturating_add(
                self.ready
                    .values()
                    .map(|transaction| transaction.columns.len())
                    .sum::<usize>(),
            )
    }

    /// Conservative, unverified ordering: ordinary updates for a column wait until every earlier
    /// cached chunk for that column has reconstructed and been emitted. Mojang's behavior here is
    /// connection-wide rather than per-column; aligning this scan remains open parity work.
    fn sequence_is_unblocked(&self, sequence: u64, columns: &[ColumnKey]) -> bool {
        let conflicts = |earlier_sequence: &u64, earlier_columns: &[ColumnKey]| {
            *earlier_sequence < sequence
                && earlier_columns
                    .iter()
                    .any(|column| columns.contains(column))
        };
        !self
            .pending
            .iter()
            .any(|(earlier, transaction)| conflicts(earlier, &transaction.columns))
            && !self
                .ready
                .iter()
                .any(|(earlier, transaction)| conflicts(earlier, &transaction.columns))
    }

    fn take_ready_sequence(&mut self) -> Result<u64, BlobCacheError> {
        let sequence = self.next_ready_sequence;
        self.next_ready_sequence = sequence
            .checked_add(1)
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        Ok(sequence)
    }

    fn prepare_recovery(&mut self, recoveries: Vec<ChunkResyncEvent>) -> Option<ChunkResyncEvent> {
        let mut recoveries = recoveries.into_iter();
        let first = recoveries.next();
        self.enqueue_recoveries(recoveries);
        // `first` goes out inline on the status, never through `enqueue_recovery`.
        if first.is_some() {
            self.stats.recovery_requests = self.stats.recovery_requests.saturating_add(1);
        }
        first
    }

    fn enqueue_recoveries(&mut self, recoveries: impl IntoIterator<Item = ChunkResyncEvent>) {
        for recovery in recoveries {
            self.enqueue_recovery(recovery);
        }
    }

    pub(super) fn enqueue_recovery(&mut self, recovery: ChunkResyncEvent) {
        self.enqueue_recovery_inner(recovery, true);
    }

    fn enqueue_precounted_recovery(&mut self, recovery: ChunkResyncEvent) {
        if !self.enqueue_recovery_inner(recovery, false) {
            self.stats.recovery_requests = self.stats.recovery_requests.saturating_sub(1);
        }
    }

    fn enqueue_recovery_inner(&mut self, recovery: ChunkResyncEvent, count_new: bool) -> bool {
        let (key, incoming) = RecoveryReady::from_event(recovery);
        let Some(existing) = self.recovery_ready.get_mut(&key) else {
            self.recovery_ready.insert(key, incoming);
            if count_new {
                self.stats.recovery_requests = self.stats.recovery_requests.saturating_add(1);
            }
            return true;
        };

        let incoming_is_full = incoming.requested_sub_chunk_ys.is_none();
        let incoming_ys = incoming.requested_sub_chunk_ys;
        match (existing.requested_sub_chunk_ys.as_mut(), incoming_ys) {
            (Some(existing_ys), Some(incoming_ys)) => existing_ys.extend(incoming_ys),
            (Some(_), None) => existing.requested_sub_chunk_ys = None,
            (None, Some(_)) | (None, None) => {}
        }
        if incoming_is_full {
            existing.requested_sub_chunks = incoming.requested_sub_chunks;
        }
        false
    }

    /// The only constructor for `BlobCacheStatus`: extracts the complete reference set from the
    /// actual cached packet and partitions every unique hash. The private construction marker
    /// prevents callers and future skip paths from fabricating an unclassified status with a
    /// struct literal or `Default`.
    fn classify_status<T: ReferencedBlobHashes>(
        &mut self,
        packet: &T,
        recovery: Option<ChunkResyncEvent>,
        pin: bool,
    ) -> BlobCacheStatus {
        let mut status = BlobCacheStatus::classify(&self.cache, packet, recovery, pin);
        status.omit_outstanding(&self.pending_by_hash);
        self.stats.hashes_classified = self
            .stats
            .hashes_classified
            .saturating_add(u64::try_from(status.classified_hashes()).unwrap_or(u64::MAX));
        self.stats.hits = self
            .stats
            .hits
            .saturating_add(u64::try_from(status.have().len()).unwrap_or(u64::MAX));
        self.stats.misses = self.stats.misses.saturating_add(
            u64::try_from(
                status
                    .missing()
                    .len()
                    .saturating_add(status.outstanding().len()),
            )
            .unwrap_or(u64::MAX),
        );
        self.stats.redundant_missing_requests = self
            .stats
            .redundant_missing_requests
            .saturating_add(u64::try_from(status.outstanding().len()).unwrap_or(u64::MAX));
        status
    }

    fn record_staged_skip(&mut self) {
        self.stats.skipped_cached_packets = self.stats.skipped_cached_packets.saturating_add(1);
        self.stats.cached_packet_staged_pressure =
            self.stats.cached_packet_staged_pressure.saturating_add(1);
    }

    fn record_pending_skip(&mut self) {
        self.stats.skipped_cached_packets = self.stats.skipped_cached_packets.saturating_add(1);
        self.stats.cached_packet_pending_pressure =
            self.stats.cached_packet_pending_pressure.saturating_add(1);
    }

    fn record_ready_skip(&mut self) {
        self.stats.skipped_cached_packets = self.stats.skipped_cached_packets.saturating_add(1);
        self.stats.cached_packet_ready_pressure =
            self.stats.cached_packet_ready_pressure.saturating_add(1);
    }

    fn record_reconstruction_skip(&mut self) {
        self.stats.skipped_cached_packets = self.stats.skipped_cached_packets.saturating_add(1);
        self.stats.cached_packet_reconstruction_pressure = self
            .stats
            .cached_packet_reconstruction_pressure
            .saturating_add(1);
    }
}

impl Drop for BlobCacheResolver {
    fn drop(&mut self) {
        self.reset_pending();
    }
}

trait ReferencedBlobHashes {
    fn referenced_blob_hashes(&self) -> Vec<u64>;
}

impl ReferencedBlobHashes for Packet {
    fn referenced_blob_hashes(&self) -> Vec<u64> {
        match &self.data {
            McpePacketData::LevelChunkPacket(packet) => packet
                .blobs
                .as_ref()
                .map_or_else(Vec::new, |blobs| blobs.hashes.clone()),
            McpePacketData::SubChunkPacket(packet) => {
                subchunk_referenced_blob_hashes(&packet.entries)
            }
            _ => Vec::new(),
        }
    }
}

impl ReferencedBlobHashes for PendingPacket {
    fn referenced_blob_hashes(&self) -> Vec<u64> {
        match self {
            Self::LevelChunk(packet) => packet
                .blobs
                .as_ref()
                .map_or_else(Vec::new, |blobs| blobs.hashes.clone()),
            Self::SubChunk(packet) => subchunk_referenced_blob_hashes(&packet.entries),
        }
    }
}

fn subchunk_referenced_blob_hashes(entries: &SubchunkPacketEntries) -> Vec<u64> {
    let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = entries else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|entry| entry.result == SubChunkEntryWithCachingItemResult::Success)
        .map(|entry| entry.blob_id)
        .collect()
}
