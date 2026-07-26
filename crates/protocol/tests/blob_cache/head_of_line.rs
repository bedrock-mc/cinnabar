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
        .expect("truthfully requested pressure miss remains solicited");
    assert!(resolver.cache().contains(skipped_hash));
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert_eq!(resolver.stats().skipped_miss_responses, 0);
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
        .accept_world_event(WorldEvent::SetTime(SetTimeEvent { time: 42 }), usize::MAX)
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
        .accept_cached_packet(cached_level(vec![ah, ah, ah], b"first"))
        .expect("first transaction");
    resolver
        .accept_cached_packet(cached_level(vec![bh, bh, bh], b"second"))
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
