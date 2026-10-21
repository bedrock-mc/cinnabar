use super::*;

#[test]
fn block_entity_visual_diagnostics_preserve_zero_remesh_live_updates() {
    let mut stream = block_entity_visual_stream();
    let routes = [
        ("Barrel", "minecraft:barrel"),
        ("BlastFurnace", "minecraft:blast_furnace"),
        ("Furnace", "minecraft:lit_furnace"),
        ("Smoker", "minecraft:smoker"),
        ("Jukebox", "minecraft:jukebox"),
    ]
    .map(|(id, name)| (id, block_entity_runtime_id(name)));
    let mut sequence = 1;
    for (chunk_x, (id, runtime_id)) in routes.into_iter().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        stream
            .submit(
                sequence,
                inline_block_entity_event(
                    chunk_x as i32,
                    runtime_id,
                    block_entity_nbt(id, position),
                ),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }
    let note_position = [5 * 16 + 1, -63, 2];
    stream
        .submit(
            sequence,
            inline_block_entity_event(
                5,
                block_entity_runtime_id("minecraft:noteblock"),
                idless_note_block_entity_nbt(note_position, 24, 1, 0),
            ),
        )
        .unwrap();
    sequence += 1;
    complete_pending_decode_jobs(&mut stream);

    let stats = stream.stats();
    assert_eq!(stats.adjudicated_static_block_entities, 4);
    assert_eq!(stats.adjudicated_logical_block_entities, 2);
    assert_eq!(stats.deferred_block_entities, 0);
    assert_eq!(stats.unknown_block_entities, 0);

    let revision_before = stream.revisions.next_revision;
    let pending_before = stream.mesh_jobs.pending.len();
    let changes_before = stream.mesh_changes.len();
    for (chunk_x, (id, _)) in routes.into_iter().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        stream
            .submit(
                sequence,
                WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                    dimension: 0,
                    position,
                    nbt: block_entity_nbt_with_marker(id, position, 1),
                }),
            )
            .unwrap();
        sequence += 1;
    }
    stream
        .submit(
            sequence,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position: note_position,
                nbt: idless_note_block_entity_nbt(note_position, 0, 0, 1),
            }),
        )
        .unwrap();
    sequence += 1;
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.revisions.next_revision, revision_before);
    assert_eq!(stream.mesh_jobs.pending.len(), pending_before);
    assert_eq!(stream.mesh_changes.len(), changes_before);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);

    let retained = stream.authority.terrain().block_entity(BlockEntityKey::new(
        0,
        note_position[0],
        note_position[1],
        note_position[2],
    ));
    stream
        .submit(
            sequence,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position: note_position,
                nbt: vec![10, 0, 8],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(Arc::ptr_eq(
        retained.as_ref().unwrap(),
        &stream
            .authority
            .terrain()
            .block_entity(BlockEntityKey::new(
                0,
                note_position[0],
                note_position[1],
                note_position[2],
            ))
            .unwrap()
    ));
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);
    assert_eq!(stream.revisions.next_revision, revision_before);
}

#[test]
fn block_entity_visual_diagnostics_preserve_zero_churn_inline_nbt_replacements() {
    let mut stream = block_entity_visual_stream();
    let mut cases = [
        ("Barrel", "minecraft:barrel"),
        ("BlastFurnace", "minecraft:blast_furnace"),
        ("Furnace", "minecraft:lit_furnace"),
        ("Smoker", "minecraft:smoker"),
        ("Jukebox", "minecraft:jukebox"),
        ("", "minecraft:noteblock"),
    ]
    .map(|(id, name)| (id, block_entity_runtime_id(name), Vec::new(), Vec::new()));
    for (chunk_x, (id, _, initial, replacement)) in cases.iter_mut().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        if id.is_empty() {
            *initial = idless_note_block_entity_nbt(position, 24, 1, 0);
            *replacement = idless_note_block_entity_nbt(position, 0, 0, 1);
        } else {
            *initial = block_entity_nbt(id, position);
            *replacement = block_entity_nbt_with_marker(id, position, 1);
        }
    }

    let mut sequence = 1;
    for (chunk_x, (_, runtime_id, initial, _)) in cases.iter().enumerate() {
        stream
            .submit(
                sequence,
                inline_block_entity_event(chunk_x as i32, *runtime_id, initial.clone()),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);

    let mesh_revision_before = stream.revisions.next_revision;
    let block_generation_before = stream.lighting.next_block_generation;
    let light_revision_before = stream.lighting.revisions.next_revision;
    let render_generation_before = stream.connectivity_generation;
    let block_generations_before = stream.lighting.block_generations.clone();
    let applied_mesh_generations_before = stream.applied_mesh_generations.clone();
    let pending_mesh_before = stream.mesh_jobs.pending.len();
    let pending_light_before = stream.lighting.jobs.pending.len();
    let mesh_changes_before = stream.mesh_changes.len();

    for (chunk_x, (_, runtime_id, _, replacement)) in cases.iter().enumerate() {
        stream
            .submit(
                sequence,
                inline_block_entity_event(chunk_x as i32, *runtime_id, replacement.clone()),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }

    assert_eq!(stream.revisions.next_revision, mesh_revision_before);
    assert_eq!(
        stream.lighting.next_block_generation,
        block_generation_before
    );
    assert_eq!(
        stream.lighting.revisions.next_revision,
        light_revision_before
    );
    assert_eq!(stream.connectivity_generation, render_generation_before);
    assert_eq!(stream.lighting.block_generations, block_generations_before);
    assert_eq!(
        stream.applied_mesh_generations,
        applied_mesh_generations_before
    );
    assert_eq!(stream.mesh_jobs.pending.len(), pending_mesh_before);
    assert_eq!(stream.lighting.jobs.pending.len(), pending_light_before);
    assert_eq!(stream.mesh_changes.len(), mesh_changes_before);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);
    for (chunk_x, (_, _, _, replacement)) in cases.iter().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        let retained = stream
            .authority
            .terrain()
            .block_entity(BlockEntityKey::new(
                0,
                position[0],
                position[1],
                position[2],
            ))
            .expect("inline replacement retained");
        assert_eq!(retained.bytes(), replacement);
    }
}

