use super::*;
use world::SUB_CHUNK_SIDE;

fn destination() -> [f32; 3] {
    let side = SUB_CHUNK_SIDE as f32;
    [
        side / 2.0,
        side * 4.0 + client_world::ingestion::PLAYER_NETWORK_OFFSET,
        side / 2.0,
    ]
}

fn stationary_light_view(stream: &mut WorldStream, position: [f32; 3]) {
    let view = SchedulerView {
        position,
        forward: stream.view_forward,
    };
    stream
        .lighting
        .jobs
        .refresh
        .refresh(view, &mut stream.lighting.jobs.lanes, None, |_, _| true);
    assert!(!stream.lighting.jobs.refresh.refresh(
        view,
        &mut stream.lighting.jobs.lanes,
        None,
        |_, _| true,
    ));
}

#[test]
fn late_destination_light_dependencies_precede_stationary_view_backlog() {
    let mut stream = lit_stream(0);
    let position = destination();
    stream.set_view_forward([0.0, 0.0, 1.0]);
    stationary_light_view(&mut stream, position);
    // This halo column is behind the camera. It still supports the player's
    // first mesh, and its upper skylight dependency must run before its lower cell.
    let lower = SubChunkKey::new(0, 0, 4, -1);
    let upper = SubChunkKey::new(0, 0, 5, -1);
    let above = SubChunkKey::new(0, 0, 6, -1);
    for key in [lower, upper, above] {
        install_current_light(&mut stream, key, 0, 15, true);
    }
    let lower_revision = stream.mark_light_dirty_exact(lower).unwrap();
    let upper_revision = stream.mark_light_dirty_exact(upper).unwrap();
    let generation = stream.lighting.block_generations[&lower];
    assert!(!stream.light_dispatch_ready(lower));

    let unrelated = SubChunkKey::new(0, 2, 4, 0);
    install_current_light(&mut stream, unrelated, 0, 15, true);
    install_current_light(&mut stream, SubChunkKey::new(0, 2, 5, 0), 0, 15, true);
    let unrelated_revision = stream
        .mark_light_dirty_exact_with_priority(unrelated, true)
        .unwrap();
    // The critical jobs have not reached the ordinary ingress queue yet.
    stream.lighting.jobs.scan.clear();
    stream.lighting.jobs.lanes[0]
        .ready
        .push(PendingSchedulerCandidate::new(
            unrelated,
            unrelated_revision,
            SchedulerView {
                position,
                forward: stream.view_forward,
            },
            true,
        ));
    stream.set_dimension_transfer_priority(Some(position));

    assert_eq!(stream.dispatch_light_jobs(position, 1), 2);
    assert_eq!(
        stream.lighting.jobs.in_flight[&upper].revision,
        upper_revision
    );
    assert_eq!(
        stream.lighting.jobs.in_flight[&lower].revision,
        lower_revision
    );
    assert!(!stream.lighting.jobs.in_flight.contains_key(&unrelated));
    assert_eq!(stream.lighting.block_generations[&lower], generation);
    assert_eq!(
        stream.lighting.jobs.pending[&unrelated].revision,
        unrelated_revision
    );
    for _ in 0..2 {
        let result = stream
            .lighting
            .rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        stream.accept_light_completion(result);
    }
}

#[test]
fn transfer_meshes_precede_backlog_and_release_priority_after_completion() {
    let mut stream = lit_stream(protocol::NETHER_DIMENSION_ID);
    let position = destination();
    let feet = SubChunkKey::new(protocol::NETHER_DIMENSION_ID, 0, 4, 0);
    let footing = SubChunkKey::new(protocol::NETHER_DIMENSION_ID, 0, 3, 0);
    let unrelated = SubChunkKey::new(protocol::NETHER_DIMENSION_ID, 0, 4, 2);
    for key in [feet, footing, unrelated] {
        stream
            .authority
            .commit_sub_chunk(key, super::super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
    }
    stream.set_view_forward([0.0, 0.0, 1.0]);
    let view = SchedulerView {
        position,
        forward: stream.view_forward,
    };
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let feet_revision = stream.mark_dirty_exact(feet, Instant::now());
    let footing_revision = stream.mark_dirty_exact(footing, Instant::now());
    let unrelated_revision = stream.mark_dirty_exact_with_priority(unrelated, Instant::now(), true);
    stream.mesh_jobs.scan.clear();
    stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
        .ready
        .push(PendingSchedulerCandidate::new(
            unrelated,
            unrelated_revision,
            view,
            true,
        ));
    stream.set_dimension_transfer_priority(Some(position));

    for (key, revision) in [(feet, feet_revision), (footing, footing_revision)] {
        assert_eq!(stream.dispatch_mesh_jobs_with_limits(position, 1, 0), 1);
        assert_eq!(stream.mesh_jobs.in_flight[&key], revision);
        assert!(!stream.mesh_jobs.in_flight.contains_key(&unrelated));
        let result = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(result.key, key);
        stream.accept_mesh_completion(result);
    }
    stream.set_dimension_transfer_priority(None);
    assert_eq!(stream.dispatch_mesh_jobs_with_limits(position, 1, 0), 1);
    assert_eq!(stream.mesh_jobs.in_flight[&unrelated], unrelated_revision);
    let result = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    stream.accept_mesh_completion(result);
}

#[test]
fn transfer_priority_keeps_missing_light_a_mesh_blocker() {
    let mut stream = lit_stream(protocol::NETHER_DIMENSION_ID);
    let position = destination();
    let feet = SubChunkKey::new(protocol::NETHER_DIMENSION_ID, 0, 4, 0);
    let halo = SubChunkKey::new(protocol::NETHER_DIMENSION_ID, 1, 4, 0);
    stream
        .authority
        .commit_sub_chunk(feet, super::super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, feet, 0, 0, false);
    install_current_light(&mut stream, halo, 0, 0, false);
    stream.mark_light_dirty_exact(halo).unwrap();
    let revision = stream.mark_dirty_exact(feet, Instant::now());
    stream.set_dimension_transfer_priority(Some(position));
    assert_eq!(stream.dispatch_mesh_jobs_with_limits(position, 1, 0), 0);
    assert_eq!(stream.mesh_jobs.pending[&feet].revision, revision);
    assert!(!stream.mesh_jobs.in_flight.contains_key(&feet));
}

#[test]
fn nonfinite_positions_and_dimension_changes_clear_transfer_priority() {
    let mut stream = lit_stream(0);
    stream.set_dimension_transfer_priority(Some(destination()));
    assert!(stream.dimension_transfer_priority.is_some());
    stream.set_dimension_transfer_priority(Some([f32::NAN, 0.0, 0.0]));
    assert!(stream.dimension_transfer_priority.is_none());
    stream.set_dimension_transfer_priority(Some(destination()));
    stream
        .submit(
            1,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: protocol::NETHER_DIMENSION_ID,
                position: destination(),
                ..Default::default()
            }),
        )
        .unwrap();
    assert!(stream.dimension_transfer_priority.is_none());
}
