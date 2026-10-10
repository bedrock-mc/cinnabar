//! Turning the camera over water draws every newly visible sub-chunk in order at once.
use super::transparent_strafe::{
    Drawn, Fixture, drawn_water, fixture_with, is_shore, ocean_surface,
};
use super::*;
use bevy::{
    core_pipeline::core_3d::Transparent3d, ecs::system::RunSystemOnce,
    render::render_phase::ViewSortedRenderPhases,
};

/// One in five sub-chunks has side faces, so the sorted path is exercised too.
const SHORE_PERIOD: i32 = 5;

/// The back-to-front order of `key`'s faces for `camera`, as the worker computes it.
fn expected_refs(
    fixture: &Fixture,
    key: SubChunkKey,
    camera: Vec3,
) -> Vec<PackedTransparentDrawRef> {
    let world = fixture.app.world();
    let &(entity, _) = fixture
        .surfaces
        .iter()
        .find(|(_, surface)| *surface == key)
        .unwrap();
    let allocation = world.get::<GpuChunkAllocation>(entity).unwrap();
    let identity = TransparentAllocationIdentity::new(
        key,
        allocation.generation,
        allocation.liquid_range.clone().unwrap(),
        allocation.liquid_lighting_range.clone().unwrap(),
        allocation.metadata_index,
    );
    let group = build_transparent_group(
        world.get::<ChunkRenderInstance>(entity).unwrap(),
        identity,
        world.resource::<ChunkBiomeTints>(),
    )
    .unwrap();
    sort_group(TransparentFaceMetric::new(camera), &group).to_vec()
}

/// Asserts that exactly the visible water is drawn and that each draw is in order.
fn assert_drawn_in_order(fixture: &Fixture, camera: Vec3, forward: Vec3) {
    let visible = fixture.visible_water(camera, forward);
    let drawn = drawn_water(fixture);
    assert_eq!(
        drawn.keys().copied().collect::<BTreeSet<_>>(),
        visible,
        "drawn water differs from the visible water looking along {forward}"
    );
    for (key, water) in drawn {
        match water {
            Drawn::Sorted(refs) => assert_eq!(
                refs,
                expected_refs(fixture, key, camera),
                "{key:?} is not in back-to-front order"
            ),
            Drawn::Direct => assert!(
                !is_shore(key, SHORE_PERIOD),
                "{key:?} has overlapping faces but was drawn unsorted"
            ),
        }
    }
}

/// When a reloaded sub-chunk is briefly visible under two entities, its water is drawn
/// once, from the entity whose upload the resident index holds.
#[test]
fn a_duplicate_visible_key_draws_its_current_upload_once() {
    let mut fixture = fixture_with(SHORE_PERIOD);
    let camera = Vec3::new(8.5, 64.6, 8.5);
    for _ in 0..4 {
        fixture.frame_looking(camera, Vec3::Z);
    }
    let key = SubChunkKey::new(0, 1, 3, 1);
    assert!(!is_shore(key, SHORE_PERIOD));
    let older = fixture
        .surfaces
        .iter()
        .find(|(_, surface)| *surface == key)
        .unwrap()
        .0;
    fixture.spawn_surface(key, false);
    let newer = fixture.surfaces.last().unwrap().0;
    // Make the larger entity the current upload, so drawing the smaller one is wrong.
    let current = if newer > older {
        newer
    } else {
        let mut surface = ocean_surface(key, false);
        surface.generation += 1;
        let world = fixture.app.world_mut();
        world.entity_mut(older).insert(surface);
        while world.resource::<ChunkGpuArena>().allocations[&older]
            .gpu
            .generation
            == 1
        {
            world.run_system_once(prepare_gpu_chunks).unwrap();
        }
        older
    };
    fixture.frame_looking(camera, Vec3::Z);
    let world = fixture.app.world();
    let resident = world
        .resource::<ChunkGpuArena>()
        .transparent_liquids
        .get(key)
        .unwrap()
        .entity;
    assert_eq!(resident, current);
    let phase = world
        .resource::<ViewSortedRenderPhases<Transparent3d>>()
        .get(&fixture.retained)
        .unwrap();
    let drawn = phase
        .items
        .iter()
        .map(|item| item.entity.0)
        .filter(|&entity| {
            world
                .get::<GpuChunkAllocation>(entity)
                .is_some_and(|allocation| allocation.key == key)
        })
        .collect::<Vec<_>>();
    assert_eq!(drawn, [current]);
}

