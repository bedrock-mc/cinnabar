#[test]
fn partial_target_column_coverage_never_settles_the_teleport_stream() {
    let started = Instant::now();
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
    tracker.observe(
        &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 70.0, 1_040.5],
            pitch: 0.0,
            yaw: 0.0,
            ..Default::default()
        }),
        started,
        0,
    );
    tracker.observe(
        &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
            center: [1_040, 70, 1_040],
            radius_blocks: 256,
        }),
        started + Duration::from_millis(100),
        0,
    );
    let mut partial = settled_teleport_snapshot();
    let status = partial.cohort.as_mut().unwrap();
    status.loaded_target = status.expected - 1;
    status.missing_target = status.expected - status.loaded_target;
    partial.loaded_columns = status.loaded_target;

    assert_eq!(
        tracker.observe_snapshot(partial, started + Duration::from_millis(200)),
        None
    );
    assert_eq!(
        tracker.observe_snapshot(partial, started + Duration::from_secs(5)),
        None,
        "a quiet partial target view passed the coverage gate"
    );
    assert!(tracker.is_pending());
}