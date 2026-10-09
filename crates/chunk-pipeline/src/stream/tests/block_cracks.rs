use super::*;

fn fixture() -> WorldStream {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 2,
            block_network_ids_are_hashes: false,
        },
        Arc::new(non_default_air_runtime_assets()),
        [0.0; 3],
        None,
    );
    let mut payload = vec![9, 1, (-4_i8) as u8, 1, 0];
    payload.extend(biome_payload(0, 1));
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream
}

fn crack(stream: &mut WorldStream, sequence: u64, position: [i32; 3], action: BlockCrackAction) {
    stream
        .submit(
            sequence,
            WorldEvent::BlockCrack(BlockCrackEvent { position, action }),
        )
        .unwrap();
    // Production admission bounds the handoff batch independently of active state.
    stream.take_committed_ui();
}

fn start() -> BlockCrackAction {
    BlockCrackAction::Start {
        progress_per_tick: 7,
    }
}

/// A stationary start must remain available for a later speed update.
#[test]
fn block_crack_zero_start_can_advance_pause_and_resume() {
    let mut stream = fixture();
    let position = [0, -64, 0];
    crack(
        &mut stream,
        2,
        position,
        BlockCrackAction::Start {
            progress_per_tick: 0,
        },
    );
    let started = stream.block_crack_snapshot();
    assert_eq!(started.entries.len(), 1);
    assert_eq!(started.entries[0].server_value, 0);
    for (sequence, rate) in [(3, 1_092), (4, 0), (5, 2_184)] {
        crack(
            &mut stream,
            sequence,
            position,
            BlockCrackAction::UpdateSpeed {
                progress_per_tick: rate,
            },
        );
        let snapshot = stream.block_crack_snapshot();
        assert_eq!(snapshot.entries[0].start_sequence, 2);
        assert_eq!(snapshot.entries[0].server_value, rate);
        assert_eq!(snapshot.status.orphan_updates, 0);
        assert_eq!(snapshot.status.unsupported_values, 0);
    }
    crack(&mut stream, 6, position, BlockCrackAction::Stop);
    assert!(stream.block_crack_snapshot().entries.is_empty());
}

fn update(stream: &mut WorldStream, sequence: u64, position: [i32; 3], network_id: u32) {
    stream
        .submit(
            sequence,
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position,
                layer: 0,
                network_id,
            }]),
        )
        .unwrap();
    complete_pending_decode_jobs(stream);
}

#[test]
fn block_crack_ordered_mutations_preserve_only_the_exact_target() {
    let mut stream = fixture();
    crack(&mut stream, 2, [0, -64, 0], start());
    update(&mut stream, 3, [1, -64, 0], 1);
    assert_eq!(stream.block_crack_snapshot().status.active, 1);
    update(&mut stream, 4, [0, -64, 0], 1);
    update(&mut stream, 5, [0, -64, 0], 0);
    assert_eq!(stream.block_crack_snapshot().status.active, 0);
    assert_eq!(stream.block_crack_snapshot().status.retired_targets, 1);
}

