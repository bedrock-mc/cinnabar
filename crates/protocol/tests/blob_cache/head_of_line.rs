use super::*;

#[test]
fn cinnabar_transaction_safety_bound_and_status_packet_limit_are_defaults() {
    assert_eq!(protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS, 2_048);
    assert_eq!(protocol::MAX_CLIENT_BLOB_PENDING_BYTES, 64 * 1024 * 1024);
    assert_eq!(protocol::MAX_CLIENT_BLOB_HASHES_PER_PACKET, 4_095);
    assert_eq!(
        protocol::MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION,
        32 * 1024 * 1024
    );
    assert_eq!(protocol::MAX_CLIENT_BLOB_READY_BYTES, 32 * 1024 * 1024);
    assert_eq!(
        ClientBlobCache::default().limits(),
        BlobCacheLimits {
            trim_trigger_bytes: 100 * 1024 * 1024,
            trim_floor_bytes: 80 * 1024 * 1024,
        }
    );
}

#[test]
fn two_hundred_fifty_six_large_inline_pending_packets_are_bounded_and_recovered() {
    const INLINE_PAYLOAD_BYTES: usize = 1024 * 1024;

    let cache = ClientBlobCache::with_limits(limits(1));
    let mut resolver = BlobCacheResolver::new(cache.clone());
    let mut last_abandoned = None;
    for x in 0..256 {
        let missing_payload = format!("missing-inline-{x}");
        let missing_hash = client_blob_hash(missing_payload.as_bytes());
        let status = resolver
            .accept_cached_packet(
                cached_level_chunk(x, -13, vec![missing_hash], &[0x5a; INLINE_PAYLOAD_BYTES])
                    .into(),
            )
            .expect("pending-byte pressure must stay non-fatal");
        assert_eq!(status.missing(), [missing_hash]);
        assert!(status.have().is_empty());
        if let Some(recovery) = status.recovery {
            assert_eq!((recovery.x, recovery.z), (x, -13));
            last_abandoned = Some((missing_hash, missing_payload));
        }
    }

    assert!(
        resolver.stats().pending_bytes <= protocol::MAX_CLIENT_BLOB_PENDING_BYTES,
        "retained pending bytes must stay within the restored 64 MiB ceiling; observed {}",
        resolver.stats().pending_bytes
    );
    assert!(
        resolver.stats().pending_transactions < 256,
        "byte pressure must engage before the transaction-count ceiling"
    );
    assert!(
        resolver.stats().skipped_cached_packets > 0,
        "pending-byte abandonment must be classified as a cached-packet skip"
    );
    assert_eq!(
        resolver.stats().cached_packet_pending_pressure,
        resolver.stats().skipped_cached_packets,
        "every skip in this fixture is specifically pending-byte pressure"
    );
    assert_eq!(resolver.stats().cached_packet_transaction_pressure, 0);

    let (abandoned_hash, abandoned_payload) =
        last_abandoned.expect("at least one packet must route through recovery");
    assert_eq!(
        client_blob_hash(abandoned_payload.as_bytes()),
        abandoned_hash
    );
    cache
        .insert(abandoned_payload.as_bytes())
        .expect("insert formerly abandoned hash");
    cache
        .insert(b"replacement")
        .expect("exercise cache trimming after abandonment");
    assert!(
        !cache.contains(abandoned_hash),
        "pending-byte abandonment must release the transaction pin"
    );
}

