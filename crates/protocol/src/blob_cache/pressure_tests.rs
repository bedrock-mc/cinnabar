use super::*;

/// Pins charge existing and arriving payload once, regardless of reference count.
#[test]
fn distinct_pin_accounting_survives_duplicate_pins_and_missing_arrivals() {
    let cache = ClientBlobCache::default();
    let hit = cache.insert(b"hit").unwrap();
    let miss = client_blob_hash(b"miss");
    cache.classify(&[hit, miss], true);
    cache.classify(&[hit, miss], true);
    assert_eq!(cache.lock().pinned_bytes, 3);
    cache.insert(b"miss").unwrap();
    cache.insert(b"miss").unwrap();
    assert_eq!(cache.lock().pinned_bytes, 7);
    cache.unpin_all(&[hit, miss]);
    assert_eq!(cache.lock().pinned_bytes, 7);
    cache.unpin_all(&[hit, miss]);
    assert_eq!(cache.lock().pinned_bytes, 0);
}

/// Unpin service completes pressure trimming without a subsequent insert.
#[test]
fn releasing_pressure_pins_trims_without_touching_still_pinned_payload() {
    let cache = ClientBlobCache::with_limits(BlobCacheLimits {
        trim_trigger_bytes: 10,
        trim_floor_bytes: 8,
    });
    let first = client_blob_hash(b"first!");
    let second = client_blob_hash(b"second");
    cache.classify(&[first, second], true);
    cache.insert(b"first!").unwrap();
    cache.insert(b"second").unwrap();
    assert_eq!(cache.total_bytes(), 12);
    cache.unpin_all(&[first]);
    assert_eq!(cache.total_bytes(), 6);
    assert_eq!(cache.lock().pinned_bytes, 6);
    assert!(cache.contains(second));
    assert!(!cache.contains(first));
}

/// Aggregate pressure recovers only the offending response's column.
#[test]
fn response_pin_pressure_preserves_other_pending_columns() {
    exercise_aggregate_pin_pressure(false);
}

/// Existing cache hits also respect the global ceiling when they become pinned.
#[test]
fn classification_pin_pressure_releases_rejected_transaction_pins() {
    exercise_aggregate_pin_pressure(true);
}

/// Keeps two incomplete transactions and pushes a third past the shared payload ceiling.
fn exercise_aggregate_pin_pressure(preseed: bool) {
    let cache = ClientBlobCache::default();
    let mut resolver = BlobCacheResolver::new(cache.clone());
    let payload_len = MAX_CLIENT_BLOB_PINNED_BYTES / 3 + 1;
    assert!(payload_len < MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION);
    for x in 0..3 {
        let payload = vec![x as u8; payload_len];
        let hash = client_blob_hash(&payload);
        if preseed {
            cache.insert(&payload).unwrap();
        }
        let mut status = resolver
            .accept_cached_packet(crate::Packet::new(super::tests::cached_level_chunk(
                x,
                vec![hash, u64::MAX - x as u64],
            )))
            .unwrap();
        if preseed && x == 2 {
            assert_eq!(status.take_recovery().map(|event| event.x), Some(x));
        } else if !preseed {
            resolver
                .accept_miss_response(ClientCacheMissResponse {
                    missing_blobs: vec![MissingBlobData {
                        blob_id: wire::U64LE(hash),
                        blob_data: payload.into(),
                    }],
                })
                .unwrap();
            if x == 2 {
                assert!(!cache.contains(hash));
                let Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(event))) =
                    resolver.pop_ready()
                else {
                    panic!("pressure emits exact recovery")
                };
                assert_eq!(event.x, x);
                assert_eq!(resolver.stats().miss_response_cache_pressure, 1);
            }
        }
        assert!(resolver.stats().cache_pinned_bytes <= MAX_CLIENT_BLOB_PINNED_BYTES);
    }
    assert_eq!(resolver.stats().pending_transactions, 2);
    assert_eq!(resolver.stats().cache_pinned_bytes, 2 * payload_len);
    drop(resolver);
    assert_eq!(cache.lock().pinned_bytes, 0);
    assert!(cache.lock().pins.is_empty());
}
