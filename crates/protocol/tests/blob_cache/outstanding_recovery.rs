use super::*;

#[test]
fn abandoned_subchunk_recovery_covers_every_column_and_only_referenced_sections() {
    let payload = b"subchunk-recovery";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_subchunk_multi_column(hash, payload))
        .expect("authorize the cached SubChunk packet");
    resolver
        .accept_world_event(
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [4 * 16, 0, 9 * 16],
                layer: 0,
                network_id: 0,
            }]),
            1,
        )
        .expect("retain the ordinary event that abandons the cached packet");
    resolver
        .unblock_ordinary_lane()
        .expect("SubChunk abandonment remains non-fatal");

    let first = match resolver.pop_ready() {
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(recovery))) => recovery,
        other => panic!("expected first SubChunk recovery, got {other:?}"),
    };
    let second = match resolver.pop_ready() {
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(recovery))) => recovery,
        other => panic!("expected second SubChunk recovery, got {other:?}"),
    };
    assert_eq!(
        (first.x, first.z, first.requested_sub_chunk_ys.as_deref()),
        (4, 9, Some([-3].as_slice()))
    );
    assert_eq!(
        (second.x, second.z, second.requested_sub_chunk_ys.as_deref()),
        (7, 13, Some([-1].as_slice()))
    );
    assert!(
        resolver.pop_ready().is_some(),
        "the ordinary event follows recovery"
    );
}

#[test]
fn recovery_coalescing_scales_with_indexed_columns() {
    let payload = b"recovery-aggregation";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());

    for transaction in 0..256_i32 {
        let entries = (-128_i16..=127)
            .map(|dx| SubChunkEntryWithCachingItem {
                dx: i8::try_from(dx).unwrap(),
                dy: 1,
                dz: 0,
                result: SubChunkEntryWithCachingItemResult::Success,
                payload: Some(payload.to_vec()),
                heightmap_type: HeightMapDataType::NoData,
                heightmap: None,
                render_heightmap_type: HeightMapDataType::NoData,
                render_heightmap: None,
                blob_id: hash,
            })
            .collect();
        resolver
            .accept_cached_packet(
                SubchunkPacket {
                    dimension: 0,
                    origin: Vec3I {
                        x: transaction.saturating_mul(512),
                        y: -4,
                        z: 0,
                    },
                    entries: SubchunkPacketEntries::SubChunkEntryWithCaching(entries),
                }
                .into(),
            )
            .expect("large indexed recovery fixture remains bounded");
    }

    let updates = (0..256_i32)
        .flat_map(|transaction| {
            (-128_i32..=127).map(move |dx| BlockUpdateEvent {
                dimension: 0,
                position: [
                    (transaction.saturating_mul(512).saturating_add(dx)).saturating_mul(16),
                    0,
                    0,
                ],
                layer: 0,
                network_id: 0,
            })
        })
        .collect();
    resolver
        .accept_world_event(WorldEvent::BlockUpdates(updates), 0)
        .expect("retain the blocker for every referenced column");

    assert!(
        resolver
            .unblock_ordinary_lane()
            .expect("indexed recovery aggregation remains non-fatal")
    );
    assert_eq!(resolver.stats().recovery_ready_events, 256 * 256);
    assert_eq!(resolver.stats().recovery_requests, 256 * 256);
    assert_eq!(resolver.stats().pending_transactions, 0);
}

#[test]
fn recovery_requests_count_coalesced_emissions() {
    let payload = b"coalesced-recovery";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());

    for (index, _) in [1, 2].into_iter().enumerate() {
        resolver
            .accept_cached_packet(cached_subchunk(hash, payload))
            .expect("authorize a cached SubChunk transaction");
        assert_eq!(resolver.stats().pending_transactions, index + 1);
    }
    resolver
        .accept_world_event(
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [4 * 16, 0, 8 * 16],
                layer: 0,
                network_id: 0,
            }]),
            0,
        )
        .expect("retain the blocker for both transactions");

    resolver
        .unblock_ordinary_lane()
        .expect("coalesced recovery remains non-fatal");
    // `cached_subchunk` references two columns, (4,8) and (5,9), so abandoning both
    // transactions raises four recovery contributions. Coalescing by column collapses
    // them to two queued events and two counted emissions; without coalescing this
    // would be four.
    assert_eq!(resolver.stats().recovery_ready_events, 2);
    assert_eq!(resolver.stats().recovery_requests, 2);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
    ));
}

#[test]
fn redundant_missing_requests_count_each_pending_hash_once_and_omit_status_hashes() {
    let payload = b"shared-response";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());

    let first = resolver
        .accept_cached_packet(cached_request_level(1, hash))
        .expect("authorize first shared miss");
    assert_eq!(first.missing(), [hash]);
    assert_eq!(resolver.stats().redundant_missing_requests, 0);

    let second = resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("authorize second shared miss");
    assert!(second.missing().is_empty());
    assert!(second.have().is_empty());
    assert_eq!(resolver.stats().redundant_missing_requests, 1);

    assert_eq!(first.into_packets().len(), 1);
    assert!(second.into_packets().is_empty());
}

#[test]
fn plain_cache_misses_do_not_increment_redundant_missing_requests() {
    let shared_payload = b"shared-response";
    let shared_hash = client_blob_hash(shared_payload);
    let plain_hash = client_blob_hash(b"plain-cache-miss");
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());

    resolver
        .accept_cached_packet(cached_request_level(1, shared_hash))
        .expect("authorize shared miss owner");
    resolver
        .accept_cached_packet(cached_request_level(2, shared_hash))
        .expect("authorize redundant shared miss");
    assert_eq!(resolver.stats().redundant_missing_requests, 1);

    let plain = resolver
        .accept_cached_packet(cached_request_level(3, plain_hash))
        .expect("authorize plain cache miss");
    assert_eq!(plain.missing(), [plain_hash]);
    assert_eq!(
        resolver.stats().redundant_missing_requests,
        1,
        "a miss without an outstanding owner is not redundant"
    );
}

#[test]
fn resolver_accepts_authorized_response_after_another_resolver_fills_shared_cache() {
    let payload = b"cross-resolver";
    let hash = client_blob_hash(payload);
    let cache = ClientBlobCache::default();
    let mut first = BlobCacheResolver::new(cache.clone());
    let mut second = BlobCacheResolver::new(cache);
    first
        .accept_cached_packet(cached_request_level(1, hash))
        .expect("first authorization");
    second
        .accept_cached_packet(cached_request_level(2, hash))
        .expect("second authorization");
    let response = || ClientCacheMissResponsePacket {
        blobs: vec![Blob {
            hash,
            payload: payload.to_vec(),
        }],
    };

    first.accept_miss_response(response()).expect("first fill");
    second
        .accept_miss_response(response())
        .expect("second resolver retains independent authorization");
    let _ = pop_packet(&mut second, "second resolver transaction");
}
