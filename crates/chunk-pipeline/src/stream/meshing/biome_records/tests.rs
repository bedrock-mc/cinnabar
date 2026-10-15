use super::*;
use world::RawBiomeIds;

/// One immutable uniform raw-biome storage.
fn storage(id: u8) -> Arc<BiomeStorage> {
    DecodedBiomeColumn::decode(0, 1, &[1, id * 2], &RawBiomeIds { default_biome: 0 })
        .storage(0)
        .unwrap()
}

/// A standalone stream whose asset and tint snapshots can be replaced independently.
fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [8.0, 64.0, 8.0],
        world_spawn_position: [8, 64, 8],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

#[test]
fn exact_uniform_ids_share_records_across_sections_and_remeshes() {
    let mut stream = stream();
    let cache = stream.biome_record_cache(0);
    let sources = std::array::from_fn(|slot| Some(storage(if slot % 3 == 2 { 9 } else { 7 })));
    let first = cache.record(&sources);
    let same_ids = std::array::from_fn(|slot| Some(storage(if slot % 3 == 2 { 9 } else { 7 })));
    let second = cache.record(&same_ids);
    assert_eq!(first.words().as_ptr(), second.words().as_ptr());
    assert_eq!(
        cache.record(&sources).words().as_ptr(),
        first.words().as_ptr()
    );
    assert_eq!(first, pack_biome_record(&sources, &cache.resolved));
    let mut changed = same_ids;
    changed[4] = None;
    let missing_center = cache.record(&changed);
    assert!(missing_center.is_fallback());
    assert_ne!(missing_center.words().as_ptr(), first.words().as_ptr());
}

#[test]
fn mixed_sources_are_retained_and_different_allocations_miss() {
    let mut stream = stream();
    let cache = stream.biome_record_cache(0);
    let mut bytes = vec![3];
    bytes.extend(std::iter::repeat_n(0_u8, 128 * size_of::<u32>()));
    bytes.extend([4, 14, 18]);
    let mixed = DecodedBiomeColumn::decode(0, 1, &bytes, &RawBiomeIds { default_biome: 0 })
        .storage(0)
        .unwrap();
    let sources = std::array::from_fn(|_| Some(mixed.clone()));
    let first = cache.record(&sources);
    assert_eq!(
        cache.record(&sources).words().as_ptr(),
        first.words().as_ptr()
    );
    let copy = std::array::from_fn(|_| Some(Arc::new((*mixed).clone())));
    let second = cache.record(&copy);
    assert_eq!(first, second);
    assert_ne!(first.words().as_ptr(), second.words().as_ptr());
    assert!(Arc::strong_count(&mixed) > BIOME_NEIGHBOUR_SLOT_COUNT);
}

#[test]
fn pack_reload_and_dimension_change_replace_cache_namespaces() {
    let mut stream = stream();
    let old = stream.biome_record_cache(0);
    let sources = std::array::from_fn(|_| Some(storage(7)));
    let first = old.record(&sources);
    stream.reload_resource_assets(Arc::new(RuntimeAssets::diagnostic()));
    assert!(stream.biome_records.is_none());
    let after_reload = stream.biome_record_cache(0);
    assert!(!Arc::ptr_eq(&old, &after_reload));
    assert_ne!(
        first.words().as_ptr(),
        after_reload.record(&sources).words().as_ptr()
    );
    stream
        .submit(
            1,
            WorldEvent::ChangeDimension(client_world::ingestion::ChangeDimensionEvent {
                dimension: 1,
                position: [8.0, 64.0, 8.0],
                ..Default::default()
            }),
        )
        .unwrap();
    assert!(stream.biome_records.is_none());
    let destination = stream.biome_record_cache(1);
    assert!(!Arc::ptr_eq(&after_reload, &destination));
    // A late old worker can only insert into the old namespace.
    old.record(&std::array::from_fn(|_| Some(storage(9))));
    assert!(destination.entries.lock().unwrap().records.is_empty());
}

#[test]
fn changed_tint_sources_rebuild_dense_indices() {
    let mut stream = stream();
    let sources = std::array::from_fn(|_| Some(storage(7)));
    let old = stream.biome_record_cache(0);
    assert_eq!(old.record(&sources).uniform_tint_index(), Some(0));
    stream.replace_biome_definitions(Arc::from([BiomeDefinitionEvent {
        biome_id: Some(7),
        name: "cache_biome".into(),
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_color: 0xff44_88aa,
    }]));
    let current = stream.biome_record_cache(0);
    assert!(!Arc::ptr_eq(&old, &current));
    assert_eq!(current.record(&sources).uniform_tint_index(), Some(1));
    assert_eq!(old.record(&sources).uniform_tint_index(), Some(0));
}

