use protocol::{
    BedrockSession, BlobCacheError, BlobCacheLimits, BlobCacheResolver, ClientBlobCache,
    client_blob_hash,
};
use std::mem::size_of;
use std::sync::{Arc, Barrier};
use valentine::bedrock::version::v1_26_30::{
    Blob, ClientCacheMissResponsePacket, HeightMapDataType, LevelChunkPacket,
    LevelChunkPacketBlobs, McpePacketData, SetTimePacket, SubChunkEntryWithCachingItem,
    SubChunkEntryWithCachingItemResult, SubChunkEntryWithoutCachingItemResult, SubchunkPacket,
    SubchunkPacketEntries, Vec3I,
};

fn limits(entries: usize, bytes: usize) -> BlobCacheLimits {
    BlobCacheLimits {
        max_entries: entries,
        max_total_bytes: bytes,
        max_blob_bytes: 64,
        max_hashes_per_packet: 8,
        max_pending_transactions: 4,
        max_pending_bytes: 16 * 1024,
    }
}

fn cached_level(hashes: Vec<u64>, tail: &[u8]) -> protocol::Packet {
    LevelChunkPacket {
        x: 4,
        z: -7,
        dimension: 0,
        sub_chunk_count: 2,
        blobs: Some(LevelChunkPacketBlobs { hashes }),
        payload: tail.to_vec(),
        ..Default::default()
    }
    .into()
}

fn cached_subchunk(hash: u64, tail: &[u8]) -> protocol::Packet {
    SubchunkPacket {
        dimension: 0,
        origin: Vec3I { x: 4, y: -4, z: 9 },
        entries: SubchunkPacketEntries::SubChunkEntryWithCaching(vec![
            SubChunkEntryWithCachingItem {
                dx: 0,
                dy: 1,
                dz: -1,
                result: SubChunkEntryWithCachingItemResult::Success,
                payload: Some(tail.to_vec()),
                heightmap_type: HeightMapDataType::NoData,
                heightmap: None,
                render_heightmap_type: HeightMapDataType::NoData,
                render_heightmap: None,
                blob_id: hash,
            },
            SubChunkEntryWithCachingItem {
                dx: 1,
                dy: 2,
                dz: 0,
                result: SubChunkEntryWithCachingItemResult::SuccessAllAir,
                blob_id: u64::MAX,
                ..Default::default()
            },
        ]),
    }
    .into()
}

fn cached_request_level(x: i32, hash: u64) -> protocol::Packet {
    LevelChunkPacket {
        x,
        sub_chunk_count: -1,
        blobs: Some(LevelChunkPacketBlobs { hashes: vec![hash] }),
        ..Default::default()
    }
    .into()
}

fn pop_packet(resolver: &mut BlobCacheResolver, label: &str) -> protocol::Packet {
    resolver
        .pop_ready()
        .unwrap_or_else(|| panic!("{label}"))
        .into_packet()
        .unwrap_or_else(|| panic!("{label} was not a packet"))
}

#[test]
fn bedrock_blob_ids_are_seed_zero_xxhash64() {
    assert_eq!(client_blob_hash(b""), 0xef46_db37_51d8_e999);
    assert_eq!(client_blob_hash(b"hello"), 0x26c7_827d_889f_6da3);
    assert_eq!(client_blob_hash(b"subchunk-a"), 0x283c_6a98_a9b9_fd25);
    assert_eq!(client_blob_hash(b"subchunk-b"), 0x9e95_2256_92d7_18f4);
    assert_eq!(client_blob_hash(b"biome-data"), 0xdd63_3fd0_a101_21df);
}

#[test]
fn shared_cache_concurrent_inserts_do_not_lose_committed_entries() {
    for round in 0..32_u8 {
        let cache = ClientBlobCache::with_limits(limits(32, 1_024));
        let barrier = Arc::new(Barrier::new(16));
        let threads: Vec<_> = (0..16_u8)
            .map(|index| {
                let cache = cache.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let payload = [round, index, index.wrapping_mul(17)];
                    barrier.wait();
                    let hash = cache.insert(&payload).expect("concurrent insert");
                    (hash, payload)
                })
            })
            .collect();
        let inserted: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().expect("insert thread"))
            .collect();
        assert_eq!(cache.entry_count(), inserted.len());
        for (hash, _) in inserted {
            assert!(cache.contains(hash));
        }
    }
}

