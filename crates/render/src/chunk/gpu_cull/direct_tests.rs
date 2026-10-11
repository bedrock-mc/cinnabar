//! Direct-draw occlusion: verdict policy, readback ring, device selection and app wiring.

use super::app_tests::{
    KEYS, camera_transform, chunk_app, frame, insert_meshes, noop_render_plugin, render_plugin,
};
use super::direct::{DirectOcclusion, VerdictQueue, direct_occlusion_supported, view_basis};
use super::occlusion::{OcclusionBasis, OcclusionHistory, VerdictTag};
use super::*;

const EYE: [f32; 3] = [8.5, 70.0, 8.5];

fn view(eye: [f32; 3], world: u64) -> OcclusionBasis {
    OcclusionBasis {
        eye,
        view_rotation: Mat3::IDENTITY.to_cols_array(),
        clip_from_view: Mat4::perspective_infinite_reverse_rh(1.2, 1.0, 0.05).to_cols_array(),
        viewport: [0, 0, 256, 256],
        depth_size: [256, 256],
        depth_samples: 1,
        world,
    }
}

fn tag(frame: u64, basis: OcclusionBasis, slots: u32) -> VerdictTag {
    VerdictTag {
        frame,
        basis,
        slots,
    }
}

fn words(occluded: &[u32]) -> Vec<u32> {
    let mut words = vec![0; 2];
    for &slot in occluded {
        words[slot as usize / 32] |= 1 << (slot % 32);
    }
    words
}

fn turned(view: OcclusionBasis, angle: f32) -> OcclusionBasis {
    OcclusionBasis {
        view_rotation: Mat3::from_rotation_y(angle).to_cols_array(),
        ..view
    }
}

#[test]
fn a_slot_is_skipped_only_after_consecutive_occluded_verdicts_of_an_unchanged_basis() {
    let mut history = OcclusionHistory::default();
    for slot in 0..4 {
        history.assign(slot, 1);
    }
    let still = view(EYE, history.world());
    history.apply(&tag(2, still, 4), &words(&[1, 2]));
    assert!(!history.skips(1, &still), "one verdict is not enough");
    history.apply(&tag(3, still, 4), &words(&[1]));
    assert!(history.skips(1, &still));
    assert!(
        !history.skips(2, &still),
        "a visible verdict restarts the run"
    );
    assert!(!history.skips(0, &still));

    // Any eye translation voids every verdict: near occluders uncover far terrain by parallax.
    let moved = view([EYE[0] + 1.0e-3, EYE[1], EYE[2]], history.world());
    assert!(!history.skips(1, &moved));
    let mut zoomed = still;
    zoomed.clip_from_view[0] *= 1.1;
    assert!(!history.skips(1, &zoomed));
    let mut resized = still;
    resized.depth_size = [512, 256];
    assert!(!history.skips(1, &resized));
    // Any turn too: the near plane swings with the view and can clip a near occluder.
    assert!(!history.skips(1, &turned(still, 1.0e-3)));

    // A verdict under a new basis restarts every run, even if the old pose returns.
    history.apply(&tag(4, moved, 4), &words(&[1]));
    assert!(!history.skips(1, &moved) && !history.skips(1, &still));
}

#[test]
fn verdicts_settle_until_the_view_or_a_record_changes() {
    let mut history = OcclusionHistory::default();
    history.assign(0, 1);
    let still = view(EYE, history.world());
    assert!(!history.settled(&still));
    history.apply(&tag(2, still, 1), &words(&[0]));
    assert!(!history.settled(&still));
    history.apply(&tag(3, still, 1), &words(&[0]));
    assert!(history.settled(&still));
    let turned = turned(still, 0.01);
    assert!(!history.settled(&turned));
    history.apply(&tag(4, turned, 1), &words(&[0]));
    assert!(!history.settled(&turned) && !history.settled(&still));
    history.apply(&tag(5, turned, 1), &words(&[0]));
    assert!(history.settled(&turned));
    // A new record wants verdicts computed after it; one already in flight does not count.
    history.assign(1, 7);
    history.apply(&tag(6, turned, 2), &words(&[0, 1]));
    history.apply(&tag(7, turned, 2), &words(&[0, 1]));
    assert!(!history.settled(&turned));
    history.apply(&tag(8, turned, 2), &words(&[0, 1]));
    assert!(history.settled(&turned) && history.skips(1, &turned));
    history.invalidate_world();
    assert!(!history.settled(&view(EYE, history.world())));
}

