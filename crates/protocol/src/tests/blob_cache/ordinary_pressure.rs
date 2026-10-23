use super::*;

/// Retains a cached column until its payload arrives and returns the requested hash.
fn retain_column(resolver: &mut BlobCacheResolver, x: i32, z: i32, payload: &[u8]) -> u64 {
    let hash = client_blob_hash(payload);
    let status = resolver
        .accept_cached_packet(protocol::Packet::new(cached_level_chunk(
            x,
            z,
            vec![hash],
            b"",
        )))
        .unwrap();
    assert_eq!(status.missing(), [hash]);
    hash
}

/// Queues one ordinary update on the requested column.
fn retain_update(resolver: &mut BlobCacheResolver, x: i32, z: i32) {
    resolver
        .accept_world_event(
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [x * 16, 0, z * 16],
                layer: 0,
                network_id: 123,
            }]),
            size_of::<BlockUpdateEvent>(),
        )
        .unwrap();
}

#[test]
fn ordinary_drain_preserves_pending_work_newer_than_its_ready_dependency() {
    let cache = ClientBlobCache::default();
    let resident = cache.insert(b"resident").unwrap();
    let mut resolver = BlobCacheResolver::new(cache);
    retain_column(&mut resolver, 4, 9, b"earlier column");
    resolver
        .accept_cached_packet(cached_subchunk_multi_column(resident, b"tail"))
        .unwrap();
    let later_payload = b"later column";
    let later = retain_column(&mut resolver, 7, 13, later_payload);
    retain_update(&mut resolver, 4, 9);

    assert!(resolver.pop_ready().is_none());
    assert!(resolver.unblock_ordinary_lane().unwrap());
    assert_eq!(resolver.stats().abandoned_cached_transactions, 1);
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 4, z: 9, .. }
        )))
    ));
    assert!(matches!(
        &*pop_packet(&mut resolver, "reconstructed terrain").data,
        packets::Packet::SubChunk(_)
    ));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(_)))
    ));
    assert!(resolver.pop_ready().is_none());

    resolver
        .accept_miss_response(miss_response(vec![(later, later_payload.to_vec())]))
        .unwrap();
    assert!(matches!(
        &*pop_packet(&mut resolver, "later pending terrain survives").data,
        packets::Packet::LevelChunk(packet)
            if packet.chunk_position.x.0 == 7 && packet.chunk_position.z.0 == 13
    ));
    assert!(resolver.pop_ready().is_none());
    assert_eq!(resolver.stats().recovery_requests, 1);
}

#[test]
fn ordinary_drain_does_not_follow_ready_work_newer_than_the_update() {
    let cache = ClientBlobCache::default();
    let resident = cache.insert(b"resident").unwrap();
    let mut resolver = BlobCacheResolver::new(cache);
    retain_column(&mut resolver, 7, 13, b"unrelated pending column");
    retain_update(&mut resolver, 4, 9);
    resolver
        .accept_cached_packet(cached_subchunk_multi_column(resident, b"tail"))
        .unwrap();

    assert!(!resolver.unblock_ordinary_lane().unwrap());
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert_eq!(resolver.stats().abandoned_cached_transactions, 0);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(_)))
    ));
    assert!(resolver.pop_ready().is_none());
    assert_eq!(resolver.stats().recovery_requests, 0);
}

#[test]
fn ordinary_drain_preserves_pending_work_newer_than_the_update() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    retain_update(&mut resolver, 4, 9);
    retain_column(&mut resolver, 4, 9, b"later column");

    assert!(!resolver.unblock_ordinary_lane().unwrap());
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert_eq!(resolver.stats().abandoned_cached_transactions, 0);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(_)))
    ));
    assert!(resolver.pop_ready().is_none());
    assert_eq!(resolver.stats().recovery_requests, 0);
}

#[test]
fn ordinary_drain_follows_multiple_earlier_ready_dependencies() {
    let cache = ClientBlobCache::default();
    let resident = cache.insert(b"resident").unwrap();
    let mut resolver = BlobCacheResolver::new(cache);
    retain_column(&mut resolver, 10, 17, b"earlier column");
    // The first reply joins columns B and C; the second joins columns A and B.
    resolver
        .accept_cached_packet(protocol::Packet::new(cached_sub_chunk_packet(
            [(3, 0, 4), (6, 0, 8)]
                .map(|offset| {
                    sub_chunk_entry(
                        offset,
                        SubChunkRequestResult::Success,
                        Some(vec![]),
                        Some(resident),
                    )
                })
                .to_vec(),
        )))
        .unwrap();
    resolver
        .accept_cached_packet(cached_subchunk_multi_column(resident, b"tail"))
        .unwrap();
    retain_column(&mut resolver, 10, 17, b"later column");
    retain_update(&mut resolver, 4, 9);

    assert!(resolver.pop_ready().is_none());
    assert!(resolver.unblock_ordinary_lane().unwrap());
    assert_eq!(resolver.stats().abandoned_cached_transactions, 1);
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 10, z: 17, .. }
        )))
    ));
    for _ in 0..2 {
        assert!(matches!(
            &*pop_packet(&mut resolver, "earlier reconstructed terrain").data,
            packets::Packet::SubChunk(_)
        ));
    }
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(_)))
    ));
    assert!(resolver.pop_ready().is_none());
    assert_eq!(resolver.stats().recovery_requests, 1);
}

#[test]
fn ordinary_drain_keeps_the_latest_bound_for_each_column() {
    let cache = ClientBlobCache::default();
    let resident = cache.insert(b"resident").unwrap();
    let mut resolver = BlobCacheResolver::new(cache);
    retain_column(&mut resolver, 7, 13, b"earlier column");
    resolver
        .accept_cached_packet(cached_subchunk_multi_column(resident, b"tail"))
        .unwrap();
    retain_update(&mut resolver, 4, 9);
    retain_column(&mut resolver, 7, 13, b"later column");
    retain_update(&mut resolver, 7, 13);
    retain_update(&mut resolver, 4, 9);

    assert!(resolver.pop_ready().is_none());
    assert!(resolver.unblock_ordinary_lane().unwrap());
    assert_eq!(resolver.stats().abandoned_cached_transactions, 2);
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 7, z: 13, .. }
        )))
    ));
    assert!(matches!(
        &*pop_packet(&mut resolver, "reconstructed terrain").data,
        packets::Packet::SubChunk(_)
    ));
    for (x, z) in [(4, 9), (7, 13), (4, 9)] {
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(updates)))
                if updates[0].position == [x * 16, 0, z * 16]
        ));
    }
    assert!(resolver.pop_ready().is_none());
}
