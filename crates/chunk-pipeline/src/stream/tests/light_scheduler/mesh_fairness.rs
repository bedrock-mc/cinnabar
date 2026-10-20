use super::*;

/// Queues a visible mesh outside local probes ahead of a lower-priority ready generation.
fn deferred_mesh_stream() -> (WorldStream, SchedulerView, SubChunkKey, SubChunkKey) {
    seed_deferred_meshes(lit_stream(1))
}

/// Queues a visible far mesh ahead of lower-priority work in a dimension-one test stream.
fn seed_deferred_meshes(
    mut stream: WorldStream,
) -> (WorldStream, SchedulerView, SubChunkKey, SubChunkKey) {
    stream.set_view_forward([1.0, 0.0, 0.0]);
    let view = stream.scheduler_view([8.0; 3]);
    assert!(view.startup_center.is_none());
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let visible = SubChunkKey::new(1, scheduler::NEAR_CAMERA_RADIUS * 3, 0, 0);
    let backlog = SubChunkKey::new(1, visible.x + 4, 0, 0);
    assert!(!scheduler::mesh_is_near(view, visible));
    let ready = (0..8).map(|offset| {
        (
            SubChunkKey::new(1, backlog.x + offset * 2, 0, 0),
            offset != 7,
        )
    });
    for (key, ready) in std::iter::once((visible, false)).chain(ready) {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        let revision = stream.mark_dirty_exact(key, Instant::now());
        let candidate = PendingSchedulerCandidate::new(key, revision, view, false);
        let lane = &mut stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        if ready {
            lane.ready.push(candidate);
        } else {
            lane.deferred.push(candidate);
        }
    }
    stream.mesh_jobs.scan.clear();
    (stream, view, visible, backlog)
}

/// A settled far halo must publish its higher-priority mesh before the ready generation drains.
#[test]
fn far_settled_halo_publishes_before_ready_backlog() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    let corner = SubChunkKey::new(1, visible.x + 1, visible.y + 1, visible.z + 1);
    install_current_light(&mut stream, corner, 0, 0, false);
    stream.mark_light_dirty_exact(corner).unwrap();
    assert!(stream.mesh_light_halo(visible).is_none());
    let revision = stream.mesh_jobs.pending[&visible].revision;

    install_current_light(&mut stream, corner, 0, 0, false);
    let direct = stream.lighting.direct_sky[&corner].clone();
    stream.finish_accepted_light_completion(corner, 0, &direct, [false; 6], false);
    assert_eq!(stream.mesh_jobs.pending[&visible].revision, revision);
    assert!(stream.mesh_jobs.scan.is_empty());

    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, visible);
    assert_eq!(completion.revision, revision);
    stream.accept_mesh_completion(completion);
    assert!(
        stream
            .take_mesh_changes()
            .iter()
            .any(|change| change.key() == visible)
    );
    assert!(stream.mesh_jobs.pending.contains_key(&backlog));
}

/// A successful retry spends the expired poll's single dispatch allowance.
#[test]
fn expired_far_retry_dispatches_once_before_ready_backlog() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    stream.poll_deadline = Some(Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 2), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, visible);
    stream.accept_mesh_completion(completion);
    assert!(stream.mesh_jobs.pending.contains_key(&backlog));
    assert!(!stream.mesh_jobs.in_flight.contains_key(&backlog));
}

/// Rechecking a far mesh must keep stale light blocked and let ready geometry progress.
#[test]
fn far_stale_halo_cannot_starve_ready_backlog() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    stream.mark_light_dirty_exact(visible).unwrap();
    for _ in 0..4 {
        stream.poll_deadline = Some(Instant::now());
        stream.dispatch_mesh_jobs(view.position, 1);
        if stream.mesh_jobs.in_flight.contains_key(&backlog) {
            break;
        }
    }
    assert!(!stream.mesh_jobs.in_flight.contains_key(&visible));
    assert!(stream.mesh_jobs.pending.contains_key(&visible));
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, backlog);
    stream.accept_mesh_completion(completion);
}