#[test]
fn concurrent_classification_pins_every_reported_hit_atomically() {
    for _ in 0..2_000 {
        let cache = ClientBlobCache::with_limits(limits(1, 8));
        let a = client_blob_hash(b"a");
        let b = client_blob_hash(b"b");
        cache.insert(b"a").expect("seed hit");
        let barrier = Arc::new(Barrier::new(2));
        let resolver_cache = cache.clone();
        let resolver_barrier = barrier.clone();
        let resolver_thread = std::thread::spawn(move || {
            let mut resolver = BlobCacheResolver::new(resolver_cache);
            resolver_barrier.wait();
            let status = resolver
                .accept_cached_packet(cached_level(vec![a, b, a], b""))
                .expect("classify cached packet");
            (resolver, status)
        });
        let insert_cache = cache.clone();
        let insert_thread = std::thread::spawn(move || {
            barrier.wait();
            insert_cache.insert(b"c")
        });
        let (mut resolver, status) = resolver_thread.join().expect("resolver thread");
        let _ = insert_thread.join().expect("insert thread");
        if status.have.contains(&a) {
            assert!(cache.contains(a), "a reported hit must remain pinned");
        }
        resolver.reset_pending();
    }
}

#[test]
fn cached_inline_level_chunk_classifies_unique_hashes_and_reconstructs_wire_order() {
    let first = b"subchunk-a";
    let missing = b"subchunk-b";
    let first_hash = client_blob_hash(first);
    let missing_hash = client_blob_hash(missing);
    let cache = ClientBlobCache::with_limits(limits(4, 128));
    cache.insert(first).expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);

    let status = resolver
        .accept_cached_packet(cached_level(
            vec![first_hash, missing_hash, first_hash],
            b"tail",
        ))
        .expect("accept cached level chunk");
    assert_eq!(status.have, vec![first_hash]);
    assert_eq!(status.missing, vec![missing_hash]);
    assert!(resolver.pop_ready().is_none());

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: missing_hash,
                payload: missing.to_vec(),
            }],
        })
        .expect("resolve miss");
    let packet = pop_packet(&mut resolver, "resolved packet");
    let McpePacketData::PacketLevelChunk(packet) = packet.data else {
        panic!("expected level chunk")
    };
    assert!(packet.blobs.is_none());
    assert_eq!(
        packet.payload,
        [
            first.as_slice(),
            missing.as_slice(),
            first.as_slice(),
            b"tail"
        ]
        .concat()
    );
    assert_eq!(resolver.stats().reconstructed_level_chunks, 1);
    assert_eq!(resolver.stats().hashes_classified, 2);
}

#[test]
fn request_mode_level_chunk_reconstructs_biome_before_uncached_tail() {
    let biome = b"biome-data";
    let hash = client_blob_hash(biome);
    let cache = ClientBlobCache::with_limits(limits(4, 128));
    cache.insert(biome).expect("seed biome");
    let mut resolver = BlobCacheResolver::new(cache);
    let packet: protocol::Packet = LevelChunkPacket {
        x: 1,
        z: 2,
        dimension: 0,
        sub_chunk_count: -2,
        highest_subchunk_count: Some(7),
        blobs: Some(LevelChunkPacketBlobs { hashes: vec![hash] }),
        payload: vec![0],
    }
    .into();

    let status = resolver.accept_cached_packet(packet).expect("cached biome");
    assert_eq!(status.have, vec![hash]);
    let packet = pop_packet(&mut resolver, "hit resolves immediately");
    let McpePacketData::PacketLevelChunk(packet) = packet.data else {
        panic!("expected level chunk")
    };
    assert_eq!(packet.payload, [biome.as_slice(), &[0]].concat());
}