#[test]
fn block_entity_visual_diagnostics_preserve_zero_remesh_request_mode_nbt_replacements() {
    let mut stream = block_entity_visual_stream();
    let mut cases = [
        ("Barrel", "minecraft:barrel"),
        ("BlastFurnace", "minecraft:blast_furnace"),
        ("Furnace", "minecraft:lit_furnace"),
        ("Smoker", "minecraft:smoker"),
        ("Jukebox", "minecraft:jukebox"),
        ("", "minecraft:noteblock"),
    ]
    .map(|(id, name)| (id, block_entity_runtime_id(name), Vec::new(), Vec::new()));
    for (chunk_x, (id, _, initial, replacement)) in cases.iter_mut().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        if id.is_empty() {
            *initial = idless_note_block_entity_nbt(position, 24, 1, 0);
            *replacement = idless_note_block_entity_nbt(position, 0, 0, 1);
        } else {
            *initial = block_entity_nbt(id, position);
            *replacement = block_entity_nbt_with_marker(id, position, 1);
        }
    }

    let mut sequence = 1;
    for (chunk_x, (_, runtime_id, initial, _)) in cases.iter().enumerate() {
        stream
            .submit(
                sequence,
                request_block_entity_event(chunk_x as i32, initial.clone()),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.take_requests().len(), 1);
        stream
            .submit(
                sequence,
                requested_block_entity_sub_chunk_event(
                    chunk_x as i32,
                    *runtime_id,
                    initial.clone(),
                ),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);

    let revision_before = stream.revisions.next_revision;
    let pending_before = stream.mesh_jobs.pending.len();
    let changes_before = stream.mesh_changes.len();
    for (chunk_x, (_, runtime_id, _, replacement)) in cases.iter().enumerate() {
        stream
            .submit(
                sequence,
                request_block_entity_event(chunk_x as i32, replacement.clone()),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.take_requests().len(), 1);
        stream
            .submit(
                sequence,
                requested_block_entity_sub_chunk_event(
                    chunk_x as i32,
                    *runtime_id,
                    replacement.clone(),
                ),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }

    assert_eq!(stream.revisions.next_revision, revision_before);
    assert_eq!(stream.mesh_jobs.pending.len(), pending_before);
    assert_eq!(stream.mesh_changes.len(), changes_before);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);
    for (chunk_x, (_, _, _, replacement)) in cases.iter().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        let retained = stream
            .authority
            .terrain()
            .block_entity(BlockEntityKey::new(
                0,
                position[0],
                position[1],
                position[2],
            ))
            .expect("request replacement retained");
        assert_eq!(retained.bytes(), replacement);
    }
}