#[test]
fn verdicts_never_cover_newer_records_or_changed_geometry() {
    let mut history = OcclusionHistory::default();
    history.assign(0, 1);
    history.assign(1, 5);
    let still = view(EYE, history.world());
    for frame in 2..=4 {
        history.apply(&tag(frame, still, 2), &words(&[0, 1]));
    }
    assert!(history.skips(0, &still));
    assert!(
        !history.skips(1, &still),
        "slot 1's record is newer than every verdict"
    );
    history.assign(0, 6);
    assert!(!history.skips(0, &still), "a rewritten slot starts over");

    for frame in 7..=8 {
        history.apply(&tag(frame, still, 2), &words(&[0]));
    }
    assert!(history.skips(0, &still));
    history.invalidate_world();
    let current = view(EYE, history.world());
    assert!(!history.skips(0, &current) && !history.skips(0, &still));
    // Verdicts computed before the change and read back after it stay void.
    for frame in 9..=10 {
        history.apply(&tag(frame, still, 2), &words(&[0]));
    }
    assert!(!history.skips(0, &current));
    for frame in 11..=12 {
        history.apply(&tag(frame, current, 2), &words(&[0]));
    }
    assert!(history.skips(0, &current));
    assert!(
        !history.skips(1, &current),
        "slots past a verdict's count stay drawn"
    );
}

#[test]
fn the_verdict_ring_drops_frames_when_full_and_drains_oldest_first() {
    let still = view(EYE, 0);
    let mut ring = VerdictQueue::default();
    let slots = (1..=3)
        .map(|frame| {
            let slot = ring.acquire(tag(frame, still, 1)).unwrap();
            drop(ring.submit(slot));
            slot
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ring.acquire(tag(4, still, 1)),
        None,
        "a full ring drops the frame"
    );
    let mut drained = Vec::new();
    ring.complete(slots[1], true);
    ring.drain(|slot, tag| drained.push((slot, tag.map(|tag| tag.frame))));
    assert!(drained.is_empty(), "the oldest readback is still pending");
    ring.complete(slots[0], false);
    ring.drain(|slot, tag| drained.push((slot, tag.map(|tag| tag.frame))));
    assert_eq!(drained, [(slots[0], None), (slots[1], Some(2))]);

    // Freed slots are reused; a slot that never encoded is released unread.
    let reused = ring.acquire(tag(5, still, 1)).unwrap();
    ring.release(reused);
    ring.complete(slots[2], true);
    drained.clear();
    ring.drain(|slot, tag| drained.push((slot, tag.map(|tag| tag.frame))));
    assert_eq!(drained, [(slots[2], Some(3))]);
}

#[test]
fn only_direct_draw_devices_with_compute_read_occlusion_back() {
    let compute = DownlevelFlags::COMPUTE_SHADERS;
    assert!(direct_occlusion_supported(
        ChunkDrawMode::Direct,
        compute,
        false
    ));
    assert!(!direct_occlusion_supported(
        ChunkDrawMode::Direct,
        compute,
        true
    ));
    assert!(!direct_occlusion_supported(
        ChunkDrawMode::Direct,
        DownlevelFlags::empty(),
        false
    ));
    assert!(!direct_occlusion_supported(
        ChunkDrawMode::MultiDrawIndirect,
        compute,
        false
    ));
    // Metal draws directly and reads back; Vulkan keeps the count-driven GPU cull.
    let flags = DownlevelFlags::all();
    let features = WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT | WgpuFeatures::INDIRECT_FIRST_INSTANCE;
    let metal = select_chunk_draw_mode(flags, features, Backends::METAL, false);
    assert!(direct_occlusion_supported(metal, flags, false));
    assert!(!gpu_cull_supported(metal, features, flags, false));
    let vulkan = select_chunk_draw_mode(flags, features, Backends::VULKAN, false);
    assert!(!direct_occlusion_supported(vulkan, flags, false));
    assert!(gpu_cull_supported(vulkan, features, flags, false));
}

fn stats(app: &App) -> super::direct::DirectOcclusionStats {
    app.sub_app(RenderApp)
        .world()
        .resource::<DirectOcclusion>()
        .stats
}

/// Readbacks still arrive when the device is polled once per frame and never by their owner.
#[test]
fn one_device_poll_per_frame_still_delivers_occlusion_readbacks() {
    let (mut app, _) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Sample4,
        camera_transform(),
    );
    insert_meshes(&mut app, &KEYS);
    let polls = |app: &App| {
        app.sub_app(RenderApp)
            .world()
            .resource::<crate::device_poll::DevicePolls>()
            .0
    };
    let start = polls(&app);
    for frames in 1..=8 {
        app.update();
        assert_eq!(polls(&app) - start, frames);
    }
    assert!(stats(&app).verdicts_applied > 0);
}

