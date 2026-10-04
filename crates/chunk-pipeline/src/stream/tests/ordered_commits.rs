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
