use super::*;

#[test]
fn dropping_resolver_releases_pending_pins_for_other_resolvers() {
    let mut bounded = limits(1, 8);
    bounded.max_blob_bytes = 8;
    bounded.max_pending_bytes = 4_096;
    let cache = ClientBlobCache::with_limits(bounded);
    let pinned = cache.insert(b"pinned").expect("seed pinned entry");
    {
        let mut resolver = BlobCacheResolver::new(cache.clone());
        let missing = client_blob_hash(b"missing");
        resolver
            .accept_cached_packet(cached_level(vec![pinned, missing, pinned], b""))
            .expect("pending transaction pins hit");
    }

    let replacement = cache
        .insert(b"replace")
        .expect("Drop releases the old resolver's pin");
    assert!(cache.contains(replacement));
    assert!(!cache.contains(pinned));
}

#[test]
fn fast_transfer_candidate_retires_only_unresolved_cached_hol_work() {
    let old_payload = b"old-backend-column";
    let old_hash = client_blob_hash(old_payload);
    let cache = ClientBlobCache::default();
    let mut resolver = BlobCacheResolver::new(cache.clone());
    resolver
        .accept_cached_packet(cached_request_level(1, old_hash))
        .expect("old unresolved cached head");
    resolver
        .accept_passthrough(SetTimePacket { time: 42 }.into(), 32)
        .expect("ordinary event queues behind old head");

    resolver.arm_fast_transfer_rotation();
    assert!(
        resolver
            .rotate_pending_for_fast_transfer_candidate()
            .expect("selective rotation")
    );

    let ordinary = pop_packet(&mut resolver, "ordinary event survives rotation");
    assert!(matches!(ordinary.data, McpePacketData::PacketSetTime(_)));
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(!cache.contains(old_hash));

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: old_hash,
                payload: old_payload.to_vec(),
            }],
        })
        .expect("late retired response remains authorized and cacheable");
    assert!(cache.contains(old_hash));
    assert!(
        resolver.pop_ready().is_none(),
        "dropped packet is not rebuilt"
    );
}

#[test]
fn armed_rotation_is_harmless_when_old_response_wins_the_race() {
    let payload = b"old-response-first";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(1, hash))
        .expect("old unresolved transaction");
    resolver.arm_fast_transfer_rotation();
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect("old response resolves normally");

    assert!(
        !resolver
            .rotate_pending_for_fast_transfer_candidate()
            .expect("resolved work needs no retirement")
    );
    let packet = pop_packet(&mut resolver, "resolved old transaction is preserved");
    assert!(matches!(packet.data, McpePacketData::PacketLevelChunk(_)));
}

#[test]
fn retired_generation_does_not_admit_unrelated_blobs() {
    let old_hash = client_blob_hash(b"authorized-old");
    let unsolicited_payload = b"not-authorized";
    let unsolicited_hash = client_blob_hash(unsolicited_payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(1, old_hash))
        .expect("authorize old miss");
    resolver.arm_fast_transfer_rotation();
    resolver
        .rotate_pending_for_fast_transfer_candidate()
        .expect("retire old miss");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: unsolicited_hash,
                payload: unsolicited_payload.to_vec(),
            }],
        })
        .expect("unrelated well-formed response is skipped");
    assert!(!resolver.cache().contains(unsolicited_hash));
    assert_eq!(resolver.stats().skipped_miss_responses, 1);
}

#[test]
fn empty_miss_response_retires_the_blocked_head_and_keeps_late_resolution_bounded() {
    let payload = b"empty-response-wanted";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(21, hash))
        .expect("unresolved cached FIFO head");
    resolver
        .accept_passthrough(SetTimePacket { time: 42 }.into(), 32)
        .expect("ordinary packet queues behind the cached head");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket { blobs: Vec::new() })
        .expect("well-formed empty response is a recoverable skip");

    assert_eq!(resolver.stats().skipped_miss_responses, 1);
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert_eq!(resolver.stats().retired_cached_transactions, 1);
    let ordinary = pop_packet(&mut resolver, "ordinary FIFO work after retirement");
    assert!(matches!(ordinary.data, McpePacketData::PacketSetTime(_)));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 21, .. }
        )))
    ));

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect("late valid response remains authorized and cacheable");
    assert!(resolver.cache().contains(hash));
    assert_eq!(resolver.stats().pending_transactions, 0);
}

#[test]
fn wholly_unmatched_miss_response_is_skipped_without_poisoning_or_stalling_fifo() {
    let wanted = b"unmatched-response-wanted";
    let wanted_hash = client_blob_hash(wanted);
    let unsolicited = b"unmatched-response-unsolicited";
    let unsolicited_hash = client_blob_hash(unsolicited);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(22, wanted_hash))
        .expect("unresolved cached FIFO head");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: unsolicited_hash,
                payload: unsolicited.to_vec(),
            }],
        })
        .expect("well-formed unmatched response is a recoverable skip");

    assert_eq!(resolver.stats().skipped_miss_responses, 1);
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().retired_cached_transactions, 1);
    assert!(!resolver.cache().contains(unsolicited_hash));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 22, .. }
        )))
    ));

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: wanted_hash,
                payload: wanted.to_vec(),
            }],
        })
        .expect("late requested blob remains boundedly admissible");
    assert!(resolver.cache().contains(wanted_hash));
}

