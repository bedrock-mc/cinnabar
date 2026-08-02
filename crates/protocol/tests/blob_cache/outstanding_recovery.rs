use super::*;

#[test]
fn abandoned_subchunk_emits_exact_scheduler_rollback() {
    let payload = b"subchunk-recovery";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    let status = resolver
        .accept_cached_packet(cached_subchunk_multi_column(hash, payload))
        .expect("authorize the cached SubChunk packet");
    assert!(status.recovery.is_none());
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

    assert_eq!(resolver.stats().abandoned_cached_transactions, 1);
    assert_eq!(resolver.stats().recovery_ready_events, 2);
    assert_eq!(resolver.stats().recovery_requests, 2);
    for (x, z, y) in [(4, 9, -3), (7, 13, -1)] {
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
                ChunkResyncEvent {
                    dimension: 0,
                    x: event_x,
                    z: event_z,
                    requested_sub_chunks: None,
                    requested_sub_chunk_ys: Some(ys),
                }
            ))) if event_x == x && event_z == z && ys == vec![y]
        ));
    }
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(_)))
    ));
    assert!(resolver.pop_ready().is_none());
}

#[test]
fn transaction_pressure_rotates_oldest_and_preserves_current_recovery_contracts() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    for x in 0..protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS {
        let payload = format!("bound-{x}");
        resolver
            .accept_cached_packet(cached_request_level(
                i32::try_from(x).expect("transaction coordinate fits"),
                client_blob_hash(payload.as_bytes()),
            ))
            .expect("transactions through the safety bound remain accepted");
    }

    let subchunk_payload = b"subchunk-at-bound";
    let subchunk_hash = client_blob_hash(subchunk_payload);
    let subchunk_status = resolver
        .accept_cached_packet(cached_subchunk(subchunk_hash, subchunk_payload))
        .expect("transaction-pressure SubChunk rotation remains non-fatal");
    assert_eq!(subchunk_status.missing(), [subchunk_hash]);
    assert_eq!(
        subchunk_status.recovery.as_ref().map(|recovery| recovery.x),
        Some(0),
        "the inline recovery belongs to the rotated oldest transaction"
    );
    assert_eq!(resolver.stats().pending_transactions, 255);
    assert_eq!(resolver.stats().skipped_cached_packets, 1);
    assert_eq!(resolver.stats().cached_packet_transaction_pressure, 1);
    assert_eq!(resolver.stats().abandoned_cached_transactions, 2);
    assert_eq!(resolver.stats().recovery_ready_events, 0);
    assert_eq!(resolver.stats().recovery_requests, 1);
    assert!(resolver.pop_ready().is_none());

    let second_status = resolver
        .accept_cached_packet(cached_subchunk(subchunk_hash, subchunk_payload))
        .expect("a second rotation creates recovery-slot headroom");
    assert_eq!(second_status.missing(), [subchunk_hash]);
    assert_eq!(
        second_status.recovery.as_ref().map(|recovery| recovery.x),
        Some(1)
    );
    assert_eq!(resolver.stats().pending_transactions, 254);
    assert_eq!(resolver.stats().skipped_cached_packets, 2);
    assert_eq!(resolver.stats().cached_packet_transaction_pressure, 2);
    assert_eq!(resolver.stats().abandoned_cached_transactions, 4);

    let admitted_status = resolver
        .accept_cached_packet(cached_subchunk(subchunk_hash, subchunk_payload))
        .expect("the retry is admitted only after prior recovery can be delivered");
    assert_eq!(admitted_status.missing(), [subchunk_hash]);
    assert!(admitted_status.recovery.is_none());
    assert_eq!(resolver.stats().pending_transactions, 255);
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: subchunk_hash,
                payload: subchunk_payload.to_vec(),
            }],
        })
        .expect("the admitted retry accepts its blob");
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::Packet(packet))
            if matches!(&packet.data, McpePacketData::PacketSubchunk(_))
    ));
}

#[test]
fn abandoned_levelchunk_recovery_does_not_stop_cached_intake() {
    let payload = b"intake-under-recovery";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());

    resolver
        .accept_cached_packet(cached_request_level(4, hash))
        .expect("authorize the transaction that will be abandoned");
    resolver
        .accept_world_event(
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [4 * 16, 0, 0],
                layer: 0,
                network_id: 0,
            }]),
            0,
        )
        .expect("retain the blocker");
    resolver
        .unblock_ordinary_lane()
        .expect("abandonment remains non-fatal");
    assert!(resolver.stats().recovery_ready_events > 0);

    let skipped_before = resolver.stats().skipped_cached_packets;
    let pending_before = resolver.stats().pending_transactions;
    resolver
        .accept_cached_packet(cached_request_level(9, client_blob_hash(b"fresh-intake")))
        .expect("cached intake continues while recovery is queued");
    assert_eq!(
        resolver.stats().skipped_cached_packets,
        skipped_before,
        "a queued recovery must not skip an admissible cached packet"
    );
    assert_eq!(
        resolver.stats().pending_transactions,
        pending_before + 1,
        "the packet is admitted as a pending transaction"
    );
}

#[test]
fn abandoned_subchunk_recovery_index_is_bounded_by_admission() {
    let payload = b"recovery-aggregation";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    let mut inline_recoveries = Vec::new();

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
        let mut status = resolver
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
            .expect("large indexed SubChunk fixture remains bounded");
        inline_recoveries.extend(status.take_recovery());
    }
    assert_eq!(
        inline_recoveries.len(),
        1,
        "one recovery slot is carried inline when pressure rotates the admitted transaction"
    );

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
        .expect("retain the ordinary event after proactive pressure recovery");

    assert!(
        !resolver
            .unblock_ordinary_lane()
            .expect("the proactive rotation already removed the cached blocker")
    );
    assert_eq!(resolver.stats().recovery_ready_events, 255);
    assert_eq!(resolver.stats().recovery_requests, 256);
    assert_eq!(resolver.stats().abandoned_cached_transactions, 256);
    assert_eq!(resolver.stats().pending_transactions, 0);
    for _ in 0..255 {
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
                ChunkResyncEvent {
                    requested_sub_chunk_ys: Some(ys),
                    ..
                }
            ))) if ys.len() == 1
        ));
    }
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(_)))
    ));
    assert!(resolver.pop_ready().is_none());
}

#[test]
fn abandoned_subchunks_emit_coalesced_exact_recovery() {
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
        .expect("SubChunk abandonment remains non-fatal");
    assert_eq!(resolver.stats().recovery_ready_events, 2);
    assert_eq!(resolver.stats().recovery_requests, 2);
    assert_eq!(resolver.stats().abandoned_cached_transactions, 2);
    for (x, z, y) in [(4, 8, -3), (5, 9, -2)] {
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
                ChunkResyncEvent {
                    dimension: 0,
                    x: event_x,
                    z: event_z,
                    requested_sub_chunks: None,
                    requested_sub_chunk_ys: Some(ys),
                }
            ))) if event_x == x && event_z == z && ys == vec![y]
        ));
    }
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(_)))
    ));
    assert!(resolver.pop_ready().is_none());
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
