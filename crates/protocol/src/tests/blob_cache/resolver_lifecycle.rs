use super::*;

/// A late reply for retired terrain must not discard other columns in the same response.
#[test]
fn late_blob_in_a_mixed_response_preserves_needed_columns() {
    let retired = b"retired-column";
    let wanted = b"current-column";
    let retired_hash = client_blob_hash(retired);
    let wanted_hash = client_blob_hash(wanted);
    for late_first in [false, true] {
        let cache = ClientBlobCache::default();
        let mut resolver = BlobCacheResolver::new(cache.clone());
        resolver
            .accept_cached_packet(cached_request_level(1, retired_hash))
            .unwrap();
        resolver.arm_fast_transfer_reset();
        assert!(
            resolver
                .reset_pending_for_fast_transfer_candidate()
                .unwrap()
        );
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(event))) if event.x == 1
        ));
        resolver
            .accept_cached_packet(cached_request_level(2, wanted_hash))
            .unwrap();
        let mut blobs = vec![
            (wanted_hash, wanted.to_vec()),
            (retired_hash, retired.to_vec()),
        ];
        if late_first {
            blobs.reverse();
        }
        resolver.accept_miss_response(miss_response(blobs)).unwrap();

        let packet = pop_packet(&mut resolver, "the still-needed column resolves");
        assert!(matches!(*packet.data, packets::Packet::LevelChunk(packet)
            if packet.chunk_position.x.0 == 2));
        assert!(cache.contains(wanted_hash));
        assert!(!cache.contains(retired_hash));
        assert!(resolver.pop_ready().is_none());
        assert_eq!(resolver.stats().abandoned_cached_transactions, 0);
        assert_eq!(resolver.stats().recovery_requests, 1);
        assert_eq!(resolver.stats().miss_response_unsolicited, 1);
        assert_eq!(resolver.stats().skipped_miss_responses, 0);
    }
}

#[test]
fn empty_miss_response_is_a_noop_that_leaves_cached_work_untouched() {
    let payload = b"empty-response-wanted";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(21, hash))
        .expect("unresolved cached transaction");

    resolver
        .accept_miss_response(miss_response(Vec::new()))
        .expect("well-formed empty response is a successful no-op");

    assert_eq!(resolver.stats().empty_miss_responses, 1);
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert!(resolver.pop_ready().is_none());

    resolver
        .accept_miss_response(miss_response(vec![(hash, payload.to_vec())]))
        .expect("the original one-shot response still resolves the transaction");
    assert!(matches!(
        *pop_packet(&mut resolver, "resolved cached transaction").data,
        packets::Packet::LevelChunk(_)
    ));
}

#[test]
fn fast_transfer_reset_does_not_authorize_a_late_prior_backend_response() {
    let payload = b"prior-backend";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(1, hash))
        .expect("prior backend transaction");
    resolver.arm_fast_transfer_reset();
    assert!(
        resolver
            .reset_pending_for_fast_transfer_candidate()
            .expect("confirmed transfer boundary resets cached work")
    );

    resolver
        .accept_miss_response(miss_response(vec![(hash, payload.to_vec())]))
        .expect("well-formed late response is skipped without disconnecting");
    assert!(!resolver.cache().contains(hash));
    assert_eq!(resolver.stats().miss_response_unsolicited, 1);
}

#[test]
fn semantic_reset_recovers_unresolved_subchunk_admission() {
    let payload = b"pending-reset-recovery";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_subchunk(hash, payload))
        .expect("retain unresolved cached SubChunk");

    resolver
        .recover_pending()
        .expect("semantic reset preserves admission rollback");

    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().recovery_ready_events, 2);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
    ));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
    ));
}

#[test]
fn semantic_reset_recovers_reconstructed_unpublished_subchunk() {
    let payload = b"ready-reset-recovery";
    let cache = ClientBlobCache::default();
    let hash = cache.insert(payload).expect("seed reconstructed blob");
    let mut resolver = BlobCacheResolver::new(cache);
    resolver
        .accept_cached_packet(cached_subchunk(hash, payload))
        .expect("reconstruct cached SubChunk before publication");
    assert_eq!(resolver.stats().retained_cached_transactions, 1);

    resolver
        .recover_pending()
        .expect("semantic reset recovers reconstructed cached SubChunk");

    assert_eq!(resolver.stats().retained_cached_transactions, 0);
    assert_eq!(resolver.stats().recovery_ready_events, 2);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
    ));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
    ));
}
