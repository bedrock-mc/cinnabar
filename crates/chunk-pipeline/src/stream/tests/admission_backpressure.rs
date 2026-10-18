use super::*;
use client_world::ingestion::MAX_ADMITTED_WORLD_EVENTS;

#[test]
fn undrained_ui_commits_apply_bounded_backpressure_without_panicking() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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

/// A stationary dirty storm must not grow the mesh scan history without bound.
#[test]
fn stationary_dirty_storm_keeps_the_mesh_scan_bounded_by_live_work() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
        stream.mesh_jobs.scan.len() <= keys.len(),
        "scan retained {} entries for {} live keys",
        stream.mesh_jobs.scan.len(),
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
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream
        .authority
        .update_block(
            key,
            BlockUpdate::new(0, 0, 0, 0, 99),
            protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        )
        .unwrap();
    stream.resident.insert(key);
    stream.mark_light_changed_sources([key]);
    assert!(stream.lighting.in_flight_batches.is_empty());

    stream
        .lighting
        .running_jobs
        .store(MAX_IN_FLIGHT_LIGHT_JOBS, Ordering::Release);
    assert_eq!(
        stream.dispatch_light_jobs([0.0; 3], LIGHT_DISPATCH_BUDGET_PER_POLL),
        0
    );
    stream.lighting.running_jobs.store(0, Ordering::Release);
    assert!(stream.dispatch_light_jobs([0.0; 3], LIGHT_DISPATCH_BUDGET_PER_POLL) > 0);
}
