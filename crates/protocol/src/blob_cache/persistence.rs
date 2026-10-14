use super::resolver::ReferencedBlobHashes;
use super::*;
use std::{future::Future, pin::Pin, sync::Weak};

/// Edge-owned persistence. Synchronous methods only enqueue bounded work and must never do I/O
/// or wait for a worker. Prefetch must finish within a short edge-owned deadline, even on failure.
pub trait BlobCacheStore: std::fmt::Debug + Send + Sync {
    /// Queues a verified shared body; saturation may discard this optional disk write.
    fn persist(&self, hash: u64, payload: Arc<[u8]>);
    /// Records access order in memory for a later batch flush.
    fn touch(&self, hashes: &[u64]);
    /// Requests background reads; completion never promises that a body was admitted to memory.
    fn prefetch(&self, hashes: Vec<u64>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// A worker's non-owning handle to the bounded memory tier; it cannot keep the client alive.
#[derive(Debug, Clone)]
pub struct WeakClientBlobCache {
    limits: BlobCacheLimits,
    store: Weak<Mutex<CacheStore>>,
}

impl ClientBlobCache {
    /// Attaches an optional edge-owned store; the default cache remains memory-only.
    #[must_use]
    pub fn with_store(mut self, persistence: Arc<dyn BlobCacheStore>) -> Self {
        self.persistence = Some(persistence);
        self
    }

    /// Returns a worker handle that verifies disk bodies before admitting them.
    #[must_use]
    pub fn downgrade(&self) -> WeakClientBlobCache {
        WeakClientBlobCache {
            limits: self.limits,
            store: Arc::downgrade(&self.store),
        }
    }

    /// Gives the disk tier a bounded chance to supply references before status classification.
    /// Callers must retain the packet across cancellation; only resident bodies count as hits.
    pub async fn prefetch_packet(&self, packet: &Packet) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let hashes = {
            let store = self.lock();
            packet
                .referenced_blob_hashes()
                .into_iter()
                .filter(|hash| !store.entries.contains_key(hash) && !store.pins.contains_key(hash))
                .take(MAX_CLIENT_BLOB_CACHE_ENTRIES)
                .collect::<Vec<_>>()
        };
        if !hashes.is_empty() {
            persistence.prefetch(hashes).await;
        }
    }

    /// Queues persistence after the memory lock has been released.
    pub(super) fn persist(&self, hash: u64, payload: Arc<[u8]>) {
        if let Some(persistence) = &self.persistence {
            persistence.persist(hash, payload);
        }
    }

    /// Batches a group of memory hits into the edge's access journal.
    pub(super) fn touch(&self, hashes: &[u64]) {
        if let Some(persistence) = &self.persistence {
            persistence.touch(hashes);
        }
    }
}

impl WeakClientBlobCache {
    /// Rechecks a disk body's content hash and admits it only when memory and pin limits allow.
    /// Returns false if the cache is gone or already holds or pins the body. Background callers
    /// wait for the memory lock; no disk work runs while it is held.
    pub fn admit(&self, hash: u64, payload: Arc<[u8]>) -> Result<bool, BlobCacheError> {
        let actual = client_blob_hash(&payload);
        if actual != hash {
            return Err(BlobCacheError::HashMismatch {
                claimed: hash,
                actual,
            });
        }
        let Some(store) = self.store.upgrade() else {
            return Ok(false);
        };
        let mut store = store.lock().unwrap_or_else(|error| error.into_inner());
        if store.entries.contains_key(&hash) || store.pins.contains_key(&hash) {
            return Ok(false);
        }
        // Disk prefetch may fill the ordinary tier, but cannot inflate it past its trigger.
        if payload.len() > self.limits.trim_trigger_bytes {
            return Ok(false);
        }
        insert_shared(&mut store, self.limits, hash, payload)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Debug)]
    struct TestStore {
        cache: WeakClientBlobCache,
        body: Arc<[u8]>,
        deliver: AtomicBool,
        writes: Mutex<Vec<(u64, Arc<[u8]>)>>,
        reads: Mutex<Vec<Vec<u64>>>,
    }

    impl BlobCacheStore for TestStore {
        /// Captures a verified memory admission for the disk writer.
        fn persist(&self, hash: u64, payload: Arc<[u8]>) {
            self.writes.lock().unwrap().push((hash, payload));
        }
        /// This fixture has no age eviction.
        fn touch(&self, _: &[u64]) {}
        /// Models either a verified completed read or an unavailable disk body.
        fn prefetch(&self, hashes: Vec<u64>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(async move {
                self.reads.lock().unwrap().push(hashes.clone());
                if self.deliver.load(Ordering::Relaxed) {
                    for hash in hashes {
                        let _ = self.cache.admit(hash, self.body.clone());
                    }
                }
            })
        }
    }

    /// Gives one memory cache a controllable store with a single candidate disk body.
    fn fixture(deliver: bool) -> (ClientBlobCache, Arc<TestStore>, u64) {
        let cache = ClientBlobCache::default();
        let body: Arc<[u8]> = Arc::from(b"disk body".as_slice());
        let hash = client_blob_hash(&body);
        let store = Arc::new(TestStore {
            cache: cache.downgrade(),
            body,
            deliver: AtomicBool::new(deliver),
            writes: Mutex::new(Vec::new()),
            reads: Mutex::new(Vec::new()),
        });
        (cache.with_store(store.clone()), store, hash)
    }

