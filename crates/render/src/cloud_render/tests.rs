use bevy::ecs::system::RunSystemOnce;
use sha2::Digest;
use std::sync::Arc;

use assets::{
    AtmosphereRole, AtmosphereTexture, CompiledAtmosphereAssets, RuntimeAtmosphereAssets,
    encode_atmosphere_blob,
};
use bevy::render::view::RetainedViewEntity;
use sha2::Sha256;

use super::*;

fn runtime(seed: u8) -> Arc<RuntimeAtmosphereAssets> {
    runtime_with_cloud_mask(seed, false)
}

/// Builds original cloud artwork with optional holes for geometry and pixel tests.
fn runtime_with_cloud_mask(seed: u8, patterned: bool) -> Arc<RuntimeAtmosphereAssets> {
    let textures = [
        (AtmosphereRole::Sun, "textures/environment/sun.png", 32, 32),
        (
            AtmosphereRole::MoonPhases,
            "textures/environment/moon_phases.png",
            128,
            64,
        ),
        (
            AtmosphereRole::Clouds,
            "textures/environment/clouds.png",
            meshing::CLOUD_MASK_SIZE,
            meshing::CLOUD_MASK_SIZE,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, path, width, height))| {
        let mut rgba8 = vec![seed; (width * height * 4) as usize];
        if patterned && role == AtmosphereRole::Clouds {
            for (index, texel) in rgba8.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = index % width as usize;
                let z = index / width as usize;
                if (x / 4 + z / 4).is_multiple_of(2) {
                    texel[3] = 0;
                }
            }
        }
        AtmosphereTexture {
            role,
            source_path: path.into(),
            source_bytes: 1,
            source_sha256: [index as u8 + 1; 32],
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width,
            height,
            rgba8: rgba8.into_boxed_slice(),
        }
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let blob = encode_atmosphere_blob(&CompiledAtmosphereAssets {
        source_manifest_sha256: [0x66; 32],
        textures,
        biome_profiles: Box::new([]),
        fog_profiles: Box::new([]),
    })
    .unwrap();
    Arc::new(RuntimeAtmosphereAssets::decode(&blob).unwrap())
}

fn world() -> World {
    bevy::tasks::AsyncComputeTaskPool::get_or_init(|| {
        bevy::tasks::TaskPoolBuilder::new().num_threads(2).build()
    });
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue::new(queue));
    world.init_resource::<CloudVisibility>();
    world.insert_resource(AtmosphereFrame::from_bedrock_time(0.0, 0.0, 0.0));
    world.insert_resource(AtmosphereTextureAssets::new(runtime(255), [0x41; 32]));
    world.run_system_once(init_cloud_gpu).unwrap();
    world
}

#[test]
fn hidden_clouds_do_not_prepare_or_upload_geometry() {
    let mut world = world();
    spawn_view(&mut world, Vec3::ZERO);
    world.insert_resource(CloudVisibility(false));
    world.run_system_once(prepare_cloud_records).unwrap();
    assert!(world.resource::<CloudGpu>().views.is_empty());
    assert_eq!(world.resource::<CloudGpu>().upload_count, 0);
}

