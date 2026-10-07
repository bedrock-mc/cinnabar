use super::*;

#[test]
fn newer_update_waits_for_older_decode_and_wins() {
    let key = SubChunkKey::new(0, 0, -4, 0);
    let decoded = DecodedLevelChunk::decode(
        -4,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
        &RAW_IDS,
    );
    let mut ordered = client_world::ingestion::OrderedCommitState::new(1);
    ordered.admit(2, true, 0).unwrap();
    ordered
        .insert_ready(
            2,
            PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: key.dimension,
                position: [0, key.y * 16, 0],
                layer: 0,
                network_id: 99,
            }])),
        )
        .unwrap();
    assert!(ordered.next_commit().is_none(), "sequence two must wait");
    ordered.admit(1, true, 0).unwrap();
    ordered
        .insert_ready(
            1,
            PreparedWorldEvent::InlineLevelChunk {
                event: LevelChunkEvent {
                    dimension: key.dimension,
                    x: key.x,
                    z: key.z,
                    mode: LevelChunkMode::Inline { count: 1 },
                    payload: Vec::new(),
                },
                decoded,
                duration: Duration::ZERO,
            },
        )
        .unwrap();

    let mut store = ChunkStore::new();
    while let Some(step) = ordered.next_commit() {
        let sequence = match step {
            CommitStep::Apply {
                sequence,
                event: PreparedWorldEvent::InlineLevelChunk { event, decoded, .. },
            } => {
                store
                    .commit_level_chunk(ChunkKey::new(event.dimension, event.x, event.z), decoded)
                    .unwrap();
                sequence
            }
            CommitStep::BlockUpdates { sequence, events } => {
                assert_eq!(events.len(), 1);
                let (key, update) = split_block_update(events.into_iter().next().unwrap()).unwrap();
                store.update_block(key, update, RAW_IDS.air).unwrap();
                sequence
            }
            other => panic!("unexpected commit step {other:?}"),
        };
        assert!(ordered.finish_commit(sequence));
    }

    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(99)
    );
}

/// Parallel decode workers may finish out of order; commits must still follow wire order.
#[test]
fn out_of_order_decode_completions_commit_in_sequence() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream
        .submit(1, inline_block_entity_event(0, 5, Vec::new()))
        .unwrap();
    stream
        .submit(2, inline_block_entity_event(0, 7, Vec::new()))
        .unwrap();
    let completions: Vec<_> = std::iter::from_fn(|| stream.pending_decode.pop_front())
        .map(|queued| queued.job.run(queued.queued_at))
        .collect();
    assert_eq!(completions.len(), 2);

    let mut completions = completions.into_iter().rev();
    stream.accept_decode_completion(completions.next().unwrap());
    stream.apply_ready();
    assert_eq!(stream.order.next_sequence(), 1, "sequence two must wait");
    assert!(stream.authority.terrain().sub_chunk(key).is_none());

    stream.accept_decode_completion(completions.next().unwrap());
    // Each pass commits at least one ready event; the frame budget may stop it after one.
    stream.apply_ready();
    stream.apply_ready();
    assert_eq!(stream.order.next_sequence(), 3);
    assert_eq!(
        stream
            .authority
            .terrain()
            .sub_chunk(key)
            .unwrap()
            .runtime_id(0, 0, 0, 0),
        Some(7),
        "the later wire column must win"
    );
}