#[test]
fn staged_pinned_bytes_are_bounded_before_every_miss_resolves_and_released_on_skip() {
    let blob_len = protocol::MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION / 3;
    let first = vec![0x11; blob_len];
    let second = vec![0x22; blob_len];
    let third = vec![0x33; blob_len + 3];
    let hashes = [
        client_blob_hash(&first),
        client_blob_hash(&second),
        client_blob_hash(&third),
        client_blob_hash(b"still-missing"),
    ];
    let cache = ClientBlobCache::with_limits(limits(1));
    let mut resolver = BlobCacheResolver::new(cache.clone());
    let status = resolver
        .accept_cached_packet(cached_level_chunk(41, -9, hashes.to_vec(), b"").into())
        .expect("transaction is initially admitted");
    assert_eq!(status.missing(), hashes);

    for (hash, payload) in [(hashes[0], first), (hashes[1], second)] {
        resolver
            .accept_miss_response(miss_response(vec![(hash, payload)]))
            .expect("staged payload remains below the bound");
    }
    assert_eq!(resolver.stats().pending_transactions, 1);

    resolver
        .accept_miss_response(miss_response(vec![(hashes[2], third)]))
        .expect("staged-byte excess is a non-fatal semantic skip");

    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().retained_cached_transactions, 0);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            protocol::ChunkResyncEvent { x: 41, z: -9, .. }
        )))
    ));

    cache
        .insert(b"replacement")
        .expect("exercise cache trimming");
    assert!(
        !cache.contains(hashes[0]) && !cache.contains(hashes[1]),
        "abandonment must release every pin so later cache pressure can evict staged blobs"
    );
}

#[test]
fn aggregate_reconstructed_ready_bytes_are_bounded_with_explicit_recovery() {
    let blob_len = protocol::MAX_CLIENT_BLOB_READY_BYTES / 2 + 1;
    let first = vec![0x44; blob_len];
    let second = vec![0x55; blob_len];
    let cache = ClientBlobCache::default();
    let first_hash = cache.insert(&first).expect("seed first ready payload");
    let second_hash = cache.insert(&second).expect("seed second ready payload");
    let mut resolver = BlobCacheResolver::new(cache);

    resolver
        .accept_cached_packet(cached_request_level(51, first_hash))
        .expect("first output fits the aggregate ready bound");
    resolver
        .accept_cached_packet(cached_request_level(52, second_hash))
        .expect("aggregate ready excess is non-fatal");

    assert_eq!(
        resolver.stats().retained_cached_transactions,
        1,
        "only the first reconstructed output may remain retained"
    );
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            protocol::ChunkResyncEvent { x: 52, .. }
        )))
    ));
    let packet = pop_packet(&mut resolver, "first retained output remains lossless");
    let McpePacketData::LevelChunkPacket(packet) = packet.data else {
        panic!("expected reconstructed LevelChunk")
    };
    assert_eq!(packet.chunk_position.x, 51);
    assert_eq!(packet.serialized_chunk_data, first);
}

#[test]
fn zero_reference_status_is_suppressed_but_have_only_status_is_sent() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    let empty = resolver
        .accept_cached_packet(
            SubchunkPacket {
                entries: SubchunkPacketEntries::SubChunkEntryWithCaching(Vec::new()),
                ..Default::default()
            }
            .into(),
        )
        .expect("well-formed empty cached SubChunk");
    assert!(
        empty.into_packets().is_empty(),
        "vanilla sends no totally empty cache-status packet"
    );

    let payload = b"full-cache-hit";
    let cache = ClientBlobCache::default();
    let hash = cache.insert(payload).expect("seed full hit");
    let mut resolver = BlobCacheResolver::new(cache);
    let packets = resolver
        .accept_cached_packet(cached_request_level(61, hash))
        .expect("have-only cached status")
        .into_packets();
    assert_eq!(packets.len(), 1);
    assert!(packets[0].missing_ids.is_empty());
    assert_eq!(packets[0].found_ids, vec![hash]);
}

#[test]
fn ordinary_lane_is_not_bounded_by_the_cached_transaction_limit() {
    let bounded = limits(256);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(bounded));
    resolver
        .accept_cached_packet(cached_request_level(
            1,
            client_blob_hash(b"unresolved-cache-transaction"),
        ))
        .expect("unresolved cached transaction");

    for time in 0..3 {
        resolver
            .accept_world_event(WorldEvent::SetTime(SetTimeEvent { time }), 8)
            .expect("ordinary traffic has its own non-backpressured lane");
    }
    for time in 0..3 {
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::SetTime(
                SetTimeEvent { time: actual }
            ))) if actual == time
        ));
    }
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert_eq!(resolver.stats().skipped_world_events, 0);
}