#[test]
fn cached_subchunk_attaches_block_entity_tail_and_ignores_all_air_blob_id() {
    let subchunk = b"subchunk";
    let nbt_tail = b"block-entity-nbt";
    let hash = client_blob_hash(subchunk);
    let cache = ClientBlobCache::with_limits(limits(4, 128));
    let mut resolver = BlobCacheResolver::new(cache);

    let status = resolver
        .accept_cached_packet(cached_subchunk(hash, nbt_tail))
        .expect("accept cached subchunk");
    assert_eq!(status.missing, vec![hash]);
    assert!(status.have.is_empty());
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: subchunk.to_vec(),
            }],
        })
        .expect("resolve subchunk");

    let packet = pop_packet(&mut resolver, "resolved subchunk");
    let McpePacketData::PacketSubchunk(packet) = packet.data else {
        panic!("expected subchunk")
    };
    let SubchunkPacketEntries::SubChunkEntryWithoutCaching(entries) = packet.entries else {
        panic!("cache marker must be removed")
    };
    assert_eq!(
        entries[0].result,
        SubChunkEntryWithoutCachingItemResult::Success
    );
    assert_eq!(
        entries[0].payload,
        [subchunk.as_slice(), nbt_tail.as_slice()].concat()
    );
    assert_eq!(
        entries[1].result,
        SubChunkEntryWithoutCachingItemResult::SuccessAllAir
    );
    assert!(entries[1].payload.is_empty());
    assert_eq!(resolver.stats().reconstructed_sub_chunks, 1);
}

#[test]
fn miss_packets_complete_transactions_in_original_fifo_order() {
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
    assert!(resolver.pop_ready().is_none());
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: ah,
                payload: a.to_vec(),
            }],
        })
        .expect("earlier transaction resolves");

    let first = pop_packet(&mut resolver, "first packet");
    let second = pop_packet(&mut resolver, "second packet");
    let McpePacketData::PacketLevelChunk(first) = first.data else {
        panic!()
    };
    let McpePacketData::PacketLevelChunk(second) = second.data else {
        panic!()
    };
    assert!(first.payload.ends_with(b"first"));
    assert!(second.payload.ends_with(b"second"));
}

#[test]
fn ordinary_packets_are_fifo_barriers_between_cached_transactions() {
    let a = b"missing-a";
    let b = b"cached-b";
    let ah = client_blob_hash(a);
    let bh = client_blob_hash(b);
    let cache = ClientBlobCache::with_limits(limits(4, 128));
    cache.insert(b).expect("seed b hit");
    let mut resolver = BlobCacheResolver::new(cache);
    resolver
        .accept_cached_packet(cached_level(vec![ah, ah, ah], b"A"))
        .expect("pending A");
    resolver
        .accept_passthrough(SetTimePacket { time: 42 }.into(), 8)
        .expect("ordinary FIFO barrier");
    let b_status = resolver
        .accept_cached_packet(cached_level(vec![bh, bh, bh], b"B"))
        .expect("hit B");
    assert_eq!(b_status.have, vec![bh]);
    assert!(resolver.pop_ready().is_none());

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: ah,
                payload: a.to_vec(),
            }],
        })
        .expect("resolve A");

    let a_packet = pop_packet(&mut resolver, "A first");
    let ordinary = pop_packet(&mut resolver, "ordinary second");
    let b_packet = pop_packet(&mut resolver, "B third");
    assert!(matches!(a_packet.data, McpePacketData::PacketLevelChunk(_)));
    assert!(matches!(
        ordinary.data,
        McpePacketData::PacketSetTime(SetTimePacket { time: 42 })
    ));
    assert!(matches!(b_packet.data, McpePacketData::PacketLevelChunk(_)));
}

#[test]
fn invalid_miss_is_atomic_resets_pending_and_does_not_poison_cache() {
    let wanted = b"wanted";
    let hash = client_blob_hash(wanted);
    let cache = ClientBlobCache::with_limits(limits(4, 128));
    let mut resolver = BlobCacheResolver::new(cache.clone());
    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b"tail"))
        .expect("pending transaction");

    let error = resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: b"poison".to_vec(),
            }],
        })
        .expect_err("hash mismatch must fail");
    assert!(error.to_string().contains("hash"));
    assert!(!cache.contains(hash));
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().rejected_blobs, 1);
    assert_eq!(resolver.stats().pending_resets, 1);
}