#[test]
fn block_entity_request_mode_changed_biome_keeps_destructive_column_replacement() {
    let mut stream = block_entity_visual_stream();
    define_custom_biomes(&mut stream, [1, 2]);
    let position = [1, -63, 2];
    let key = SubChunkKey::new(0, 0, -4, 0);
    let initial = block_entity_nbt("Jukebox", position);
    stream
        .submit(1, request_block_entity_event(0, initial.clone()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);
    stream
        .submit(
            2,
            requested_block_entity_sub_chunk_event(
                0,
                block_entity_runtime_id("minecraft:jukebox"),
                initial,
            ),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.authority.terrain().sub_chunk(key).is_some());
    assert!(stream.resident.contains(&key));
    let revision_before = stream.revisions.next_revision;

    stream
        .submit(
            3,
            request_block_entity_event_with_biome(
                0,
                2,
                block_entity_nbt_with_marker("Jukebox", position, 1),
            ),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(stream.authority.terrain().sub_chunk(key).is_none());
    assert!(!stream.resident.contains(&key));
    assert!(stream.revisions.next_revision > revision_before);
    assert!(stream.mesh_jobs.pending.contains_key(&key));
    assert_eq!(stream.take_requests().len(), 1);
}

#[test]
fn block_entity_request_mode_changed_backing_dirties_and_replaces_preserved_column() {
    let mut stream = block_entity_visual_stream();
    let initial_id = block_entity_runtime_id("minecraft:barrel");
    let replacement_id = block_entity_visual_registry()
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:barrel" && record.sequential_id != initial_id
        })
        .expect("barrel has another pinned state")
        .sequential_id;
    let position = [1, -63, 2];
    let key = SubChunkKey::new(0, 0, -4, 0);
    let initial = block_entity_nbt("Barrel", position);
    stream
        .submit(1, request_block_entity_event(0, initial.clone()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);
    stream
        .submit(
            2,
            requested_block_entity_sub_chunk_event(0, initial_id, initial),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 1);
    let revision_before = stream.revisions.next_revision;
    let block_generation_before = stream.lighting.next_block_generation;

    let replacement = block_entity_nbt_with_marker("Barrel", position, 1);
    stream
        .submit(3, request_block_entity_event(0, replacement.clone()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.revisions.next_revision, revision_before);
    assert_eq!(stream.take_requests().len(), 1);
    stream
        .submit(
            4,
            requested_block_entity_sub_chunk_event(0, replacement_id, replacement),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert_eq!(stream.revisions.next_revision, revision_before);
    assert!(stream.lighting.next_block_generation > block_generation_before);
    assert!(stream.mesh_jobs.pending.contains_key(&key));
    assert_eq!(
        stream
            .authority
            .terrain()
            .sub_chunk(key)
            .and_then(|sub_chunk| sub_chunk.runtime_id(0, 1, 1, 2)),
        Some(replacement_id)
    );
    assert_eq!(stream.stats().adjudicated_static_block_entities, 1);
}

#[test]
fn block_entity_visual_diagnostics_follow_request_eviction_and_dimension_lifecycle() {
    let mut stream = block_entity_visual_stream();
    let position = [1, -63, 2];
    let key = BlockEntityKey::new(0, position[0], position[1], position[2]);
    let mut request_payload = biome_payload(0, 1);
    request_payload.extend(block_entity_nbt("Jukebox", position));
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: request_payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.authority.terrain().block_entity(key).is_some());
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 0);
    assert_eq!(stream.take_requests().len(), 1);

    let mut payload = vec![9, 1, (-4_i8) as u8, 1];
    payload.extend(zig_zag_i32(
        block_entity_runtime_id("minecraft:jukebox") as i32
    ));
    payload.extend(block_entity_nbt("Jukebox", position));
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    diagnostics: None,
                    position: [0, -4, 0],
                    result: SubChunkResult::Success { payload },
                }],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 1);

    stream.evict_column(ChunkKey::new(0, 0, 0));
    assert_eq!(stream.stats().adjudicated_static_block_entities, 0);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 0);
    assert_eq!(stream.stats().deferred_block_entities, 0);
    assert_eq!(stream.stats().unknown_block_entities, 0);

    let position = [17, -63, 2];
    stream
        .submit(
            3,
            inline_block_entity_event(
                1,
                block_entity_runtime_id("minecraft:barrel"),
                block_entity_nbt("Barrel", position),
            ),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 1);
    let deferred_position = [33, -63, 2];
    stream
        .submit(
            4,
            inline_block_entity_event(
                2,
                block_entity_runtime_id("minecraft:beacon"),
                block_entity_nbt("Beacon", deferred_position),
            ),
        )
        .unwrap();
    let unknown_position = [49, -63, 2];
    stream
        .submit(
            5,
            inline_block_entity_event(
                3,
                block_entity_runtime_id("minecraft:noteblock"),
                block_entity_nbt("Barrel", unknown_position),
            ),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().deferred_block_entities, 1);
    assert_eq!(stream.stats().unknown_block_entities, 1);
    stream
        .submit(
            6,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .unwrap();
    assert_eq!(stream.stats().adjudicated_static_block_entities, 0);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 0);
    assert_eq!(stream.stats().deferred_block_entities, 0);
    assert_eq!(stream.stats().unknown_block_entities, 0);

    let replacement = block_entity_visual_stream();
    assert_eq!(replacement.stats().adjudicated_static_block_entities, 0);
    assert_eq!(replacement.stats().adjudicated_logical_block_entities, 0);
}