/// The terrain pass runs only while a still camera's verdicts are unsettled.
#[test]
fn direct_draw_devices_split_solid_terrain_out_only_until_a_still_view_settles() {
    let (mut app, camera) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Sample4,
        camera_transform(),
    );
    insert_meshes(&mut app, &KEYS);
    let run = |app: &mut App, frames: usize| {
        (0..frames)
            .map(|_| {
                frame(app);
                stats(app)
            })
            .collect::<Vec<_>>()
    };
    let still = run(&mut app, 8);
    let render_world = app.sub_app(RenderApp).world();
    assert_eq!(
        render_world.resource::<DirectOcclusionSupport>(),
        &DirectOcclusionSupport(true)
    );
    assert_eq!(
        render_world.resource::<GpuCullSupport>(),
        &GpuCullSupport(false)
    );
    assert!(still.iter().any(|stats| stats.solid_drawn == 2));
    let settled = still.last().unwrap();
    assert_eq!(
        (settled.candidates, settled.solid_drawn, settled.skipped),
        (2, 0, 0)
    );
    assert_eq!(
        still[still.len() - 2].verdicts_applied,
        settled.verdicts_applied,
        "a settled still view stops asking for verdicts"
    );
    for _ in 0..2 {
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .translation
            .x += 0.25;
        let moving = run(&mut app, 1)[0];
        assert_eq!((moving.candidates, moving.solid_drawn), (2, 0));
    }
    let stopped = run(&mut app, 3);
    assert!(
        stopped.iter().any(|stats| stats.solid_drawn == 2),
        "stopping asks for verdicts again"
    );
}

/// On a real adapter, a still camera skips the row a wall hides and nothing in front of it.
#[test]
fn a_still_camera_skips_sub_chunks_behind_a_wall_on_a_native_device() {
    let Some(render) = render_plugin(wgpu::Backends::PRIMARY, WgpuFeatures::empty()) else {
        eprintln!("skipping direct occlusion app: missing native GPU adapter fixture");
        return;
    };
    let eye = Transform::from_xyz(8.0, 8.0, -24.0).looking_at(Vec3::new(8.0, 8.0, 100.0), Vec3::Y);
    let (mut app, _) = chunk_app(render, Msaa::Off, eye);
    let wall = (-3..=3).flat_map(|x| (-1..=1).map(move |y| SubChunkKey::new(0, x, y, 0)));
    let hidden = (-1..=1).map(|x| SubChunkKey::new(0, x, 0, 3));
    insert_meshes(&mut app, &wall.chain(hidden).collect::<Vec<_>>());
    for _ in 0..16 {
        frame(&mut app);
    }
    let still = stats(&app);
    assert!(still.verdicts_applied > 0);
    assert_eq!(still.skipped, 3, "{still:?}");
    assert_eq!(
        still.solid_drawn, 0,
        "a settled view draws in the opaque pass: {still:?}"
    );
}

