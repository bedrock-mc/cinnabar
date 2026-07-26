use super::*;

mod helpers;
use helpers::*;
mod ordering;
use ordering::*;

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
            pending: HashMap::new(),
            pending_order: BTreeSet::new(),
            pending_by_hash: HashMap::new(),
            resolved_pending: BTreeSet::new(),
            ready: BTreeMap::new(),
            immediate_ready: BTreeMap::new(),
            column_barriers: HashMap::new(),
            recovery_ready: VecDeque::new(),
            authorized_misses: Vec::new(),
            retired_authorized_misses: Vec::new(),
            fast_transfer_rotation_armed: false,
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
    pub fn arm_fast_transfer_rotation(&mut self) {
        self.fast_transfer_rotation_armed = true;
    }

    /// Selectively retires unresolved cached transactions that precede a new
    /// chunk candidate while preserving ready and ordinary FIFO work.
    pub fn rotate_pending_for_fast_transfer_candidate(&mut self) -> Result<bool, BlobCacheError> {
        if !std::mem::take(&mut self.fast_transfer_rotation_armed) {
            return Ok(false);
        }

        self.retired_authorized_misses
            .try_reserve(self.authorized_misses.len())
            .map_err(|_| BlobCacheError::ByteCountOverflow)?;
        let mut retired = std::mem::take(&mut self.retired_authorized_misses);
        let mut removed = false;
        let candidates = self.pending_order.iter().copied().collect::<Vec<_>>();
        for sequence in candidates {
            if self
                .pending
                .get(&sequence)
                .is_none_or(|transaction| transaction.unresolved_hashes == 0)
            {
                continue;
            }

            removed = true;
            let transaction = self
                .remove_pending_transaction(sequence)
                .expect("pending order references a transaction");
            for hash in transaction
                .unique_hashes
                .iter()
                .copied()
                .filter(|hash| !self.cache.contains(*hash))
            {
                if decrement_authorization(&mut self.authorized_misses, hash) {
                    increment_authorization(&mut retired, hash)?;
                }
            }
            self.cache.unpin_all(&transaction.unique_hashes);
            self.remove_column_barriers(sequence, &transaction.columns);
        }
        if self.authorized_misses.is_empty() {
            self.authorized_misses = Vec::new();
        } else {
            self.authorized_misses.shrink_to_fit();
        }
        self.retired_authorized_misses = retired;
        if removed {
            self.stats.pending_resets = self.stats.pending_resets.saturating_add(1);
        }
        self.refresh_pending_accounting()?;
        Ok(removed)
    }

    pub(super) fn retained_pending_bytes(&self) -> Result<usize, BlobCacheError> {
        self.retained_cached_bytes()?
            .checked_add(self.retained_immediate_bytes()?)
            .ok_or(BlobCacheError::ByteCountOverflow)
    }

    fn retained_cached_bytes(&self) -> Result<usize, BlobCacheError> {
        self.pending
            .capacity()
            .checked_mul(size_of::<(u64, PendingTransaction)>())
            .and_then(|bytes| {
                self.ready
                    .len()
                    .checked_mul(size_of::<(u64, ReadyTransaction)>())
                    .and_then(|ready| bytes.checked_add(ready))
            })
            .and_then(|bytes| {
                self.pending_order
                    .len()
                    .checked_mul(size_of::<u64>())
                    .and_then(|order| bytes.checked_add(order))
            })
            .and_then(|bytes| {
                self.resolved_pending
                    .len()
                    .checked_mul(size_of::<u64>())
                    .and_then(|resolved| bytes.checked_add(resolved))
            })
            .and_then(|bytes| {
                self.pending_by_hash
                    .capacity()
                    .checked_mul(size_of::<(u64, HashSet<u64>)>())
                    .and_then(|index| bytes.checked_add(index))
            })
            .and_then(|bytes| {
                self.pending_by_hash.values().try_fold(bytes, |total, ids| {
                    ids.capacity()
                        .checked_mul(size_of::<u64>())
                        .and_then(|index| total.checked_add(index))
                })
            })
            .and_then(|bytes| {
                self.column_barriers
                    .capacity()
                    .checked_mul(size_of::<(ColumnKey, BTreeSet<u64>)>())
                    .and_then(|columns| bytes.checked_add(columns))
            })
            .and_then(|bytes| {
                self.column_barriers.values().try_fold(bytes, |total, ids| {
                    ids.len()
                        .checked_mul(size_of::<u64>())
                        .and_then(|index| total.checked_add(index))
                })
            })
            .and_then(|bytes| {
                self.authorized_misses
                    .capacity()
                    .checked_mul(size_of::<(u64, usize)>())
                    .and_then(|authorized| bytes.checked_add(authorized))
            })
            .and_then(|bytes| {
                self.retired_authorized_misses
                    .capacity()
                    .checked_mul(size_of::<(u64, usize)>())
                    .and_then(|retired| bytes.checked_add(retired))
            })
            .and_then(|bytes| {
                self.pending.iter().try_fold(bytes, |total, transaction| {
                    total.checked_add(transaction.1.accounted_bytes)
                })
            })
            .and_then(|bytes| {
                self.ready.iter().try_fold(bytes, |total, transaction| {
                    total.checked_add(transaction.1.accounted_bytes)
                })
            })
            .ok_or(BlobCacheError::ByteCountOverflow)
    }

    fn retained_immediate_bytes(&self) -> Result<usize, BlobCacheError> {
        self.immediate_ready
            .len()
            .checked_mul(size_of::<(u64, ImmediateReady)>())
            .and_then(|bytes| {
                self.immediate_ready
                    .values()
                    .try_fold(bytes, |total, ready| {
                        total.checked_add(ready.accounted_bytes)
                    })
            })
            .and_then(|bytes| {
                self.recovery_ready
                    .capacity()
                    .checked_mul(size_of::<ReadyRecovery>())
                    .and_then(|recovery| bytes.checked_add(recovery))
            })
            .ok_or(BlobCacheError::ByteCountOverflow)
    }

    fn refresh_pending_accounting(&mut self) -> Result<(), BlobCacheError> {
        if self.pending.is_empty() {
            self.pending = HashMap::new();
            self.pending_order = BTreeSet::new();
            self.resolved_pending = BTreeSet::new();
        }
        if self.pending_by_hash.is_empty() {
            self.pending_by_hash = HashMap::new();
        }
        if self.column_barriers.is_empty() {
            self.column_barriers = HashMap::new();
        }
        let pending_bytes = self.retained_pending_bytes()?;
        self.stats.pending_bytes = pending_bytes;
        self.stats.pending_transactions = self.pending.len().saturating_add(self.ready.len());
        if self.retained_cached_bytes()? > self.cache.limits.max_pending_bytes {
            return Err(BlobCacheError::TooManyPendingBytes {
                max: self.cache.limits.max_pending_bytes,
            });
        }
        if self.retained_immediate_bytes()? > self.cache.limits.max_pending_bytes {
            return Err(BlobCacheError::ImmediateReadyBytePressure {
                max: self.cache.limits.max_pending_bytes,
            });
        }
        Ok(())
    }

    pub fn accept_cached_packet(
        &mut self,
        packet: Packet,
    ) -> Result<ClientCacheBlobStatusPacket, BlobCacheError> {
        self.accept_cached_packet_with_raw_size(packet, None)
    }

    pub fn accept_cached_packet_with_size(
        &mut self,
        packet: Packet,
        raw_packet_bytes: usize,
    ) -> Result<ClientCacheBlobStatusPacket, BlobCacheError> {
        self.accept_cached_packet_with_raw_size(packet, Some(raw_packet_bytes))
    }

    fn accept_cached_packet_with_raw_size(
        &mut self,
        packet: Packet,
        raw_packet_bytes: Option<usize>,
    ) -> Result<ClientCacheBlobStatusPacket, BlobCacheError> {
        let pressure_packet = packet.clone();
        let sequence_before = self.next_ready_sequence;
        match self.accept_cached_packet_inner(packet, raw_packet_bytes) {
            Ok(status) => Ok(status),
            Err(BlobCacheError::TooManyPendingTransactions { .. }) => {
                self.rollback_pressure_admission(sequence_before)?;
                self.stats.skipped_cached_packets =
                    self.stats.skipped_cached_packets.saturating_add(1);
                self.stats.cached_packet_transaction_pressure = self
                    .stats
                    .cached_packet_transaction_pressure
                    .saturating_add(1);
                self.queue_level_chunk_resync(&pressure_packet)?;
                Ok(self.classify_skipped_packet(&pressure_packet))
            }
            Err(BlobCacheError::TooManyPendingBytes { .. }) => {
                self.rollback_pressure_admission(sequence_before)?;
                self.stats.skipped_cached_packets =
                    self.stats.skipped_cached_packets.saturating_add(1);
                self.stats.cached_packet_byte_pressure =
                    self.stats.cached_packet_byte_pressure.saturating_add(1);
                self.queue_level_chunk_resync(&pressure_packet)?;
                Ok(self.classify_skipped_packet(&pressure_packet))
            }
            Err(
                BlobCacheError::InvalidLevelChunkCount(_)
                | BlobCacheError::InvalidLevelChunkHashCount { .. }
                | BlobCacheError::TooManyHashes { .. },
            ) => {
                self.rollback_pressure_admission(sequence_before)?;
                self.stats.skipped_cached_packets =
                    self.stats.skipped_cached_packets.saturating_add(1);
                self.stats.cached_packet_semantic_shape =
                    self.stats.cached_packet_semantic_shape.saturating_add(1);
                self.queue_level_chunk_resync(&pressure_packet)?;
                Ok(self.classify_skipped_packet(&pressure_packet))
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
        if self.immediate_ready.len() >= self.cache.limits.max_pending_transactions {
            return Err(BlobCacheError::ImmediateReadyCountPressure {
                max: self.cache.limits.max_pending_transactions,
            });
        }
        let projected = self
            .retained_immediate_bytes()?
            .checked_add(size_of::<(u64, ImmediateReady)>())
            .and_then(|bytes| bytes.checked_add(accounted_bytes))
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        if projected > self.cache.limits.max_pending_bytes {
            return Err(BlobCacheError::ImmediateReadyBytePressure {
                max: self.cache.limits.max_pending_bytes,
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

    fn classify_skipped_packet(&self, packet: &Packet) -> ClientCacheBlobStatusPacket {
        classified_cached_status(&self.cache, packet)
    }

    fn accept_cached_packet_inner(
        &mut self,
        packet: Packet,
        raw_packet_bytes: Option<usize>,
    ) -> Result<ClientCacheBlobStatusPacket, BlobCacheError> {
        let (packet, hashes, packet_retained_bytes) = match packet.data {
            McpePacketData::PacketLevelChunk(packet) => {
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
            McpePacketData::PacketSubchunk(packet) => {
                let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = &packet.entries
                else {
                    return Err(BlobCacheError::NotCachedPacket);
                };
                let mut hashes = Vec::new();
                let mut bytes = entries
                    .capacity()
                    .checked_mul(size_of::<
                        valentine::bedrock::version::v1_26_30::SubChunkEntryWithCachingItem,
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
        if hashes.len() > self.cache.limits.max_hashes_per_packet {
            return Err(BlobCacheError::TooManyHashes {
                count: hashes.len(),
                max: self.cache.limits.max_hashes_per_packet,
            });
        }
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
        if self.pending.len().saturating_add(self.ready.len())
            >= self.cache.limits.max_pending_transactions
        {
            return Err(BlobCacheError::TooManyPendingTransactions {
                max: self.cache.limits.max_pending_transactions,
            });
        }
        let preliminary_pending_bytes = self
            .retained_cached_bytes()?
            .checked_add(accounted_bytes)
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        if preliminary_pending_bytes > self.cache.limits.max_pending_bytes {
            return Err(BlobCacheError::TooManyPendingBytes {
                max: self.cache.limits.max_pending_bytes,
            });
        }

        let (have, missing) = self.cache.classify_and_pin(&unique_hashes);
        let mut authorized_candidate = self.authorized_misses.clone();
        for hash in &missing {
            if let Some((_, count)) = authorized_candidate
                .iter_mut()
                .find(|(candidate, _)| candidate == hash)
            {
                let Some(next) = count.checked_add(1) else {
                    self.cache.unpin_all(&unique_hashes);
                    return Err(BlobCacheError::ByteCountOverflow);
                };
                *count = next;
            } else {
                if authorized_candidate.try_reserve(1).is_err() {
                    self.cache.unpin_all(&unique_hashes);
                    return Err(BlobCacheError::ByteCountOverflow);
                }
                authorized_candidate.push((*hash, 1));
            }
        }
        let Some(authorized_candidate_bytes) = authorized_candidate
            .capacity()
            .checked_mul(size_of::<(u64, usize)>())
        else {
            self.cache.unpin_all(&unique_hashes);
            return Err(BlobCacheError::ByteCountOverflow);
        };
        let Some(pending_bytes) = self
            .retained_cached_bytes()?
            .checked_sub(
                self.authorized_misses
                    .capacity()
                    .checked_mul(size_of::<(u64, usize)>())
                    .ok_or(BlobCacheError::ByteCountOverflow)?,
            )
            .and_then(|bytes| bytes.checked_add(authorized_candidate_bytes))
            .and_then(|bytes| bytes.checked_add(accounted_bytes))
        else {
            self.cache.unpin_all(&unique_hashes);
            return Err(BlobCacheError::ByteCountOverflow);
        };
        if pending_bytes > self.cache.limits.max_pending_bytes {
            self.cache.unpin_all(&unique_hashes);
            return Err(BlobCacheError::TooManyPendingBytes {
                max: self.cache.limits.max_pending_bytes,
            });
        }
        self.authorized_misses = authorized_candidate;
        let columns = pending_packet_columns(&packet);
        let sequence = self.take_ready_sequence()?;
        self.pending.insert(
            sequence,
            PendingTransaction {
                packet,
                hashes,
                unique_hashes,
                unresolved_hashes: missing.len(),
                columns: columns.clone(),
                accounted_bytes,
            },
        );
        self.pending_order.insert(sequence);
        for hash in &missing {
            self.pending_by_hash
                .entry(*hash)
                .or_default()
                .insert(sequence);
        }
        self.add_column_barriers(sequence, &columns);
        self.refresh_pending_accounting()?;
        self.stats.hashes_classified = self
            .stats
            .hashes_classified
            .saturating_add(u64::try_from(have.len() + missing.len()).unwrap_or(u64::MAX));
        self.stats.hits = self
            .stats
            .hits
            .saturating_add(u64::try_from(have.len()).unwrap_or(u64::MAX));
        self.stats.misses = self
            .stats
            .misses
            .saturating_add(u64::try_from(missing.len()).unwrap_or(u64::MAX));
        if missing.is_empty() {
            self.resolved_pending.insert(sequence);
            self.drain_ready()?;
        }
        Ok(ClientCacheBlobStatusPacket { missing, have })
    }

    pub fn accept_miss_response(
        &mut self,
        response: ClientCacheMissResponsePacket,
    ) -> Result<(), BlobCacheError> {
        if response.blobs.is_empty() {
            self.stats.empty_miss_responses = self.stats.empty_miss_responses.saturating_add(1);
            return Ok(());
        }
        let rejected = u64::try_from(response.blobs.len().max(1)).unwrap_or(u64::MAX);
        match self.accept_miss_response_inner(response) {
            Ok(()) => Ok(()),
            Err(BlobCacheError::TooManyPendingBytes { .. }) => {
                self.stats.miss_response_byte_pressure =
                    self.stats.miss_response_byte_pressure.saturating_add(1);
                self.recover_repeated_ready_byte_pressure()
            }
            Err(BlobCacheError::UnsolicitedBlob(_)) => {
                self.stats.miss_response_unsolicited =
                    self.stats.miss_response_unsolicited.saturating_add(1);
                self.recover_skipped_miss_response()
            }
            Err(BlobCacheError::TooManyHashes { .. }) => {
                self.stats.miss_response_semantic_shape =
                    self.stats.miss_response_semantic_shape.saturating_add(1);
                self.stats.rejected_blobs = self.stats.rejected_blobs.saturating_add(rejected);
                self.recover_skipped_miss_response()
            }
            Err(
                BlobCacheError::BlobTooLarge { .. }
                | BlobCacheError::HashMismatch { .. }
                | BlobCacheError::ConflictingDuplicate(_),
            ) => {
                self.stats.miss_response_integrity_rejection = self
                    .stats
                    .miss_response_integrity_rejection
                    .saturating_add(1);
                self.stats.rejected_blobs = self.stats.rejected_blobs.saturating_add(rejected);
                self.recover_skipped_miss_response()
            }
            Err(BlobCacheError::CacheCapacity { .. }) => {
                self.stats.miss_response_cache_pressure =
                    self.stats.miss_response_cache_pressure.saturating_add(1);
                self.stats.rejected_blobs = self.stats.rejected_blobs.saturating_add(rejected);
                self.recover_skipped_miss_response()
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
        if response.blobs.len() > self.cache.limits.max_hashes_per_packet {
            return Err(BlobCacheError::TooManyHashes {
                count: response.blobs.len(),
                max: self.cache.limits.max_hashes_per_packet,
            });
        }
        let mut unique = Vec::<(u64, Vec<u8>)>::new();
        let mut positions = HashMap::<u64, usize>::new();
        for blob in response.blobs {
            if blob.payload.len() > self.cache.limits.max_blob_bytes {
                return Err(BlobCacheError::BlobTooLarge {
                    bytes: blob.payload.len(),
                    max: self.cache.limits.max_blob_bytes,
                });
            }
            if self
                .authorized_misses
                .iter()
                .find(|(hash, _)| *hash == blob.hash)
                .map_or(0, |(_, count)| *count)
                .saturating_add(
                    self.retired_authorized_misses
                        .iter()
                        .find(|(hash, _)| *hash == blob.hash)
                        .map_or(0, |(_, count)| *count),
                )
                == 0
            {
                return Err(BlobCacheError::UnsolicitedBlob(blob.hash));
            }
            if let Some(&index) = positions.get(&blob.hash) {
                if unique[index].1 != blob.payload {
                    return Err(BlobCacheError::ConflictingDuplicate(blob.hash));
                }
                continue;
            }
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
        for (hash, _) in &unique {
            if !decrement_authorization(&mut self.authorized_misses, *hash) {
                let consumed = decrement_authorization(&mut self.retired_authorized_misses, *hash);
                debug_assert!(consumed, "validated miss response retained authorization");
            }
        }
        if self.authorized_misses.is_empty() {
            self.authorized_misses = Vec::new();
        } else {
            self.authorized_misses.shrink_to_fit();
        }
        if self.retired_authorized_misses.is_empty() {
            self.retired_authorized_misses = Vec::new();
        } else {
            self.retired_authorized_misses.shrink_to_fit();
        }
        self.refresh_pending_accounting()?;
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

    pub fn reset_pending(&mut self) {
        if !self.pending.is_empty()
            || !self.ready.is_empty()
            || !self.authorized_misses.is_empty()
            || !self.retired_authorized_misses.is_empty()
        {
            self.stats.pending_resets = self.stats.pending_resets.saturating_add(1);
        }
        for transaction in self.pending.drain() {
            self.cache.unpin_all(&transaction.1.unique_hashes);
        }
        self.pending = HashMap::new();
        self.pending_order = BTreeSet::new();
        self.pending_by_hash = HashMap::new();
        self.resolved_pending = BTreeSet::new();
        self.ready = BTreeMap::new();
        self.immediate_ready = BTreeMap::new();
        self.column_barriers = HashMap::new();
        self.recovery_ready = VecDeque::new();
        self.authorized_misses = Vec::new();
        self.retired_authorized_misses = Vec::new();
        self.fast_transfer_rotation_armed = false;
        self.next_ready_sequence = 0;
        self.stats.pending_transactions = 0;
        self.stats.pending_bytes = 0;
    }

    fn rollback_pressure_admission(&mut self, sequence_before: u64) -> Result<(), BlobCacheError> {
        let pending = self
            .pending_order
            .range(sequence_before..)
            .copied()
            .collect::<Vec<_>>();
        for sequence in pending {
            let transaction = self
                .remove_pending_transaction(sequence)
                .expect("new pending transaction");
            for hash in transaction
                .unique_hashes
                .iter()
                .copied()
                .filter(|hash| !self.cache.contains(*hash))
            {
                let _ = decrement_authorization(&mut self.authorized_misses, hash);
            }
            self.cache.unpin_all(&transaction.unique_hashes);
            self.remove_column_barriers(sequence, &transaction.columns);
        }
        let ready = self
            .ready
            .range(sequence_before..)
            .map(|(sequence, _)| *sequence)
            .collect::<Vec<_>>();
        for sequence in ready {
            if let Some(transaction) = self.ready.remove(&sequence) {
                self.remove_column_barriers(sequence, &transaction.columns);
            }
        }
        self.next_ready_sequence = sequence_before;
        if self.authorized_misses.is_empty() {
            self.authorized_misses = Vec::new();
        } else {
            self.authorized_misses.shrink_to_fit();
        }
        self.refresh_pending_accounting()
    }

    fn retire_one_transaction_for_pressure(&mut self) -> Result<bool, BlobCacheError> {
        if self.retained_cached_bytes()? > self.cache.limits.max_pending_bytes
            && let Some((sequence, transaction)) = self.ready.pop_last()
        {
            self.stats.retired_cached_transactions =
                self.stats.retired_cached_transactions.saturating_add(1);
            self.remove_column_barriers(sequence, &transaction.columns);
            self.refresh_pending_accounting()?;
            return Ok(true);
        }
        let Some(sequence) = self.pending_order.first().copied() else {
            return Ok(false);
        };
        let transaction = self
            .remove_pending_transaction(sequence)
            .expect("pending order references a transaction");
        self.stats.retired_cached_transactions =
            self.stats.retired_cached_transactions.saturating_add(1);
        for hash in transaction.unique_hashes.iter().copied() {
            if decrement_authorization(&mut self.authorized_misses, hash)
                && !self.cache.contains(hash)
            {
                increment_authorization(&mut self.retired_authorized_misses, hash)?;
            }
        }
        if self.authorized_misses.is_empty() {
            self.authorized_misses = Vec::new();
        }
        self.cache.unpin_all(&transaction.unique_hashes);
        self.remove_column_barriers(sequence, &transaction.columns);
        self.refresh_pending_accounting()?;
        Ok(true)
    }

    fn recover_repeated_ready_byte_pressure(&mut self) -> Result<(), BlobCacheError> {
        let mut retirement_budget = self.pending.len().saturating_add(self.ready.len());
        while retirement_budget != 0 {
            if !self.retire_one_transaction_for_pressure()? {
                break;
            }
            retirement_budget -= 1;
            match self.drain_ready() {
                Ok(()) => return Ok(()),
                Err(BlobCacheError::TooManyPendingBytes { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        self.refresh_pending_accounting()
    }

    fn recover_skipped_miss_response(&mut self) -> Result<(), BlobCacheError> {
        self.stats.skipped_miss_responses = self.stats.skipped_miss_responses.saturating_add(1);
        self.refresh_pending_accounting()
    }

    fn queue_level_chunk_resync(&mut self, packet: &Packet) -> Result<(), BlobCacheError> {
        let Some(recovery) = level_chunk_resync(packet) else {
            return Ok(());
        };
        self.queue_chunk_resync(recovery)
    }

    fn queue_chunk_resync(&mut self, recovery: ChunkResyncEvent) -> Result<(), BlobCacheError> {
        let recovery_limit = self.cache.limits.max_pending_transactions.max(1);
        if self.recovery_ready.len() >= recovery_limit {
            self.stats.resync_queue_full_drops =
                self.stats.resync_queue_full_drops.saturating_add(1);
            return Ok(());
        }
        self.recovery_ready
            .try_reserve(1)
            .map_err(|_| BlobCacheError::ByteCountOverflow)?;
        self.recovery_ready
            .push_back(ReadyRecovery { event: recovery });
        self.stats.resync_queued = self.stats.resync_queued.saturating_add(1);
        Ok(())
    }

    pub fn pop_ready(&mut self) -> Option<BlobCacheReady> {
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
            self.remove_column_barriers(sequence, &ready.columns);
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
        let recovery = self.recovery_ready.pop_front()?;
        if self.recovery_ready.is_empty() {
            self.recovery_ready = VecDeque::new();
        }
        self.stats.resync_emitted = self.stats.resync_emitted.saturating_add(1);
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            recovery.event,
        )))
    }

    fn drain_ready(&mut self) -> Result<(), BlobCacheError> {
        while let Some(sequence) = self.resolved_pending.first().copied() {
            self.move_pending_to_ready(sequence)?;
        }
        self.refresh_pending_accounting()?;
        Ok(())
    }

    fn move_pending_to_ready(&mut self, sequence: u64) -> Result<(), BlobCacheError> {
        let (packet, ready_bytes) = {
            let transaction = self
                .pending
                .get(&sequence)
                .expect("resolved index references a pending transaction");
            let estimated_ready_bytes = reconstructed_accounted_bytes(&self.cache, transaction)?;
            let pending_bytes = self
                .retained_cached_bytes()?
                .saturating_sub(transaction.accounted_bytes)
                .checked_add(estimated_ready_bytes)
                .and_then(|bytes| bytes.checked_add(size_of::<(u64, ReadyTransaction)>()))
                .ok_or(BlobCacheError::ByteCountOverflow)?;
            if pending_bytes > self.cache.limits.max_pending_bytes {
                return Err(BlobCacheError::TooManyPendingBytes {
                    max: self.cache.limits.max_pending_bytes,
                });
            }
            let packet = reconstruct(&self.cache, transaction, &mut self.stats)?;
            let ready_bytes = ready_value_accounted_bytes(&packet)?;
            (packet, ready_bytes)
        };
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

    fn add_column_barriers(&mut self, sequence: u64, columns: &[ColumnKey]) {
        for column in columns {
            self.column_barriers
                .entry(*column)
                .or_default()
                .insert(sequence);
        }
    }

    fn remove_column_barriers(&mut self, sequence: u64, columns: &[ColumnKey]) {
        for column in columns {
            let remove_column = if let Some(sequences) = self.column_barriers.get_mut(column) {
                sequences.remove(&sequence);
                sequences.is_empty()
            } else {
                false
            };
            if remove_column {
                self.column_barriers.remove(column);
            }
        }
    }

    fn sequence_is_unblocked(&self, sequence: u64, columns: &[ColumnKey]) -> bool {
        columns.iter().all(|column| {
            self.column_barriers
                .get(column)
                .and_then(BTreeSet::first)
                .is_none_or(|barrier| *barrier >= sequence)
        })
    }

    fn take_ready_sequence(&mut self) -> Result<u64, BlobCacheError> {
        let sequence = self.next_ready_sequence;
        self.next_ready_sequence = sequence
            .checked_add(1)
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        Ok(sequence)
    }
}

impl Drop for BlobCacheResolver {
    fn drop(&mut self) {
        self.reset_pending();
    }
}

fn reconstruct(
    cache: &ClientBlobCache,
    transaction: &PendingTransaction,
    stats: &mut BlobCacheStats,
) -> Result<BlobCacheReady, BlobCacheError> {
    match &transaction.packet {
        PendingPacket::LevelChunk(packet) => {
            let mut packet = (**packet).clone();
            let payload_len =
                transaction
                    .hashes
                    .iter()
                    .try_fold(packet.payload.len(), |bytes, hash| {
                        let blob = cache
                            .get(*hash)
                            .ok_or(BlobCacheError::MissingResolvedBlob(*hash))?;
                        bytes
                            .checked_add(blob.len())
                            .ok_or(BlobCacheError::ByteCountOverflow)
                    })?;
            let mut payload = Vec::with_capacity(payload_len);
            for &hash in &transaction.hashes {
                let blob = cache
                    .get(hash)
                    .ok_or(BlobCacheError::MissingResolvedBlob(hash))?;
                payload.extend_from_slice(&blob);
            }
            payload.extend_from_slice(&packet.payload);
            packet.payload = payload;
            packet.blobs = None;
            stats.reconstructed_level_chunks = stats.reconstructed_level_chunks.saturating_add(1);
            Ok(BlobCacheReady::Packet(packet.into()))
        }
        PendingPacket::SubChunk(packet) => {
            let mut packet = (**packet).clone();
            let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = packet.entries else {
                return Err(BlobCacheError::NotCachedPacket);
            };
            let mut ordinary = Vec::with_capacity(entries.len());
            for entry in entries {
                let result = match entry.result {
                    SubChunkEntryWithCachingItemResult::Undefined => {
                        SubChunkEntryWithoutCachingItemResult::Undefined
                    }
                    SubChunkEntryWithCachingItemResult::Success => {
                        SubChunkEntryWithoutCachingItemResult::Success
                    }
                    SubChunkEntryWithCachingItemResult::ChunkNotFound => {
                        SubChunkEntryWithoutCachingItemResult::ChunkNotFound
                    }
                    SubChunkEntryWithCachingItemResult::InvalidDimension => {
                        SubChunkEntryWithoutCachingItemResult::InvalidDimension
                    }
                    SubChunkEntryWithCachingItemResult::PlayerNotFound => {
                        SubChunkEntryWithoutCachingItemResult::PlayerNotFound
                    }
                    SubChunkEntryWithCachingItemResult::YIndexOutOfBounds => {
                        SubChunkEntryWithoutCachingItemResult::YIndexOutOfBounds
                    }
                    SubChunkEntryWithCachingItemResult::SuccessAllAir => {
                        SubChunkEntryWithoutCachingItemResult::SuccessAllAir
                    }
                    SubChunkEntryWithCachingItemResult::Unknown(value) => {
                        SubChunkEntryWithoutCachingItemResult::Unknown(value)
                    }
                };
                let payload = if entry.result == SubChunkEntryWithCachingItemResult::Success {
                    let blob = cache
                        .get(entry.blob_id)
                        .ok_or(BlobCacheError::MissingResolvedBlob(entry.blob_id))?;
                    let tail = entry.payload.unwrap_or_default();
                    let payload_len = blob
                        .len()
                        .checked_add(tail.len())
                        .ok_or(BlobCacheError::ByteCountOverflow)?;
                    let mut payload = Vec::with_capacity(payload_len);
                    payload.extend_from_slice(&blob);
                    payload.extend_from_slice(&tail);
                    payload
                } else {
                    entry.payload.unwrap_or_default()
                };
                ordinary.push(SubChunkEntryWithoutCachingItem {
                    dx: entry.dx,
                    dy: entry.dy,
                    dz: entry.dz,
                    result,
                    payload,
                    heightmap_type: entry.heightmap_type,
                    heightmap: entry.heightmap,
                    render_heightmap_type: entry.render_heightmap_type,
                    render_heightmap: entry.render_heightmap,
                });
            }
            packet.entries = SubchunkPacketEntries::SubChunkEntryWithoutCaching(ordinary);
            stats.reconstructed_sub_chunks = stats.reconstructed_sub_chunks.saturating_add(1);
            Ok(BlobCacheReady::Packet(packet.into()))
        }
    }
}
