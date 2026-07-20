use super::*;

#[test]
fn empty_miss_response_is_tolerated_and_drops_outstanding_pending_work() {
    let missing_hash = client_blob_hash(b"never-delivered");
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    let status = resolver
        .accept_cached_packet(cached_request_level(1, missing_hash))
        .expect("authorize the outstanding miss");
    assert_eq!(status.missing, vec![missing_hash]);
    assert_eq!(resolver.stats().pending_transactions, 1);

    // A server that answers a blob status with zero blobs cannot satisfy the
    // outstanding miss. That is degenerate, but it must not fail the session:
    // the stuck cached transaction is dropped and decoding continues.
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket { blobs: Vec::new() })
        .expect("an empty miss response is recoverable, not fatal");

    assert!(resolver.pop_ready().is_none());
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_resets, 1);
}

#[test]
fn spurious_empty_miss_response_without_pending_work_is_a_no_op() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket { blobs: Vec::new() })
        .expect("an unsolicited empty miss response is ignored, not fatal");

    assert!(resolver.pop_ready().is_none());
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_resets, 0);
}