/// The newest sort generation requested so far.
fn sort_requests(fixture: &Fixture) -> u64 {
    fixture
        .app
        .world()
        .resource::<TransparentSortMetrics>()
        .snapshot()
        .request_generation
}

/// A turn at a fixed position shows the water behind the camera in the very next frame,
/// in order, without requesting a sort.
#[test]
fn turning_draws_newly_visible_water_in_order_without_a_new_sort() {
    let mut fixture = fixture_with(SHORE_PERIOD);
    let camera = Vec3::new(8.5, 64.6, 8.5);
    for _ in 0..8 {
        fixture.frame_looking(camera, Vec3::Z);
    }
    assert_drawn_in_order(&fixture, camera, Vec3::Z);
    let (requests, jobs) = (sort_requests(&fixture), fixture.jobs);

    fixture.frame_looking(camera, Vec3::NEG_Z);
    assert_drawn_in_order(&fixture, camera, Vec3::NEG_Z);
    assert_eq!(
        sort_requests(&fixture),
        requests,
        "the turn requested a sort"
    );
    assert_eq!(fixture.jobs, jobs, "the turn ran a sort job");
}

/// Sweeping the view around a fixed position never re-sorts or re-uploads water, and
/// never leaves visible water undrawn.
#[test]
fn rotation_only_camera_changes_sort_and_upload_nothing() {
    const SWEEP_FRAMES: usize = 64;
    let mut fixture = fixture_with(SHORE_PERIOD);
    let camera = Vec3::new(8.5, 64.6, 8.5);
    for _ in 0..8 {
        fixture.frame_looking(camera, Vec3::Z);
    }
    let metrics = || {
        fixture
            .app
            .world()
            .resource::<TransparentSortMetrics>()
            .snapshot()
    };
    let before = metrics();
    let (jobs, sorted_refs) = (fixture.jobs, fixture.sorted_refs);
    let mut incomplete_frames = 0;
    for frame in 0..SWEEP_FRAMES {
        let yaw = frame as f32 * std::f32::consts::TAU / 16.0;
        let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
        fixture.frame_looking(camera, forward);
        let drawn = drawn_water(&fixture);
        if drawn.keys().copied().collect::<BTreeSet<_>>() != fixture.visible_water(camera, forward)
        {
            incomplete_frames += 1;
        }
    }
    let after = fixture
        .app
        .world()
        .resource::<TransparentSortMetrics>()
        .snapshot();
    let (jobs, sorted_refs) = (fixture.jobs - jobs, fixture.sorted_refs - sorted_refs);
    let requests = after.request_generation - before.request_generation;
    let uploaded = after.upload_bytes - before.upload_bytes;
    println!(
        "rotation sweep over {SWEEP_FRAMES} frames: sort_requests={requests} sort_jobs={jobs} \
         sorted_refs={sorted_refs} upload_bytes={uploaded} frames_missing_visible_water={incomplete_frames} \
         committed_refs={}",
        after.ref_count
    );
    assert_eq!(
        incomplete_frames, 0,
        "visible water was missing while turning"
    );
    assert_eq!((requests, jobs, sorted_refs, uploaded), (0, 0, 0, 0));
}

