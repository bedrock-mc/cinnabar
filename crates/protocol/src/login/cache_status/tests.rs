use super::*;
use crate::{BlobCacheResolver, ClientBlobCache, Packet};
use futures::task::noop_waker;

/// Uses the real resolver so tests cannot invent or shorten sealed classifications.
fn classify(cache: &ClientBlobCache, hashes: Vec<u64>) -> BlobCacheStatus {
    BlobCacheResolver::new(cache.clone())
        .accept_cached_packet(Packet::new(crate::login::tests::cached_level_chunk(hashes)))
        .unwrap()
}

/// Creates a monotonic observation without reading a clock.
fn at(nanos: u64) -> Moment {
    Moment::from_elapsed(Duration::from_nanos(nanos))
}

#[test]
fn exclusion_is_strict_and_empty_opportunities_advance_the_deadline() {
    let mut outbox = CacheStatusOutbox::default();
    assert!(outbox.select(at(0)).is_none());
    let cache = ClientBlobCache::default();
    outbox.push(classify(&cache, vec![1]));
    let deadline = STATUS_EXCLUSION.as_nanos() as u64;
    assert!(outbox.select(at(deadline)).is_none());
    let packet = outbox.select(at(deadline + 1)).unwrap();
    assert_eq!(packet.missing_ids, [wire::U64LE(1)]);
    assert!(packet.found_ids.is_empty());
    assert!(outbox.select(at(deadline + 1)).is_none());
    outbox.push(classify(&cache, vec![2]));
    assert!(outbox.select(at(2 * deadline + 1)).is_none());
    assert_eq!(
        outbox.select(at(2 * deadline + 2)).unwrap().missing_ids,
        [wire::U64LE(2)]
    );
}

#[test]
fn global_miss_priority_precedes_earlier_hits_with_one_packet_per_opportunity() {
    let cache = ClientBlobCache::default();
    let hits: Vec<_> = (0_u32..4000)
        .map(|i| cache.insert(&i.to_le_bytes()).unwrap())
        .collect();
    let missing: Vec<_> = (0_u32..200)
        .map(|i| crate::client_blob_hash(&i.to_be_bytes().repeat(2)))
        .collect();
    assert!(missing.iter().all(|hash| !cache.contains(*hash)));
    let mut outbox = CacheStatusOutbox::default();
    outbox.push(classify(&cache, hits.clone()));
    outbox.push(classify(&cache, missing.clone()));
    let packet = outbox.select(at(0)).unwrap();
    let hit_count = MAX_CLIENT_BLOB_HASHES_PER_PACKET - missing.len();
    assert_eq!(
        packet.missing_ids,
        missing.into_iter().map(wire::U64LE).collect::<Vec<_>>()
    );
    assert_eq!(
        packet.found_ids,
        hits[..hit_count]
            .iter()
            .copied()
            .map(wire::U64LE)
            .collect::<Vec<_>>()
    );
    assert!(outbox.select(at(1)).is_none());
    let rest = outbox
        .select(Moment::from_elapsed(
            STATUS_EXCLUSION + Duration::from_nanos(1),
        ))
        .unwrap();
    assert!(rest.missing_ids.is_empty());
    assert_eq!(
        rest.found_ids,
        hits[hit_count..]
            .iter()
            .copied()
            .map(wire::U64LE)
            .collect::<Vec<_>>()
    );
}

#[test]
fn sealed_repeats_and_an_earlier_miss_survive_later_cache_changes() {
    let cache = ClientBlobCache::default();
    let hash = crate::client_blob_hash(b"later");
    let mut resolver = BlobCacheResolver::new(cache.clone());
    let mut outbox = CacheStatusOutbox::default();
    let missing = resolver
        .accept_cached_packet(Packet::new(crate::login::tests::cached_level_chunk(vec![
            hash, hash,
        ])))
        .unwrap();
    assert_eq!(missing.missing(), [hash]);
    outbox.push(missing);
    let mut second = crate::login::tests::cached_level_chunk(vec![hash]);
    second.chunk_position.x = wire::ZigZag32(1);
    let outstanding = resolver.accept_cached_packet(Packet::new(second)).unwrap();
    assert!(outstanding.missing().is_empty());
    outbox.push(outstanding);
    assert_eq!(cache.insert(b"later").unwrap(), hash);
    outbox.push(classify(&cache, vec![hash, hash]));
    outbox.push(classify(&cache, vec![hash]));
    let packet = outbox.select(at(0)).unwrap();
    assert_eq!(packet.missing_ids, [wire::U64LE(hash)]);
    assert_eq!(packet.found_ids, [wire::U64LE(hash), wire::U64LE(hash)]);
}

#[test]
fn full_intake_waits_for_selection_or_reset_instead_of_dropping_values() {
    let mut outbox = CacheStatusOutbox::default();
    let hashes: Vec<_> = (0..MAX_CLIENT_BLOB_HASHES_PER_PACKET as u64).collect();
    let status = classify(&ClientBlobCache::default(), hashes.clone());
    let count = MAX_CLIENT_BLOB_PENDING_BYTES / size_of::<u64>();
    for _ in (0..count).step_by(hashes.len()) {
        outbox.push(status.clone());
    }
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(outbox.poll_capacity(&mut cx).is_pending());
    let packet = outbox.select(at(0)).unwrap();
    assert_eq!(
        packet.missing_ids,
        hashes.into_iter().map(wire::U64LE).collect::<Vec<_>>()
    );
    assert!(outbox.poll_capacity(&mut cx).is_ready());
    outbox.reset();
    assert!(outbox.poll_capacity(&mut cx).is_ready());
    assert!(
        outbox.select(at(0)).is_none(),
        "reset clears both values and deadline"
    );
}