/// A viewport that grows at the same aspect inside an unchanged target voids old verdicts.
#[test]
fn a_resized_viewport_voids_old_verdicts() {
    let extracted = |viewport: UVec4| ExtractedView {
        retained_view_entity: bevy::render::view::RetainedViewEntity::new(
            Entity::PLACEHOLDER.into(),
            None,
            0,
        ),
        clip_from_view: Mat4::perspective_infinite_reverse_rh(1.2, 16.0 / 9.0, 0.05),
        world_from_view: GlobalTransform::from_translation(Vec3::from_array(EYE)),
        clip_from_world: None,
        target_format: crate::SCENE_COLOR_FORMAT,
        viewport,
        color_grading: default(),
        invert_culling: false,
    };
    let small = view_basis(&extracted(UVec4::new(0, 0, 960, 540)), 1);
    let large = view_basis(&extracted(UVec4::new(0, 0, 1920, 1080)), 1);
    let mut history = OcclusionHistory::default();
    history.assign(0, 1);
    for frame in 2..=3 {
        history.apply(&tag(frame, small, 1), &words(&[0]));
    }
    assert!(history.skips(0, &small));
    assert!(!history.skips(0, &large));
}

/// Removing many sub-chunks at once voids the verdicts once, not once per sub-chunk.
#[test]
fn a_bulk_removal_voids_verdicts_once_per_frame() {
    let (mut app, _) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Sample4,
        camera_transform(),
    );
    let keys = (0..8)
        .map(|x| SubChunkKey::new(0, x, 0, 0))
        .collect::<Vec<_>>();
    insert_meshes(&mut app, &keys);
    for _ in 0..4 {
        frame(&mut app);
    }
    let entities = keys
        .iter()
        .map(|key| app.world().resource::<ChunkEntities>().0[key])
        .collect::<Vec<_>>();
    for entity in entities {
        app.world_mut()
            .entity_mut(entity)
            .remove::<ChunkRenderInstance>();
    }
    let mut before = stats(&app).world_invalidations;
    let mut total = 0;
    for _ in 0..4 {
        frame(&mut app);
        let after = stats(&app).world_invalidations;
        assert!(
            after - before <= 1,
            "{} invalidations in one frame",
            after - before
        );
        total += after - before;
        before = after;
    }
    assert!(total >= 1, "removals void the verdicts");
    assert_eq!(
        app.sub_app(RenderApp)
            .world()
            .resource::<DirectOcclusion>()
            .stats
            .candidates,
        0
    );
}

/// A settled occlusion result cannot survive a different pixel coverage pattern.
#[test]
fn changing_multisample_coverage_restarts_settled_direct_occlusion() {
    let (mut app, camera) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Sample4,
        camera_transform(),
    );
    insert_meshes(&mut app, &KEYS);
    for _ in 0..16 {
        frame(&mut app);
    }
    let settled = stats(&app);
    assert!(settled.verdicts_applied > 0);
    assert_eq!(settled.solid_drawn, 0);
    *app.world_mut().get_mut::<Msaa>(camera).unwrap() = Msaa::Off;
    let mut refreshed = false;
    for _ in 0..8 {
        frame(&mut app);
        refreshed |= stats(&app).solid_drawn == KEYS.len() as u32;
    }
    assert!(
        refreshed,
        "a new coverage pattern must request fresh depth verdicts"
    );
    assert!(stats(&app).verdicts_applied > settled.verdicts_applied);
}

/// Readbacks from every other sample count remain stale even when they arrive late.
#[test]
fn sample_count_changes_void_occlusion_history_before_new_readbacks() {
    for samples in [1, 2, 4, 8] {
        let mut history = OcclusionHistory::default();
        history.assign(0, 1);
        let mut basis = view(EYE, history.world());
        basis.depth_samples = samples;
        for frame in 2..=3 {
            history.apply(&tag(frame, basis, 1), &words(&[0]));
        }
        assert!(history.skips(0, &basis));
        for changed in [1, 2, 4, 8]
            .into_iter()
            .filter(|changed| *changed != samples)
        {
            let current = OcclusionBasis {
                depth_samples: changed,
                ..basis
            };
            assert!(!history.skips(0, &current));
            assert!(!history.settled(&current));
            history.apply(&tag(4, basis, 1), &words(&[0]));
            assert!(!history.skips(0, &current));
        }
    }
}
