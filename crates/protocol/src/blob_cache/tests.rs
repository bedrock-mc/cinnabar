use super::*;

#[test]
fn private_level_chunk_ready_lane_aliases_bytes_while_public_pop_returns_independent_vec() {
    let event = crate::LevelChunkEvent {
        dimension: 0,
        x: 1,
        z: 2,
        mode: crate::LevelChunkMode::Inline { count: 0 },
        payload: Vec::new(),
    };
    let payload = bytes::Bytes::from(vec![0x7c; 1024 * 1024]);
    let pointer = payload.as_ptr();
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_level_chunk_bytes(event.clone(), payload, 1024 * 1024)
        .expect("admit private LevelChunk bytes");
    let Some(ResolverReady::LevelChunkBytes(_, payload)) = resolver.pop_ready_ingress() else {
        panic!("private pop must retain the byte-bearing variant")
    };
    assert_eq!(payload.as_ptr(), pointer);

    resolver
        .accept_level_chunk_bytes(event, payload, 1024 * 1024)
        .expect("readmit private LevelChunk bytes");
    let Some(BlobCacheReady::WorldEvent(WorldEvent::LevelChunk(legacy))) = resolver.pop_ready()
    else {
        panic!("public pop must preserve the legacy WorldEvent surface")
    };
    assert_ne!(legacy.payload.as_ptr(), pointer);
    assert_eq!(legacy.payload, vec![0x7c; 1024 * 1024]);
}

/// Builds a cache-enabled LevelChunk for one column referencing `hashes`.
///
/// 1.26.40 writes the blob hashes unconditionally and states cache
/// participation with `cache_enabled`, so the protocol-1001 `blobs: Some(..)`
/// literal has no direct equivalent. `subchunks_count` is 0 because
/// gophertunnel's packet/level_chunk.go expects SubChunkCount + 1 hashes and
/// these fixtures carry exactly one; the old -1 request-mode sentinel is gone.
fn cached_level_chunk(x: i32, hashes: Vec<u64>) -> LevelChunkPacket {
    let subchunks_count = i32::try_from(hashes.len().saturating_sub(1)).expect("fixture count");
    LevelChunkPacket {
        chunk_position: valentine::bedrock::version::v1_26_40::ChunkPos { x, z: 0 },
        subchunks_count,
        cache_enabled: true,
        cache_metadata: hashes
            .into_iter()
            .map(|blob_id| {
                valentine::bedrock::version::v1_26_40::LevelChunkPacketPayloadSubChunkMetadata {
                    blob_id,
                }
            })
            .collect(),
        ..Default::default()
    }
}

/// Builds one sub-chunk entry at the given offset.
fn sub_chunk_entry(
    dx: i8,
    dy: i8,
    dz: i8,
    result: SubChunkRequestResult,
    payload: Option<Vec<u8>>,
    blob_id: Option<u64>,
) -> SubChunkPacketPayloadSubChunkPacketData {
    SubChunkPacketPayloadSubChunkPacketData {
        sub_chunk_pos_offset:
            valentine::bedrock::version::v1_26_40::SubChunkPacketPayloadSubChunkPosOffset {
                subchunk_offset_x: dx,
                subchunk_offset_y: dy,
                subchunk_offset_z: dz,
            },
        sub_chunk_request_result: result,
        serialized_sub_chunk: payload,
        blob_id,
        ..Default::default()
    }
}

#[test]
fn ready_subchunk_accounting_uses_retained_entry_and_payload_capacities() {
    let mut payload = Vec::with_capacity(4_096);
    payload.push(0x5a);
    let mut entries = Vec::with_capacity(16);
    entries.push(sub_chunk_entry(
        0,
        0,
        0,
        SubChunkRequestResult::Success,
        Some(payload),
        None,
    ));
    let expected = size_of::<SubChunkPacket>()
        + entries.capacity() * size_of::<SubChunkPacketPayloadSubChunkPacketData>()
        + entries[0]
            .serialized_sub_chunk
            .as_ref()
            .expect("fixture payload")
            .capacity();
    let value = BlobCacheReady::Packet(
        SubChunkPacket {
            cache_enabled: false,
            sub_chunk_data: entries,
            ..Default::default()
        }
        .into(),
    );

    assert_eq!(
        ready_value_accounted_bytes(&ResolverReady::from(value)),
        Ok(expected)
    );
}