#[test]
fn lru_eviction_never_removes_a_blob_pinned_by_a_pending_transaction() {
    let a = b"aaaaaaaa";
    let b = b"bbbbbbbb";
    let c = b"cccccccc";
    let ah = client_blob_hash(a);
    let bh = client_blob_hash(b);
    let ch = client_blob_hash(c);
    let cache = ClientBlobCache::with_limits(limits(2, 16));
    cache.insert(a).expect("insert a");
    cache.insert(b).expect("insert b");
    let mut resolver = BlobCacheResolver::new(cache.clone());

    let status = resolver
        .accept_cached_packet(cached_level(vec![ah, ch, ah], b""))
        .expect("pin a while c is missing");
    assert_eq!(status.have, vec![ah]);
    assert_eq!(status.missing, vec![ch]);
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: ch,
                payload: c.to_vec(),
            }],
        })
        .expect("insert c");

    assert!(cache.contains(ah));
    assert!(cache.contains(ch));
    assert!(!cache.contains(bh));
    assert_eq!(resolver.stats().evictions, 1);
}

#[test]
fn exact_hash_transaction_and_blob_bounds_fail_closed() {
    let mut strict = limits(2, 16);
    strict.max_hashes_per_packet = 1;
    strict.max_pending_transactions = 1;
    strict.max_blob_bytes = 4;
    let cache = ClientBlobCache::with_limits(strict);
    let mut resolver = BlobCacheResolver::new(cache.clone());
    let a = client_blob_hash(b"a");

    assert!(
        resolver
            .accept_cached_packet(cached_level(vec![a, a, a], b""))
            .is_err()
    );
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(cache.insert(b"12345").is_err());
}

#[test]
fn ready_transactions_remain_counted_and_byte_bounded_until_consumed() {
    let mut probe_limits = limits(2, 16);
    probe_limits.max_blob_bytes = 8;
    let probe_cache = ClientBlobCache::with_limits(probe_limits);
    let probe_hash = probe_cache.insert(b"12345678").expect("seed probe hit");
    let mut probe = BlobCacheResolver::new(probe_cache);
    probe
        .accept_cached_packet(cached_level(vec![probe_hash, probe_hash, probe_hash], b""))
        .expect("measure one ready transaction");
    let ready_bytes = probe.stats().pending_bytes;

    let mut bounded = probe_limits;
    bounded.max_pending_bytes = ready_bytes * 3 - 1;
    let cache = ClientBlobCache::with_limits(bounded);
    let hash = cache.insert(b"12345678").expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);

    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("first ready transaction");
    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("second ready transaction at exact byte ceiling");
    assert_eq!(resolver.stats().pending_transactions, 2);
    assert_eq!(resolver.stats().pending_bytes, ready_bytes * 2);

    assert!(
        resolver
            .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
            .is_err()
    );
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_bytes, 0);
}

#[test]
fn terminal_pending_counters_reach_zero_only_after_ready_is_popped() {
    let cache = ClientBlobCache::with_limits(limits(2, 256));
    let hash = cache.insert(b"hit").expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);
    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("hit-only transaction");

    assert_eq!(resolver.stats().pending_transactions, 1);
    assert!(resolver.stats().pending_bytes > 0);
    let _ = pop_packet(&mut resolver, "ready packet");
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_bytes, 0);
}

#[test]
fn passthrough_stats_include_ready_items_that_have_not_been_consumed() {
    let cache = ClientBlobCache::with_limits(limits(2, 128));
    let hash = cache.insert(b"hit").expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);
    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("ready cached packet");
    resolver
        .accept_passthrough(SetTimePacket { time: 7 }.into(), 8)
        .expect("ready passthrough");

    assert_eq!(resolver.stats().pending_transactions, 2);
    let _ = pop_packet(&mut resolver, "cached first");
    assert_eq!(resolver.stats().pending_transactions, 1);
    let _ = pop_packet(&mut resolver, "passthrough second");
    assert_eq!(resolver.stats().pending_transactions, 0);
}

#[test]
fn lunar_sized_many_small_blobs_are_not_charged_as_worst_case_blobs() {
    let mut bounded = limits(256, 4_096);
    bounded.max_blob_bytes = 2 * 1024 * 1024;
    bounded.max_hashes_per_packet = 4_096;
    bounded.max_pending_bytes = 16 * 1024;
    let cache = ClientBlobCache::with_limits(bounded);
    let mut hashes = Vec::new();
    let mut expected = Vec::new();
    for value in 0..177_u16 {
        let payload = value.to_le_bytes();
        hashes.push(cache.insert(&payload).expect("seed small blob"));
        expected.extend_from_slice(&payload);
    }
    let packet: protocol::Packet = LevelChunkPacket {
        sub_chunk_count: 176,
        blobs: Some(LevelChunkPacketBlobs { hashes }),
        payload: b"tail".to_vec(),
        ..Default::default()
    }
    .into();
    let mut resolver = BlobCacheResolver::new(cache);

    resolver
        .accept_cached_packet(packet)
        .expect("177 small Lunar-style blobs fit retained and reconstructed limits");
    let packet = pop_packet(&mut resolver, "many-small ready packet");
    let McpePacketData::PacketLevelChunk(packet) = packet.data else {
        panic!("expected level chunk")
    };
    expected.extend_from_slice(b"tail");
    assert_eq!(packet.payload, expected);
}