#[test]
fn moving_cloud_window_reuses_its_gpu_storage() {
    let mut world = world();
    let view = spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    let first = buffer_id(&world, view);
    world
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(16.0, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(world.resource::<CloudGpu>().upload_count, 2);
    assert_eq!(buffer_id(&world, view), first);
}

/// Rebuilds count their record payload once; cached frames emit no upload request.
#[test]
fn rebuild_uploads_are_profiled_and_cached_frames_are_idle() {
    let mut world = world();
    let view = spawn_view(&mut world, Vec3::ZERO);
    let (_, cold) = render_scene::runtime_profile_uploads::measure(|| {
        world.run_system_once(prepare_cloud_records).unwrap();
    });
    let records = world.resource::<CloudGpu>().views[&view].record_count;
    let bytes = u64::from(records) * size_of::<ViewportCloudQuad>() as u64;
    assert!(bytes > 0);
    assert_eq!(cold, (1, bytes));
    let buffer = buffer_id(&world, view);

    let (_, idle) = render_scene::runtime_profile_uploads::measure(|| {
        world.run_system_once(prepare_cloud_records).unwrap();
    });
    assert_eq!(idle, (0, 0));
    world
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(16.0, 0.0, 0.0);
    let (_, moved) = render_scene::runtime_profile_uploads::measure(|| {
        world.run_system_once(prepare_cloud_records).unwrap();
    });
    let records = world.resource::<CloudGpu>().views[&view].record_count;
    assert_eq!(
        moved,
        (
            1,
            u64::from(records) * size_of::<ViewportCloudQuad>() as u64
        )
    );
    assert_eq!(buffer_id(&world, view), buffer);
}

fn spawn_view(world: &mut World, position: Vec3) -> Entity {
    let entity = world.spawn_empty().id();
    world.entity_mut(entity).insert((
        Camera3d::default(),
        crate::render_test_support::camera(false),
        ExtractedView {
            retained_view_entity: RetainedViewEntity::new(entity.into(), None, 0),
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::from_translation(position),
            clip_from_world: None,
            target_format: render_scene::SCENE_COLOR_FORMAT,
            viewport: UVec4::new(0, 0, 256, 256),
            color_grading: Default::default(),
            invert_culling: false,
        },
    ));
    entity
}

fn buffer_id(world: &World, entity: Entity) -> BufferId {
    world.resource::<CloudGpu>().views[&entity]
        .record_buffer
        .as_ref()
        .unwrap()
        .id()
}

#[test]
fn steady_uploads_cloud_colour_ignores_clock_only_changes() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue::new(queue));
    world.init_resource::<render_scene::AtmosphereViewInputs>();
    let frame = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
    world.insert_resource(frame);
    world.run_system_once(init_cloud_gpu).unwrap();
    let buffer = world.resource::<CloudGpu>().colour_buffer.id();
    let mut system = IntoSystem::into_system(prepare_cloud_colour);
    system.initialize(&mut world);
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<CloudGpu>().colour_uploads, 1);
    world.insert_resource(frame.with_cloud_renderer_ticks(123.0));
    let allocated = test_alloc::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<CloudGpu>().colour_uploads, 1);
    assert_eq!(test_alloc::thread_allocations() - allocated, 0);

    world.insert_resource(AtmosphereFrame::from_bedrock_time(6_000.0, 1.0, 1.0));
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<CloudGpu>().colour_uploads, 2);
    assert_eq!(world.resource::<CloudGpu>().colour_buffer.id(), buffer);
}

#[test]
fn viewport_records_are_identity_cached_with_exact_diagnostic_layout() {
    let mut world = world();
    world.resource_mut::<CloudGpu>().diagnostics = true;
    let view = spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    let gpu = world.resource::<CloudGpu>();
    let prepared = &gpu.views[&view];
    let config = CloudRenderConfig::legacy_fancy();
    let expected_count = u32::from(config.mesh_size()).pow(2) + 4 * u32::from(config.mesh_size());
    assert_eq!(prepared.record_count, expected_count);
    assert_eq!(gpu.upload_count, 1);
    let diagnostic = prepared.geometry_diagnostic.as_ref().unwrap();
    assert_eq!(diagnostic.quad_count(), expected_count);
    assert_eq!(
        diagnostic.quad_bytes(),
        expected_count * size_of::<ViewportCloudQuad>() as u32
    );
    assert_eq!(diagnostic.instance_count(), 1);
    assert_eq!(
        diagnostic.occupied_texels(),
        meshing::CLOUD_MASK_SIZE.pow(2)
    );
    assert!(
        diagnostic
            .marker_fields()
            .contains("geometry=viewport record_stride_bytes=16")
    );
    assert!(diagnostic.marker_fields().contains("calibrated=false"));
    let first = buffer_id(&world, view);
    world.insert_resource(AtmosphereTextureAssets::new(runtime(254), [0x41; 32]));
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, view), first);
    assert_eq!(world.resource::<CloudGpu>().upload_count, 1);
    world.insert_resource(AtmosphereTextureAssets::new(runtime(254), [0x42; 32]));
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, view), first);
    assert_eq!(world.resource::<CloudGpu>().upload_count, 2);
}