/// Deferred ordinary work yields to urgent edits and destination footing.
#[test]
fn far_deferred_mesh_preserves_urgent_and_transfer_precedence() {
    for transfer in [false, true] {
        let (mut stream, view, visible, backlog) = deferred_mesh_stream();
        if transfer {
            let side = world::SUB_CHUNK_SIDE as f32;
            stream.set_dimension_transfer_priority(Some([
                backlog.x as f32 * side + side / 2.0,
                client_world::ingestion::PLAYER_NETWORK_OFFSET + side / 2.0,
                side / 2.0,
            ]));
        } else {
            stream.mark_dirty_exact_with_priority(backlog, Instant::now(), true);
        }
        assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(completion.key, backlog);
        stream.accept_mesh_completion(completion);
        assert!(stream.mesh_jobs.pending.contains_key(&visible));
    }
}

/// A due neighbour can settle without revising the mesh that was waiting for it.
#[test]
fn far_settled_neighbour_publishes_before_ready_backlog() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    let missing = SubChunkKey::new(1, visible.x + 1, visible.y, visible.z);
    let now = Instant::now();
    stream
        .unsent_column_deadlines
        .insert(missing.chunk(), now + UNSENT_COLUMN_GRACE);
    assert!(stream.mesh_neighbour_is_due(visible, now));
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, backlog);
    stream.accept_mesh_completion(completion);
    stream.take_mesh_changes();
    let backlog = SubChunkKey::new(1, backlog.x + 2, backlog.y, backlog.z);
    assert!(stream.mesh_jobs.pending.contains_key(&visible));
    let revision = stream.mesh_jobs.pending[&visible].revision;

    stream.loaded_columns.insert(missing.chunk());
    assert!(!stream.mesh_neighbour_is_due(visible, now));
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, visible);
    assert_eq!(completion.revision, revision);
    stream.accept_mesh_completion(completion);
    assert!(
        stream
            .take_mesh_changes()
            .iter()
            .any(|change| change.key() == visible)
    );
    assert!(stream.mesh_jobs.pending.contains_key(&backlog));
}

/// Continuous arrivals between a settled retry and ready work cannot hide that retry.
#[test]
fn far_settled_retry_precedes_continuous_fresh_ingress() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    stream.mark_light_dirty_exact(visible).unwrap();
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, backlog);
    stream.accept_mesh_completion(completion);
    stream.take_mesh_changes();
    install_current_light(&mut stream, visible, 0, 0, false);
    let backlog = SubChunkKey::new(1, backlog.x + 2, backlog.y, backlog.z);

    let mut published = false;
    for z in 0..8 {
        let arriving = SubChunkKey::new(1, visible.x + 2, 0, z);
        stream
            .authority
            .commit_sub_chunk(arriving, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, arriving, 0, 0, false);
        stream.mark_dirty_exact(arriving, Instant::now());
        assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        published = completion.key == visible;
        stream.accept_mesh_completion(completion);
        if published {
            break;
        }
    }
    assert!(
        published,
        "fresh lower-priority ingress hid the settled retry"
    );
    assert!(stream.mesh_jobs.pending.contains_key(&backlog));
}

/// Output-memory pressure must not starve a smaller ready mesh or duplicate retry heap records.
#[test]
fn memory_blocked_retry_keeps_ready_progress_and_bounded_storage() {
    assert_memory_blocked_retry_progress(1);
}

/// More blocked retries than the cooldown must still leave ordinary work an admission turn.
#[test]
fn many_memory_blocked_retries_preserve_ready_progress_across_rotations() {
    let blocked = usize::try_from(scheduler::MAX_RECHECK_BACKOFF_POLLS).unwrap() + 1;
    assert_memory_blocked_retry_progress(blocked);
}

