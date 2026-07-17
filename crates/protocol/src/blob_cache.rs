use std::collections::{HashMap, HashSet, VecDeque};
use std::mem::size_of;
use std::sync::{Arc, Mutex, MutexGuard};

use thiserror::Error;
use valentine::bedrock::version::v1_26_30::{
    ClientCacheBlobStatusPacket, ClientCacheMissResponsePacket, LevelChunkPacket, McpePacketData,
    SubChunkEntryWithCachingItemResult, SubChunkEntryWithoutCachingItem,
    SubChunkEntryWithoutCachingItemResult, SubchunkPacket, SubchunkPacketEntries,
};

use crate::{Packet, WorldEvent};

pub const MAX_CLIENT_BLOB_CACHE_ENTRIES: usize = 4_096;
pub const MAX_CLIENT_BLOB_CACHE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_CLIENT_BLOB_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CLIENT_BLOB_HASHES_PER_PACKET: usize = 4_096;
/// A 256-transaction burst covers Lunar's observed 177 request-mode columns while the independent
/// 64 MiB byte ceiling keeps retained packet state bounded to a 256 KiB average at the count cap.
pub const MAX_CLIENT_BLOB_PENDING_TRANSACTIONS: usize = 256;
pub const MAX_CLIENT_BLOB_PENDING_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobCacheLimits {
    pub max_entries: usize,
    pub max_total_bytes: usize,
    pub max_blob_bytes: usize,
    pub max_hashes_per_packet: usize,
    pub max_pending_transactions: usize,
    pub max_pending_bytes: usize,
}