#[test]
fn dimension_changes_remove_clouds_and_restore_them_only_in_the_overworld() {
    let mut world = world();
    let view = spawn_view(&mut world, Vec3::ZERO);
    let base = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert!(world.resource::<CloudGpu>().views[&view].record_count > 0);

    for kind in [render_scene::SkyKind::End, render_scene::SkyKind::Nether] {
        world.insert_resource(base.with_sky_kind(kind));
        world.run_system_once(prepare_cloud_records).unwrap();
        assert!(
            world.resource::<CloudGpu>().views.is_empty(),
            "clouds survived the switch to {kind:?}"
        );

        world.insert_resource(base.with_sky_kind(render_scene::SkyKind::Overworld));
        world.run_system_once(prepare_cloud_records).unwrap();
        assert!(world.resource::<CloudGpu>().views[&view].record_count > 0);
    }
}

#[test]
fn per_view_cache_does_not_churn_on_subpixel_moves_and_is_pruned_on_removal() {
    let mut world = world();
    let first = spawn_view(&mut world, Vec3::ZERO);
    let second = spawn_view(&mut world, Vec3::new(-1000.0, 0.0, -1000.0));
    world.run_system_once(prepare_cloud_records).unwrap();
    let first_buffer = buffer_id(&world, first);
    let second_buffer = buffer_id(&world, second);
    world
        .get_mut::<ExtractedView>(first)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(0.125, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, first), first_buffer);
    assert_eq!(buffer_id(&world, second), second_buffer);
    world
        .get_mut::<ExtractedView>(first)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(15.125, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, first), first_buffer);
    assert_eq!(buffer_id(&world, second), second_buffer);
    world.despawn(first);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(world.resource::<CloudGpu>().views.len(), 1);
}

#[test]
fn scroll_is_continuous_until_native_rebuild_threshold_and_invalid_view_is_discarded() {
    let mut world = world();
    let view = spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    let first = buffer_id(&world, view);
    world.insert_resource(
        AtmosphereFrame::from_bedrock_time(0.0, 0.0, 0.0).with_cloud_renderer_ticks(1.0),
    );
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, view), first);
    world.insert_resource(
        AtmosphereFrame::from_bedrock_time(0.0, 0.0, 0.0).with_cloud_renderer_ticks(1000.0),
    );
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, view), first);
    world
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(f32::NAN, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert!(world.resource::<CloudGpu>().views.is_empty());
}

#[test]
fn window_bounds_use_admitted_sampling_centre_and_current_subpixel_scroll() {
    let viewport = CloudViewport::try_new([-0.001; 2], 2, 1, false, false).unwrap();
    let midpoint_y = (CLOUD_UNDERSIDE_Y + CLOUD_TOP_Y) * 0.5;
    assert_eq!(
        cloud_bounds_center(viewport, 0.0),
        [-16.0, midpoint_y, -16.0]
    );
    assert_eq!(
        cloud_bounds_center(viewport, 0.125),
        [-16.125, midpoint_y, -16.0]
    );
}

#[test]
fn diagnostics_are_opt_in_and_hidden_views_cancel_speculation() {
    let mut world = world();
    world.resource_mut::<CloudGpu>().diagnostics = false;
    let view = spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert!(
        world.resource::<CloudGpu>().views[&view]
            .geometry_diagnostic
            .is_none()
    );
    world.insert_resource(CloudVisibility(false));
    world
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(100.0, 140.0, 0.0);
    let uploads = world.resource::<CloudGpu>().upload_count;
    world.run_system_once(prepare_cloud_records).unwrap();
    assert!(world.resource::<CloudGpu>().views.is_empty());
    assert_eq!(world.resource::<CloudGpu>().upload_count, uploads);
    world.insert_resource(CloudVisibility(true));
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(
        world.resource::<CloudGpu>().views[&view].viewport.centre(),
        [96.0, 0.0]
    );
}