/// Keeps smaller ready meshes publishing while a FIFO of larger models cannot reserve output.
fn assert_memory_blocked_retry_progress(blocked: usize) {
    let base = light_test_assets();
    let model_id = u32::try_from(base.visual_count()).unwrap();
    let side = assets::MODEL_POSITION_SCALE;
    let uv = assets::MODEL_UV_SCALE;
    let overlay = assets::BlockOverlay {
        visuals: vec![BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            support: assets::VisualSupport::Exact,
            faces: [0; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            contributor_role: ContributorRole::Primary,
            model_template: 0,
            animation: NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![LightProperties::new(0, 0).unwrap()],
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            ..Material::unvaried()
        }],
        model_templates: vec![assets::ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        model_quads: vec![assets::ModelQuad {
            positions: [[0, 0, 0], [0, side, 0], [side, side, 0], [side, 0, 0]],
            uvs: [[0, 0], [0, uv], [uv, uv], [uv, 0]],
            material: 0,
            flags: 0,
        }],
        ..assets::BlockOverlay::default()
    };
    let assets = base.with_block_overlay(model_id, &overlay).unwrap();
    let mut stream = lit_stream_with_assets(1, assets);
    stream.set_view_forward([1.0, 0.0, 0.0]);
    let view = stream.scheduler_view([8.0; 3]);
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let visible = SubChunkKey::new(1, scheduler::NEAR_CAMERA_RADIUS * 3, 0, 0);
    let backlog = SubChunkKey::new(1, visible.x + i32::try_from(blocked).unwrap() * 2 + 4, 0, 0);
    for offset in 0..=blocked {
        let ready = offset == blocked;
        let key = if ready {
            backlog
        } else {
            SubChunkKey::new(1, visible.x + i32::try_from(offset).unwrap() * 2, 0, 0)
        };
        stream
            .authority
            .commit_sub_chunk(
                key,
                super::uniform_sub_chunk(if ready { 2 } else { model_id }),
            )
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        let revision = stream.mark_dirty_exact(key, Instant::now());
        let candidate = PendingSchedulerCandidate::new(key, revision, view, false);
        let lane = &mut stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        if ready {
            lane.ready.push(candidate);
        } else {
            lane.deferred.push(candidate);
        }
    }
    stream.mesh_jobs.scan.clear();
    let revision = stream.mesh_jobs.pending[&visible].revision;
    let source = stream.authority.terrain().sub_chunk(visible).unwrap();
    let small = stream.authority.terrain().sub_chunk(backlog).unwrap();
    let reserve = |source| {
        stream
            .mesh_memory
            .try_admit(
                source,
                stream.authority.runtime_assets(),
                stream.authority.network_id_mode(),
            )
            .unwrap()
    };
    let large_permit = reserve(&source);
    let large_bytes = stream.mesh_memory.retained.load(Ordering::Acquire);
    drop(large_permit);
    let small_permit = reserve(&small);
    let small_bytes = stream.mesh_memory.retained.load(Ordering::Acquire);
    drop(small_permit);
    assert!(large_bytes > small_bytes);
    let held = crate::stream::meshing::memory::MAX_MESH_OUTPUT_BYTES - small_bytes;
    stream.mesh_memory.retained.store(held, Ordering::Release);
    assert!(
        stream
            .mesh_memory
            .try_admit(
                &source,
                stream.authority.runtime_assets(),
                stream.authority.network_id_mode()
            )
            .is_none()
    );
    drop(reserve(&small));
    let lane = &stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
    let storage_limit = lane.ready.len() + lane.deferred.len() + stream.mesh_jobs.scan.len();
    let mut published = 0;
    for _ in 0..(blocked * 3).max(12) {
        if !stream.mesh_jobs.pending.contains_key(&backlog) {
            stream.mark_dirty_exact(backlog, Instant::now());
        }
        stream.poll_deadline = Some(Instant::now());
        if stream.dispatch_mesh_jobs(view.position, 1) != 0 {
            let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_ne!(completion.key, visible);
            assert_eq!(completion.key, backlog);
            published += 1;
            stream.accept_mesh_completion(completion);
            stream.take_mesh_changes();
        }
        let lane = &stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        assert!(
            lane.ready.len() + lane.deferred.len() <= storage_limit,
            "unchanged retry accumulated heap records"
        );
        assert_eq!(stream.mesh_jobs.pending[&visible].revision, revision);
    }
    assert!(
        published >= 3,
        "memory-blocked retries starved admissible ready work across rotations: {published}"
    );
}

/// Successful far retries must retire lane ownership while a lower-ranked ready backlog stays busy.
#[test]
fn repeated_far_retry_publications_keep_storage_bounded_by_live_work() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    for _ in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
        stream.poll_deadline = Some(Instant::now());
        assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(completion.key, visible);
        stream.accept_mesh_completion(completion);
        stream.take_mesh_changes();
        assert!(stream.mesh_jobs.pending.contains_key(&backlog));
        let lane = &stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        assert!(
            lane.ready.len() + lane.deferred.len() <= stream.mesh_jobs.pending.len(),
            "published retry retained obsolete lane ownership"
        );
        stream.mark_dirty_exact(visible, Instant::now());
    }
}