#[test]
fn repeated_large_blob_expansion_is_rejected_before_reconstruction() {
    let payload = [0x5a; 512];
    let hash = client_blob_hash(&payload);
    let mut probe_limits = limits(2, 2_048);
    probe_limits.max_blob_bytes = 1_024;
    let probe_cache = ClientBlobCache::with_limits(probe_limits);
    let mut probe = BlobCacheResolver::new(probe_cache);
    probe
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("measure retained transaction");
    let retained_bytes = probe.stats().pending_bytes;
    probe
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect("measure ready expansion");
    let ready_bytes = probe.stats().pending_bytes;
    assert!(ready_bytes > retained_bytes);

    let mut bounded = probe_limits;
    bounded.max_pending_bytes = ready_bytes - 1;
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(bounded));
    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("retained transaction fits before expansion");
    let error = resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect_err("repeated expansion exceeds the exact ready ceiling");

    assert!(matches!(
        error,
        protocol::BlobCacheError::TooManyPendingBytes { max } if max == ready_bytes - 1
    ));
    assert_eq!(
        resolver.stats().hashes_classified,
        1,
        "the retained transaction must be admitted before exact ready-size rejection"
    );
    assert!(resolver.pop_ready().is_none());
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_bytes, 0);
}

#[test]
fn blob_status_round_trips_exact_have_and_missing_hashes_on_the_wire() {
    let hit = b"wire-hit";
    let miss = b"wire-miss";
    let hit_hash = client_blob_hash(hit);
    let miss_hash = client_blob_hash(miss);
    let cache = ClientBlobCache::with_limits(limits(4, 128));
    cache.insert(hit).expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);
    let status = resolver
        .accept_cached_packet(cached_level(vec![hit_hash, miss_hash, hit_hash], b""))
        .expect("classify status");
    let session = BedrockSession { shield_item_id: 0 };
    let encoded = protocol::encode(&status.into(), &session).expect("encode status");
    let decoded = protocol::decode_batch(encoded, &session).expect("decode status");
    let McpePacketData::PacketClientCacheBlobStatus(status) = &decoded[0].data else {
        panic!("expected cache blob status")
    };

    assert_eq!(status.have, vec![hit_hash]);
    assert_eq!(status.missing, vec![miss_hash]);
}

#[test]
fn unsolicited_conflicting_and_partially_valid_miss_responses_are_atomic() {
    let wanted = b"wanted";
    let wanted_hash = client_blob_hash(wanted);

    let exercise = |blobs: Vec<Blob>| {
        let cache = ClientBlobCache::with_limits(limits(4, 128));
        let mut resolver = BlobCacheResolver::new(cache.clone());
        resolver
            .accept_cached_packet(cached_level(
                vec![wanted_hash, wanted_hash, wanted_hash],
                b"",
            ))
            .expect("pending wanted blob");
        let error = resolver
            .accept_miss_response(ClientCacheMissResponsePacket { blobs })
            .expect_err("poison response must fail");
        assert!(!cache.contains(wanted_hash));
        assert_eq!(resolver.stats().pending_transactions, 0);
        error
    };

    let unsolicited = b"unsolicited";
    assert!(matches!(
        exercise(vec![Blob {
            hash: client_blob_hash(unsolicited),
            payload: unsolicited.to_vec(),
        }]),
        BlobCacheError::UnsolicitedBlob(_)
    ));
    assert!(matches!(
        exercise(vec![
            Blob {
                hash: wanted_hash,
                payload: wanted.to_vec(),
            },
            Blob {
                hash: wanted_hash,
                payload: b"different".to_vec(),
            },
        ]),
        BlobCacheError::ConflictingDuplicate(hash) if hash == wanted_hash
    ));
    assert!(matches!(
        exercise(vec![
            Blob {
                hash: wanted_hash,
                payload: wanted.to_vec(),
            },
            Blob {
                hash: wanted_hash,
                payload: b"poison".to_vec(),
            },
        ]),
        BlobCacheError::ConflictingDuplicate(_)
    ));
}