impl Default for BlobCacheLimits {
    fn default() -> Self {
        Self {
            max_entries: MAX_CLIENT_BLOB_CACHE_ENTRIES,
            max_total_bytes: MAX_CLIENT_BLOB_CACHE_BYTES,
            max_blob_bytes: MAX_CLIENT_BLOB_BYTES,
            max_hashes_per_packet: MAX_CLIENT_BLOB_HASHES_PER_PACKET,
            max_pending_transactions: MAX_CLIENT_BLOB_PENDING_TRANSACTIONS,
            max_pending_bytes: MAX_CLIENT_BLOB_PENDING_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlobCacheStats {
    pub hashes_classified: u64,
    pub hits: u64,
    pub misses: u64,
    pub admitted_blobs: u64,
    pub rejected_blobs: u64,
    pub evictions: u64,
    pub pending_transactions: usize,
    pub pending_bytes: usize,
    pub pending_resets: u64,
    pub reconstructed_level_chunks: u64,
    pub reconstructed_sub_chunks: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BlobCacheError {
    #[error("packet contains {count} blob hashes, maximum is {max}")]
    TooManyHashes { count: usize, max: usize },
    #[error("blob contains {bytes} bytes, maximum is {max}")]
    BlobTooLarge { bytes: usize, max: usize },
    #[error("blob cache cannot admit {bytes} bytes within {entries} entries")]
    CacheCapacity { bytes: usize, entries: usize },
    #[error("pending blob-cache transaction count exceeds {max}")]
    TooManyPendingTransactions { max: usize },
    #[error("pending blob-cache bytes would exceed {max}")]
    TooManyPendingBytes { max: usize },
    #[error("cached LevelChunk hash count {actual} does not match expected {expected}")]
    InvalidLevelChunkHashCount { actual: usize, expected: usize },
    #[error("cached LevelChunk has invalid sub-chunk count {0}")]
    InvalidLevelChunkCount(i32),
    #[error("packet is not a cached LevelChunk or SubChunk")]
    NotCachedPacket,
    #[error("cache miss response contains no requested blobs")]
    EmptyMissResponse,
    #[error("cache miss response contains unsolicited hash {0:#018x}")]
    UnsolicitedBlob(u64),
    #[error("cache miss response hash {claimed:#018x} disagrees with payload hash {actual:#018x}")]
    HashMismatch { claimed: u64, actual: u64 },
    #[error("cache miss response contains conflicting payloads for hash {0:#018x}")]
    ConflictingDuplicate(u64),
    #[error("cached transaction references a missing blob after resolution: {0:#018x}")]
    MissingResolvedBlob(u64),
    #[error("cached payload byte accounting overflowed")]
    ByteCountOverflow,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    payload: Arc<[u8]>,
    last_used: u64,
}

#[derive(Debug, Clone, Default)]
struct CacheStore {
    entries: HashMap<u64, CacheEntry>,
    pins: HashMap<u64, usize>,
    total_bytes: usize,
    clock: u64,
}

#[derive(Debug, Clone)]
pub struct ClientBlobCache {
    limits: BlobCacheLimits,
    store: Arc<Mutex<CacheStore>>,
}

impl Default for ClientBlobCache {
    fn default() -> Self {
        Self::with_limits(BlobCacheLimits::default())
    }
}

impl ClientBlobCache {
    #[must_use]
    pub fn with_limits(limits: BlobCacheLimits) -> Self {
        Self {
            limits,
            store: Arc::new(Mutex::new(CacheStore::default())),
        }
    }

    #[must_use]
    pub const fn limits(&self) -> BlobCacheLimits {
        self.limits
    }

    pub fn insert(&self, payload: &[u8]) -> Result<u64, BlobCacheError> {
        let hash = client_blob_hash(payload);
        let mut store = self.lock();
        let mut candidate = store.clone();
        insert_verified(&mut candidate, self.limits, hash, payload)?;
        *store = candidate;
        Ok(hash)
    }

    #[must_use]
    pub fn contains(&self, hash: u64) -> bool {
        self.lock().entries.contains_key(&hash)
    }

    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.lock().entries.len()
    }

    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.lock().total_bytes
    }

    fn lock(&self) -> MutexGuard<'_, CacheStore> {
        self.store.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn get(&self, hash: u64) -> Option<Arc<[u8]>> {
        let mut store = self.lock();
        store.clock = store.clock.saturating_add(1);
        let clock = store.clock;
        let entry = store.entries.get_mut(&hash)?;
        entry.last_used = clock;
        Some(entry.payload.clone())
    }

    fn classify_and_pin(&self, hashes: &[u64]) -> (Vec<u64>, Vec<u64>) {
        let mut store = self.lock();
        let mut have = Vec::new();
        let mut missing = Vec::new();
        for &hash in hashes {
            if store.entries.contains_key(&hash) {
                store.clock = store.clock.saturating_add(1);
                let clock = store.clock;
                if let Some(entry) = store.entries.get_mut(&hash) {
                    entry.last_used = clock;
                }
                have.push(hash);
            } else {
                missing.push(hash);
            }
            *store.pins.entry(hash).or_default() += 1;
        }
        (have, missing)
    }

    fn unpin_all(&self, hashes: &[u64]) {
        let mut store = self.lock();
        for &hash in hashes {
            let remove = if let Some(count) = store.pins.get_mut(&hash) {
                *count = count.saturating_sub(1);
                *count == 0
            } else {
                false
            };
            if remove {
                store.pins.remove(&hash);
            }
        }
    }
}

#[must_use]
pub fn client_blob_hash(payload: &[u8]) -> u64 {
    xxhash_rust::xxh64::xxh64(payload, 0)
}

#[derive(Debug)]
enum PendingPacket {
    LevelChunk(Box<LevelChunkPacket>),
    SubChunk(Box<SubchunkPacket>),
    Ordinary(Packet),
    WorldEvent(WorldEvent),
}

#[derive(Debug)]
struct PendingTransaction {
    packet: PendingPacket,
    hashes: Vec<u64>,
    unique_hashes: Vec<u64>,
    accounted_bytes: usize,
}

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

#[derive(Debug)]
struct ReadyTransaction {
    value: BlobCacheReady,
    accounted_bytes: usize,
}

#[derive(Debug)]
pub struct BlobCacheResolver {
    cache: ClientBlobCache,
    pending: VecDeque<PendingTransaction>,
    ready: VecDeque<ReadyTransaction>,
    authorized_misses: Vec<(u64, usize)>,
    stats: BlobCacheStats,
}

impl BlobCacheResolver {
    #[must_use]
    pub fn new(cache: ClientBlobCache) -> Self {
        Self {
            cache,
            pending: VecDeque::new(),
            ready: VecDeque::new(),
            authorized_misses: Vec::new(),
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

    fn retained_pending_bytes(&self) -> Result<usize, BlobCacheError> {
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
            if let Some(index) = self
                .authorized_misses
                .iter()
                .position(|(candidate, _)| candidate == hash)
            {
                self.authorized_misses[index].1 = self.authorized_misses[index].1.saturating_sub(1);
                if self.authorized_misses[index].1 == 0 {
                    self.authorized_misses.remove(index);
                }
            }
        }
        if self.authorized_misses.is_empty() {
            self.authorized_misses = Vec::new();
        } else {
            self.authorized_misses.shrink_to_fit();
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
        if !self.pending.is_empty() || !self.ready.is_empty() || !self.authorized_misses.is_empty()
        {
            self.stats.pending_resets = self.stats.pending_resets.saturating_add(1);
        }
        for transaction in self.pending.drain(..) {
            self.cache.unpin_all(&transaction.unique_hashes);
        }
        self.pending = VecDeque::new();
        self.ready = VecDeque::new();
        self.authorized_misses = Vec::new();
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

fn ready_value_accounted_bytes(value: &BlobCacheReady) -> Result<usize, BlobCacheError> {
    match value {
        BlobCacheReady::Packet(Packet {
            data: McpePacketData::PacketLevelChunk(packet),
            ..
        }) => {
            let hash_bytes = packet.blobs.as_ref().map_or(Ok(0), |blobs| {
                blobs
                    .hashes
                    .capacity()
                    .checked_mul(size_of::<u64>())
                    .ok_or(BlobCacheError::ByteCountOverflow)
            })?;
            size_of::<LevelChunkPacket>()
                .checked_add(packet.payload.capacity())
                .and_then(|bytes| bytes.checked_add(hash_bytes))
                .ok_or(BlobCacheError::ByteCountOverflow)
        }
        BlobCacheReady::Packet(Packet {
            data: McpePacketData::PacketSubchunk(packet),
            ..
        }) => {
            let SubchunkPacketEntries::SubChunkEntryWithoutCaching(entries) = &packet.entries
            else {
                return Err(BlobCacheError::NotCachedPacket);
            };
            entries
                .capacity()
                .checked_mul(size_of::<SubChunkEntryWithoutCachingItem>())
                .and_then(|bytes| bytes.checked_add(size_of::<SubchunkPacket>()))
                .and_then(|bytes| {
                    entries.iter().try_fold(bytes, |total, entry| {
                        total.checked_add(entry.payload.capacity())
                    })
                })
                .ok_or(BlobCacheError::ByteCountOverflow)
        }
        BlobCacheReady::Packet(_) | BlobCacheReady::WorldEvent(_) => {
            Err(BlobCacheError::NotCachedPacket)
        }
    }
}

fn stable_unique(hashes: &[u64]) -> Vec<u64> {
    let mut seen = HashSet::with_capacity(hashes.len());
    hashes
        .iter()
        .copied()
        .filter(|hash| seen.insert(*hash))
        .collect()
}

fn insert_verified(
    store: &mut CacheStore,
    limits: BlobCacheLimits,
    hash: u64,
    payload: &[u8],
) -> Result<(), BlobCacheError> {
    if payload.len() > limits.max_blob_bytes {
        return Err(BlobCacheError::BlobTooLarge {
            bytes: payload.len(),
            max: limits.max_blob_bytes,
        });
    }
    if let Some(existing) = store.entries.get(&hash) {
        if existing.payload.as_ref() != payload {
            return Err(BlobCacheError::ConflictingDuplicate(hash));
        }
        return Ok(());
    }
    let target_bytes = store
        .total_bytes
        .checked_add(payload.len())
        .ok_or(BlobCacheError::ByteCountOverflow)?;
    while store.entries.len() >= limits.max_entries || target_bytes > limits.max_total_bytes {
        let Some((&evict, _)) = store
            .entries
            .iter()
            .filter(|(candidate, _)| !store.pins.contains_key(candidate))
            .min_by_key(|(candidate, entry)| (entry.last_used, **candidate))
        else {
            return Err(BlobCacheError::CacheCapacity {
                bytes: payload.len(),
                entries: 1,
            });
        };
        let removed = store.entries.remove(&evict).expect("selected cache entry");
        store.total_bytes = store.total_bytes.saturating_sub(removed.payload.len());
        if store
            .total_bytes
            .checked_add(payload.len())
            .is_some_and(|bytes| bytes <= limits.max_total_bytes)
            && store.entries.len() < limits.max_entries
        {
            break;
        }
    }
    store.clock = store.clock.saturating_add(1);
    store.total_bytes = store
        .total_bytes
        .checked_add(payload.len())
        .ok_or(BlobCacheError::ByteCountOverflow)?;
    store.entries.insert(
        hash,
        CacheEntry {
            payload: Arc::from(payload),
            last_used: store.clock,
        },
    );
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_subchunk_accounting_uses_retained_entry_and_payload_capacities() {
        let mut payload = Vec::with_capacity(4_096);
        payload.push(0x5a);
        let mut entries = Vec::with_capacity(16);
        entries.push(SubChunkEntryWithoutCachingItem {
            payload,
            ..Default::default()
        });
        let expected = size_of::<SubchunkPacket>()
            + entries.capacity() * size_of::<SubChunkEntryWithoutCachingItem>()
            + entries[0].payload.capacity();
        let value = BlobCacheReady::Packet(
            SubchunkPacket {
                entries: SubchunkPacketEntries::SubChunkEntryWithoutCaching(entries),
                ..Default::default()
            }
            .into(),
        );

        assert_eq!(ready_value_accounted_bytes(&value), Ok(expected));
    }

    #[test]
    fn pending_queue_high_water_is_exact_and_reset_releases_backing_allocations() {
        let cache = ClientBlobCache::default();
        let mut resolver = BlobCacheResolver::new(cache);
        for x in 0..8 {
            let payload = [x as u8];
            let hash = client_blob_hash(&payload);
            resolver
                .accept_cached_packet(
                    LevelChunkPacket {
                        x,
                        sub_chunk_count: -1,
                        blobs: Some(
                            valentine::bedrock::version::v1_26_30::LevelChunkPacketBlobs {
                                hashes: vec![hash],
                            },
                        ),
                        ..Default::default()
                    }
                    .into(),
                )
                .expect("grow unresolved pending queue");
        }
        assert!(resolver.pending.capacity() >= 8);
        assert_eq!(
            resolver.stats.pending_bytes,
            resolver
                .retained_pending_bytes()
                .expect("exact retained bytes")
        );

        resolver.reset_pending();

        assert_eq!(resolver.pending.capacity(), 0);
        assert_eq!(resolver.ready.capacity(), 0);
        assert_eq!(resolver.authorized_misses.capacity(), 0);
        assert_eq!(resolver.stats.pending_bytes, 0);
    }

    #[test]
    fn classify_and_pin_is_one_cache_operation() {
        let limits = BlobCacheLimits {
            max_entries: 1,
            max_total_bytes: 8,
            max_blob_bytes: 8,
            max_hashes_per_packet: 4,
            max_pending_transactions: 2,
            max_pending_bytes: 32,
        };
        let cache = ClientBlobCache::with_limits(limits);
        let hit = cache.insert(b"hit").expect("seed hit");
        let miss = client_blob_hash(b"miss");

        let (have, missing) = cache.classify_and_pin(&[hit, miss]);

        assert_eq!(have, vec![hit]);
        assert_eq!(missing, vec![miss]);
        assert!(
            cache.insert(b"new").is_err(),
            "reported hit is already pinned"
        );
        cache.unpin_all(&[hit, miss]);
    }
}