#[test]
fn cache_eviction_is_bounded_and_live_records_survive_it() {
    let mut stream = stream();
    let cache = stream.biome_record_cache(0);
    let sources = std::array::from_fn(|_| Some(storage(7)));
    let first = cache.record(&sources);
    // Vary missing slots to exceed the entry limit without growing the source arrays.
    for pattern in 0..CACHE_ENTRIES + 1 {
        let mut varied = sources.clone();
        for (slot, source) in varied.iter_mut().enumerate().skip(5) {
            if pattern & (1 << (slot - 5)) != 0 {
                *source = None;
            }
        }
        cache.record(&varied);
    }
    let entries = cache.entries.lock().unwrap();
    assert!(entries.records.len() <= CACHE_ENTRIES);
    assert!(entries.bytes <= CACHE_BYTES);
    drop(entries);
    let rebuilt = cache.record(&sources);
    assert_eq!(first, rebuilt);
    assert_ne!(first.words().as_ptr(), rebuilt.words().as_ptr());
}

/// A legal full-size palette makes retained source bytes dominate cache entries.
fn large_storage() -> Arc<BiomeStorage> {
    let mut bytes = vec![(16 << 1) | 1];
    bytes.extend(std::iter::repeat_n(
        0_u8,
        world::BLOCKS_PER_SUB_CHUNK.div_ceil(u32::BITS as usize / 16) * size_of::<u32>(),
    ));
    let mut append = |value: u32| {
        let mut encoded = value << 1;
        while encoded >= 128 {
            bytes.push((encoded as u8 & 127) | 128);
            encoded >>= 7;
        }
        bytes.push(encoded as u8);
    };
    append(world::BLOCKS_PER_SUB_CHUNK as u32);
    for id in 32..32 + world::BLOCKS_PER_SUB_CHUNK as u32 {
        append(id);
    }
    let storage = DecodedBiomeColumn::decode(0, 1, &bytes, &RawBiomeIds { default_biome: 0 })
        .storage(0)
        .unwrap();
    assert_eq!(storage.palette().len(), world::BLOCKS_PER_SUB_CHUNK);
    storage
}

#[test]
fn retained_source_byte_budget_evicts_before_the_entry_limit() {
    let mut stream = stream();
    let cache = stream.biome_record_cache(0);
    let large = large_storage();
    let sources = std::array::from_fn(|_| Some(large.clone()));
    let first = cache.record(&sources);
    for _ in 0..20 {
        let source = Arc::new((*large).clone());
        cache.record(&std::array::from_fn(|_| Some(source.clone())));
    }
    let entries = cache.entries.lock().unwrap();
    assert!(entries.bytes <= CACHE_BYTES);
    assert!(entries.records.len() < 20);
    drop(entries);
    let rebuilt = cache.record(&sources);
    assert_eq!(first, rebuilt);
    assert_ne!(first.words().as_ptr(), rebuilt.words().as_ptr());
}

/// Extruded missing biomes retain palette counts even when their encoded words match raw input.
#[test]
fn biome_cache_charges_edit_metadata_in_identically_encoded_sources() {
    let mut bytes = vec![3];
    for _ in 0..world::BLOCKS_PER_SUB_CHUNK / u32::BITS as usize {
        bytes.extend(0xffff0000_u32.to_le_bytes());
    }
    bytes.extend([4, 14, 18, 0xff]);
    let column = DecodedBiomeColumn::decode(0, 2, &bytes, &RawBiomeIds { default_biome: 0 });
    let raw = column.storage(0).unwrap();
    let extruded = column.storage(1).unwrap();
    assert_eq!(raw, extruded);
    let extra = extruded.retained_heap_bytes_bound() - raw.retained_heap_bytes_bound();
    assert!(extra > 0);
    let mut stream = stream();
    let cache = stream.biome_record_cache(0);
    let first = cache.record(&std::array::from_fn(|_| Some(Arc::clone(&raw))));
    let raw_bytes = cache.entries.lock().unwrap().bytes;
    let second = cache.record(&std::array::from_fn(|_| Some(Arc::clone(&extruded))));
    assert_eq!(first, second);
    let entries = cache.entries.lock().unwrap();
    assert_eq!(entries.records.len(), 2);
    let extruded_bytes = entries.bytes - raw_bytes;
    assert_eq!(
        extruded_bytes - raw_bytes,
        (BIOME_NEIGHBOUR_SLOT_COUNT * extra) as u64
    );
    assert!(entries.bytes <= CACHE_BYTES);
}