#[test]
fn ordinary_lane_is_separately_bounded_accounted_and_lossless_under_backpressure() {
    let hash = client_blob_hash(b"blocked-column");
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(4, hash))
        .expect("cached column barrier");
    resolver
        .accept_world_event(
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [4 * 16, 0, 0],
                layer: 0,
                network_id: 123,
            }]),
            size_of::<BlockUpdateEvent>(),
        )
        .expect("ordinary event is retained");

    let retained = resolver.stats();
    assert_eq!(retained.ordinary_ready_events, 1);
    assert!(retained.ordinary_ready_bytes >= size_of::<BlockUpdateEvent>());
    assert_eq!(retained.retained_cached_transactions, 1);
    assert!(resolver.ordinary_lane_needs_drain());
    assert!(
        resolver
            .unblock_ordinary_lane()
            .expect("bounded cache abandonment"),
        "the receive-side backpressure gate must make retained work drainable"
    );

    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
    ));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(updates)))
            if updates[0].network_id == 123
    ));
    assert_eq!(resolver.stats().ordinary_ready_events, 0);
    assert_eq!(resolver.stats().ordinary_ready_bytes, 0);
    assert_eq!(resolver.stats().skipped_world_events, 0);
}

#[test]
fn later_complete_transaction_resolves_while_earlier_transaction_is_pending() {
    let a = b"a";
    let b = b"b";
    let ah = client_blob_hash(a);
    let bh = client_blob_hash(b);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(limits(128)));
    resolver
        .accept_cached_packet(cached_level_at(1, vec![ah, ah, ah], b"first"))
        .expect("first transaction");
    resolver
        .accept_cached_packet(cached_level_at(2, vec![bh, bh, bh], b"second"))
        .expect("second transaction");

    resolver
        .accept_miss_response(miss_response(vec![(bh, b.to_vec())]))
        .expect("later transaction resolves first");
    let second = pop_packet(&mut resolver, "later completed packet");
    let McpePacketData::LevelChunkPacket(second) = second.data else {
        panic!()
    };
    assert!(second.payload.ends_with(b"second"));
    assert_eq!(resolver.stats().pending_transactions, 1);

    resolver
        .accept_miss_response(miss_response(vec![(ah, a.to_vec())]))
        .expect("earlier transaction resolves");

    let first = pop_packet(&mut resolver, "first packet");
    let McpePacketData::LevelChunkPacket(first) = first.data else {
        panic!()
    };
    assert!(first.payload.ends_with(b"first"));
}

#[test]
fn same_column_block_update_waits_for_cached_chunk_and_survives_replacement() {
    let chunk_payload = b"cached-column";
    let hash = client_blob_hash(chunk_payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(limits(256)));
    resolver
        .accept_cached_packet(cached_request_level(4, hash))
        .expect("pending cached column");

    let same_column_update = BlockUpdateEvent {
        dimension: 0,
        position: [4 * 16 + 3, 0, 7],
        layer: 0,
        network_id: 99,
    };
    resolver
        .accept_world_event(
            WorldEvent::BlockUpdates(vec![same_column_update]),
            size_of::<BlockUpdateEvent>(),
        )
        .expect("same-column update is retained behind the chunk");
    resolver
        .accept_world_event(
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [5 * 16, 0, 0],
                layer: 0,
                network_id: 77,
            }]),
            size_of::<BlockUpdateEvent>(),
        )
        .expect("unrelated update remains independently ready");

    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(updates)))
            if updates[0].network_id == 77
    ));
    assert!(
        resolver.pop_ready().is_none(),
        "same-column update must not overtake its unresolved chunk"
    );

    resolver
        .accept_miss_response(miss_response(vec![(hash, chunk_payload.to_vec())]))
        .expect("resolve cached column");

    let mut consumed_column = None;
    for _ in 0..2 {
        match resolver.pop_ready().expect("chunk and update become ready") {
            BlobCacheReady::Packet(packet) => {
                let McpePacketData::LevelChunkPacket(packet) = packet.data else {
                    panic!("expected cached LevelChunk")
                };
                assert_eq!(packet.chunk_position.x, 4);
                consumed_column = Some(0);
            }
            BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(updates)) => {
                assert_eq!(updates, vec![same_column_update]);
                consumed_column = Some(updates[0].network_id);
            }
            other => panic!("unexpected ready work: {other:?}"),
        }
    }
    assert_eq!(
        consumed_column,
        Some(99),
        "the ordering-sensitive consumer must retain the update after chunk replacement"
    );
}

