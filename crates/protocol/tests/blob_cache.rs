use protocol::{
    BedrockSession, BlobCacheError, BlobCacheLimits, BlobCacheReady, BlobCacheResolver,
    BlockUpdateEvent, ClientBlobCache, SetTimeEvent, WorldEvent, client_blob_hash,
};
use std::sync::{Arc, Barrier};
use valentine::bedrock::version::v1_26_30::{
    Blob, ClientCacheMissResponsePacket, HeightMapDataType, LevelChunkPacket,
    LevelChunkPacketBlobs, McpePacketData, SetTimePacket, SubChunkEntryWithCachingItem,
    SubChunkEntryWithCachingItemResult, SubChunkEntryWithoutCachingItemResult, SubchunkPacket,
    SubchunkPacketEntries, Vec3I,
};

#[path = "blob_cache/head_of_line.rs"]
mod head_of_line;
#[path = "blob_cache/resolver_lifecycle.rs"]
mod resolver_lifecycle;

fn limits(bytes: usize) -> BlobCacheLimits {
    BlobCacheLimits {
        trim_trigger_bytes: bytes,
        trim_floor_bytes: bytes.saturating_mul(4) / 5,
    }
}

fn cached_level(hashes: Vec<u64>, tail: &[u8]) -> protocol::Packet {
    cached_level_at(4, hashes, tail)
}

fn cached_level_at(x: i32, hashes: Vec<u64>, tail: &[u8]) -> protocol::Packet {
    LevelChunkPacket {
        x,
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
fn world_events_are_independent_of_cached_transaction_pressure() {
    let bounded = limits(256);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(bounded));
    let missing = client_blob_hash(b"head-miss");
    resolver
        .accept_cached_packet(cached_request_level(1, missing))
        .expect("unresolved cached FIFO head");
    resolver
        .accept_world_event(WorldEvent::SetTime(SetTimeEvent { time: 1 }), 8)
        .expect("event at the exact transaction boundary");

    resolver
        .accept_world_event(WorldEvent::SetTime(SetTimeEvent { time: 2 }), 8)
        .expect("well-formed event pressure must be recoverable");

    assert_eq!(resolver.stats().pending_transactions, 1);
    assert_eq!(resolver.stats().pending_resets, 0);
    assert_eq!(resolver.stats().skipped_world_events, 0);
    for time in [1, 2] {
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::SetTime(
                SetTimeEvent { time: actual }
            ))) if actual == time
        ));
    }
}

