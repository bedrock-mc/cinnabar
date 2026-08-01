use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::mem::size_of;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, MutexGuard};

use thiserror::Error;
use valentine::bedrock::version::v1_26_30::{
    ClientCacheBlobStatusPacket, ClientCacheMissResponsePacket, LevelChunkPacket, McpePacketData,
    SubChunkEntryWithCachingItemResult, SubChunkEntryWithoutCachingItem,
    SubChunkEntryWithoutCachingItemResult, SubchunkPacket, SubchunkPacketEntries,
};

use crate::{ChunkResyncEvent, Packet, WorldEvent};

#[cfg(test)]
static RECOVERY_ORDER_COMPARISONS: AtomicUsize = AtomicUsize::new(0);

mod resolver;
pub use resolver::{BlobCacheReady, BlobCacheStatus};

pub const CLIENT_BLOB_CACHE_TRIM_TRIGGER_BYTES: usize = 100 * 1024 * 1024;
pub const CLIENT_BLOB_CACHE_TRIM_FLOOR_BYTES: usize = 80 * 1024 * 1024;
/// Mojang's cache design limits each `ClientCacheBlobStatusPacket` to 4,095 IDs:
/// <https://gist.github.com/Tomcc/4be79d3eafcd158c5059abd4ab2e8d35>.
pub const MAX_CLIENT_BLOB_HASHES_PER_PACKET: usize = 4_095;
/// Cinnabar's own memory-safety bound, not a Bedrock protocol limit.
///
/// Bedrock 1.26.30 enforces transfer concurrency on the server at 20, 40, 100, or 200 according
/// to network status; its client has no corresponding cap. Keeping 256 retained transactions
/// accepts the largest observed server setting with headroom while bounding remotely controlled
/// resolver memory. Excess work is abandoned non-fatally and recovered through a chunk resync.
pub const MAX_CLIENT_BLOB_PENDING_TRANSACTIONS: usize = 256;
/// Cinnabar's maximum aggregate accounted bytes across retained cached transactions.
///
/// This is a Cinnabar memory-safety bound, not a vanilla or protocol limit. It independently
/// charges decoded packet containers and inline payload capacities retained while cache misses are
/// unresolved, even when reconstruction size is unknown and no cached payload is staged.
pub const MAX_CLIENT_BLOB_PENDING_BYTES: usize = 64 * 1024 * 1024;
/// Ordinary decoded work is retained independently from cache transactions. The session receive
/// loop stops reading as soon as any ordinary work is blocked, while this larger defensive ceiling
/// keeps direct resolver users bounded too.
pub const MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS: usize = 64;
/// The transport retains at most 16 MiB of deferred raw packet data. Two frames of headroom cover
/// decoded container allocation overhead without coupling ordinary traffic to cache limits.
pub const MAX_CLIENT_BLOB_ORDINARY_READY_BYTES: usize = 32 * 1024 * 1024;
/// Cinnabar's maximum aggregate payload bytes allocated while reconstructing one cached packet.
///
/// This is a Cinnabar memory-safety bound, not a vanilla or protocol limit. It matches the
/// independently bounded 32 MiB ordinary-ready byte lane so one remotely controlled cached packet
/// cannot allocate more payload memory than that entire lane. Every blob reference contributes its
/// payload length, including duplicate references, because reconstruction copies each occurrence.
pub const MAX_CLIENT_BLOB_RECONSTRUCTED_BYTES: usize = 32 * 1024 * 1024;
/// Cinnabar's maximum pinned cache payload retained for one unresolved transaction.
///
/// This is a Cinnabar memory-safety bound, not a vanilla or protocol limit. Unique cached blob
/// payloads are charged on initial classification and as solicited misses arrive.
pub const MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION: usize = 32 * 1024 * 1024;
/// Cinnabar's maximum aggregate accounted bytes across retained reconstructed outputs.
///
/// This is a Cinnabar memory-safety bound, not a vanilla or protocol limit. Accounted bytes include
/// reconstructed payload capacities and their decoded packet containers.
pub const MAX_CLIENT_BLOB_READY_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobCacheLimits {
    pub trim_trigger_bytes: usize,
    pub trim_floor_bytes: usize,
}

