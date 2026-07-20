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

mod resolver;
pub use resolver::BlobCacheReady;

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
    retired_authorized_misses: Vec<(u64, usize)>,
    fast_transfer_rotation_armed: bool,
    stats: BlobCacheStats,
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

#[cfg(test)]
mod tests;