#[test]
fn cache_miss_response_resolves_after_independent_world_events() {
    let payload = b"head-miss";
    let hash = client_blob_hash(payload);
    let bounded = limits(256);
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::with_limits(bounded));
    resolver
        .accept_cached_packet(cached_request_level(1, hash))
        .expect("unresolved cached FIFO head");
    resolver
        .accept_world_event(WorldEvent::SetTime(SetTimeEvent { time: 1 }), 8)
        .expect("event at the exact transaction boundary");
    resolver
        .accept_world_event(WorldEvent::SetTime(SetTimeEvent { time: 2 }), 8)
        .expect("second event is also independent");

    for time in [1, 2] {
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::SetTime(
                SetTimeEvent { time: actual }
            ))) if actual == time
        ));
    }

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect("authorized miss response remains admissible under pressure");

    let resolved = pop_packet(&mut resolver, "resolved cached FIFO head");
    assert!(matches!(resolved.data, McpePacketData::PacketLevelChunk(_)));
    assert!(resolver.pop_ready().is_none());
    assert_eq!(resolver.stats().skipped_world_events, 0);
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
        let cache = ClientBlobCache::with_limits(limits(1_024));
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
        let cache = ClientBlobCache::with_limits(limits(8));
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
    let cache = ClientBlobCache::with_limits(limits(128));
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
    let cache = ClientBlobCache::with_limits(limits(128));
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
    let cache = ClientBlobCache::with_limits(limits(128));
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
fn ordinary_packets_and_later_ready_chunks_bypass_unresolved_chunks() {
    let a = b"missing-a";
    let b = b"cached-b";
    let ah = client_blob_hash(a);
    let bh = client_blob_hash(b);
    let cache = ClientBlobCache::with_limits(limits(128));
    cache.insert(b).expect("seed b hit");
    let mut resolver = BlobCacheResolver::new(cache);
    resolver
        .accept_cached_packet(cached_level(vec![ah, ah, ah], b"A"))
        .expect("pending A");
    resolver
        .accept_passthrough(SetTimePacket { time: 42 }.into(), 8)
        .expect("ordinary packet");
    let b_status = resolver
        .accept_cached_packet(cached_level_at(5, vec![bh, bh, bh], b"B"))
        .expect("hit B");
    assert_eq!(b_status.have, vec![bh]);
    let ordinary = pop_packet(&mut resolver, "ordinary packet is immediately ready");
    let b_packet = pop_packet(&mut resolver, "later cached hit is ready");
    assert!(matches!(
        ordinary.data,
        McpePacketData::PacketSetTime(SetTimePacket { time: 42 })
    ));
    assert!(matches!(b_packet.data, McpePacketData::PacketLevelChunk(_)));

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: ah,
                payload: a.to_vec(),
            }],
        })
        .expect("resolve A");

    let a_packet = pop_packet(&mut resolver, "A resolves last");
    assert!(matches!(a_packet.data, McpePacketData::PacketLevelChunk(_)));
}

#[test]
fn invalid_miss_is_atomic_abandons_pending_and_does_not_poison_cache() {
    let wanted = b"wanted";
    let hash = client_blob_hash(wanted);
    let cache = ClientBlobCache::with_limits(limits(128));
    let mut resolver = BlobCacheResolver::new(cache.clone());
    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b"tail"))
        .expect("pending transaction");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: b"poison".to_vec(),
            }],
        })
        .expect("hash mismatch is rejected without ending the session");
    assert!(!cache.contains(hash));
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
    ));
    assert_eq!(resolver.stats().rejected_blobs, 1);
    assert_eq!(resolver.stats().skipped_miss_responses, 1);
    assert_eq!(resolver.stats().pending_resets, 0);
}

#[test]
fn cache_pressure_never_refuses_a_requested_blob() {
    let bounded = limits(0);
    let payload = b"admit-even-above-trigger";
    let hash = client_blob_hash(payload);
    let cache = ClientBlobCache::with_limits(bounded);
    let mut resolver = BlobCacheResolver::new(cache.clone());
    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("pending transaction");

    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash,
                payload: payload.to_vec(),
            }],
        })
        .expect("cache pressure never refuses a well-formed requested blob");

    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().miss_response_cache_pressure, 0);
    assert_eq!(resolver.stats().admitted_blobs, 1);
    assert!(cache.contains(hash));
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::Packet(_))
    ));
}