#[test]
fn pending_queue_high_water_is_exact_and_reset_releases_backing_allocations() {
    let cache = ClientBlobCache::default();
    let mut resolver = BlobCacheResolver::new(cache);
    for x in 0..8 {
        let payload = [x as u8];
        let hash = client_blob_hash(&payload);
        resolver
            .accept_cached_packet(cached_level_chunk(x, vec![hash]).into())
            .expect("grow unresolved pending queue");
    }
    assert!(resolver.pending.capacity() >= 8);
    assert_eq!(
        resolver.stats.pending_bytes,
        resolver
            .retained_cached_bytes()
            .expect("exact retained bytes")
    );

    resolver.reset_pending();

    assert_eq!(resolver.pending.capacity(), 0);
    assert!(resolver.ready.is_empty());
    assert_eq!(resolver.pending_by_hash.capacity(), 0);
    assert_eq!(resolver.stats.pending_bytes, 0);
}

#[test]
fn pending_accounting_includes_owned_hash_capacity() {
    let hashes = vec![
        client_blob_hash(b"owned-0"),
        client_blob_hash(b"owned-1"),
        client_blob_hash(b"owned-2"),
    ];
    let mut payload = Vec::with_capacity(17);
    payload.extend_from_slice(b"retained-payload");
    let packet = LevelChunkPacket {
        serialized_chunk_data: payload,
        ..cached_level_chunk(0, hashes.clone())
    };
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(packet.into())
        .expect("retain the unresolved transaction");

    let transaction = resolver
        .pending
        .values()
        .next()
        .expect("the missing hashes remain pending");
    let PendingPacket::LevelChunk(packet) = &transaction.packet else {
        panic!("expected a pending LevelChunk");
    };
    let packet_bytes = size_of::<LevelChunkPacket>()
        + packet.serialized_chunk_data.capacity()
        + packet.cache_metadata.capacity() * size_of::<u64>();
    let expected = packet_bytes
        + transaction.hashes.capacity() * size_of::<u64>()
        + transaction.unique_hashes.capacity() * size_of::<u64>()
        + transaction.owned_hashes.capacity() * size_of::<u64>();
    assert_eq!(transaction.accounted_bytes, expected);
    assert_eq!(transaction.owned_hashes, hashes);
}

#[test]
fn recovery_index_has_deterministically_bounded_ordering_work() {
    const RECOVERY_COUNT: usize = 4_096;
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    RECOVERY_ORDER_COMPARISONS.store(0, AtomicOrdering::Relaxed);

    for x in 0..RECOVERY_COUNT {
        resolver.enqueue_recovery(ChunkResyncEvent {
            dimension: 0,
            x: i32::try_from(x).expect("test coordinate fits"),
            z: 0,
            requested_sub_chunks: None,
            requested_sub_chunk_ys: None,
        });
    }

    let comparisons = RECOVERY_ORDER_COMPARISONS.load(AtomicOrdering::Relaxed);
    assert_eq!(resolver.recovery_ready.len(), RECOVERY_COUNT);
    assert!(
        comparisons < RECOVERY_COUNT * 64,
        "recovery index ordering work grew unexpectedly: {comparisons} comparisons"
    );
}

#[test]
fn classify_and_pin_is_one_cache_operation() {
    let limits = BlobCacheLimits {
        trim_trigger_bytes: 4,
        trim_floor_bytes: 3,
    };
    let cache = ClientBlobCache::with_limits(limits);
    let hit = cache.insert(b"hit").expect("seed hit");
    let miss = client_blob_hash(b"miss");

    let (have, missing, staged_bytes) = cache.classify(&[hit, miss], true);

    assert_eq!(have, vec![hit]);
    assert_eq!(missing, vec![miss]);
    assert_eq!(staged_bytes, 3);
    let new = cache
        .insert(b"new")
        .expect("cache pressure never refuses an insert");
    assert!(cache.contains(hit), "reported hit is already pinned");
    assert!(
        cache.contains(new),
        "the triggering insert remains admitted"
    );
    cache.unpin_all(&[hit, miss]);
}