#[test]
fn worker_records_match_direct_meshing_above_inside_and_below_the_layer() {
    use meshing::cloud_viewport::mesh_cloud_viewport;
    let mut world = world();
    world.insert_resource(AtmosphereTextureAssets::new(
        runtime_with_cloud_mask(255, true),
        [0x41; 32],
    ));
    let view = spawn_view(&mut world, Vec3::ZERO);
    for (index, y) in [
        CLOUD_UNDERSIDE_Y - 1.0,
        CLOUD_UNDERSIDE_Y + 1.0,
        CLOUD_TOP_Y + 1.0,
    ]
    .into_iter()
    .enumerate()
    {
        let x = -16.125 + index as f32 * 70.0;
        world
            .get_mut::<ExtractedView>(view)
            .unwrap()
            .world_from_view = GlobalTransform::from_xyz(x, y, -0.125);
        world.run_system_once(prepare_cloud_records).unwrap();
        let runtime = Arc::clone(
            world
                .resource::<AtmosphereTextureAssets>()
                .runtime()
                .unwrap(),
        );
        let mut gpu = world.resource_mut::<CloudGpu>();
        let prepared = gpu.views.get_mut(&view).unwrap();
        let (mesh, waited) = prepared
            .mesh_cache
            .get(
                &runtime,
                prepared.viewport,
                y >= CLOUD_UNDERSIDE_Y.ceil(),
                false,
            )
            .unwrap();
        assert!(!waited);
        assert_ne!(mesh.worker_thread, std::thread::current().id());
        assert_eq!(
            mesh.records.as_ref(),
            mesh_cloud_viewport(
                runtime.texture(AtmosphereRole::Clouds).unwrap(),
                prepared.viewport
            )
            .unwrap()
        );
        assert!(mesh.diagnostic.is_none());
        assert!(prepared.mesh_cache.len() <= mesh_cache::MAX_CACHED_WINDOWS);
    }
}

#[test]
fn steady_cloud_geometry_preparation_does_not_allocate() {
    let mut world = world();
    spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    let mut system = IntoSystem::into_system(prepare_cloud_records);
    system.initialize(&mut world);
    system.run((), &mut world).unwrap();
    let before = test_alloc::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(test_alloc::thread_allocations() - before, 0);
}

#[path = "raster_tests.rs"]
mod raster_tests;

#[test]
fn prefetched_flight_preserves_admission_frames_without_reallocating_or_waiting() {
    for speed in [32.0, 128.0] {
        let mut world = world();
        let view = spawn_view(&mut world, Vec3::ZERO);
        world.run_system_once(prepare_cloud_records).unwrap();
        let initial_waits = world.resource::<CloudGpu>().work.waited;
        let mut expected_viewport = world.resource::<CloudGpu>().views[&view].viewport;
        let mut expected_rebuilds = 1;
        let config = CloudRenderConfig::legacy_fancy();
        for frame in 1..=600 {
            world.resource::<CloudGpu>().views[&view]
                .mesh_cache
                .finish_pending();
            let x = frame as f32 * speed / 60.0;
            let position = Vec3::new(x * 0.8, 0.0, -x * 0.6);
            let requested = CloudViewport::try_new(
                [f64::from(position.x), f64::from(position.z)],
                config.mesh_size(),
                config.grid_size(),
                false,
                false,
            )
            .unwrap();
            if expected_viewport.needs_rebuild(requested) {
                expected_viewport = requested;
                expected_rebuilds += 1;
            }
            world
                .get_mut::<ExtractedView>(view)
                .unwrap()
                .world_from_view = GlobalTransform::from_translation(position);
            world.run_system_once(prepare_cloud_records).unwrap();
            let gpu = world.resource::<CloudGpu>();
            assert_eq!(
                gpu.views[&view].viewport, expected_viewport,
                "{speed} blocks/s frame {frame}"
            );
            assert_eq!(u64::from(gpu.upload_count), expected_rebuilds);
            assert_eq!(gpu.work.waited, initial_waits);
            assert_eq!(gpu.work.buffer_creations, 1);
            assert!(gpu.views[&view].mesh_cache.len() <= mesh_cache::MAX_CACHED_WINDOWS);
        }
        let gpu = world.resource::<CloudGpu>();
        println!(
            "cloud flight {speed} blocks/s, 600 frames: rebuilds={}, buffer creations {} -> {}, post-warmup unfinished admissions={}, uploaded_bytes={}",
            expected_rebuilds,
            expected_rebuilds,
            gpu.work.buffer_creations,
            gpu.work.waited - initial_waits,
            gpu.work.uploaded_bytes
        );
    }
}

