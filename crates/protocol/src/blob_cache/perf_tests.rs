use super::*;

/// Measures a one-blob response against increasing retained cache indexes.
#[test]
#[ignore = "benchmark"]
fn blob_cache_delta_cost() {
    for entries in [0_u64, 4_096, 32_768] {
        let cache = ClientBlobCache::default();
        {
            let mut store = cache.lock();
            for index in 0..entries {
                let payload = index.to_le_bytes();
                insert_verified(
                    &mut store,
                    cache.limits,
                    client_blob_hash(&payload),
                    &payload,
                )
                .unwrap();
            }
        }
        let mut insert_us = Vec::new();
        let mut response_us = Vec::new();
        for index in entries..entries + 64 {
            let payload = index.to_le_bytes();
            let start = std::time::Instant::now();
            cache.insert(&payload).unwrap();
            insert_us.push(start.elapsed().as_secs_f64() * 1e6);
            let payload = (index + 1_000_000).to_le_bytes();
            let hash = client_blob_hash(&payload);
            let mut resolver = BlobCacheResolver::new(cache.clone());
            resolver
                .accept_cached_packet(crate::Packet::new(super::tests::cached_level_chunk(
                    0,
                    vec![hash],
                )))
                .unwrap();
            let response = ClientCacheMissResponsePacket {
                missing_blobs: vec![valentine::bedrock::version::v1_26_51::MissingBlobData {
                    blob_id: hash,
                    blob_data: payload.to_vec(),
                }],
            };
            let start = std::time::Instant::now();
            resolver.accept_miss_response(response).unwrap();
            response_us.push(start.elapsed().as_secs_f64() * 1e6);
            let Some(BlobCacheReady::Packet(packet)) = resolver.pop_ready() else {
                panic!("resolved packet")
            };
            let McpePacketData::LevelChunkPacket(packet) = packet.data else {
                panic!("level chunk")
            };
            assert_eq!(packet.serialized_chunk_data, payload);
        }
        insert_us.sort_by(f64::total_cmp);
        response_us.sort_by(f64::total_cmp);
        eprintln!(
            "BLOB_DELTA entries={entries} insert_p50_us={:.3} insert_p95_us={:.3} response_p50_us={:.3} response_p95_us={:.3}",
            insert_us[32], insert_us[60], response_us[32], response_us[60]
        );
    }
}

/// Measures retained payload and unpin service after a pinned burst.
#[test]
#[ignore = "benchmark"]
fn blob_cache_unpin_cost() {
    let cache = ClientBlobCache::with_limits(BlobCacheLimits {
        trim_trigger_bytes: 1024 * 1024,
        trim_floor_bytes: 800 * 1024,
    });
    let mut hashes = Vec::new();
    for index in 0_u64..512 {
        let mut payload = vec![0; 4096];
        payload[..8].copy_from_slice(&index.to_le_bytes());
        let hash = client_blob_hash(&payload);
        cache.classify(&[hash], true);
        cache.insert(&payload).unwrap();
        hashes.push(hash);
    }
    let before = cache.total_bytes();
    let start = std::time::Instant::now();
    cache.unpin_all(&hashes);
    eprintln!(
        "BLOB_UNPIN before_bytes={before} after_bytes={} service_us={:.3}",
        cache.total_bytes(),
        start.elapsed().as_secs_f64() * 1e6
    );
}