impl Default for BlobCacheLimits {
    fn default() -> Self {
        Self {
            trim_trigger_bytes: CLIENT_BLOB_CACHE_TRIM_TRIGGER_BYTES,
            trim_floor_bytes: CLIENT_BLOB_CACHE_TRIM_FLOOR_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlobCacheStats {
    pub hashes_classified: u64,
    pub hits: u64,
    pub misses: u64,
    /// Absent hash references suppressed because another pending transaction already owns them.
    pub redundant_missing_requests: u64,
    pub admitted_blobs: u64,
    pub rejected_blobs: u64,
    pub evictions: u64,
    pub pending_transactions: usize,
    pub pending_bytes: usize,
    pub retained_cached_transactions: usize,
    pub ordinary_ready_events: usize,
    pub ordinary_ready_bytes: usize,
    pub recovery_ready_events: usize,
    pub recovery_ready_bytes: usize,
    pub pending_resets: u64,
    pub skipped_packets: u64,
    pub skipped_world_events: u64,
    pub skipped_cached_packets: u64,
    pub skipped_miss_responses: u64,
    pub empty_miss_responses: u64,
    pub cached_packet_semantic_shape: u64,
    pub cached_packet_transaction_pressure: u64,
    pub cached_packet_pending_pressure: u64,
    pub cached_packet_reconstruction_pressure: u64,
    pub cached_packet_staged_pressure: u64,
    pub cached_packet_ready_pressure: u64,
    pub miss_response_unsolicited: u64,
    pub miss_response_integrity_rejection: u64,
    pub miss_response_cache_pressure: u64,
    pub abandoned_cached_transactions: u64,
    /// Distinct recovery events queued after coalescing.
    ///
    /// Two abandoned transactions recovering the same column produce one
    /// `recovery_ready` entry and increment this once, so it tracks live
    /// recovery traffic rather than pre-coalescing demand. A recovery returned
    /// inline on a status packet is counted when it is queued.
    pub recovery_requests: u64,
    pub ordinary_backpressure: u64,
    pub reconstructed_level_chunks: u64,
    pub reconstructed_sub_chunks: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BlobCacheError {
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
    #[error(
        "ordinary resolver lane is full at {events} events / {bytes} bytes (maximum {max_events} events / {max_bytes} bytes)"
    )]
    OrdinaryLaneFull {
        events: usize,
        bytes: usize,
        max_events: usize,
        max_bytes: usize,
    },
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

    fn classify(&self, hashes: &[u64], pin: bool) -> (Vec<u64>, Vec<u64>, usize) {
        let mut store = self.lock();
        let mut have = Vec::new();
        let mut missing = Vec::new();
        let mut staged_bytes = 0usize;
        for &hash in hashes {
            if let Some(payload_len) = store.entries.get(&hash).map(|entry| entry.payload.len()) {
                store.clock = store.clock.saturating_add(1);
                let clock = store.clock;
                if let Some(entry) = store.entries.get_mut(&hash) {
                    entry.last_used = clock;
                }
                have.push(hash);
                staged_bytes = staged_bytes.saturating_add(payload_len);
            } else {
                missing.push(hash);
            }
            if pin {
                *store.pins.entry(hash).or_default() += 1;
            }
        }
        (have, missing, staged_bytes)
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
}

#[derive(Debug)]
struct PendingTransaction {
    packet: PendingPacket,
    hashes: Vec<u64>,
    unique_hashes: Vec<u64>,
    owned_hashes: Vec<u64>,
    unresolved_hashes: usize,
    staged_bytes: usize,
    columns: Vec<ColumnKey>,
    accounted_bytes: usize,
}

#[derive(Debug)]
struct ReadyTransaction {
    value: BlobCacheReady,
    columns: Vec<ColumnKey>,
    accounted_bytes: usize,
    sequence: u64,
}

#[derive(Debug)]
struct ImmediateReady {
    value: BlobCacheReady,
    columns: Vec<ColumnKey>,
    accounted_bytes: usize,
    sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ColumnKey {
    dimension: i32,
    x: i32,
    z: i32,
}

impl Ord for ColumnKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        #[cfg(test)]
        RECOVERY_ORDER_COMPARISONS.fetch_add(1, AtomicOrdering::Relaxed);
        self.dimension
            .cmp(&other.dimension)
            .then_with(|| self.x.cmp(&other.x))
            .then_with(|| self.z.cmp(&other.z))
    }
}

impl PartialOrd for ColumnKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug)]
struct RecoveryReady {
    dimension: i32,
    x: i32,
    z: i32,
    requested_sub_chunks: Option<usize>,
    requested_sub_chunk_ys: Option<BTreeSet<i32>>,
}

impl RecoveryReady {
    fn from_event(event: ChunkResyncEvent) -> (ColumnKey, Self) {
        let key = ColumnKey {
            dimension: event.dimension,
            x: event.x,
            z: event.z,
        };
        let ys = event
            .requested_sub_chunk_ys
            .map(|ys| ys.into_iter().collect());
        (
            key,
            Self {
                dimension: event.dimension,
                x: event.x,
                z: event.z,
                requested_sub_chunks: event.requested_sub_chunks,
                requested_sub_chunk_ys: ys,
            },
        )
    }

    fn into_event(self) -> ChunkResyncEvent {
        ChunkResyncEvent {
            dimension: self.dimension,
            x: self.x,
            z: self.z,
            requested_sub_chunks: self.requested_sub_chunks,
            requested_sub_chunk_ys: self
                .requested_sub_chunk_ys
                .map(|ys| ys.into_iter().collect()),
        }
    }
}

#[derive(Debug)]
pub struct BlobCacheResolver {
    cache: ClientBlobCache,
    pending: HashMap<u64, PendingTransaction>,
    pending_order: BTreeSet<u64>,
    pending_by_hash: HashMap<u64, HashSet<u64>>,
    resolved_pending: BTreeSet<u64>,
    ready: BTreeMap<u64, ReadyTransaction>,
    immediate_ready: BTreeMap<u64, ImmediateReady>,
    recovery_ready: BTreeMap<ColumnKey, RecoveryReady>,
    fast_transfer_reset_armed: bool,
    next_ready_sequence: u64,
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
    if let Some(existing) = store.entries.get(&hash) {
        if existing.payload.as_ref() != payload {
            return Err(BlobCacheError::ConflictingDuplicate(hash));
        }
        return Ok(());
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
    trim_if_needed(store, limits, hash);
    Ok(())
}

fn trim_if_needed(store: &mut CacheStore, limits: BlobCacheLimits, inserted_hash: u64) {
    if store.total_bytes <= limits.trim_trigger_bytes {
        return;
    }
    let floor = limits.trim_floor_bytes.min(limits.trim_trigger_bytes);
    while store.total_bytes > floor {
        let Some((&evict, _)) = store
            .entries
            .iter()
            .filter(|(candidate, _)| {
                **candidate != inserted_hash && !store.pins.contains_key(candidate)
            })
            .min_by_key(|(candidate, entry)| (entry.last_used, **candidate))
        else {
            break;
        };
        let removed = store.entries.remove(&evict).expect("selected cache entry");
        store.total_bytes = store.total_bytes.saturating_sub(removed.payload.len());
    }
}

#[cfg(test)]
mod tests;