#[test]
fn control_effects_are_exposed_only_after_older_heavy_sequence_commits_in_fifo_order() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let movement = MovePlayerEvent {
        runtime_id: 1,
        position: [4.0, 70.0, 5.0],
        pitch: 7.0,
        yaw: 9.0,
        ..Default::default()
    };
    let change = ChangeDimensionEvent {
        dimension: 1,
        position: [8.0, 80.0, 9.0],
        ..Default::default()
    };
    stream.submit(1, inline_air_event(0)).unwrap();
    stream.submit(2, WorldEvent::MovePlayer(movement)).unwrap();
    stream
        .submit(3, WorldEvent::ChangeDimension(change))
        .unwrap();

    assert_eq!(stream.current_dimension(), 0);
    assert!(stream.take_committed_controls().is_empty());

    let super::DecodeJob::InlineLevelChunk {
        event,
        payload,
        slots,
        count,
        ids,
        ..
    } = stream.pending_decode.pop_front().unwrap().job
    else {
        panic!("expected inline decode job")
    };
    let chunk = ChunkKey::new(event.dimension, event.x, event.z);
    let decoded = DecodedLevelChunk::decode_inline(chunk, slots, count, &payload, &ids, &ids);
    stream
        .order
        .insert_ready(
            1,
            super::PreparedWorldEvent::InlineLevelChunk {
                event,
                decoded,
                duration: std::time::Duration::ZERO,
            },
        )
        .unwrap();
    // Force one FIFO event per poll slice. A heavy commit can spend the normal frame
    // budget, so controls need later slices even though all three events are ready.
    stream.poll_deadline = Some(Instant::now());
    stream.polling = true;
    stream.apply_ready();
    assert_eq!(stream.order.next_sequence(), 2);
    assert_eq!(stream.current_dimension(), 0);
    assert!(stream.take_committed_controls().is_empty());

    stream.apply_ready();
    assert_eq!(stream.order.next_sequence(), 3);
    assert_eq!(stream.current_dimension(), 0);
    stream.apply_ready();
    assert_eq!(stream.order.next_sequence(), 4);
    assert_eq!(stream.current_dimension(), 1);
    assert_eq!(
        stream.take_committed_controls(),
        vec![
            super::CommittedControlEvent::MovePlayer {
                sequence: 2,
                movement,
                resolved: super::server_position::ResolvedServerPosition {
                    position: movement.position,
                    surface_anchor: None,
                },
                source_cohort: None,
            },
            super::CommittedControlEvent::ChangeDimension {
                sequence: 3,
                change,
                resolved: super::server_position::ResolvedServerPosition {
                    position: change.position,
                    surface_anchor: None,
                },
            },
        ]
    );
}

#[test]
fn movement_correction_commits_in_fifo_without_move_player_capture_metadata() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let correction = PlayerMovementCorrectionEvent {
        position: [27.5, 111.0, 91.5],
        delta: [0.25, -0.5, 0.75],
        pitch: -12.0,
        yaw: 143.0,
        subject: MovementCorrectionSubject::Player,
        on_ground: true,
        tick: 4_096,
    };
    stream.submit(1, inline_air_event(0)).unwrap();
    stream
        .submit(2, WorldEvent::PlayerMovementCorrection(correction))
        .unwrap();

    assert!(stream.take_committed_controls().is_empty());

    let super::DecodeJob::InlineLevelChunk {
        event,
        payload,
        slots,
        count,
        ids,
        ..
    } = stream.pending_decode.pop_front().unwrap().job
    else {
        panic!("expected inline decode job")
    };
    let chunk = ChunkKey::new(event.dimension, event.x, event.z);
    let decoded = DecodedLevelChunk::decode_inline(chunk, slots, count, &payload, &ids, &ids);
    stream
        .order
        .insert_ready(
            1,
            super::PreparedWorldEvent::InlineLevelChunk {
                event,
                decoded,
                duration: std::time::Duration::ZERO,
            },
        )
        .unwrap();
    // One event per slice: a wall-clock budget must not decide whether the correction commits.
    stream.poll_deadline = Some(Instant::now());
    stream.polling = true;
    stream.apply_ready();
    assert_eq!(stream.order.next_sequence(), 2);
    assert!(stream.take_committed_controls().is_empty());
    stream.apply_ready();

    assert_eq!(
        stream.take_committed_controls(),
        vec![super::CommittedControlEvent::PlayerMovementCorrection {
            sequence: 2,
            correction,
            resolved: super::server_position::ResolvedServerPosition {
                position: correction.position,
                surface_anchor: None,
            },
        }]
    );
}