#[test]
fn rejected_response_abandons_dead_transaction_and_unblocks_its_column() {
    let payload = b"expected";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(4, hash))
        .expect("pending cached column");
    resolver
        .accept_world_event(
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [4 * 16, 0, 0],
                layer: 0,
                network_id: 91,
            }]),
            size_of::<BlockUpdateEvent>(),
        )
        .expect("same-column update");

    resolver
        .accept_miss_response(miss_response(vec![(hash, b"wrong-payload".to_vec())]))
        .expect("semantic integrity rejection stays non-fatal");

    assert_eq!(
        resolver.stats().pending_transactions,
        0,
        "a response the transaction cannot use must abandon that transaction"
    );
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            protocol::ChunkResyncEvent {
                dimension: 0,
                x: 4,
                requested_sub_chunks: None,
                ..
            }
        )))
    ));
    assert!(
        matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(updates)))
                if updates[0].network_id == 91
        ),
        "abandonment must release the same-column barrier"
    );
}

#[test]
fn transaction_bound_rotates_oldest_pending_work_without_growth() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    for index in 0..protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS as u64 {
        let status = resolver
            .accept_cached_packet(cached_request_level(index as i32, index + 1))
            .expect("transactions through the Cinnabar safety bound remain accepted");
        assert_eq!(status.missing(), [index + 1]);
    }

    let excess = resolver
        .accept_cached_packet(cached_request_level(9_999, 9_999))
        .expect("Cinnabar safety-bound rotation stays non-fatal");

    assert_eq!(excess.missing(), [9_999]);
    assert!(excess.have().is_empty());
    assert_eq!(excess.classified_hashes(), 1);
    assert_eq!(
        excess.recovery.map(|recovery| recovery.x),
        Some(0),
        "the oldest retained transaction must receive inline recovery"
    );
    assert_eq!(
        resolver.stats().pending_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS
    );
    assert_eq!(resolver.stats().skipped_cached_packets, 0);
    assert_eq!(resolver.stats().cached_packet_transaction_pressure, 1);
    assert_eq!(resolver.stats().abandoned_cached_transactions, 1);
    assert!(
        resolver.pop_ready().is_none(),
        "the rotated recovery is carried inline and the replacement remains pending"
    );
}

#[test]
fn pressure_rotation_and_pending_byte_rejection_count_each_recovery_once() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    for index in 0..protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS as u64 {
        resolver
            .accept_cached_packet(cached_request_level(index as i32, index + 1))
            .expect("fill the transaction bound");
    }

    let status = resolver
        .accept_cached_packet_with_size(
            cached_request_level(999, 999),
            protocol::MAX_CLIENT_BLOB_PENDING_BYTES,
        )
        .expect("pending-byte rejection after rotation remains non-fatal");

    assert_eq!(status.recovery.map(|recovery| recovery.x), Some(0));
    assert_eq!(
        resolver.stats().pending_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 1
    );
    assert_eq!(resolver.stats().abandoned_cached_transactions, 2);
    assert_eq!(resolver.stats().cached_packet_pending_pressure, 1);
    assert_eq!(
        resolver.stats().recovery_requests,
        2,
        "the inline old recovery and queued current recovery are each counted once"
    );
    assert_eq!(resolver.stats().recovery_ready_events, 1);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 999, .. }
        )))
    ));
}