/// Water whose first sort has not committed yet still draws, in mesh order, instead of
/// waiting for the sort.
#[test]
fn water_awaiting_its_first_sort_still_draws() {
    let mut fixture = fixture_with(SHORE_PERIOD);
    let camera = Vec3::new(8.5, 64.6, 8.5);
    fixture.frame_looking(camera, Vec3::Z);
    let world = fixture.app.world();
    assert!(
        world
            .resource::<TransparentSortRuntime>()
            .state
            .committed()
            .is_none()
    );
    let drawn = drawn_water(&fixture);
    assert_eq!(
        drawn.keys().copied().collect::<BTreeSet<_>>(),
        fixture.visible_water(camera, Vec3::Z)
    );
    assert!(
        drawn
            .iter()
            .any(|(key, water)| is_shore(*key, SHORE_PERIOD) && matches!(water, Drawn::Direct))
    );
}

/// Past the ref ceiling the water nearest the camera stays sorted and the rest draws from
/// its records, so no visible water is dropped, and the sorted set follows the camera.
#[test]
fn water_past_the_ref_ceiling_draws_and_the_nearest_stays_sorted() {
    const SHORE_FACES: usize = 272;
    let mut fixture = fixture_with(1);
    // Every sub-chunk is a shore, so the ceiling admits the camera's 3x3 neighbourhood.
    fixture
        .app
        .world_mut()
        .resource_mut::<TransparentSortRuntime>()
        .ref_ceiling = 9 * SHORE_FACES;
    for centre in [0, 5] {
        let camera = Vec3::new(16.0 * centre as f32 + 8.5, 64.6, 16.0 * centre as f32 + 8.5);
        for _ in 0..8 {
            fixture.frame_looking(camera, Vec3::Z);
        }
        let near = |key: SubChunkKey| (key.x - centre).abs() <= 1 && (key.z - centre).abs() <= 1;
        let drawn = drawn_water(&fixture);
        assert_eq!(
            drawn.keys().copied().collect::<BTreeSet<_>>(),
            fixture.visible_water(camera, Vec3::Z),
            "visible water past the ceiling was dropped"
        );
        let mut sorted = 0;
        for (key, water) in drawn {
            match water {
                Drawn::Sorted(refs) => {
                    assert!(near(key), "{key:?} is sorted but not near the camera");
                    assert_eq!(refs, expected_refs(&fixture, key, camera));
                    sorted += 1;
                }
                Drawn::Direct => assert!(!near(key), "{key:?} is near the camera but unsorted"),
            }
        }
        assert!(sorted >= 3, "only {sorted} nearby sub-chunks drew sorted");
        let committed = fixture
            .app
            .world()
            .resource::<TransparentSortRuntime>()
            .state
            .committed()
            .unwrap()
            .live_ref_count();
        assert_eq!(committed, 9 * SHORE_FACES);
    }
    let metrics = fixture
        .app
        .world()
        .resource::<TransparentSortMetrics>()
        .snapshot();
    assert!(metrics.ceiling_reject_count >= 2);
}

/// Streaming that changes the resident water every frame cannot starve a sort whose
/// upload spans several frames: each staged upload finishes before the next request.
#[test]
fn resident_churn_cannot_starve_a_multi_frame_sort_upload() {
    const CHURN_FRAMES: i32 = 40;
    let mut fixture = fixture_with(SHORE_PERIOD);
    let camera = Vec3::new(8.5, 64.6, 8.5);
    fixture.frame_looking(camera, Vec3::Z);
    // Every snapshot of this ocean now takes several frames to stage.
    fixture
        .app
        .world_mut()
        .resource_mut::<TransparentSortRuntime>()
        .state
        .upload_cap = 4096;
    let committed = |fixture: &Fixture| {
        fixture
            .app
            .world()
            .resource::<TransparentSortMetrics>()
            .snapshot()
            .committed_generation
    };
    let (mut commits, mut last) = (0, committed(&fixture));
    for frame in 0..CHURN_FRAMES {
        // A new shore streams in ahead of the camera every frame.
        let x = frame % 17 - 8;
        let key = SubChunkKey::new(0, x, 4 + frame / 17, 10 - (x + 10).rem_euclid(SHORE_PERIOD));
        assert!(is_shore(key, SHORE_PERIOD));
        fixture.spawn_surface(key, true);
        fixture.frame_looking(camera, Vec3::Z);
        let generation = committed(&fixture);
        if generation != last {
            commits += 1;
            last = generation;
        }
    }
    println!("resident churn over {CHURN_FRAMES} frames: commits={commits}");
    assert!(
        commits >= 4,
        "only {commits} sorts committed while water streamed in"
    );
    for _ in 0..16 {
        fixture.frame_looking(camera, Vec3::Z);
    }
    assert_drawn_in_order(&fixture, camera, Vec3::Z);
}

