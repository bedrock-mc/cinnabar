use super::*;

#[test]
fn authoritative_cached_transaction_and_status_packet_limits_are_defaults() {
    assert_eq!(protocol::MAX_CLIENT_BLOB_PENDING_TRANSACTIONS, 8);
    assert_eq!(protocol::MAX_CLIENT_BLOB_HASHES_PER_PACKET, 4_095);
}

#[test]
fn ordinary_lane_is_not_bounded_by_the_cached_transaction_limit() {
    let bounded = limits(8, 256);
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
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(limits(4, 128)));
    resolver
        .accept_cached_packet(cached_level_at(1, vec![ah, ah, ah], b"first"))
        .expect("first transaction");
    resolver
        .accept_cached_packet(cached_level_at(2, vec![bh, bh, bh], b"second"))
        .expect("second transaction");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: bh,
                payload: b.to_vec(),
            }],
        })
        .expect("later transaction resolves first");
    let second = pop_packet(&mut resolver, "later completed packet");
    let McpePacketData::PacketLevelChunk(second) = second.data else {
        panic!()
    };
    assert!(second.payload.ends_with(b"second"));
    assert_eq!(resolver.stats().pending_transactions, 1);

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: ah,
                payload: a.to_vec(),
            }],
        })
        .expect("earlier transaction resolves");

    let first = pop_packet(&mut resolver, "first packet");
    let McpePacketData::PacketLevelChunk(first) = first.data else {
        panic!()
    };
    assert!(first.payload.ends_with(b"first"));
}

#[test]
fn same_column_block_update_waits_for_cached_chunk_and_survives_replacement() {
    let chunk_payload = b"cached-column";
    let hash = client_blob_hash(chunk_payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(limits(8, 256)));
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
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: chunk_payload.to_vec(),
            }],
        })
        .expect("resolve cached column");

    let mut consumed_column = None;
    for _ in 0..2 {
        match resolver.pop_ready().expect("chunk and update become ready") {
            BlobCacheReady::Packet(packet) => {
                let McpePacketData::PacketLevelChunk(packet) = packet.data else {
                    panic!("expected cached LevelChunk")
                };
                assert_eq!(packet.x, 4);
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
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: b"wrong-payload".to_vec(),
            }],
        })
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
fn ninth_server_controlled_transaction_is_rejected_without_retained_growth() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    for index in 0..8_u64 {
        let status = resolver
            .accept_cached_packet(cached_request_level(index as i32, index + 1))
            .expect("the documented eight transactions remain accepted");
        assert_eq!(status.missing, vec![index + 1]);
    }
    let retained_at_limit = resolver.stats().pending_bytes;

    let excess = resolver
        .accept_cached_packet(cached_request_level(99, 99))
        .expect("server-controlled excess stays non-fatal");

    assert!(
        excess.missing.is_empty() && excess.have.is_empty(),
        "discarded cached work must not solicit a response"
    );
    assert_eq!(
        excess.recovery.map(|recovery| recovery.x),
        Some(99),
        "discarded cached work must request its affected column again"
    );
    assert_eq!(resolver.stats().pending_transactions, 8);
    assert_eq!(resolver.stats().pending_bytes, retained_at_limit);
}

#[test]
fn completed_cached_transactions_share_the_same_eight_item_retained_bound() {
    let cache = ClientBlobCache::default();
    let hash = cache.insert(b"ready-hit").expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);
    for x in 0..8 {
        resolver
            .accept_cached_packet(cached_request_level(x, hash))
            .expect("eight completed cached transactions");
    }
    let retained_at_limit = resolver.stats().pending_bytes;
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().retained_cached_transactions, 8);

    let excess = resolver
        .accept_cached_packet(cached_request_level(99, hash))
        .expect("ready-lane excess stays non-fatal");

    assert!(excess.missing.is_empty() && excess.have.is_empty());
    assert!(excess.recovery.is_some());
    assert_eq!(resolver.stats().retained_cached_transactions, 8);
    assert_eq!(resolver.stats().pending_bytes, retained_at_limit);
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
