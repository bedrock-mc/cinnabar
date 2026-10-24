use super::*;

/// Sends one stored section or empty slot, followed by a uniform biome column.
fn inline_biome_event(block: Option<u32>, biome: i32) -> WorldEvent {
    let range = vanilla_dimension_range(0).unwrap();
    let mut payload = vec![
        9,
        u8::from(block.is_some()),
        range.base_sub_chunk_y as i8 as u8,
    ];
    if let Some(block) = block {
        payload.push(1);
        payload.extend(zig_zag_i32(block as i32));
    }
    payload.extend(biome_payload(0, biome));
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension: 0,
        x: 0,
        z: 0,
        mode: LevelChunkMode::Inline { count: 1 },
        payload,
    })
}

/// Installs current light throughout a decoded column before a metadata-only resend.
fn settled_biome_column(block: Option<u32>) -> (WorldStream, SubChunkKey) {
    let mut stream = super::super::light_scheduler::lit_stream(0);
    stream.apply_immediate(
        WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
            definitions: Arc::from([(7, 0.2, 0xff44_6688), (9, 0.8, 0xff88_aa44)].map(
                |(id, temperature, color)| BiomeDefinitionEvent {
                    biome_id: Some(id),
                    name: Arc::from(format!("test:biome_{id}")),
                    temperature,
                    downfall: 0.4,
                    snow_foliage: 0.0,
                    max_snow_accumulation: None,
                    map_water_color: color,
                },
            )),
        }),
        None,
    );
    stream.submit(1, inline_biome_event(block, 7)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    for key in stream.resident.iter().copied().collect::<Vec<_>>() {
        install_current_light(&mut stream, key, 0, 15, true);
    }
    stream.mesh_jobs.pending.clear();
    stream.mesh_jobs.clear_queued();
    let key = SubChunkKey::new(
        0,
        0,
        vanilla_dimension_range(0).unwrap().base_sub_chunk_y,
        0,
    );
    (stream, key)
}

/// Captures an inline decode against the current column, leaving its commit to the caller.
fn prepare_biome_column(
    stream: &WorldStream,
    block: Option<u32>,
    biome: i32,
) -> PreparedWorldEvent {
    let WorldEvent::LevelChunk(event) = inline_biome_event(block, biome) else {
        unreachable!();
    };
    let key = ChunkKey::new(event.dimension, event.x, event.z);
    let previous = stream
        .authority
        .terrain()
        .chunk(key)
        .map(world::Chunk::shared_sub_chunks);
    let ids = stream.decode_ids(event.dimension);
    DecodeJob::InlineLevelChunk {
        sequence: 2,
        payload: bytes::Bytes::copy_from_slice(&event.payload),
        event,
        slots: client_world::ingestion::dimension_slots(vanilla_dimension_range(0).unwrap()),
        count: 1,
        ids,
        previous,
    }
    .run(Instant::now())
    .event
}

/// Builds and publishes one pending geometry mesh through the ordinary completion path.
fn publish_biome_mesh(stream: &mut WorldStream, key: SubChunkKey) -> PackedBiomeRecord {
    let pending = stream.mesh_jobs.pending.remove(&key).unwrap();
    let snapshot = stream.mesh_snapshot(
        key,
        stream.authority.terrain().sub_chunk(key).unwrap(),
        stream
            .mesh_light_halo(key)
            .expect("current light after metadata change"),
    );
    let assets = stream.authority.runtime_assets();
    let mode = stream.authority.network_id_mode();
    let biome = pack_biome_record(&snapshot.biomes, &stream.resolved_biome_tints_snapshot());
    let mesh = snapshot.mesh(stream.classifier, assets, mode, &biome);
    assert!(!mesh.is_empty());
    let dependency_mask = snapshot.dependency_mask(stream.classifier, assets, mode);
    stream.accept_mesh_completion(MeshCompletion {
        output_permit: None,
        _job_permit: None,
        key,
        revision: pending.revision,
        source: snapshot.center,
        biome_sources: snapshot.biomes,
        biome,
        tint_identity: stream.biome_tint_identity(),
        mesh,
        dependency_mask,
        light_halo: snapshot.light_halo,
        queue_wait: Duration::ZERO,
        dispatch_wait: Duration::ZERO,
        duration: Duration::ZERO,
        urgent: pending.urgent,
    });
    stream
        .take_mesh_changes()
        .into_iter()
        .find_map(|change| match change {
            WorldMeshChange::Upsert {
                key: changed,
                biome,
                ..
            } if changed == key => Some(biome),
            _ => None,
        })
        .expect("the new tint mesh must publish")
}

