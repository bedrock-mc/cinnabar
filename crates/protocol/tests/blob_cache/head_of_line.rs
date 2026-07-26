use super::*;

#[test]
fn cached_subchunk_pressure_is_skipped_for_the_bounded_request_retry_path() {
    let mut bounded = limits(8, 256);
    bounded.max_pending_transactions = 1;
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(bounded));
    let head_hash = client_blob_hash(b"head-miss");
    resolver
        .accept_cached_packet(cached_request_level(1, head_hash))
        .expect("unresolved cached FIFO head");

    let skipped_hash = client_blob_hash(b"retry-subchunk");
    let status = resolver
        .accept_cached_packet(cached_subchunk(skipped_hash, b"tail"))
        .expect("requested SubChunk pressure must not disconnect");

    assert_eq!(status.missing, vec![skipped_hash]);
    assert!(status.have.is_empty());
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert!(resolver.stats().pending_bytes <= bounded.max_pending_bytes);
    assert_eq!(resolver.stats().skipped_cached_packets, 1);

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: skipped_hash,
                payload: b"retry-subchunk".to_vec(),
            }],
        })
        .expect("a response for a skipped packet is a recoverable unsolicited skip");
    assert!(!resolver.cache().contains(skipped_hash));
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert_eq!(resolver.stats().skipped_miss_responses, 1);
    assert_eq!(resolver.stats().retired_cached_transactions, 0);
}

#[test]
fn hash_free_world_event_is_ready_while_cached_transaction_is_unresolved() {
    let mut bounded = limits(8, 256);
    bounded.max_pending_transactions = 1;
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(bounded));
    resolver
        .accept_cached_packet(cached_request_level(
            1,
            client_blob_hash(b"unresolved-cache-transaction"),
        ))
        .expect("unresolved cached transaction");

    resolver
        .accept_world_event(WorldEvent::SetTime(SetTimeEvent { time: 42 }), 8)
        .expect("hash-free event is independent of cache pressure");

    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::SetTime(
            SetTimeEvent { time: 42 }
        )))
    ));
    assert_eq!(resolver.stats().pending_transactions, 1);
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