#[test]
fn live_block_entity_updates_decode_off_thread_and_commit_in_fifo() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, inline_air_event(0)).unwrap();
    complete_pending_decode_jobs(&mut stream);

    let position = [1, -63, 2];
    let key = BlockEntityKey::new(0, position[0], position[1], position[2]);
    stream
        .submit(
            2,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position,
                nbt: block_entity_nbt("Chest", position),
            }),
        )
        .unwrap();
    let movement = MovePlayerEvent {
        runtime_id: 1,
        position: [1.0, 70.0, 2.0],
        pitch: 0.0,
        yaw: 0.0,
        ..Default::default()
    };
    stream.submit(3, WorldEvent::MovePlayer(movement)).unwrap();

    assert!(stream.authority.terrain().block_entity(key).is_none());
    assert!(stream.take_committed_controls().is_empty());
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream.authority.terrain().block_entity(key).unwrap().id(),
        Some("Chest")
    );
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [super::CommittedControlEvent::MovePlayer { sequence: 3, .. }]
    ));

    let before = stream.authority.terrain().block_entity(key).unwrap();
    stream
        .submit(
            4,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position,
                nbt: vec![1, 0, 0],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(Arc::ptr_eq(
        &before,
        &stream.authority.terrain().block_entity(key).unwrap()
    ));
    assert_eq!(stream.stats().decode_errors, 1);

    let invalid_position = [1, 320, 2];
    let invalid_key = BlockEntityKey::new(
        0,
        invalid_position[0],
        invalid_position[1],
        invalid_position[2],
    );
    stream
        .submit(
            5,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position: invalid_position,
                nbt: block_entity_nbt("Chest", invalid_position),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(
        stream
            .authority
            .terrain()
            .block_entity(invalid_key)
            .is_none()
    );
    assert_eq!(
        stream
            .stats()
            .normalization_reasons
            .invalid_block_entity_positions,
        1
    );
}

#[test]
fn inline_and_sub_chunk_ingestion_commit_sparse_block_entity_tails() {
    let bootstrap = WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    };
    let inline_position = [1, -63, 2];
    let inline_key = BlockEntityKey::new(
        0,
        inline_position[0],
        inline_position[1],
        inline_position[2],
    );
    let mut inline_payload = vec![9, 0, (-4_i8) as u8];
    inline_payload.extend(biome_payload(0, 1));
    inline_payload.extend(block_entity_nbt("Chest", inline_position));
    let mut stream = WorldStream::new(bootstrap);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: inline_payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream
            .authority
            .terrain()
            .block_entity(inline_key)
            .unwrap()
            .id(),
        Some("Chest")
    );

    let request_position = [5, -47, 6];
    let request_key = BlockEntityKey::new(
        0,
        request_position[0],
        request_position[1],
        request_position[2],
    );
    let mut request_payload = biome_payload(0, 2);
    request_payload.extend(block_entity_nbt("Note", request_position));
    let mut stream = WorldStream::new(bootstrap);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: request_payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream
            .authority
            .terrain()
            .block_entity(request_key)
            .unwrap()
            .id(),
        Some("Note")
    );

    let (mut stream, sub_chunk) = stream_with_one_expected_sub_chunk();
    let sub_chunk_position = [3, -63, 4];
    let sub_chunk_entity = BlockEntityKey::new(
        0,
        sub_chunk_position[0],
        sub_chunk_position[1],
        sub_chunk_position[2],
    );
    let mut payload = vec![9, 1, (-4_i8) as u8, 1];
    payload.extend(zig_zag_i32(7));
    payload.extend(block_entity_nbt("Sign", sub_chunk_position));
    let mut prepared = client_world::ingestion::prepare_sub_chunks(
        SubChunkBatchEvent {
            dimension: 0,
            entries: vec![SubChunkEntryEvent {
                diagnostics: None,
                position: [sub_chunk.x, sub_chunk.y, sub_chunk.z],
                result: SubChunkResult::Success { payload },
            }],
        },
        &test_decode_ids(),
    );
    apply_sub_chunk_result(&mut stream, sub_chunk, prepared.remove(0).result);
    assert_eq!(
        stream
            .authority
            .terrain()
            .block_entity(sub_chunk_entity)
            .unwrap()
            .id(),
        Some("Sign")
    );
}
