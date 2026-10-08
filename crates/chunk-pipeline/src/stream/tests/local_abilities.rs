use super::*;

fn ability(owner: i64, count: u32) -> WorldEvent {
    let mut body = owner.to_le_bytes().to_vec();
    body.extend_from_slice(&[0, 0, count as u8]);
    body.resize(body.len() + count as usize * 22, 0);
    WorldEvent::Abilities(protocol::decode_abilities_update(&body).unwrap())
}

#[test]
fn ability_fifo_waits_for_predecessor_and_joins_unique_not_runtime_identity() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        local_player_unique_id: 7,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(2, ability(7, 0)).unwrap();
    stream.apply_ready();
    assert!(stream.take_committed_ui().is_empty());
    stream.submit(1, ability(42, 33)).unwrap();
    stream.apply_ready();
    let events = stream.take_committed_ui();
    assert!(
        matches!(&events[..], [CommittedUiEvent::LocalAbilities { sequence: 2, event, stream_identity }] if event.actor_unique_id == 7 && *stream_identity == stream.biome_tint_identity().stream())
    );
    assert!(stream.submit(2, ability(7, 33)).is_err());
}

#[test]
fn ability_suffix_waits_for_actual_block_mutation_and_survives_malformed_chunk_prefix() {
    let mut stream = block_entity_visual_stream();
    let local = stream.authority().local_player_unique_id();
    stream.submit(1, inline_air_event(0)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.submit(2, worker_block_batch(0, 1)).unwrap();
    stream.submit(3, ability(local, 0)).unwrap();
    stream.apply_ready();
    assert_eq!(stream.order.blocking_block_updates(), Some(2));
    assert!(stream.take_committed_ui().is_empty());
    complete_pending_decode_jobs(&mut stream);
    assert!(matches!(
        &stream.take_committed_ui()[..],
        [CommittedUiEvent::LocalAbilities { sequence: 3, .. }]
    ));

    let mut stream = block_entity_visual_stream();
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: vec![0xff],
            }),
        )
        .unwrap();
    stream
        .submit(2, ability(stream.authority().local_player_unique_id(), 0))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.take_fatal_error().is_none());
    assert!(matches!(
        &stream.take_committed_ui()[..],
        [CommittedUiEvent::LocalAbilities { sequence: 2, .. }]
    ));
}

#[test]
fn genuine_zero_unique_identity_is_not_rejected_or_used_as_a_wildcard() {
    let mut stream = block_entity_visual_stream_for_player(0);
    stream.submit(1, ability(99, 33)).unwrap();
    stream.submit(2, ability(0, 0)).unwrap();
    stream.apply_ready();
    assert!(
        matches!(&stream.take_committed_ui()[..], [CommittedUiEvent::LocalAbilities { sequence: 2, event, .. }] if event.actor_unique_id == 0)
    );
}
