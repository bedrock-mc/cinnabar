use super::*;

#[test]
fn negative_absolute_updates_use_euclidean_chunk_coordinates() {
    let event = BlockUpdateEvent {
        dimension: 2,
        position: [-1, -65, 16],
        layer: 1,
        network_id: 0xdead_beef,
    };
    let (key, update) = split_block_update(event).unwrap();

    assert_eq!(key, SubChunkKey::new(2, -1, -5, 1));
    assert_eq!(update, BlockUpdate::new(15, 15, 0, 1, 0xdead_beef));
}

#[test]
fn normalization_breakdown_distinguishes_inactive_and_malformed_world_traffic() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream.chunk_radius = Some(0);

    let batches = stream.snapshot_block_mutation_batches(vec![
        BlockUpdateEvent {
            dimension: 0,
            // Confirmed radius zero still retains the existing grid slack;
            // three columns away is outside both data-interest scopes.
            position: [48, 0, 0],
            layer: 0,
            network_id: 1,
        },
        BlockUpdateEvent {
            dimension: 0,
            position: [0, 0, 0],
            layer: usize::MAX,
            network_id: 2,
        },
    ]);
    assert!(batches.is_empty());

    stream.apply_prepared(super::PreparedWorldEvent::SubChunks {
        dimension: 0,
        entries: vec![
            super::PreparedSubChunk {
                diagnostics: None,
                position: [0, 0, 0],
                result: super::PreparedSubChunkResult::AllAir,
            },
            super::PreparedSubChunk {
                diagnostics: None,
                position: [3, 0, 0],
                result: super::PreparedSubChunkResult::AllAir,
            },
        ],
        duration: std::time::Duration::ZERO,
    });

    let stats = stream.stats();
    assert_eq!(stats.normalization_errors, 3);
    assert_eq!(stats.normalization_reasons.inactive_block_updates, 1);
    assert_eq!(stats.normalization_reasons.malformed_block_updates, 1);
    assert_eq!(stats.normalization_reasons.unexpected_sub_chunks, 1);
    assert_eq!(stats.normalization_reasons.inactive_sub_chunks, 0);
    assert_eq!(stats.normalization_reasons.total(), 3);
}

#[test]
fn max_block_update_batch_prepares_off_thread_and_commits_atomically_in_fifo() {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(block_entity_visual_assets()),
        [0.0, crate::server_position::SAFE_SERVER_HEIGHT, 0.0],
        None,
    );
    let mut updates = (0..4_095)
        .map(|linear| BlockUpdateEvent {
            dimension: 0,
            position: [linear >> 8, linear & 15, (linear >> 4) & 15],
            layer: 0,
            network_id: linear as u32 + 1,
        })
        .collect::<Vec<_>>();
    updates.push(BlockUpdateEvent {
        dimension: 0,
        position: [0, 0, 0],
        layer: 0,
        network_id: 15_000,
    });
    let movement = MovePlayerEvent {
        runtime_id: 1,
        position: [1.0, 70.0, 2.0],
        pitch: 0.0,
        yaw: 0.0,
        ..Default::default()
    };

    stream.submit(1, WorldEvent::BlockUpdates(updates)).unwrap();
    stream.submit(2, WorldEvent::MovePlayer(movement)).unwrap();

    assert_eq!(stream.stats().queued_decode_jobs, 1);
    assert!(stream.take_committed_controls().is_empty());
    assert!(
        stream
            .authority
            .terrain()
            .sub_chunk(SubChunkKey::new(0, 0, 0, 0))
            .is_none()
    );

    complete_pending_decode_jobs(&mut stream);

    let committed = stream
        .authority
        .terrain()
        .sub_chunk(SubChunkKey::new(0, 0, 0, 0))
        .unwrap();
    assert_eq!(committed.runtime_id(0, 0, 0, 0), Some(15_000));
    assert_eq!(committed.runtime_id(0, 15, 14, 15), Some(4_095));
    let key = SubChunkKey::new(0, 0, 0, 0);
    assert!(stream.lighting.block_generations.contains_key(&key));
    // The urgent relight starts as soon as the batch commits.
    assert!(
        stream.lighting.jobs.pending.contains_key(&key)
            || stream.lighting.jobs.in_flight.contains_key(&key)
    );
    assert_eq!(
        stream.lighting.store.kind(key),
        world::LightSubChunkKind::Resident
    );
    assert_eq!(
        stream.take_committed_controls(),
        vec![super::CommittedControlEvent::MovePlayer {
            sequence: 2,
            movement,
            resolved: super::server_position::ResolvedServerPosition {
                position: movement.position,
                surface_anchor: None,
            },
            source_cohort: None,
        }]
    );
}