#[test]
fn block_crack_capacity_retirement_then_new_start_has_one_admission_authority() {
    let mut stream = fixture();
    let positions = (0..MAX_ACTIVE_BLOCK_CRACKS)
        .map(|index| {
            [
                i32::try_from(index % 16).unwrap(),
                -64 + i32::try_from(index / 256).unwrap(),
                i32::try_from((index / 16) % 16).unwrap(),
            ]
        })
        .collect::<Vec<_>>();
    for (index, position) in positions.iter().enumerate() {
        crack(&mut stream, index as u64 + 2, *position, start());
    }
    let mut sequence = MAX_ACTIVE_BLOCK_CRACKS as u64 + 2;
    let new_position = [0, -60, 0];
    crack(&mut stream, sequence, new_position, start());
    assert_eq!(stream.block_crack_snapshot().status.capacity_rejections, 1);
    sequence += 1;
    crack(
        &mut stream,
        sequence,
        positions[1],
        BlockCrackAction::UpdateSpeed {
            progress_per_tick: 65535,
        },
    );
    assert_eq!(
        stream
            .block_crack_snapshot()
            .entries
            .iter()
            .find(|entry| entry.position == positions[1])
            .unwrap()
            .server_value,
        65535
    );
    sequence += 1;
    update(&mut stream, sequence, positions[0], 1);
    sequence += 1;
    crack(&mut stream, sequence, new_position, start());
    assert_eq!(
        stream.block_crack_snapshot().status.active,
        MAX_ACTIVE_BLOCK_CRACKS
    );
    assert!(
        stream
            .block_crack_snapshot()
            .entries
            .iter()
            .any(|entry| entry.position == new_position)
    );
    sequence += 1;
    crack(&mut stream, sequence, positions[1], BlockCrackAction::Stop);
    assert_eq!(
        stream.block_crack_snapshot().status.active,
        MAX_ACTIVE_BLOCK_CRACKS - 1
    );
    sequence += 1;
    crack(
        &mut stream,
        sequence,
        positions[1],
        BlockCrackAction::UpdateSpeed {
            progress_per_tick: 8,
        },
    );
    sequence += 1;
    crack(
        &mut stream,
        sequence,
        new_position,
        BlockCrackAction::Start {
            progress_per_tick: 0,
        },
    );
    let status = stream.block_crack_snapshot().status;
    assert_eq!(status.orphan_updates, 1);
    assert_eq!(status.unsupported_values, 0);
    assert_eq!(status.active, MAX_ACTIVE_BLOCK_CRACKS - 1);
    assert_eq!(
        stream
            .block_crack_snapshot()
            .entries
            .iter()
            .find(|entry| entry.position == new_position)
            .unwrap()
            .server_value,
        0
    );
    assert!(stream.take_fatal_error().is_none());
}

#[test]
fn block_crack_same_dimension_replacement_fences_and_session_replacement_is_fresh() {
    let mut stream = fixture();
    crack(&mut stream, 2, [0, -64, 0], start());
    stream
        .submit(
            3,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 0,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .unwrap();
    let snapshot = stream.block_crack_snapshot();
    assert_eq!(snapshot.status.active, 0);
    assert_eq!(snapshot.dimension_sequence, Some(3));
    let fresh = fixture().block_crack_snapshot();
    assert_ne!(snapshot.session_id, fresh.session_id);
    assert_eq!(fresh.status, BlockCrackStatus::default());
    assert_eq!(fresh.dimension_sequence, None);
}

#[test]
fn block_crack_requested_subchunks_retire_before_identical_reload() {
    let mut stream = fixture();
    crack(&mut stream, 2, [0, -64, 0], start());
    stream
        .submit(
            3,
            WorldEvent::ChunkResync(ChunkResyncEvent {
                dimension: 0,
                x: 0,
                z: 0,
                requested_sub_chunks: None,
                requested_sub_chunk_ys: Some(vec![-4]),
            }),
        )
        .unwrap();
    stream
        .submit(
            4,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    diagnostics: None,
                    position: [0, -4, 0],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.block_crack_snapshot().status.active, 0);
    stream
        .submit(
            5,
            WorldEvent::ChunkResync(ChunkResyncEvent {
                dimension: 0,
                x: 0,
                z: 0,
                requested_sub_chunks: None,
                requested_sub_chunk_ys: Some(vec![-4]),
            }),
        )
        .unwrap();
    stream
        .submit(
            6,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    diagnostics: None,
                    position: [0, -4, 0],
                    result: SubChunkResult::Success {
                        payload: vec![9, 1, (-4_i8) as u8, 1, 0],
                    },
                }],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream
            .authority
            .terrain()
            .sub_chunk(SubChunkKey::new(0, 0, -4, 0))
            .unwrap()
            .runtime_id(0, 0, 0, 0),
        Some(0)
    );
    assert_eq!(stream.block_crack_snapshot().status.active, 0);
    assert_eq!(stream.block_crack_snapshot().status.retired_targets, 1);
}

#[test]
fn block_crack_layer_replacement_and_unloaded_start_are_semantic_not_fatal() {
    let mut stream = fixture();
    crack(&mut stream, 2, [0, -64, 0], start());
    stream
        .submit(
            3,
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [0, -64, 0],
                layer: 1,
                network_id: 1,
            }]),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.block_crack_snapshot().status.active, 0);
    crack(&mut stream, 4, [1000, -64, 1000], start());
    assert_eq!(stream.block_crack_snapshot().status.unsupported_targets, 1);
    assert!(stream.take_fatal_error().is_none());
}