#[test]
fn lru_eviction_never_removes_a_blob_pinned_by_a_pending_transaction() {
    let a = b"aaaaaaaa";
    let b = b"bbbbbbbb";
    let c = b"cccccccc";
    let ah = client_blob_hash(a);
    let bh = client_blob_hash(b);
    let ch = client_blob_hash(c);
    let cache = ClientBlobCache::with_limits(limits(16));
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
fn semantic_shape_skips_truthfully_classify_every_referenced_hash() {
    let cache = ClientBlobCache::with_limits(limits(16));
    let hit = cache.insert(b"hit").expect("seed semantic-shape hit");
    let miss = client_blob_hash(b"miss");

    let packets: [protocol::Packet; 2] = [
        LevelChunkPacket {
            x: 4,
            z: -7,
            dimension: 0,
            sub_chunk_count: -3,
            blobs: Some(LevelChunkPacketBlobs {
                hashes: vec![hit, miss],
            }),
            ..Default::default()
        }
        .into(),
        LevelChunkPacket {
            x: 4,
            z: -7,
            dimension: 0,
            sub_chunk_count: 0,
            blobs: Some(LevelChunkPacketBlobs {
                hashes: vec![hit, miss],
            }),
            ..Default::default()
        }
        .into(),
    ];

    for packet in packets {
        let mut resolver = BlobCacheResolver::new(cache.clone());
        let status = resolver
            .accept_cached_packet(packet)
            .expect("semantic shape must recover without disconnecting");
        assert_eq!(status.have, vec![hit]);
        assert_eq!(status.missing, vec![miss]);
        assert_eq!(
            status.classified_hashes(),
            2,
            "every referenced hash must be classified on every skip path"
        );
        assert_eq!(status.recovery.map(|recovery| recovery.x), Some(4));
        assert_eq!(resolver.stats().pending_transactions, 0);
        assert_eq!(resolver.stats().skipped_cached_packets, 1);
    }
    cache
        .insert(b"12345")
        .expect("cache storage has no per-blob maximum");
}

#[test]
fn resolved_transaction_is_not_outstanding_while_ready_bytes_remain_accounted() {
    let cache = ClientBlobCache::with_limits(limits(256));
    let hash = cache.insert(b"hit").expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);
    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("hit-only transaction");

    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(resolver.stats().pending_bytes > 0);
    let _ = pop_packet(&mut resolver, "ready packet");
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_bytes, 0);
}

#[test]
fn ready_queue_retained_bytes_track_live_entries_and_compact_when_empty() {
    let bounded = limits(64);
    let cache = ClientBlobCache::with_limits(bounded);
    let hash = cache.insert(b"hit").expect("seed hit");

    let mut one = BlobCacheResolver::new(cache.clone());
    one.accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("single ready transaction");
    let one_ready_bytes = one.stats().pending_bytes;

    let mut high_water = BlobCacheResolver::new(cache);
    for _ in 0..8 {
        high_water
            .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
            .expect("grow ready queue");
    }
    for _ in 0..7 {
        let _ = pop_packet(&mut high_water, "drain high-water ready transaction");
    }
    assert_eq!(high_water.stats().pending_transactions, 0);
    assert!(
        high_water.stats().pending_bytes >= one_ready_bytes,
        "the remaining ready transaction must stay charged until it is consumed"
    );
    let _ = pop_packet(&mut high_water, "final high-water transaction");
    assert_eq!(high_water.stats().pending_bytes, 0);
}

#[test]
fn passthrough_items_are_excluded_from_cache_transaction_stats() {
    let cache = ClientBlobCache::with_limits(limits(128));
    let hash = cache.insert(b"hit").expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);
    resolver
        .accept_cached_packet(cached_level(vec![hash, hash, hash], b""))
        .expect("ready cached packet");
    resolver
        .accept_passthrough(SetTimePacket { time: 7 }.into(), 8)
        .expect("ready passthrough");

    assert_eq!(resolver.stats().pending_transactions, 0);
    let _ = pop_packet(&mut resolver, "cached first");
    assert_eq!(resolver.stats().pending_transactions, 0);
    let _ = pop_packet(&mut resolver, "passthrough second");
    assert_eq!(resolver.stats().pending_transactions, 0);
}