#[test]
fn pressure_rotation_and_staged_rejection_refresh_queued_recovery_accounting() {
    let cache = ClientBlobCache::default();
    let blob_len = protocol::MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION / 3 + 1;
    let hit_hashes = [0x31, 0x32, 0x33].map(|byte| {
        cache
            .insert(&vec![byte; blob_len])
            .expect("seed a staged cache hit")
    });
    let mut resolver = BlobCacheResolver::new(cache);
    for index in 0..protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS as u64 {
        let payload = format!("staged-pressure-{index}");
        resolver
            .accept_cached_packet(cached_request_level(
                index as i32,
                client_blob_hash(payload.as_bytes()),
            ))
            .expect("fill the transaction bound");
    }
    let missing_hash = client_blob_hash(b"staged-current-miss");
    let mut hashes = hit_hashes.to_vec();
    hashes.push(missing_hash);

    let status = resolver
        .accept_cached_packet(cached_level_chunk(999, 0, hashes, b"").into())
        .expect("staged rejection after rotation remains non-fatal");

    assert_eq!(status.recovery.map(|recovery| recovery.x), Some(0));
    assert_eq!(
        resolver.stats().pending_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 1
    );
    assert_eq!(resolver.stats().abandoned_cached_transactions, 2);
    assert_eq!(resolver.stats().cached_packet_staged_pressure, 1);
    assert_eq!(resolver.stats().recovery_requests, 2);
    assert_eq!(resolver.stats().recovery_ready_events, 1);
    assert!(resolver.stats().recovery_ready_bytes > 0);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 999, .. }
        )))
    ));
}

#[test]
fn transaction_pressure_releases_pending_work_that_blocks_a_cached_ready_packet() {
    let cache = ClientBlobCache::default();
    let ready_hash = cache
        .insert(b"ready-behind-pending")
        .expect("seed ready hit");
    let mut resolver = BlobCacheResolver::new(cache);
    for index in 0..(protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 1) as u64 {
        resolver
            .accept_cached_packet(cached_request_level(index as i32, index + 1))
            .expect("retain unresolved work through one slot below the safety bound");
    }

    resolver
        .accept_cached_packet(cached_request_level(0, ready_hash))
        .expect("the ready transition must release its same-column blocker at the safety bound");

    assert_eq!(
        resolver.stats().pending_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 2
    );
    assert_eq!(
        resolver.stats().retained_cached_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 1
    );
    assert_eq!(resolver.stats().skipped_cached_packets, 0);
    assert_eq!(resolver.stats().abandoned_cached_transactions, 1);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 0, .. }
        )))
    ));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::Packet(packet))
            if matches!(
                &packet.data,
                McpePacketData::LevelChunkPacket(packet) if packet.chunk_position.x == 0
            )
    ));

    let fresh_hash = client_blob_hash(b"fresh-after-ready-pressure");
    let fresh = resolver
        .accept_cached_packet(cached_request_level(999, fresh_hash))
        .expect("cached intake resumes after the blocked ready lane is released");
    assert_eq!(fresh.missing(), [fresh_hash]);
    assert!(fresh.recovery.is_none());
    assert_eq!(
        resolver.stats().pending_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 1
    );
    assert_eq!(
        resolver.stats().retained_cached_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 1
    );
    assert_eq!(resolver.stats().skipped_cached_packets, 0);
}

