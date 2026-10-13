use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_scene::{
    ChunkRenderInstance, ChunkRenderQueue, ChunkUploadAcknowledgements, ChunkUploadBudget,
};

/// Creates a render instance through the same queue handoff used during play.
fn instance() -> ChunkRenderInstance {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::with_budget(ChunkUploadBudget::new(
            1,
            u64::MAX,
        )));
    let solid = solid_test_mesh();
    let mesh = meshing::ChunkMesh::from_streams(
        solid
            .quads()
            .iter()
            .flat_map(|quad| std::iter::repeat_n(*quad, 64))
            .collect(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        solid.connectivity(),
    );
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update(
            SubChunkKey::new(0, 0, 0, 0),
            mesh,
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();
    app.update();
    let world = app.world_mut();
    let mut query = world.query::<&ChunkRenderInstance>();
    query.single(world).unwrap().clone()
}

/// A deterministic 12-chunk-radius, full-height join with a bounded upload allowance.
#[test]
fn join_sized_arena_uploads_avoid_repeated_migrations() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let mut app = App::new();
    app.insert_resource(ChunkGpuArena::new(&device))
        .insert_resource(device)
        .insert_resource(RenderQueue::new(queue))
        .insert_resource(ChunkUploadBudget::new(
            512,
            PublicationServiceConfig::DEFAULT.maximum_frame_bytes,
        ))
        .insert_resource(ChunkGpuUploadStats::default())
        .insert_resource(ChunkBiomeTints::default())
        .insert_resource(ChunkTextureAssets::default())
        .insert_resource(ChunkUploadAcknowledgements::default())
        .insert_resource(ChunkGpuRemovalQueue::default())
        .insert_resource(TransparentRetirementFence::default())
        .insert_resource(GpuUpdateFairness::default());
    let template = instance();
    let sections = 25 * 25 * 24;
    for index in 0..sections {
        let mut instance = template.clone();
        instance.key = SubChunkKey::new(0, index % 25, index / 625 - 4, (index / 25) % 25);
        instance.origin = chunk_origin(instance.key);
        app.world_mut().spawn(instance);
    }
    let mut migrations = 0;
    let mut copied = 0;
    let mut blocked = 0;
    let mut growth_wait_frames = 0;
    let mut frames = 0;
    let mut max_prepare = Duration::ZERO;
    let mut total_prepare = Duration::ZERO;
    while app.world().resource::<ChunkGpuArena>().allocations.len() < sections as usize {
        frames += 1;
        assert!(frames < 100);
        let started = Instant::now();
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        let elapsed = started.elapsed();
        max_prepare = max_prepare.max(elapsed);
        total_prepare += elapsed;
        let stats = *app.world().resource::<ChunkGpuUploadStats>();
        migrations += stats.migrations_started;
        copied += stats.gpu_copy_bytes;
        growth_wait_frames += usize::from(stats.uploads_waiting_for_growth > 0);
        blocked += usize::from(stats.uploads_waiting_for_growth > 0 && stats.chunk_updates == 0);
        assert!(stats.total_bytes <= PublicationServiceConfig::DEFAULT.maximum_frame_bytes);
    }
    eprintln!(
        "join sections={sections} frames={frames} migrations={migrations} copied_bytes={copied} blocked_frames={blocked} growth_wait_frames={growth_wait_frames} max_prepare_us={} total_prepare_us={}",
        max_prepare.as_micros(),
        total_prepare.as_micros()
    );
    assert!(migrations <= 3);
    assert_eq!(copied, 0);
    assert_eq!(blocked, 0);
    assert_eq!(growth_wait_frames, 0);
    assert_eq!(frames, (sections as usize).div_ceil(512));
}

/// Small allocations leave a larger hole intact instead of fragmenting it.
#[test]
fn best_fit_preserves_large_holes_under_small_allocation_churn() {
    let mut len = 256;
    let mut free = vec![0..128, 160..168, 192..200];
    assert_eq!(allocate_quad_range(&mut len, &mut free, 8, 256), Some(160));
    assert_eq!(allocate_quad_range(&mut len, &mut free, 8, 256), Some(192));
    assert_eq!(allocate_quad_range(&mut len, &mut free, 128, 256), Some(0));
    assert_eq!(len, 256);
    assert!(free.is_empty());
}

/// Uploads fitting the old buffer still publish while a large migration is incomplete.
#[test]
fn ongoing_migration_does_not_block_uploads_in_existing_capacity() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let mut arena = ChunkGpuArena::new(&device);
    let capacity = (3 * ARENA_MIGRATION_FRAME_BYTES / PACKED_QUAD_BYTES) as usize;
    arena.quad_capacity = capacity;
    arena.quad_buffer = create_storage_buffer(
        &device,
        "test migration source",
        capacity as u64 * PACKED_QUAD_BYTES,
    );
    arena.geometry_stream_capacity = 4096;
    arena.geometry_stream_buffer = create_storage_buffer(
        &device,
        "test stream space",
        4096 * GEOMETRY_STREAM_WORD_BYTES,
    );
    begin_arena_migration(
        &mut arena,
        &device,
        ArenaStream::Quads,
        ArenaGrowthPlan {
            new_capacity: capacity * 2,
            gpu_copy_bytes: capacity as u64 * PACKED_QUAD_BYTES,
        },
    );
    advance_arena_migration(
        &mut arena,
        TerrainUploads {
            device: &device,
            queue: &queue,
            staging: None,
        },
        ARENA_MIGRATION_FRAME_BYTES,
    );
    let mut app = App::new();
    app.insert_resource(arena)
        .insert_resource(device)
        .insert_resource(queue)
        .insert_resource(ChunkUploadBudget::new(
            4,
            PublicationServiceConfig::DEFAULT.maximum_frame_bytes,
        ))
        .insert_resource(ChunkGpuUploadStats::default())
        .insert_resource(ChunkBiomeTints::default())
        .insert_resource(ChunkTextureAssets::default())
        .insert_resource(ChunkUploadAcknowledgements::default())
        .insert_resource(ChunkGpuRemovalQueue::default())
        .insert_resource(TransparentRetirementFence::default())
        .insert_resource(GpuUpdateFairness::default());
    app.world_mut().spawn(instance());
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    assert!(app.world().resource::<ChunkGpuArena>().migration.is_some());
    assert_eq!(app.world().resource::<ChunkGpuArena>().allocations.len(), 1);
    assert_eq!(
        app.world().resource::<ChunkGpuUploadStats>().chunk_updates,
        1
    );
}

/// Invalid nearby entries cannot prevent a valid distant entry from using the upload allowance.
#[test]
fn rejected_candidates_do_not_starve_a_valid_upload_beyond_the_item_ceiling() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(ChunkUploadBudget::new(
        1,
        PublicationServiceConfig::DEFAULT.maximum_frame_bytes,
    ));
    let template = instance();
    for index in 0..PublicationServiceConfig::DEFAULT.maximum_frame_items + 1 {
        let mut rejected = template.clone();
        rejected.key = SubChunkKey::new(0, index as i32, 0, 0);
        rejected.cube_lighting = Arc::from([]);
        app.world_mut().spawn(rejected);
    }
    let mut valid = template;
    valid.key = SubChunkKey::new(0, 10_000, 0, 0);
    let entity = app.world_mut().spawn(valid).id();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    assert!(
        app.world()
            .resource::<ChunkGpuArena>()
            .allocations
            .contains_key(&entity)
    );
    assert_eq!(
        app.world().resource::<ChunkGpuUploadStats>().chunk_updates,
        1
    );
}
