use super::*;

#[test]
fn undrained_ui_commits_apply_bounded_backpressure_without_panicking() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    for sequence in 1..=MAX_ADMITTED_WORLD_EVENTS as u64 {
        stream
            .submit(
                sequence,
                WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 20 })),
            )
            .unwrap();
    }

    assert_eq!(stream.remaining_admission_capacity(), 0);
    assert!(matches!(
        stream.submit(
            MAX_ADMITTED_WORLD_EVENTS as u64 + 1,
            WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 19 })),
        ),
        Err(WorldStreamError::AdmissionFull { .. })
    ));
    assert_eq!(stream.take_committed_ui().len(), MAX_ADMITTED_WORLD_EVENTS);
}

#[test]
fn chunk_grid_retention_follows_player_and_ignores_publisher() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();

    // Two columns loaded within the grid around the origin.
    let near = ChunkKey::new(0, 2, 0);
    let slack_edge = ChunkKey::new(0, 10, 0);
    stream.loaded_columns.insert(near);
    stream.loaded_columns.insert(slack_edge);

    // A publisher update never drives retention: both loaded columns survive it
    // even though its tiny active radius sits thousands of blocks away.
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [2_000, 70, 2_000],
                radius_blocks: 16,
            }),
        )
        .unwrap();
    assert!(stream.tracked_columns().contains(&near));
    assert!(stream.tracked_columns().contains(&slack_edge));

    // Moving the local player recenters the grid on its chunk (20, 0). `near`
    // leaves the grid; `slack_edge` survives at exactly Chebyshev radius + 2.
    stream
        .submit(
            3,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [325.0, 70.0, 0.5],
                ..Default::default()
            }),
        )
        .unwrap();
    assert!(!stream.tracked_columns().contains(&near));
    assert!(stream.tracked_columns().contains(&slack_edge));
}

#[test]
fn shrinking_confirmed_radius_evicts_columns_that_leave_the_grid() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();

    let inner = ChunkKey::new(0, 3, 0);
    let outer = ChunkKey::new(0, 9, 0);
    stream.loaded_columns.insert(inner);
    stream.loaded_columns.insert(outer);

    // A newly confirmed, smaller radius re-evaluates retention around the player.
    // The grid view distance becomes 3, so columns survive to Chebyshev radius +
    // 2 == 4: `inner` stays, `outer` is evicted.
    stream.submit(2, WorldEvent::ChunkRadiusUpdated(2)).unwrap();
    assert!(stream.tracked_columns().contains(&inner));
    assert!(!stream.tracked_columns().contains(&outer));
}

/// Cost of one full-world cohort witness at radius 16; ordinary frames no longer pay it.
/// Run: `cargo test -p client-world --lib cohort_status_cost -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark"]
fn cohort_status_cost_at_radius_16() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 70, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();
    for x in -16..=16 {
        for z in -16..=16 {
            stream.loaded_columns.insert(ChunkKey::new(0, x, z));
            for y in -4..20 {
                stream.resident.insert(SubChunkKey::new(0, x, y, z));
            }
        }
    }
    let target = stream.committed_view_cohort().unwrap();
    let frames = 200;
    let started = Instant::now();
    for _ in 0..frames {
        std::hint::black_box(stream.cohort_status(target));
    }
    eprintln!(
        "FRAME_COST cohort_status_radius_16: old={:.3}ms new=0.000ms (gated to acceptance/metrics runs)",
        started.elapsed().as_secs_f64() * 1e3 / f64::from(frames)
    );
}

/// A stationary dirty storm must not grow the mesh scan history without bound.
#[test]
fn stationary_dirty_storm_keeps_the_mesh_scan_bounded_by_live_work() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.dispatch_mesh_jobs([0.0; 3], 1);
    let keys = (0..4)
        .map(|x| SubChunkKey::new(0, x, 0, 0))
        .collect::<Vec<_>>();
    let now = Instant::now();
    for _ in 0..5_000 {
        for key in &keys {
            stream.mark_dirty_exact(*key, now);
        }
    }
    stream.dispatch_mesh_jobs([0.0; 3], 1);
    assert!(
        stream.pending_mesh_scan.len() <= keys.len(),
        "scan retained {} entries for {} live keys",
        stream.pending_mesh_scan.len(),
        keys.len()
    );
}

/// Evicted-but-running solves still occupy light worker slots.
#[test]
fn still_running_light_solves_hold_their_worker_slots() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream
        .authority
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_light_changed_sources([key]);
    assert!(stream.in_flight_light_batches.is_empty());

    stream
        .running_light_jobs
        .store(MAX_IN_FLIGHT_LIGHT_JOBS, Ordering::Release);
    assert_eq!(
        stream.dispatch_light_jobs([0.0; 3], LIGHT_DISPATCH_BUDGET_PER_POLL),
        0
    );
    stream.running_light_jobs.store(0, Ordering::Release);
    assert!(stream.dispatch_light_jobs([0.0; 3], LIGHT_DISPATCH_BUDGET_PER_POLL) > 0);
}

/// Removal acks for evicted sub-chunks must not accumulate applied generations.
#[test]
fn evicted_removal_acks_leave_no_applied_generation_behind() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let now = Instant::now();
    let resident = SubChunkKey::new(0, 0, 0, 0);
    let evicted = SubChunkKey::new(0, 40, 0, 0);
    stream.resident.insert(resident);
    for key in [resident, evicted] {
        let generation = stream.mark_dirty_exact(key, now);
        stream.acknowledge_mesh_upload(key, generation, now, now);
    }
    assert!(stream.applied_mesh_generations.contains_key(&resident));
    assert!(!stream.applied_mesh_generations.contains_key(&evicted));
    assert_eq!(stream.applied_mesh_generations.len(), 1);
}