#[test]
fn pending_transition_to_pressure_releases_an_existing_cached_ready_packet() {
    let cache = ClientBlobCache::default();
    let ready_hash = cache
        .insert(b"ready-before-pressure")
        .expect("seed ready hit");
    let mut resolver = BlobCacheResolver::new(cache);
    resolver
        .accept_cached_packet(cached_request_level(0, 1))
        .expect("retain the same-column blocker");
    resolver
        .accept_cached_packet(cached_request_level(0, ready_hash))
        .expect("retain a blocked ready packet below the safety bound");
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert_eq!(resolver.stats().retained_cached_transactions, 2);
    assert!(resolver.pop_ready().is_none());

    for index in 0..(protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 2) as u64 {
        resolver
            .accept_cached_packet(cached_request_level(1_000 + index as i32, 2 + index))
            .expect("unrelated pending work may fill the remaining transaction slots");
    }

    assert_eq!(
        resolver.stats().pending_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 2
    );
    assert_eq!(
        resolver.stats().retained_cached_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 1
    );
    assert_eq!(resolver.stats().abandoned_cached_transactions, 1);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 0, .. }
        )))
    ));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::Packet(packet))
            if matches!(
                &packet.data,
                McpePacketData::LevelChunkPacket(packet) if packet.chunk_position.x == 0
            )
    ));
}

#[test]
fn completed_cached_transactions_share_the_same_safety_bound() {
    let cache = ClientBlobCache::default();
    let hash = cache.insert(b"ready-hit").expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);
    for x in 0..protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS {
        resolver
            .accept_cached_packet(cached_request_level(x as i32, hash))
            .expect("completed transaction through the safety bound");
    }
    let retained_at_limit = resolver.stats().pending_bytes;
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(
        resolver.stats().retained_cached_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS
    );

    let excess = resolver
        .accept_cached_packet(cached_request_level(99, hash))
        .expect("ready-lane excess stays non-fatal");

    assert!(excess.missing().is_empty());
    assert_eq!(excess.have(), [hash]);
    assert_eq!(
        excess.classified_hashes(),
        1,
        "the ready-transaction pressure path must classify every reference"
    );
    assert!(excess.recovery.is_some());
    assert_eq!(
        resolver.stats().retained_cached_transactions,
        protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS
    );
    assert_eq!(resolver.stats().pending_bytes, retained_at_limit);
}

#[test]
fn reconstruction_cost_is_bounded_before_duplicate_blob_copies_are_allocated() {
    let blob_len = protocol::MAX_CLIENT_BLOB_RECONSTRUCTED_BYTES / 2 + 1;
    let blob = vec![0x5a; blob_len];
    let cache = ClientBlobCache::default();
    let hash = cache.insert(&blob).expect("seed a large cached blob");
    let mut resolver = BlobCacheResolver::new(cache);

    let status = resolver
        .accept_cached_packet(
            cached_level_chunk(17, -4, vec![hash, hash], &[0x7f]).into(),
        )
        .expect("reconstruction safety excess stays non-fatal");

    assert!(status.missing().is_empty());
    assert_eq!(status.have(), [hash]);
    assert_eq!(
        status.classified_hashes(),
        1,
        "the reconstruction skip path must still classify every unique reference"
    );
    assert_eq!(
        status
            .recovery
            .as_ref()
            .map(|recovery| (recovery.x, recovery.z)),
        Some((17, -4))
    );
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().retained_cached_transactions, 0);
    assert_eq!(resolver.stats().pending_bytes, 0);
    assert_eq!(resolver.stats().reconstructed_level_chunks, 0);
    assert!(
        resolver.pop_ready().is_none(),
        "the projected payload must be rejected before a ready allocation is retained"
    );
}

#[test]
fn ordinary_lane_reports_its_independent_hard_boundary_without_dropping_retained_events() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    for time in 0..protocol::MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS {
        resolver
            .accept_world_event(WorldEvent::SetTime(SetTimeEvent { time: time as i32 }), 8)
            .expect("ordinary lane capacity");
    }

    assert!(matches!(
        resolver.accept_world_event(WorldEvent::SetTime(SetTimeEvent { time: 999 }), 8),
        Err(BlobCacheError::OrdinaryLaneFull { .. })
    ));
    assert_eq!(
        resolver.stats().ordinary_ready_events,
        protocol::MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS
    );
    for time in 0..protocol::MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS {
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::SetTime(SetTimeEvent {
                time: actual
            }))) if actual == time as i32
        ));
    }
    assert_eq!(resolver.stats().skipped_world_events, 0);
    assert_eq!(resolver.stats().ordinary_ready_events, 0);
}
