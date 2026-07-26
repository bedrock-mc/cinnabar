use super::*;

#[test]
fn empty_miss_response_is_a_noop_that_leaves_cached_work_untouched() {
    let payload = b"empty-response-wanted";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(cached_request_level(21, hash))
        .expect("unresolved cached transaction");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket { blobs: Vec::new() })
        .expect("well-formed empty response is a successful no-op");

    assert_eq!(resolver.stats().empty_miss_responses, 1);
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert!(resolver.pop_ready().is_none());

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect("the original one-shot response still resolves the transaction");
    assert!(matches!(
        pop_packet(&mut resolver, "resolved cached transaction").data,
        McpePacketData::PacketLevelChunk(_)
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
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect("well-formed late response is skipped without disconnecting");
    assert!(!resolver.cache().contains(hash));
    assert_eq!(resolver.stats().miss_response_unsolicited, 1);
}