    /// Disk membership without a delivered body reports a miss and preserves normal recovery.
    #[tokio::test]
    async fn blob_cache_disk_key_alone_is_not_a_hit() {
        let (cache, _, hash) = fixture(false);
        let packet = crate::Packet::new(super::super::tests::cached_level_chunk(1, vec![hash]));
        cache.prefetch_packet(&packet).await;
        let mut resolver = BlobCacheResolver::new(cache);
        let status = resolver.accept_cached_packet(packet).unwrap();
        assert_eq!(status.missing(), [hash]);
        assert!(status.have().is_empty());
        resolver
            .accept_miss_response(ClientCacheMissResponse {
                missing_blobs: vec![MissingBlobData {
                    blob_id: wire::U64LE(hash),
                    blob_data: bytes::Bytes::from_static(b"disk body"),
                }],
            })
            .unwrap();
        assert!(resolver.pop_ready().is_some());
    }

    /// Once server recovery owns a missing body, later packets only prefetch unpinned hashes.
    #[tokio::test]
    async fn blob_cache_prefetch_skips_pinned_misses() {
        let (cache, store, hash) = fixture(false);
        let packet = crate::Packet::new(super::super::tests::cached_level_chunk(1, vec![hash]));
        cache.prefetch_packet(&packet).await;
        let mut resolver = BlobCacheResolver::new(cache.clone());
        let status = resolver.accept_cached_packet(packet.clone()).unwrap();
        assert_eq!(status.missing(), [hash]);

        cache.prefetch_packet(&packet).await;
        assert_eq!(*store.reads.lock().unwrap(), [vec![hash]]);

        let other = client_blob_hash(b"another disk body");
        let mixed = crate::Packet::new(super::super::tests::cached_level_chunk(
            1,
            vec![hash, other],
        ));
        cache.prefetch_packet(&mixed).await;
        assert_eq!(*store.reads.lock().unwrap(), [vec![hash], vec![other]]);
        assert!(!cache.contains(hash));
    }

    /// A background-verified resident body is reported as found and reconstructs without a miss.
    #[tokio::test]
    async fn blob_cache_reports_only_deliverable_disk_body() {
        let (cache, _, hash) = fixture(true);
        let packet = crate::Packet::new(super::super::tests::cached_level_chunk(1, vec![hash]));
        cache.prefetch_packet(&packet).await;
        let mut resolver = BlobCacheResolver::new(cache);
        let status = resolver.accept_cached_packet(packet).unwrap();
        assert_eq!(status.have(), [hash]);
        assert!(status.missing().is_empty());
        let BlobCacheReady::Packet(packet) = resolver.pop_ready().unwrap() else {
            panic!("resolved packet")
        };
        let packets::Packet::LevelChunk(packet) = *packet.data else {
            panic!("level chunk")
        };
        assert_eq!(packet.serialized_chunk_data.as_ref(), b"disk body");
    }

    /// Disk import refuses corrupted payloads before memory insertion or hit classification.
    #[test]
    fn blob_cache_import_rechecks_hash() {
        let (cache, store, hash) = fixture(true);
        assert!(matches!(
            store.cache.admit(hash, Arc::from(b"wrong".as_slice())),
            Err(BlobCacheError::HashMismatch { .. })
        ));
        assert!(!cache.contains(hash));
    }

    /// A late disk read cannot change unresolved-pin accounting after a miss was already sent.
    #[test]
    fn blob_cache_late_read_keeps_server_miss_ownership() {
        let (cache, store, hash) = fixture(true);
        let mut resolver = BlobCacheResolver::new(cache.clone());
        let status = resolver
            .accept_cached_packet(crate::Packet::new(super::super::tests::cached_level_chunk(
                1,
                vec![hash],
            )))
            .unwrap();
        assert_eq!(status.missing(), [hash]);
        assert!(!store.cache.admit(hash, store.body.clone()).unwrap());
        assert!(!cache.contains(hash));
        assert_eq!(resolver.stats().cache_pinned_bytes, 0);
        resolver
            .accept_miss_response(ClientCacheMissResponse {
                missing_blobs: vec![MissingBlobData {
                    blob_id: wire::U64LE(hash),
                    blob_data: bytes::Bytes::from_static(b"disk body"),
                }],
            })
            .unwrap();
        assert!(resolver.pop_ready().is_some());
        assert_eq!(store.writes.lock().unwrap().len(), 1);
    }

    /// Warming and demand reads use the ordinary cache's existing byte and metadata ceilings.
    #[test]
    fn blob_cache_disk_import_stays_within_memory_limits() {
        let cache = ClientBlobCache::with_limits(BlobCacheLimits {
            trim_trigger_bytes: 32,
            trim_floor_bytes: 16,
        });
        let weak = cache.downgrade();
        for index in 0_u64..1_000 {
            let payload: Arc<[u8]> = Arc::from(index.to_le_bytes());
            weak.admit(client_blob_hash(&payload), payload).unwrap();
            assert!(cache.total_bytes() <= cache.limits().trim_trigger_bytes);
            assert!(cache.entry_count() <= MAX_CLIENT_BLOB_CACHE_ENTRIES);
        }
    }
}
