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
            .retained_cached_bytes()
            .expect("exact retained bytes")
    );

    resolver.reset_pending();

    assert_eq!(resolver.pending.capacity(), 0);
    assert!(resolver.ready.is_empty());
    assert_eq!(resolver.pending_by_hash.capacity(), 0);
    assert_eq!(resolver.stats.pending_bytes, 0);
}

#[test]
fn classify_and_pin_is_one_cache_operation() {
    let limits = BlobCacheLimits {
        trim_trigger_bytes: 4,
        trim_floor_bytes: 3,
    };
    let cache = ClientBlobCache::with_limits(limits);
    let hit = cache.insert(b"hit").expect("seed hit");
    let miss = client_blob_hash(b"miss");

    let (have, missing) = cache.classify(&[hit, miss], true);

    assert_eq!(have, vec![hit]);
    assert_eq!(missing, vec![miss]);
    let new = cache
        .insert(b"new")
        .expect("cache pressure never refuses an insert");
    assert!(cache.contains(hit), "reported hit is already pinned");
    assert!(
        cache.contains(new),
        "the triggering insert remains admitted"
    );
    cache.unpin_all(&[hit, miss]);
}

#[test]
fn arriving_blob_visits_only_transactions_in_its_hash_index_bucket() {
    let shared_payload = b"shared-index";
    let shared = client_blob_hash(shared_payload);
    let other = client_blob_hash(b"other-index");
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    for (x, hash) in [(1, shared), (2, shared), (3, other)] {
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
            .expect("index pending transaction by its missing hash");
    }
    assert_eq!(resolver.pending_by_hash[&shared].len(), 2);
    assert_eq!(resolver.pending_by_hash[&other].len(), 1);

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![valentine::bedrock::version::v1_26_30::Blob {
                hash: shared,
                payload: shared_payload.to_vec(),
            }],
        })
        .expect("resolve only the shared index bucket");

    assert!(!resolver.pending_by_hash.contains_key(&shared));
    assert_eq!(resolver.pending_by_hash[&other].len(), 1);
    assert_eq!(resolver.pending.len(), 1);
    assert_eq!(resolver.ready.len(), 2);
}