#[test]
fn biome_only_resends_preserve_solved_light() {
    for block in [Some(2), None] {
        let (mut stream, key) = settled_biome_column(block);
        let generations = stream.lighting.block_generations.clone();
        let next_generation = stream.lighting.next_block_generation;
        let light = Arc::clone(stream.lighting.store.light(key).unwrap());
        let direct = Arc::clone(&stream.lighting.direct_sky[&key].mask);
        let ownership = stream.lighting.ownership[&key];
        let blocks = stream.authority.terrain().sub_chunk(key);
        let dispatched = stream.stats.publication_stages.light_jobs_dispatched;

        stream.submit(2, inline_biome_event(block, 9)).unwrap();
        complete_pending_decode_jobs(&mut stream);

        assert_eq!(stream.authority.terrain().biome_id(key, 0, 0, 0), Some(9));
        let after = stream.authority.terrain().sub_chunk(key);
        if let Some(blocks) = blocks {
            assert!(Arc::ptr_eq(&blocks, &after.unwrap()));
        } else {
            assert!(after.is_none());
        }
        assert_eq!(
            stream.lighting.block_generations, generations,
            "block={block:?}"
        );
        assert_eq!(stream.lighting.next_block_generation, next_generation);
        assert_eq!(stream.lighting.ownership[&key], ownership);
        assert!(Arc::ptr_eq(
            stream.lighting.store.light(key).unwrap(),
            &light
        ));
        assert!(Arc::ptr_eq(&stream.lighting.direct_sky[&key].mask, &direct));
        assert!(
            stream
                .resident
                .iter()
                .all(|key| stream.light_is_current(*key))
        );
        assert!(stream.lighting.jobs.pending.is_empty());
        assert_eq!(stream.dispatch_light_jobs([8.0; 3], usize::MAX), 0);
        assert_eq!(
            stream.stats.publication_stages.light_jobs_dispatched,
            dispatched
        );
        if block.is_some() {
            assert!(stream.mesh_jobs.pending.contains_key(&key));
        }
    }
}

#[test]
fn emitter_and_filter_changes_in_a_biome_resend_still_relight() {
    for (before, after) in [(2, 1), (1, 2), (2, 3), (3, 2)] {
        let (mut stream, key) = settled_biome_column(Some(before));
        let generation = stream.lighting.block_generations[&key];
        stream
            .submit(2, inline_biome_event(Some(after), 9))
            .unwrap();
        complete_pending_decode_jobs(&mut stream);

        assert_eq!(
            stream
                .authority
                .terrain()
                .sub_chunk(key)
                .unwrap()
                .runtime_id(0, 8, 8, 8),
            Some(after)
        );
        assert!(stream.lighting.block_generations[&key] > generation);
        assert!(stream.lighting.jobs.pending.contains_key(&key));
        assert!(!stream.light_is_current(key));
    }
}

#[test]
fn new_residency_initializes_light_even_when_only_biomes_changed_in_the_store() {
    for block in [Some(2), None] {
        let mut stream = super::super::light_scheduler::lit_stream(0);
        define_custom_biomes(&mut stream, [7, 9]);
        let PreparedWorldEvent::InlineLevelChunk { decoded, .. } =
            prepare_biome_column(&stream, block, 7)
        else {
            unreachable!();
        };
        let key = SubChunkKey::new(
            0,
            0,
            vanilla_dimension_range(0).unwrap().base_sub_chunk_y,
            0,
        );
        stream
            .authority
            .commit_level_chunk(key.chunk(), decoded)
            .unwrap();
        assert!(stream.resident.is_empty());

        let prepared = prepare_biome_column(&stream, block, 9);
        stream.apply_prepared(prepared);

        assert_eq!(
            stream.resident.len(),
            vanilla_dimension_range(0).unwrap().sub_chunk_count
        );
        assert!(
            stream
                .resident
                .iter()
                .all(|key| stream.lighting.block_generations.contains_key(key))
        );
        assert!(stream.lighting.jobs.pending.contains_key(&key));
        assert!(!stream.light_is_current(key));
        assert_eq!(stream.known_air.contains(&key), block.is_none());
    }
}

#[test]
fn new_residency_overrides_a_worker_light_neutral_summary() {
    let (mut stream, key) = settled_biome_column(Some(2));
    stream.set_custom_block_ids(4..5);
    stream.resident.remove(&key);
    let generation = stream.lighting.block_generations[&key];
    let prepared = prepare_biome_column(&stream, Some(4), 9);
    let PreparedWorldEvent::InlineLevelChunk { light_neutral, .. } = &prepared else {
        unreachable!();
    };
    assert!(light_neutral.iter().any(|(y, _)| *y == key.y));

    stream.apply_prepared(prepared);

    assert!(stream.resident.contains(&key));
    assert!(stream.lighting.block_generations[&key] > generation);
    assert!(stream.lighting.jobs.pending.contains_key(&key));
    assert!(!stream.light_is_current(key));
}

