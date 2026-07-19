use super::*;

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
            pending: VecDeque::new(),
            ready: VecDeque::new(),
            authorized_misses: Vec::new(),
            retired_authorized_misses: Vec::new(),
            fast_transfer_rotation_armed: false,
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

        let mut retained = VecDeque::with_capacity(self.pending.len());
        self.retired_authorized_misses
            .try_reserve(self.authorized_misses.len())
            .map_err(|_| BlobCacheError::ByteCountOverflow)?;
        let mut retired = std::mem::take(&mut self.retired_authorized_misses);
        let mut removed = false;
        while let Some(transaction) = self.pending.pop_front() {
            let cached = matches!(
                transaction.packet,
                PendingPacket::LevelChunk(_) | PendingPacket::SubChunk(_)
            );
            let unresolved = cached
                && transaction
                    .unique_hashes
                    .iter()
                    .any(|hash| !self.cache.contains(*hash));
            if !unresolved {
                retained.push_back(transaction);
                continue;
            }

            removed = true;
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
        }
        self.pending = retained;
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
        self.drain_ready()?;
        Ok(removed)
    }

    pub(super) fn retained_pending_bytes(&self) -> Result<usize, BlobCacheError> {
        self.pending
            .capacity()
            .checked_mul(size_of::<PendingTransaction>())
            .and_then(|bytes| {
                self.ready
                    .capacity()
                    .checked_mul(size_of::<ReadyTransaction>())
                    .and_then(|ready| bytes.checked_add(ready))
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
                    total.checked_add(transaction.accounted_bytes)
                })
            })
            .and_then(|bytes| {
                self.ready.iter().try_fold(bytes, |total, transaction| {
                    total.checked_add(transaction.accounted_bytes)
                })
            })
            .ok_or(BlobCacheError::ByteCountOverflow)
    }

    fn refresh_pending_accounting(&mut self) -> Result<(), BlobCacheError> {
        let pending_bytes = self.retained_pending_bytes()?;
        self.stats.pending_bytes = pending_bytes;
        self.stats.pending_transactions = self.pending.len().saturating_add(self.ready.len());
        if pending_bytes > self.cache.limits.max_pending_bytes {
            return Err(BlobCacheError::TooManyPendingBytes {
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
        match self.accept_cached_packet_inner(packet, raw_packet_bytes) {
            Ok(status) => Ok(status),
            Err(error) => {
                self.reset_pending();
                Err(error)
            }
        }
    }

    /// Queues an ordinary packet behind unresolved cache transactions without overtaking them.
    pub fn accept_passthrough(
        &mut self,
        packet: Packet,
        accounted_bytes: usize,
    ) -> Result<(), BlobCacheError> {
        if self.pending.len().saturating_add(self.ready.len())
            >= self.cache.limits.max_pending_transactions
        {
            self.reset_pending();
            return Err(BlobCacheError::TooManyPendingTransactions {
                max: self.cache.limits.max_pending_transactions,
            });
        }
        if self
            .stats
            .pending_bytes
            .checked_add(accounted_bytes)
            .ok_or(BlobCacheError::ByteCountOverflow)?
            > self.cache.limits.max_pending_bytes
        {
            self.reset_pending();
            return Err(BlobCacheError::TooManyPendingBytes {
                max: self.cache.limits.max_pending_bytes,
            });
        }
        self.pending.push_back(PendingTransaction {
            packet: PendingPacket::Ordinary(packet),
            hashes: Vec::new(),
            unique_hashes: Vec::new(),
            accounted_bytes,
        });
        if let Err(error) = self
            .refresh_pending_accounting()
            .and_then(|()| self.drain_ready())
        {
            self.reset_pending();
            return Err(error);
        }
        Ok(())
    }

    /// Queues an already-normalized ordinary event behind earlier cache transactions.
    pub fn accept_world_event(
        &mut self,
        event: WorldEvent,
        accounted_bytes: usize,
    ) -> Result<(), BlobCacheError> {
        if self.pending.len().saturating_add(self.ready.len())
            >= self.cache.limits.max_pending_transactions
        {
            self.reset_pending();
            return Err(BlobCacheError::TooManyPendingTransactions {
                max: self.cache.limits.max_pending_transactions,
            });
        }
        if self
            .stats
            .pending_bytes
            .checked_add(accounted_bytes)
            .ok_or(BlobCacheError::ByteCountOverflow)?
            > self.cache.limits.max_pending_bytes
        {
            self.reset_pending();
            return Err(BlobCacheError::TooManyPendingBytes {
                max: self.cache.limits.max_pending_bytes,
            });
        }
        self.pending.push_back(PendingTransaction {
            packet: PendingPacket::WorldEvent(event),
            hashes: Vec::new(),
            unique_hashes: Vec::new(),
            accounted_bytes,
        });
        if let Err(error) = self
            .refresh_pending_accounting()
            .and_then(|()| self.drain_ready())
        {
            self.reset_pending();
            return Err(error);
        }
        Ok(())
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
            .stats
            .pending_bytes
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
            .stats
            .pending_bytes
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
        self.pending.push_back(PendingTransaction {
            packet,
            hashes,
            unique_hashes,
            accounted_bytes,
        });
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
        self.drain_ready()?;
        Ok(ClientCacheBlobStatusPacket { missing, have })
    }

    pub fn accept_miss_response(
        &mut self,
        response: ClientCacheMissResponsePacket,
    ) -> Result<(), BlobCacheError> {
        let rejected = u64::try_from(response.blobs.len().max(1)).unwrap_or(u64::MAX);
        match self.accept_miss_response_inner(response) {
            Ok(()) => Ok(()),
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
        if response.blobs.is_empty() {
            return Err(BlobCacheError::EmptyMissResponse);
        }
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
        self.drain_ready()
    }

    pub fn reset_pending(&mut self) {
        if !self.pending.is_empty()
            || !self.ready.is_empty()
            || !self.authorized_misses.is_empty()
            || !self.retired_authorized_misses.is_empty()
        {
            self.stats.pending_resets = self.stats.pending_resets.saturating_add(1);
        }
        for transaction in self.pending.drain(..) {
            self.cache.unpin_all(&transaction.unique_hashes);
        }
        self.pending = VecDeque::new();
        self.ready = VecDeque::new();
        self.authorized_misses = Vec::new();
        self.retired_authorized_misses = Vec::new();
        self.fast_transfer_rotation_armed = false;
        self.stats.pending_transactions = 0;
        self.stats.pending_bytes = 0;
    }

    pub fn pop_ready(&mut self) -> Option<BlobCacheReady> {
        let ready = self.ready.pop_front()?;
        if self.ready.is_empty() {
            self.ready = VecDeque::new();
        }
        self.refresh_pending_accounting()
            .expect("retained ready accounting cannot overflow after a pop");
        Some(ready.value)
    }

    fn drain_ready(&mut self) -> Result<(), BlobCacheError> {
        while self.pending.front().is_some_and(|transaction| {
            transaction
                .unique_hashes
                .iter()
                .all(|hash| self.cache.contains(*hash))
        }) {
            let packet = {
                let transaction = self.pending.front().expect("front was present");
                let estimated_ready_bytes =
                    reconstructed_accounted_bytes(&self.cache, transaction)?;
                let pending_bytes = self
                    .stats
                    .pending_bytes
                    .saturating_sub(transaction.accounted_bytes)
                    .checked_add(estimated_ready_bytes)
                    .ok_or(BlobCacheError::ByteCountOverflow)?;
                if pending_bytes > self.cache.limits.max_pending_bytes {
                    return Err(BlobCacheError::TooManyPendingBytes {
                        max: self.cache.limits.max_pending_bytes,
                    });
                }
                reconstruct(&self.cache, transaction, &mut self.stats)?
            };
            let ready_bytes = match &self.pending.front().expect("front was present").packet {
                PendingPacket::Ordinary(_) | PendingPacket::WorldEvent(_) => {
                    self.pending
                        .front()
                        .expect("front was present")
                        .accounted_bytes
                }
                PendingPacket::LevelChunk(_) | PendingPacket::SubChunk(_) => {
                    ready_value_accounted_bytes(&packet)?
                }
            };
            let transaction = self.pending.pop_front().expect("front was present");
            self.cache.unpin_all(&transaction.unique_hashes);
            if self.pending.is_empty() {
                self.pending = VecDeque::new();
            }
            self.ready.push_back(ReadyTransaction {
                value: packet,
                accounted_bytes: ready_bytes,
            });
            self.refresh_pending_accounting()?;
        }
        self.refresh_pending_accounting()?;
        Ok(())
    }
}

impl Drop for BlobCacheResolver {
    fn drop(&mut self) {
        self.reset_pending();
    }
}

fn reconstructed_accounted_bytes(
    cache: &ClientBlobCache,
    transaction: &PendingTransaction,
) -> Result<usize, BlobCacheError> {
    match &transaction.packet {
        PendingPacket::LevelChunk(packet) => {
            let base = size_of::<LevelChunkPacket>()
                .checked_add(packet.payload.len())
                .ok_or(BlobCacheError::ByteCountOverflow)?;
            transaction.hashes.iter().try_fold(base, |bytes, hash| {
                let blob = cache
                    .get(*hash)
                    .ok_or(BlobCacheError::MissingResolvedBlob(*hash))?;
                bytes
                    .checked_add(blob.len())
                    .ok_or(BlobCacheError::ByteCountOverflow)
            })
        }
        PendingPacket::SubChunk(packet) => {
            let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = &packet.entries else {
                return Err(BlobCacheError::NotCachedPacket);
            };
            let base = entries
                .len()
                .checked_mul(size_of::<SubChunkEntryWithoutCachingItem>())
                .and_then(|bytes| bytes.checked_add(size_of::<SubchunkPacket>()))
                .ok_or(BlobCacheError::ByteCountOverflow)?;
            entries.iter().try_fold(base, |bytes, entry| {
                let bytes = bytes
                    .checked_add(entry.payload.as_ref().map_or(0, Vec::len))
                    .ok_or(BlobCacheError::ByteCountOverflow)?;
                if entry.result == SubChunkEntryWithCachingItemResult::Success {
                    let blob = cache
                        .get(entry.blob_id)
                        .ok_or(BlobCacheError::MissingResolvedBlob(entry.blob_id))?;
                    bytes
                        .checked_add(blob.len())
                        .ok_or(BlobCacheError::ByteCountOverflow)
                } else {
                    Ok(bytes)
                }
            })
        }
        PendingPacket::Ordinary(_) | PendingPacket::WorldEvent(_) => {
            Ok(transaction.accounted_bytes)
        }
    }
}

fn decrement_authorization(authorizations: &mut Vec<(u64, usize)>, hash: u64) -> bool {
    let Some(index) = authorizations
        .iter()
        .position(|(candidate, count)| *candidate == hash && *count > 0)
    else {
        return false;
    };
    authorizations[index].1 -= 1;
    if authorizations[index].1 == 0 {
        authorizations.remove(index);
    }
    true
}

fn increment_authorization(
    authorizations: &mut Vec<(u64, usize)>,
    hash: u64,
) -> Result<(), BlobCacheError> {
    if let Some((_, count)) = authorizations
        .iter_mut()
        .find(|(candidate, _)| *candidate == hash)
    {
        *count = count
            .checked_add(1)
            .ok_or(BlobCacheError::ByteCountOverflow)?;
    } else {
        authorizations.push((hash, 1));
    }
    Ok(())
}

fn stable_unique(hashes: &[u64]) -> Vec<u64> {
    let mut seen = HashSet::with_capacity(hashes.len());
    hashes
        .iter()
        .copied()
        .filter(|hash| seen.insert(*hash))
        .collect()
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
        PendingPacket::Ordinary(packet) => Ok(BlobCacheReady::Packet(packet.clone())),
        PendingPacket::WorldEvent(event) => Ok(BlobCacheReady::WorldEvent(event.clone())),
    }
}
