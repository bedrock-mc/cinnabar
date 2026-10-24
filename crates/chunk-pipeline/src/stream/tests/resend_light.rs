//! A resent inline column relights only the sections whose light inputs changed.

use super::{light_scheduler::install_current_light, *};

mod biome_only;

/// Commits chunk 1's single section as `runtime_id` with solved light and no queued meshes.
fn settled_column(runtime_id: u32) -> (WorldStream, SubChunkKey) {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 1, -4, 0);
    stream
        .submit(1, inline_block_entity_event(1, runtime_id, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    install_current_light(&mut stream, key, 0, 15, false);
    stream.mesh_jobs.pending.clear();
    stream.mesh_jobs.clear_queued();
    (stream, key)
}

/// Swapping blocks for others with the same light inputs remeshes without relighting.
#[test]
fn light_neutral_resend_remeshes_without_relighting() {
    let (mut stream, key) = settled_column(7);
    let before = stream.authority.terrain().sub_chunk(key).unwrap();

    stream
        .submit(2, inline_block_entity_event(1, 8, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    let after = stream.authority.terrain().sub_chunk(key).unwrap();
    assert!(
        !Arc::ptr_eq(&before, &after),
        "the resend replaced the section"
    );
    assert!(stream.light_is_current(key));
    assert!(!stream.lighting.jobs.pending.contains_key(&key));
    assert!(stream.mesh_jobs.pending.contains_key(&key));
}

/// A resend that changes light inputs, here to air, relights the section.
#[test]
fn light_changing_resend_relights() {
    let (mut stream, key) = settled_column(7);

    stream.submit(2, inline_air_event(1)).unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(!stream.light_is_current(key));
}

/// A summary compared against a section that is no longer resident cannot skip the relight.
#[test]
fn light_neutral_summary_of_a_replaced_section_relights() {
    let (mut stream, key) = settled_column(7);
    let mut payload = vec![9, 1, (-4_i8) as u8, 1];
    payload.extend(zig_zag_i32(8));

    stream.apply_prepared(PreparedWorldEvent::InlineLevelChunk {
        event: LevelChunkEvent {
            dimension: 0,
            x: 1,
            z: 0,
            mode: LevelChunkMode::Inline { count: 1 },
            payload: payload.clone(),
        },
        decoded: DecodedLevelChunk::decode(-4, 1, &payload, &RAW_IDS),
        // Equal blocks, but not the resident allocation the commit replaces.
        light_neutral: vec![(-4, Arc::new(uniform_sub_chunk(7)))],
        duration: std::time::Duration::ZERO,
    });

    assert!(!stream.light_is_current(key));
}

/// Byte-identical resends reuse the worker snapshot without decoding or dirtying terrain.
#[test]
fn identical_resend_reuses_blocks_without_new_decode_work() {
    let (mut stream, key) = settled_column(7);
    let resident = stream.authority.terrain().sub_chunk(key).unwrap();
    let before = stream.stats();
    stream
        .submit(2, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let after = stream.stats();
    assert_eq!(
        after.publication_stages.inline_sections_decoded,
        before.publication_stages.inline_sections_decoded
    );
    assert_eq!(
        after.publication_stages.inline_sections_reused
            - before.publication_stages.inline_sections_reused,
        1
    );
    assert!(Arc::ptr_eq(
        &resident,
        &stream.authority.terrain().sub_chunk(key).unwrap()
    ));
    assert!(!stream.mesh_jobs.pending.contains_key(&key));
    assert!(!stream.lighting.jobs.pending.contains_key(&key));
}

/// Equal resends after warm single edits preserve resident snapshots, light and mesh work.
#[test]
fn edited_section_resends_reuse_blocks_without_remesh_or_relight() {
    let (mut stream, key) = settled_column(7);
    let original = stream.authority.terrain().sub_chunk(key).unwrap();
    for (index, (y, value)) in [(0, 8), (1, 8), (0, 7), (1, 7)].into_iter().enumerate() {
        stream
            .submit(
                index as u64 + 2,
                WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                    dimension: key.dimension,
                    position: [key.x * 16, key.y * 16 + y, key.z * 16],
                    layer: 0,
                    network_id: value,
                }]),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
    }
    let edited = stream.authority.terrain().sub_chunk(key).unwrap();
    assert!(!Arc::ptr_eq(&original, &edited));
    assert!(original.blocks_match(&edited));
    install_current_light(&mut stream, key, 0, 15, false);
    stream.mesh_jobs.pending.clear();
    stream.mesh_jobs.clear_queued();

    let mut equivalent = vec![8, 1, 3];
    for _ in 0..world::BLOCKS_PER_SUB_CHUNK.div_ceil(u32::BITS as usize) {
        equivalent.extend(0xaaaa_aaaa_u32.to_le_bytes());
    }
    equivalent.extend([4, 7 << 1, 7 << 1]);
    equivalent.extend(biome_payload(0, 1));
    let equivalent = WorldEvent::LevelChunk(LevelChunkEvent {
        dimension: key.dimension,
        x: key.x,
        z: key.z,
        mode: LevelChunkMode::Inline { count: 1 },
        payload: equivalent,
    });
    for (index, event) in [
        inline_block_entity_event(1, 7, Vec::new()),
        inline_block_entity_event(1, 7, Vec::new()),
        equivalent.clone(),
        equivalent,
    ]
    .into_iter()
    .enumerate()
    {
        let before = stream.stats();
        let pending_lights = stream
            .lighting
            .jobs
            .pending
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        stream.submit(index as u64 + 6, event).unwrap();
        complete_pending_decode_jobs(&mut stream);
        let after = stream.stats();
        assert!(Arc::ptr_eq(
            &edited,
            &stream.authority.terrain().sub_chunk(key).unwrap()
        ));
        assert!(stream.mesh_jobs.pending.is_empty());
        assert_eq!(
            stream
                .lighting
                .jobs
                .pending
                .keys()
                .copied()
                .collect::<BTreeSet<_>>(),
            pending_lights
        );
        assert!(stream.light_is_current(key));
        assert_eq!(
            after.publication_stages.mesh_jobs_completed,
            before.publication_stages.mesh_jobs_completed
        );
        let decoded = after.publication_stages.inline_sections_decoded
            - before.publication_stages.inline_sections_decoded;
        let reused = after.publication_stages.inline_sections_reused
            - before.publication_stages.inline_sections_reused;
        assert_eq!(
            (decoded, reused),
            if index % 2 == 0 { (1, 0) } else { (0, 1) }
        );
    }
    assert_eq!(original.runtime_id(0, 0, 1, 0), Some(7));
}

/// Four packed sections, with an optional light-neutral replacement of the top one.
fn packed_column_payload(changed_top: bool) -> Vec<u8> {
    let range = vanilla_dimension_range(0).unwrap();
    let mut payload = Vec::new();
    for offset in 0..4 {
        let y = range.base_sub_chunk_y + offset;
        payload.extend([9, 1, y as i8 as u8]);
        if changed_top && offset == 3 {
            payload.extend([1, 1 << 1]);
        } else {
            payload.push(5);
            for _ in 0..world::BLOCKS_PER_SUB_CHUNK.div_ceil(16) {
                payload.extend(0x4444_4444_u32.to_le_bytes());
            }
            payload.extend([4, 0, 1 << 1]);
        }
    }
    payload.extend(biome_payload(0, 1));
    payload
}

/// Builds queued meshes on the test thread using production snapshots and publication.
fn build_pending_meshes(stream: &mut WorldStream) {
    let pending = std::mem::take(&mut stream.mesh_jobs.pending);
    stream.mesh_jobs.clear_queued();
    for (key, pending) in pending {
        let snapshot = stream.mesh_snapshot(
            key,
            stream
                .authority
                .terrain()
                .sub_chunk(key)
                .expect("resident geometry"),
            stream.mesh_light_halo(key).expect("settled light"),
        );
        let assets = stream.authority.runtime_assets();
        let mode = stream.authority.network_id_mode();
        let biome = pack_biome_record(
            &snapshot.biomes,
            &stream.decode_ids(key.dimension).biome_tints,
        );
        let mesh = snapshot.mesh(stream.classifier, assets, mode, &biome);
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
    }
    let now = test_time::ManualClock::new().now();
    for change in stream.take_mesh_changes() {
        let WorldMeshChange::Upsert {
            key,
            generation,
            dirty_since,
            ..
        } = change
        else {
            panic!("settled resends must not remove geometry");
        };
        stream.acknowledge_mesh_upload(key, generation, dirty_since, now);
    }
    assert_eq!(stream.stats().stale_mesh_jobs, 0);
}

/// Loads adjacent packed columns with current lighting and published dependency masks.
fn settled_packed_columns() -> WorldStream {
    let assets = Arc::new(non_default_air_runtime_assets());
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: assets.air_network_id(NetworkIdMode::Sequential).unwrap(),
            block_network_ids_are_hashes: false,
        },
        assets,
        [0.0; 3],
        None,
    );
    for x in 0..3 {
        stream
            .submit(
                x as u64 + 1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x,
                    z: 0,
                    mode: LevelChunkMode::Inline { count: 4 },
                    payload: packed_column_payload(false),
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
    }
    for key in stream.resident.iter().copied().collect::<Vec<_>>() {
        install_current_light(&mut stream, key, 0, 15, false);
    }
    stream.mesh_jobs.pending.clear();
    stream.mesh_jobs.clear_queued();
    let now = test_time::ManualClock::new().now();
    for x in 0..3 {
        for y in -4..0 {
            stream.mark_dirty_exact(SubChunkKey::new(0, x, y, 0), now);
        }
    }
    build_pending_meshes(&mut stream);
    stream
}

/// Uses either resident hash reuse or the ordinary decoder used before inline caching.
fn apply_packed_resend(stream: &mut WorldStream, x: i32, changed_top: bool, reuse: bool) {
    let chunk = ChunkKey::new(0, x, 0);
    let ids = stream.decode_ids(0);
    let previous = stream
        .authority
        .terrain()
        .chunk(chunk)
        .unwrap()
        .shared_sub_chunks();
    let payload = packed_column_payload(changed_top);
    let decoded = DecodedLevelChunk::decode_inline_reusing(
        chunk,
        client_world::ingestion::dimension_slots(vanilla_dimension_range(0).unwrap()),
        4,
        &payload,
        &ids,
        &ids,
        reuse.then_some(previous.as_ref()),
    );
    let light_neutral = client_world::ingestion::light_neutral_sections(&previous, &decoded, &ids);
    stream.apply_prepared(PreparedWorldEvent::InlineLevelChunk {
        event: LevelChunkEvent {
            dimension: 0,
            x,
            z: 0,
            mode: LevelChunkMode::Inline { count: 4 },
            payload,
        },
        decoded,
        light_neutral,
        duration: Duration::ZERO,
    });
}

/// An identical source cannot dirty itself, adjacent columns, or publish new meshes.
#[test]
fn all_identical_packed_resends_build_zero_meshes() {
    let mut stream = settled_packed_columns();
    let before = stream.stats();
    for x in 0..3 {
        apply_packed_resend(&mut stream, x, false, true);
        assert!(stream.mesh_jobs.pending.is_empty());
        assert!(stream.lighting.jobs.pending.is_empty());
        assert_eq!(stream.pending_mesh_change_count(), 0);
        build_pending_meshes(&mut stream);
    }
    let after = stream.stats();
    assert_eq!(
        after.publication_stages.mesh_jobs_completed
            - before.publication_stages.mesh_jobs_completed,
        0
    );
    assert_eq!(
        after.publication_stages.inline_sections_reused
            - before.publication_stages.inline_sections_reused,
        12
    );
}

/// Hash reuse cannot add mesh builds, even when each column's meshes finish before the next commit.
#[test]
fn mostly_identical_packed_resends_do_not_exceed_ordinary_decode_mesh_builds() {
    for finish_each_column in [false, true] {
        let mut builds = Vec::new();
        for reuse in [false, true] {
            let mut stream = settled_packed_columns();
            let before = stream.stats();
            for x in 0..3 {
                apply_packed_resend(&mut stream, x, true, reuse);
                for unchanged_y in -4..-2 {
                    for neighbour_x in 0..3 {
                        assert!(
                            stream.is_mesh_clean(SubChunkKey::new(0, neighbour_x, unchanged_y, 0)),
                            "unchanged lower sections do not depend on the changed top section"
                        );
                    }
                }
                assert!(stream.lighting.jobs.pending.is_empty());
                if finish_each_column {
                    build_pending_meshes(&mut stream);
                }
            }
            build_pending_meshes(&mut stream);
            let after = stream.stats();
            builds.push(
                after.publication_stages.mesh_jobs_completed
                    - before.publication_stages.mesh_jobs_completed,
            );
            if reuse {
                assert_eq!(
                    after.publication_stages.inline_sections_reused
                        - before.publication_stages.inline_sections_reused,
                    9
                );
            }
        }
        assert!(
            builds[0] > 0,
            "the ordinary decode control must build changed meshes"
        );
        assert!(
            builds[1] <= builds[0],
            "cached meshes={} ordinary decode meshes={}",
            builds[1],
            builds[0]
        );
    }
}