#[test]
fn intervening_emitter_mutations_invalidate_prepared_biome_resend_proofs() {
    for (before, replacement, has_neutral_summary) in [(2, 2, false), (2, 4, true)] {
        let (mut stream, key) = settled_biome_column(Some(before));
        stream.set_custom_block_ids(4..5);
        let prepared = prepare_biome_column(&stream, Some(replacement), 9);
        let PreparedWorldEvent::InlineLevelChunk { light_neutral, .. } = &prepared else {
            unreachable!();
        };
        assert_eq!(
            light_neutral.iter().any(|(y, _)| *y == key.y),
            has_neutral_summary
        );
        stream
            .authority
            .update_block(key, BlockUpdate::new(8, 8, 8, 0, 1), 0)
            .unwrap();
        install_current_light(&mut stream, key, 15, 15, true);
        let generation = stream.lighting.block_generations[&key];

        stream.apply_prepared(prepared);

        assert_eq!(
            stream
                .authority
                .terrain()
                .sub_chunk(key)
                .unwrap()
                .runtime_id(0, 8, 8, 8),
            Some(replacement)
        );
        assert!(stream.lighting.block_generations[&key] > generation);
        assert!(stream.lighting.jobs.pending.contains_key(&key));
        assert!(!stream.light_is_current(key));
        super::super::light_scheduler::settle_light(&mut stream, [8.0; 3]);
        assert_eq!(
            stream
                .lighting
                .store
                .light(key)
                .unwrap()
                .get(LightChannel::Block, 8, 8, 8),
            Some(0)
        );
    }
}

#[test]
fn removing_stored_blocks_in_a_biome_resend_still_relights_air() {
    let (mut stream, key) = settled_biome_column(Some(1));
    let generation = stream.lighting.block_generations[&key];
    stream.submit(2, inline_biome_event(None, 9)).unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(stream.authority.terrain().sub_chunk(key).is_none());
    assert!(stream.known_air.contains(&key));
    assert!(stream.lighting.block_generations[&key] > generation);
    assert!(stream.lighting.jobs.pending.contains_key(&key));
}

#[test]
fn biome_only_resend_publishes_new_tint_without_new_light_work() {
    let (mut stream, key) = settled_biome_column(Some(2));
    stream.mark_dirty_exact(key, Instant::now());
    let before = publish_biome_mesh(&mut stream, key);
    let old_light_jobs = stream.stats.publication_stages.light_jobs_dispatched;
    let old_meshes = stream.stats.publication_stages.mesh_jobs_completed;
    let tints = stream.resolved_biome_tints_snapshot();
    assert_eq!(before.tint_index(8, 8, 8), Some(tints.dense_index(7)));
    assert_ne!(tints.dense_index(7), tints.dense_index(9));

    stream.submit(2, inline_biome_event(Some(2), 9)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    let after = publish_biome_mesh(&mut stream, key);

    assert_eq!(after.tint_index(8, 8, 8), Some(tints.dense_index(9)));
    assert_ne!(before, after);
    assert_eq!(
        stream.stats.publication_stages.mesh_jobs_completed,
        old_meshes + 1
    );
    assert_eq!(stream.dispatch_light_jobs([8.0; 3], usize::MAX), 0);
    assert_eq!(
        stream.stats.publication_stages.light_jobs_dispatched,
        old_light_jobs
    );
}

#[test]
fn known_air_biome_resend_remeshes_diagonal_tint_halo_without_relighting() {
    let (mut stream, air) = settled_biome_column(None);
    let geometry = SubChunkKey::new(0, 1, air.y, 1);
    stream
        .authority
        .commit_sub_chunk(geometry, uniform_sub_chunk(2))
        .unwrap();
    let range = vanilla_dimension_range(0).unwrap();
    stream.authority.commit_biome_column(
        geometry.chunk(),
        DecodedBiomeColumn::decode(
            range.base_sub_chunk_y,
            range.sub_chunk_count,
            &biome_payload(0, 7),
            &RAW_BIOMES,
        ),
    );
    install_current_light(&mut stream, geometry, 0, 15, true);
    stream.mark_dirty_exact(geometry, Instant::now());
    let before = publish_biome_mesh(&mut stream, geometry);
    let generations = stream.lighting.block_generations.clone();
    let light = Arc::clone(stream.lighting.store.light(geometry).unwrap());
    let tints = stream.resolved_biome_tints_snapshot();
    assert_eq!(
        before.tint_index_at([-1, 8, -1]),
        Some(tints.dense_index(7))
    );

    stream.submit(2, inline_biome_event(None, 9)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    let after = publish_biome_mesh(&mut stream, geometry);

    assert_eq!(after.tint_index_at([-1, 8, -1]), Some(tints.dense_index(9)));
    assert_eq!(after.tint_index(8, 8, 8), Some(tints.dense_index(7)));
    assert_eq!(stream.lighting.block_generations, generations);
    assert!(Arc::ptr_eq(
        stream.lighting.store.light(geometry).unwrap(),
        &light
    ));
    assert!(stream.lighting.jobs.pending.is_empty());
}