#[test]
fn every_configured_ceiling_accepts_its_exact_boundary_and_stays_bounded_afterward() {
    let mut cache_limits = limits(2, 8);
    cache_limits.max_blob_bytes = 4;
    let cache = ClientBlobCache::with_limits(cache_limits);
    let first = cache.insert(b"1234").expect("exact blob maximum");
    let second = cache.insert(b"5678").expect("exact entry and byte maximum");
    assert_eq!(cache.entry_count(), 2);
    assert_eq!(cache.total_bytes(), 8);
    assert!(matches!(
        cache.insert(b"12345"),
        Err(BlobCacheError::BlobTooLarge { .. })
    ));
    let third = cache.insert(b"abcd").expect("bounded LRU replacement");
    assert_eq!(cache.entry_count(), 2);
    assert_eq!(cache.total_bytes(), 8);
    assert!(!cache.contains(first));
    assert!(cache.contains(second));
    assert!(cache.contains(third));

    let mut transaction_limits = limits(8, 128);
    transaction_limits.max_hashes_per_packet = 3;
    transaction_limits.max_pending_transactions = 2;
    let cache = ClientBlobCache::with_limits(transaction_limits);
    let mut resolver = BlobCacheResolver::new(cache);
    let a = client_blob_hash(b"a");
    let b = client_blob_hash(b"b");
    resolver
        .accept_cached_packet(cached_level(vec![a, a, a], b"12345678"))
        .expect("exact hash boundary and first transaction");
    resolver
        .accept_cached_packet(cached_level(vec![b, b, b], b"12345678"))
        .expect("exact transaction boundary");
    assert_eq!(resolver.stats().pending_transactions, 2);
    assert!(
        resolver
            .accept_cached_packet(cached_level(vec![a, a, a], b""))
            .is_err()
    );
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_bytes, 0);

    let probe_hash = client_blob_hash(b"pending-byte-boundary");
    let mut probe = BlobCacheResolver::new(ClientBlobCache::with_limits(limits(2, 128)));
    probe
        .accept_cached_packet(cached_request_level(1, probe_hash))
        .expect("measure exact pending byte boundary");
    let exact_pending_bytes = probe.stats().pending_bytes;
    let mut exact_limits = limits(2, 128);
    exact_limits.max_pending_bytes = exact_pending_bytes;
    BlobCacheResolver::new(ClientBlobCache::with_limits(exact_limits))
        .accept_cached_packet(cached_request_level(1, probe_hash))
        .expect("exact pending byte ceiling is accepted");
    exact_limits.max_pending_bytes -= 1;
    assert!(matches!(
        BlobCacheResolver::new(ClientBlobCache::with_limits(exact_limits))
            .accept_cached_packet(cached_request_level(1, probe_hash)),
        Err(BlobCacheError::TooManyPendingBytes { .. })
    ));
}

#[test]
fn default_limits_accept_177_distinct_request_transactions_and_publish_fifo() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    let fixtures: Vec<_> = (0..177_u16)
        .map(|index| {
            let payload = index.to_le_bytes().to_vec();
            let hash = client_blob_hash(&payload);
            (i32::from(index), hash, payload)
        })
        .collect();

    for (x, hash, _) in &fixtures {
        let status = resolver
            .accept_cached_packet(cached_request_level(*x, *hash))
            .expect("default accepts the full Lunar request-column burst");
        assert_eq!(status.missing, vec![*hash]);
    }
    assert_eq!(resolver.stats().pending_transactions, 177);
    assert!(resolver.stats().pending_bytes > 0);
    assert!(resolver.stats().pending_bytes <= resolver.cache().limits().max_pending_bytes);

    for (_, hash, payload) in fixtures.iter().skip(1).rev() {
        resolver
            .accept_miss_response(ClientCacheMissResponsePacket {
                blobs: vec![Blob {
                    hash: *hash,
                    payload: payload.clone(),
                }],
            })
            .expect("out-of-order response remains authorized");
        assert!(
            resolver.pop_ready().is_none(),
            "FIFO head is still unresolved"
        );
    }
    let (_, first_hash, first_payload) = &fixtures[0];
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: *first_hash,
                payload: first_payload.clone(),
            }],
        })
        .expect("resolve FIFO head");
    for (expected_x, _, _) in &fixtures {
        let packet = pop_packet(&mut resolver, "resolved request column");
        let McpePacketData::PacketLevelChunk(packet) = packet.data else {
            panic!("expected LevelChunk")
        };
        assert_eq!(packet.x, *expected_x);
    }
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_bytes, 0);
}

