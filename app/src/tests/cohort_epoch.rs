use super::*;

#[test]
fn mutation_completion_revalidates_the_frozen_raw_publisher_cohort() {
    let coordinate = [14, 71, -6];
    let key = SubChunkKey::new(0, 0, 4, -1);
    let mut acceptance = AcceptanceRun::new(Some(900), None, false, false);
    acceptance.set_mutation_coordinate(coordinate);
    let observed = Instant::now() + Duration::from_millis(1);
    let frozen = exact_destination_status();
    assert!(acceptance.bind_mutation_cohort(frozen));
    acceptance.observe_mutation(
        &WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
            dimension: 0,
            position: coordinate,
            layer: 0,
            network_id: 7,
        }]),
        observed,
    );

    let mut changed = frozen;
    changed.publisher_epoch += 1;
    assert_eq!(
        acceptance.acknowledge_mutation(key, 1, observed, observed, Some(changed)),
        None
    );

    changed = frozen;
    changed.required_hash ^= 1;
    assert_eq!(
        acceptance.acknowledge_mutation(key, 1, observed, observed, Some(changed)),
        None
    );
    assert_eq!(
        acceptance.acknowledge_mutation(key, 1, observed, observed, Some(frozen)),
        Some(Duration::ZERO)
    );
}

#[test]
fn post_world_ready_required_growth_revokes_the_emitted_cohort() {
    let mut acceptance = AcceptanceRun::new(Some(900), None, false, false);
    let frozen = exact_destination_status();
    assert!(acceptance.bind_mutation_cohort(frozen));
    acceptance.begin_world_ready(Instant::now(), [0.5, 70.0, 0.5], 1);
    assert!(acceptance.world_ready);

    let mut expanded = frozen;
    expanded.expected += 1;
    expanded.required_hash ^= 0x55aa;

    assert!(acceptance.revoke_world_ready_if_cohort_changed(Some(expanded)));
    assert!(!acceptance.world_ready);
    assert_eq!(acceptance.deadline, None);
    assert_eq!(acceptance.mutation_cohort, None);
}

/// Startup reads committed-view readiness without requiring a full diagnostic scan.
/// Ordinary play computes neither once the startup probe is disabled.
#[test]
fn frame_cohort_status_is_computed_for_startup_acceptance_or_metrics_only() {
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
            WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                center: [0, 70, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();
    let target = stream.committed_view_cohort().unwrap();

    let play = AcceptanceRun::new(None, None, false, false);
    let acceptance = AcceptanceRun::new(Some(60), None, false, false);
    let metrics = AcceptanceRun::new(None, Some("metrics.json".into()), false, false);
    let frame = crate::runtime::world::frame_cohort_status;
    assert_eq!(frame(&stream, &play, false), (None, None));
    let (progress, status) = frame(&stream, &play, true);
    assert_eq!(status, None, "loading skips the diagnostic status");
    assert_eq!(progress, Some(stream.cohort_progress(target)));
    for diagnostics in [&acceptance, &metrics] {
        let (progress, status) = frame(&stream, diagnostics, false);
        let status = status.expect("acceptance and metrics read the full status");
        assert_eq!(progress, Some(status.into()));
    }
    assert_eq!(frame(&stream, &play, false), (None, None));
}