#[test]
fn retained_storage_handles_growth_empty_windows_and_asset_replacement() {
    let mut world = world();
    let view = spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    let original = buffer_id(&world, view);
    world.insert_resource(AtmosphereTextureAssets::new(runtime(0), [0x42; 32]));
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(world.resource::<CloudGpu>().views[&view].record_count, 0);
    assert_eq!(buffer_id(&world, view), original);
    let small = world
        .resource::<RenderDevice>()
        .create_buffer(&BufferDescriptor {
            label: Some("small cloud fixture storage"),
            size: size_of::<ViewportCloudQuad>() as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    let small_id = small.id();
    world
        .resource_mut::<CloudGpu>()
        .views
        .get_mut(&view)
        .unwrap()
        .record_buffer = Some(small);
    world.insert_resource(AtmosphereTextureAssets::new(runtime(255), [0x43; 32]));
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_ne!(buffer_id(&world, view), small_id);
    assert!(world.resource::<CloudGpu>().views[&view].record_count > 0);
    assert_eq!(world.resource::<CloudGpu>().work.buffer_creations, 2);
}

#[test]
fn frame_uploads_stage_cloud_records_and_colour() {
    use crate::upload_staging::BufferUploadStaging;
    let mut world = world();
    world.init_resource::<render_scene::AtmosphereViewInputs>();
    let staging = BufferUploadStaging::new(world.resource());
    world.insert_resource(staging);
    spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    world.run_system_once(prepare_cloud_colour).unwrap();
    let stats = world.resource::<BufferUploadStaging>().stats();
    assert_eq!(stats.staged_writes, 2);
    assert_eq!(stats.fallback_writes, 0);
}

/// A cold window must not wait behind unrelated work occupying every shared worker.
#[test]
fn cold_cloud_window_does_not_wait_for_a_busy_worker_pool() {
    let _world = world();
    let pool = bevy::tasks::AsyncComputeTaskPool::get();
    let workers = pool.thread_num();
    let release = Arc::new(std::sync::Barrier::new(workers + 1));
    let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let blockers = (0..workers)
        .map(|_| {
            let release = Arc::clone(&release);
            let started = Arc::clone(&started);
            pool.spawn(async move {
                started.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                release.wait();
            })
        })
        .collect::<Vec<_>>();
    test_time::eventually("every shared worker is busy", || {
        started.load(std::sync::atomic::Ordering::SeqCst) == workers
    });
    let runtime = runtime(255);
    let config = CloudRenderConfig::legacy_fancy();
    let viewport = CloudViewport::try_new(
        [100.0, -40.0],
        config.mesh_size(),
        config.grid_size(),
        false,
        false,
    )
    .unwrap();
    let mut cache = mesh_cache::CloudMeshCache::default();
    cache.set_identity([0x41; 32]);
    cache.prefetch(&runtime, viewport, false);
    let (mesh, missed) = cache.get(&runtime, viewport, false, false).unwrap();
    release.wait();
    for blocker in blockers {
        bevy::tasks::block_on(blocker);
    }
    assert!(missed);
    assert_eq!(
        mesh.records.as_ref(),
        mesh_cloud_viewport(runtime.texture(AtmosphereRole::Clouds).unwrap(), viewport).unwrap()
    );
}