#[test]
fn repeated_authorized_responses_remain_valid_after_the_first_populates_cache() {
    let payload = b"shared-response";
    let hash = client_blob_hash(payload);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    for x in [1, 2] {
        let status = resolver
            .accept_cached_packet(cached_request_level(x, hash))
            .expect("authorize shared miss");
        assert_eq!(status.missing, vec![hash]);
    }

    let response = || ClientCacheMissResponsePacket {
        blobs: vec![Blob {
            hash,
            payload: payload.to_vec(),
        }],
    };
    resolver
        .accept_miss_response(response())
        .expect("first authorized response populates cache");
    assert_eq!(resolver.stats().pending_transactions, 2);
    let _ = pop_packet(&mut resolver, "first shared transaction");
    let _ = pop_packet(&mut resolver, "second shared transaction");
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(
        resolver.stats().pending_bytes > 0,
        "the still-authorized duplicate response retains independently bounded state"
    );
    resolver
        .accept_miss_response(response())
        .expect("second previously authorized identical response is accepted");
    assert_eq!(resolver.stats().pending_bytes, 0);
}

#[test]
fn resolver_accepts_authorized_response_after_another_resolver_fills_shared_cache() {
    let payload = b"cross-resolver";
    let hash = client_blob_hash(payload);
    let cache = ClientBlobCache::default();
    let mut first = BlobCacheResolver::new(cache.clone());
    let mut second = BlobCacheResolver::new(cache);
    first
        .accept_cached_packet(cached_request_level(1, hash))
        .expect("first authorization");
    second
        .accept_cached_packet(cached_request_level(2, hash))
        .expect("second authorization");
    let response = || ClientCacheMissResponsePacket {
        blobs: vec![Blob {
            hash,
            payload: payload.to_vec(),
        }],
    };

    first.accept_miss_response(response()).expect("first fill");
    second
        .accept_miss_response(response())
        .expect("second resolver retains independent authorization");
    let _ = pop_packet(&mut second, "second resolver transaction");
}

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
fn cached_subchunk_heightmaps_and_carriers_are_exactly_bounded() {
    let entry = SubChunkEntryWithCachingItem {
        result: SubChunkEntryWithCachingItemResult::SuccessAllAir,
        heightmap_type: HeightMapDataType::HasData,
        heightmap: Some([1; 256]),
        render_heightmap_type: HeightMapDataType::HasData,
        render_heightmap: Some([2; 256]),
        ..Default::default()
    };
    let ready_bytes = size_of::<SubchunkPacket>()
        + size_of::<valentine::bedrock::version::v1_26_30::SubChunkEntryWithoutCachingItem>();
    let mut bounded = limits(2, 2_048);
    bounded.max_pending_bytes = ready_bytes - 1;
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(bounded));
    let packet: protocol::Packet = SubchunkPacket {
        entries: SubchunkPacketEntries::SubChunkEntryWithCaching(vec![entry]),
        ..Default::default()
    }
    .into();

    assert!(matches!(
        resolver.accept_cached_packet(packet),
        Err(BlobCacheError::TooManyPendingBytes { .. })
    ));
    assert!(resolver.pop_ready().is_none());
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_bytes, 0);
}

#[test]
fn raw_cached_packet_size_participates_in_pending_admission_once() {
    let mut bounded = limits(2, 2_048);
    bounded.max_pending_bytes = 1_023;
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(bounded));
    let hash = client_blob_hash(b"missing");

    assert!(matches!(
        resolver.accept_cached_packet_with_size(cached_request_level(1, hash), 1_024),
        Err(BlobCacheError::TooManyPendingBytes { max: 1_023 })
    ));
}