#[test]
fn well_formed_invalid_blob_content_is_rejected_without_ending_the_session() {
    let payload = b"integrity-response-wanted";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(23, hash))
        .expect("unresolved cached FIFO head");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: b"integrity-response-poison".to_vec(),
            }],
        })
        .expect("well-formed invalid content is rejected without a session error");

    assert_eq!(resolver.stats().skipped_miss_responses, 1);
    assert_eq!(resolver.stats().rejected_blobs, 1);
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().retired_cached_transactions, 1);
    assert!(!resolver.cache().contains(hash));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(
            ChunkResyncEvent { x: 23, .. }
        )))
    ));

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect("late valid payload remains authorized after bounded retirement");
    assert!(resolver.cache().contains(hash));
}

#[test]
fn fast_transfer_rotation_preserves_ready_prefix_and_releases_retired_pins() {
    let hit_payload = b"verified-hit";
    let hit_hash = client_blob_hash(hit_payload);
    let missing_hash = client_blob_hash(b"missing-after-ready");
    let cache = ClientBlobCache::with_limits(limits(1, 256));
    cache.insert(hit_payload).expect("seed verified hit");
    let mut resolver = BlobCacheResolver::new(cache.clone());
    resolver
        .accept_cached_packet(cached_level(vec![hit_hash, hit_hash, hit_hash], b"ready"))
        .expect("ready prefix");
    resolver
        .accept_cached_packet(cached_level(
            vec![hit_hash, missing_hash, hit_hash],
            b"retired",
        ))
        .expect("unresolved transaction pins verified hit");

    resolver.arm_fast_transfer_rotation();
    assert!(
        resolver
            .rotate_pending_for_fast_transfer_candidate()
            .expect("rotate unresolved work")
    );
    assert!(cache.contains(hit_hash), "verified cache content survives");
    let ready = pop_packet(&mut resolver, "ready prefix survives");
    assert!(matches!(ready.data, McpePacketData::PacketLevelChunk(_)));

    let replacement = b"replacement-entry";
    let replacement_hash = cache
        .insert(replacement)
        .expect("retired pins were released");
    assert!(cache.contains(replacement_hash));
    assert!(!cache.contains(hit_hash), "released hit is now evictable");
}

#[test]
fn invalid_late_retired_response_is_atomic_and_keeps_generation_resolvable() {
    let payload = b"retired-authorized";
    let hash = client_blob_hash(payload);
    let cache = ClientBlobCache::default();
    let mut resolver = BlobCacheResolver::new(cache.clone());
    resolver
        .accept_cached_packet(cached_request_level(1, hash))
        .expect("authorize retired miss");
    resolver.arm_fast_transfer_rotation();
    resolver
        .rotate_pending_for_fast_transfer_candidate()
        .expect("retire old transaction");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
                blobs: vec![Blob {
                    hash,
                    payload: b"wrong-payload".to_vec(),
                }],
            })
        .expect("invalid well-formed response is skipped");
    assert!(!cache.contains(hash));
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect("retired authorization survives an invalid response");
    assert!(cache.contains(hash));
}

#[test]
fn active_and_retired_same_hash_authorizations_resolve_independently() {
    let payload = b"shared-generation-hash";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(1, hash))
        .expect("old generation");
    resolver.arm_fast_transfer_rotation();
    resolver
        .rotate_pending_for_fast_transfer_candidate()
        .expect("retire old generation");
    resolver
        .accept_cached_packet(cached_request_level(2, hash))
        .expect("new active generation");

    let response = || ClientCacheMissResponsePacket {
        blobs: vec![Blob {
            hash,
            payload: payload.to_vec(),
        }],
    };
    resolver
        .accept_miss_response(response())
        .expect("active authorization resolves first");
    let active = pop_packet(&mut resolver, "new active packet reconstructed");
    assert!(matches!(active.data, McpePacketData::PacketLevelChunk(_)));
    resolver
        .accept_miss_response(response())
        .expect("retired authorization remains independently consumable");
    assert!(resolver.pop_ready().is_none());
}

#[test]
fn second_rotation_merges_retired_authorizations_with_bounded_accounting() {
    let first_payload = b"first-retired-generation";
    let second_payload = b"second-retired-generation";
    let first_hash = client_blob_hash(first_payload);
    let second_hash = client_blob_hash(second_payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(1, first_hash))
        .expect("first generation");
    resolver.arm_fast_transfer_rotation();
    resolver
        .rotate_pending_for_fast_transfer_candidate()
        .expect("retire first generation");
    resolver
        .accept_cached_packet(cached_request_level(2, second_hash))
        .expect("second generation");
    resolver.arm_fast_transfer_rotation();
    resolver
        .rotate_pending_for_fast_transfer_candidate()
        .expect("retire second and merge authorization generations");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: first_hash,
                payload: first_payload.to_vec(),
            }],
        })
        .expect("prior retired response may arrive before the latest generation");
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: second_hash,
                payload: second_payload.to_vec(),
            }],
        })
        .expect("latest retired generation remains independently authorized");
    assert_eq!(resolver.stats().pending_transactions, 0);
}