#[test]
fn arriving_blob_visits_only_transactions_in_its_hash_index_bucket() {
    let shared_payload = b"shared-index";
    let shared = client_blob_hash(shared_payload);
    let other = client_blob_hash(b"other-index");
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    for (x, hash) in [(1, shared), (2, shared), (3, other)] {
        resolver
            .accept_cached_packet(cached_level_chunk(x, vec![hash]).into())
            .expect("index pending transaction by its missing hash");
    }
    assert_eq!(resolver.pending_by_hash[&shared].len(), 2);
    assert_eq!(resolver.pending_by_hash[&other].len(), 1);

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            missing_blobs: vec![valentine::bedrock::version::v1_26_40::MissingBlobData {
                blob_id: shared,
                blob_data: shared_payload.to_vec(),
            }],
        })
        .expect("resolve only the shared index bucket");

    assert!(!resolver.pending_by_hash.contains_key(&shared));
    assert_eq!(resolver.pending_by_hash[&other].len(), 1);
    assert_eq!(resolver.pending.len(), 1);
    assert_eq!(resolver.ready.len(), 2);
}

#[test]
fn retained_cached_subchunk_emits_admission_before_reconstruction() {
    let payload = b"admitted-subchunk";
    let hash = client_blob_hash(payload);
    let packet = SubChunkPacket {
        cache_enabled: true,
        dimension_type: valentine::bedrock::version::v1_26_40::DimensionType { value: 2 },
        center_pos: valentine::bedrock::version::v1_26_40::SubChunkPos {
            subchunk_position_x: 4,
            subchunk_position_y: -4,
            subchunk_position_z: 9,
        },
        sub_chunk_data: vec![
            sub_chunk_entry(
                0,
                1,
                -1,
                SubChunkRequestResult::Success,
                Some(payload.to_vec()),
                Some(hash),
            ),
            sub_chunk_entry(1, 2, 0, SubChunkRequestResult::SuccessAllAir, None, None),
        ],
    }
    .into();
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    let mut status = resolver
        .accept_cached_packet(packet)
        .expect("retain cached SubChunk while waiting for its miss");

    assert_eq!(
        status.take_admission(),
        Some(crate::SubChunkReplyAdmissionEvent {
            dimension: 2,
            positions: vec![[4, -3, 8], [5, -2, 9]],
        })
    );
    assert!(
        resolver.pop_ready().is_none(),
        "reconstructed SubChunks remain behind the unresolved miss"
    );
}

#[test]
fn pressure_rotated_cached_subchunk_has_recovery_without_admission() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    for x in 0..MAX_CLIENT_BLOB_PENDING_TRANSACTIONS {
        let payload = format!("pressure-{x}");
        let hash = client_blob_hash(payload.as_bytes());
        resolver
            .accept_cached_packet(
                cached_level_chunk(i32::try_from(x).expect("test coordinate fits"), vec![hash])
                    .into(),
            )
            .expect("fill the bounded cached transaction window");
    }

    let payload = b"pressure-subchunk";
    let hash = client_blob_hash(payload);
    let packet = SubChunkPacket {
        cache_enabled: true,
        sub_chunk_data: vec![sub_chunk_entry(
            0,
            0,
            0,
            SubChunkRequestResult::Success,
            Some(payload.to_vec()),
            Some(hash),
        )],
        ..Default::default()
    }
    .into();
    let mut status = resolver
        .accept_cached_packet(packet)
        .expect("pressure rotation remains non-fatal");

    assert!(status.take_admission().is_none());
    assert!(status.take_recovery().is_some());
    assert_eq!(
        resolver.stats().pending_transactions,
        MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 1
    );
}

#[test]
fn precounted_secondary_recovery_coalesces_without_double_counting() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver.enqueue_recovery(ChunkResyncEvent {
        dimension: 0,
        x: 999,
        z: 0,
        requested_sub_chunks: None,
        requested_sub_chunk_ys: None,
    });
    for x in 0..MAX_CLIENT_BLOB_PENDING_TRANSACTIONS - 1 {
        let payload = format!("coalesced-pressure-{x}");
        resolver
            .accept_cached_packet(
                cached_level_chunk(
                    i32::try_from(x).expect("test coordinate fits"),
                    vec![client_blob_hash(payload.as_bytes())],
                )
                .into(),
            )
            .expect("fill every recovery slot except the queued event");
    }

    let mut status = resolver
        .accept_cached_packet_with_size(
            cached_level_chunk(999, vec![client_blob_hash(b"oversized-current")]).into(),
            MAX_CLIENT_BLOB_PENDING_BYTES,
        )
        .expect("pending-byte rejection after rotation remains non-fatal");

    assert_eq!(status.take_recovery().map(|recovery| recovery.x), Some(0));
    assert_eq!(resolver.stats().recovery_ready_events, 1);
    assert_eq!(
        resolver.stats().recovery_requests,
        2,
        "one inline recovery plus one coalesced queued recovery are observable"
    );
}
