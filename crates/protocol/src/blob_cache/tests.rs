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