#[test]
fn lunar_sized_many_small_blobs_are_not_charged_as_worst_case_blobs() {
    let cache = ClientBlobCache::with_limits(limits(4_096));
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
fn blob_status_round_trips_exact_have_and_missing_hashes_on_the_wire() {
    let hit = b"wire-hit";
    let miss = b"wire-miss";
    let hit_hash = client_blob_hash(hit);
    let miss_hash = client_blob_hash(miss);
    let cache = ClientBlobCache::with_limits(limits(128));
    cache.insert(hit).expect("seed hit");
    let mut resolver = BlobCacheResolver::new(cache);
    let status = resolver
        .accept_cached_packet(cached_level(vec![hit_hash, miss_hash, hit_hash], b""))
        .expect("classify status");
    let session = BedrockSession { shield_item_id: 0 };
    let packets = status.into_packets();
    assert_eq!(packets.len(), 1);
    let encoded = protocol::encode(&packets[0].clone().into(), &session).expect("encode status");
    let decoded = protocol::decode_batch(encoded, &session).expect("decode status");
    let McpePacketData::PacketClientCacheBlobStatus(status) = &decoded[0].data else {
        panic!("expected cache blob status")
    };

    assert_eq!(status.have, vec![hit_hash]);
    assert_eq!(status.missing, vec![miss_hash]);
}

#[test]
fn blob_status_splits_at_4095_ids_without_omission_or_reclassification() {
    let missing = (0..4_096_u64).collect::<Vec<_>>();
    let cache = ClientBlobCache::default();
    let hit = cache.insert(b"split-hit").expect("seed split hit");
    let mut hashes = missing.clone();
    hashes.push(hit);
    let mut resolver = BlobCacheResolver::new(cache);
    let status = resolver
        .accept_cached_packet(
            LevelChunkPacket {
                sub_chunk_count: 4_096,
                blobs: Some(LevelChunkPacketBlobs { hashes }),
                ..Default::default()
            }
            .into(),
        )
        .expect("classify every referenced hash through the only status-producing path");
    let packets = status.into_packets();

    assert_eq!(packets.len(), 2);
    assert!(
        packets
            .iter()
            .all(|packet| packet.missing.len() + packet.have.len() <= 4_095)
    );
    assert_eq!(
        packets
            .iter()
            .flat_map(|packet| packet.missing.iter().copied())
            .collect::<Vec<_>>(),
        missing
    );
    assert_eq!(
        packets
            .iter()
            .flat_map(|packet| packet.have.iter().copied())
            .collect::<Vec<_>>(),
        vec![hit]
    );
}

#[test]
fn unsolicited_conflicting_and_partially_valid_miss_responses_are_atomic_skips() {
    let wanted = b"wanted";
    let wanted_hash = client_blob_hash(wanted);

    let exercise = |blobs: Vec<Blob>, expected_pending| {
        let cache = ClientBlobCache::with_limits(limits(128));
        let mut resolver = BlobCacheResolver::new(cache.clone());
        resolver
            .accept_cached_packet(cached_level(
                vec![wanted_hash, wanted_hash, wanted_hash],
                b"",
            ))
            .expect("pending wanted blob");
        resolver
            .accept_miss_response(ClientCacheMissResponsePacket { blobs })
            .expect("semantically invalid response is a recoverable skip");
        assert!(!cache.contains(wanted_hash));
        assert_eq!(resolver.stats().pending_transactions, expected_pending);
        assert_eq!(resolver.stats().skipped_miss_responses, 1);
        assert_eq!(
            matches!(
                resolver.pop_ready(),
                Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
            ),
            expected_pending == 0
        );
        resolver.stats().rejected_blobs
    };

    let unsolicited = b"unsolicited";
    assert_eq!(
        exercise(
            vec![Blob {
                hash: client_blob_hash(unsolicited),
                payload: unsolicited.to_vec(),
            }],
            1,
        ),
        0
    );
    assert_eq!(
        exercise(
            vec![
                Blob {
                    hash: wanted_hash,
                    payload: wanted.to_vec(),
                },
                Blob {
                    hash: wanted_hash,
                    payload: b"different".to_vec(),
                },
            ],
            0,
        ),
        2
    );
    assert_eq!(
        exercise(
            vec![
                Blob {
                    hash: wanted_hash,
                    payload: wanted.to_vec(),
                },
                Blob {
                    hash: wanted_hash,
                    payload: b"poison".to_vec(),
                },
            ],
            0,
        ),
        2
    );
    assert_eq!(
        exercise(
            vec![
                Blob {
                    hash: wanted_hash,
                    payload: wanted.to_vec(),
                },
                Blob {
                    hash: client_blob_hash(unsolicited),
                    payload: unsolicited.to_vec(),
                },
            ],
            0,
        ),
        0
    );
}

#[test]
fn cache_accepts_a_blob_larger_than_its_total_byte_trigger() {
    let oversized = vec![0x5a; 9];
    let cache = ClientBlobCache::with_limits(limits(8));

    let hash = cache
        .insert(&oversized)
        .expect("vanilla has no per-blob maximum");

    assert!(cache.contains(hash));
    assert_eq!(cache.total_bytes(), oversized.len());
}

#[test]
fn cache_trims_from_trigger_to_lower_floor_in_lru_order() {
    let cache = ClientBlobCache::with_limits(limits(10));
    let a = cache.insert(b"aaaaaa").expect("insert six-byte a");
    let b = cache.insert(b"bb").expect("insert two-byte b");

    let mut resolver = BlobCacheResolver::new(cache.clone());
    resolver
        .accept_cached_packet(cached_request_level(0, a))
        .expect("touch a as a cache hit");
    let _ = pop_packet(&mut resolver, "consume a cache hit");

    let c = cache.insert(b"ccc").expect("insert past trim trigger");

    assert!(
        !cache.contains(b),
        "the least-recently-used entry goes first"
    );
    assert!(
        !cache.contains(a),
        "trimming continues below the trigger until the lower floor is reached"
    );
    assert!(cache.contains(c), "the triggering insert is never refused");
    assert_eq!(cache.total_bytes(), 3);
}

#[test]
fn distinct_transactions_publish_out_of_order() {
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    let fixtures: Vec<_> = (0..8_u16)
        .map(|index| {
            let payload = index.to_le_bytes().to_vec();
            let hash = client_blob_hash(&payload);
            (i32::from(index), hash, payload)
        })
        .collect();

    for (x, hash, _) in &fixtures {
        let status = resolver
            .accept_cached_packet(cached_request_level(*x, *hash))
            .expect("sample transaction is below the Cinnabar safety bound");
        assert_eq!(status.missing, vec![*hash]);
    }
    assert_eq!(resolver.stats().pending_transactions, 8);
    assert!(resolver.stats().pending_bytes > 0);

    for (expected_x, hash, payload) in fixtures.iter().skip(1).rev() {
        resolver
            .accept_miss_response(ClientCacheMissResponsePacket {
                blobs: vec![Blob {
                    hash: *hash,
                    payload: payload.clone(),
                }],
            })
            .expect("out-of-order response remains authorized");
        let packet = pop_packet(&mut resolver, "out-of-order completed request column");
        let McpePacketData::PacketLevelChunk(packet) = packet.data else {
            panic!("expected LevelChunk")
        };
        assert_eq!(packet.x, *expected_x);
    }
    let (_, first_hash, first_payload) = &fixtures[0];
    resolver
        .accept_miss_response(ClientCacheMissResponsePacket {
            blobs: vec![Blob {
                hash: *first_hash,
                payload: first_payload.clone(),
            }],
        })
        .expect("resolve remaining transaction");
    let packet = pop_packet(&mut resolver, "remaining request column");
    let McpePacketData::PacketLevelChunk(packet) = packet.data else {
        panic!("expected LevelChunk")
    };
    assert_eq!(packet.x, fixtures[0].0);
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_bytes, 0);
}

#[test]
fn one_response_resolves_every_transaction_waiting_for_the_same_blob() {
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
        .expect("the server sends the requested blob once");
    assert_eq!(resolver.stats().pending_transactions, 0);
    let _ = pop_packet(&mut resolver, "first shared transaction");
    let _ = pop_packet(&mut resolver, "second shared transaction");
    assert_eq!(resolver.stats().pending_transactions, 0);
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