/// A staged snapshot holds its retired allocations as the committed one does, since a
/// readable staged upload now outlives allocation changes until it commits.
#[test]
fn staged_snapshot_keeps_its_retired_allocations() {
    let tint = ChunkBiomeTintIdentity::new(2, 2);
    let identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
    let retired = super::transparent::resident_transparent_allocation(&identity, tint);
    let key = ViewSortKey::try_new(
        [0.0; 3],
        vec![identity],
        ChunkTextureAssetIdentity::new(1, 1),
        tint,
    )
    .unwrap();
    let mut state = TransparentSortState::with_upload_cap(1);
    let generation = state.request(&key);
    let refs = vec![PackedTransparentDrawRef::new(2, 1); 2];
    assert_eq!(
        state.complete(TransparentSortResult::new(generation, key, refs).unwrap()),
        Ok(false)
    );
    assert!(state.committed().is_none());
    assert!(!transparent_retirement_can_arm(
        state.retained_keys(),
        &retired
    ));
    assert!(!state.acknowledge_upload());
    assert!(state.acknowledge_upload());
    assert!(!transparent_retirement_can_arm(
        state.retained_keys(),
        &retired
    ));
    state.reset_preserving_generation();
    assert!(transparent_retirement_can_arm(
        state.retained_keys(),
        &retired
    ));
}

/// Water drawn straight from its records satisfies a witness as committed water does.
#[test]
fn witness_counts_water_drawn_without_a_sort() {
    let flat = SubChunkKey::new(0, 1, 0, 0);
    let shore = SubChunkKey::new(0, 2, 0, 0);
    let request = TransparentWitnessRequest::try_new(1, vec![flat, shore]).unwrap();
    assert_eq!(
        transparent_view_missing_witness_keys(None, &request, |key| key == flat),
        [shore]
    );
    let sorted = ViewSortKey::try_new(
        [0.0; 3],
        vec![TransparentAllocationIdentity::new(shore, 1, 0..4, 4..6, 0)],
        ChunkTextureAssetIdentity::new(1, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    assert!(
        transparent_view_missing_witness_keys(Some(&sorted), &request, |key| key == flat)
            .is_empty()
    );
    assert_eq!(
        transparent_view_missing_witness_keys(Some(&sorted), &request, |_| false),
        [flat]
    );
}

/// A direct water draw covers the transparent records only, and gives the shader the
/// metadata index through a base vertex offset by one, so it never reads the ref buffer.
#[test]
fn direct_water_draw_covers_transparent_records_with_offset_metadata() {
    let identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 1, 16..48, 64..80, 6);
    let mut allocation = super::transparent::resident_transparent_allocation(
        &identity,
        ChunkBiomeTintIdentity::new(1, 1),
    );
    let command = transparent_liquid_direct_draw_command(&allocation).unwrap();
    assert_eq!(
        (
            command.first_instance,
            command.instance_count,
            command.base_vertex
        ),
        (4, 8, 28)
    );
    allocation.depth_liquid_range = Some(10..12);
    assert_eq!(
        transparent_liquid_direct_draw_command(&allocation)
            .unwrap()
            .instance_count,
        6
    );
    allocation.has_transparent_liquid = false;
    assert!(transparent_liquid_direct_draw_command(&allocation).is_none());
}
