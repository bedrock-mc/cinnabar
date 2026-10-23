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

/// Returns storage buffer sizes so a frame's replacements can be measured without a clock.
fn storage_buffer_bytes(arena: &ChunkGpuArena) -> [u64; 4] {
    let ArenaRequiredLengths {
        quads,
        geometry_stream_words,
        origins,
        biome_words,
    } = arena_capacities(arena);
    [
        buffer_byte_len(quads, PACKED_QUAD_BYTES),
        buffer_byte_len(geometry_stream_words, GEOMETRY_STREAM_WORD_BYTES),
        buffer_byte_len(origins, CHUNK_ORIGIN_BYTES),
        buffer_byte_len(biome_words, BIOME_WORD_BYTES),
    ]
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
    let mut prepare_samples = Vec::new();
    let mut previous_buffers = storage_buffer_bytes(app.world().resource::<ChunkGpuArena>());
    let mut largest_buffer = 0;
    let mut max_allocation_bytes = 0;
    while app.world().resource::<ChunkGpuArena>().allocations.len() < sections as usize {
        frames += 1;
        assert!(frames < 100);
        let started = Instant::now();
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        let elapsed = started.elapsed();
        max_prepare = max_prepare.max(elapsed);
        total_prepare += elapsed;
        prepare_samples.push(elapsed);
        let buffers = storage_buffer_bytes(app.world().resource::<ChunkGpuArena>());
        let allocated_bytes: u64 = buffers
            .into_iter()
            .zip(previous_buffers)
            .filter_map(|(new, old)| (new != old).then_some(new))
            .sum();
        max_allocation_bytes = max_allocation_bytes.max(allocated_bytes);
        largest_buffer = largest_buffer.max(buffers.into_iter().max().unwrap());
        previous_buffers = buffers;
        let stats = *app.world().resource::<ChunkGpuUploadStats>();
        migrations += stats.migrations_started;
        copied += stats.gpu_copy_bytes;
        growth_wait_frames += usize::from(stats.uploads_waiting_for_growth > 0);
        blocked += usize::from(stats.uploads_waiting_for_growth > 0 && stats.chunk_updates == 0);
        assert!(stats.total_bytes <= PublicationServiceConfig::DEFAULT.maximum_frame_bytes);
    }
    prepare_samples.sort_unstable();
    let median_prepare = (prepare_samples[(prepare_samples.len() - 1) / 2]
        + prepare_samples[prepare_samples.len() / 2])
        / 2;
    eprintln!(
        "join sections={sections} frames={frames} migrations={migrations} copied_bytes={copied} blocked_frames={blocked} growth_wait_frames={growth_wait_frames} max_allocation_bytes={max_allocation_bytes} max_prepare_us={} median_prepare_us={} total_prepare_us={}",
        max_prepare.as_micros(),
        median_prepare.as_micros(),
        total_prepare.as_micros()
    );
    assert!(
        max_allocation_bytes <= largest_buffer + ARENA_MIGRATION_FRAME_BYTES,
        "backlog buffer allocation must be spread across upload frames"
    );
    assert!(migrations <= 5);
    assert!(copied <= ARENA_MIGRATION_FRAME_BYTES);
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
    assert!(begin_arena_migration(
        &mut arena,
        &device,
        ArenaStream::Quads,
        ArenaGrowthPlan {
            new_capacity: capacity * 2,
            gpu_copy_bytes: capacity as u64 * PACKED_QUAD_BYTES,
        },
        crate::gpu_allocation::Requirement::Required,
    ));
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

/// Repeated allocation failures cannot hide smaller uploads that fit fragmented free space.
#[test]
fn fragmented_arena_admits_fitting_uploads_beyond_a_repeatedly_failing_prefix() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    let config = PublicationServiceConfig::DEFAULT;
    app.insert_resource(ChunkUploadBudget::new(1, config.maximum_frame_bytes));
    let template = instance();
    let quads = template.cube_quads.len();
    let capacity = quads * 3;
    let device = app.world().resource::<RenderDevice>().clone();
    {
        let mut arena = app.world_mut().resource_mut::<ChunkGpuArena>();
        arena.quad_buffer = create_storage_buffer(
            &device,
            "fragmented test quads",
            buffer_byte_len(capacity, PACKED_QUAD_BYTES),
        );
        arena.quad_capacity = capacity;
        arena.quad_len = capacity;
        arena.limits.max_quad_items = capacity;
        // Enough free space in total for a large upload, but neither hole can hold it.
        arena.free_quads = vec![0..quads as u32, (2 * quads) as u32..capacity as u32];
        let lighting_bytes = buffer_byte_len(2 * quads, PACKED_QUAD_LIGHTING_BYTES);
        arena.geometry_stream_buffer =
            create_storage_buffer(&device, "test lighting holes", lighting_bytes);
        arena.geometry_stream_capacity = (lighting_bytes / GEOMETRY_STREAM_WORD_BYTES) as usize;
        arena.origin_buffer =
            create_storage_buffer(&device, "test fitting origins", 2 * CHUNK_ORIGIN_BYTES);
        arena.origin_capacity = 2;
    }
    let mut blocked = template.clone();
    blocked.cube_quads = template.cube_quads.repeat(2).into();
    blocked.cube_lighting = template.cube_lighting.repeat(2).into();
    for index in 0..config.maximum_frame_items + 1 {
        blocked.key = SubChunkKey::new(0, index as i32, 0, 0);
        blocked.origin = chunk_origin(blocked.key);
        app.world_mut().spawn(blocked.clone());
    }
    for _ in 0..GPU_UPDATE_OVERDUE_FRAMES {
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        assert_eq!(
            app.world().resource::<ChunkGpuUploadStats>().chunk_updates,
            0
        );
    }
    let fitting = (0..2)
        .map(|index| {
            let mut fitting = template.clone();
            fitting.key = SubChunkKey::new(0, 10_000 + index, 0, 0);
            fitting.origin = chunk_origin(fitting.key);
            app.world_mut().spawn(fitting).id()
        })
        .collect::<Vec<_>>();
    for (index, &entity) in fitting.iter().enumerate() {
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        assert_eq!(
            app.world().resource::<ChunkGpuUploadStats>().chunk_updates,
            1
        );
        let arena = app.world().resource::<ChunkGpuArena>();
        assert_eq!(arena.allocations.len(), index + 1);
        assert!(arena.allocations.contains_key(&entity));
        assert_eq!(arena.quad_len, capacity);
    }
    assert!(
        app.world()
            .resource::<ChunkGpuArena>()
            .free_quads
            .is_empty()
    );
}

/// Starting growth after a nearly full payload keeps actual transfers within the frame ceiling.
#[test]
fn migration_started_after_pending_uploads_respects_the_actual_frame_byte_ceiling() {
    use crate::upload_staging::TerrainUploadStaging;

    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    let config = PublicationServiceConfig::DEFAULT;
    app.insert_resource(ChunkUploadBudget::new(2, config.maximum_frame_bytes));
    let mut first = instance();
    let mut next = first.clone();
    next.key = SubChunkKey::new(0, 1, 0, 0);
    next.origin = chunk_origin(next.key);
    let record_bytes = PACKED_QUAD_BYTES + PACKED_QUAD_LIGHTING_BYTES;
    let fixed_bytes = chunk_instance_upload_byte_len(&first)
        - buffer_byte_len(first.cube_quads.len(), record_bytes);
    let quads = ((config.maximum_frame_bytes - ARENA_MIGRATION_FRAME_BYTES - fixed_bytes)
        / record_bytes) as usize;
    first.cube_quads = vec![first.cube_quads[0]; quads].into();
    first.cube_lighting = vec![first.cube_lighting[0]; quads].into();
    let payload_bytes = chunk_instance_upload_byte_len(&first);
    let capacity_bytes = 2 * ARENA_MIGRATION_FRAME_BYTES;
    let capacity = (capacity_bytes / PACKED_QUAD_BYTES) as usize;
    let device = app.world().resource::<RenderDevice>().clone();
    app.insert_resource(TerrainUploadStaging::new(&device));
    {
        let mut arena = app.world_mut().resource_mut::<ChunkGpuArena>();
        arena.quad_buffer = create_storage_buffer(&device, "test quad prefix", capacity_bytes);
        arena.quad_capacity = capacity;
        arena.quad_len = capacity - quads;
        arena.geometry_stream_buffer =
            create_storage_buffer(&device, "test lighting space", capacity_bytes);
        arena.geometry_stream_capacity = (capacity_bytes / GEOMETRY_STREAM_WORD_BYTES) as usize;
        arena.origin_buffer =
            create_storage_buffer(&device, "test origins", 2 * CHUNK_ORIGIN_BYTES);
        arena.origin_capacity = 2;
    }
    let first_entity = app.world_mut().spawn(first).id();
    let next_entity = app.world_mut().spawn(next).id();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();

    let arena = app.world().resource::<ChunkGpuArena>();
    let migration = arena
        .migration
        .as_ref()
        .expect("the second upload needs growth");
    assert_eq!(migration.stream, ArenaStream::Quads);
    assert_eq!(migration.copied_bytes, ARENA_MIGRATION_FRAME_BYTES);
    assert!(arena.allocations.contains_key(&first_entity));
    assert!(!arena.allocations.contains_key(&next_entity));
    let stats = *app.world().resource::<ChunkGpuUploadStats>();
    assert_eq!(stats.chunk_updates, 1);
    assert_eq!(stats.migrations_started, 1);
    assert_eq!(stats.incremental_bytes, payload_bytes);
    assert_eq!(stats.gpu_copy_bytes, ARENA_MIGRATION_FRAME_BYTES);
    let staging = app.world().resource::<TerrainUploadStaging>().stats();
    assert!(staging.staged_bytes > 0);
    assert!(staging.fallback_bytes > 0);
    let transferred_bytes = staging.staged_bytes + staging.fallback_bytes + stats.gpu_copy_bytes;
    assert_eq!(transferred_bytes, stats.total_bytes);
    assert!(transferred_bytes <= config.maximum_frame_bytes);
    assert!(config.maximum_frame_bytes - transferred_bytes < record_bytes);
}

/// Supplies a validating device and queue without a hardware dependency.
fn noop_device() -> (RenderDevice, RenderQueue) {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    (RenderDevice::from(device), RenderQueue::new(queue))
}

/// Origin growth needs a matching vertex-offset buffer; without memory for it the arena
/// keeps its old buffers instead of drawing slots the offsets do not cover.
#[test]
fn origin_growth_without_memory_for_offsets_keeps_the_arena() {
    let (device, _) = noop_device();
    let mut arena = ChunkGpuArena::new(&device);
    let (origins, offsets) = (arena.origin_buffer.id(), arena.vertex_offset_buffer.id());
    let growth = ArenaGrowthPlan {
        new_capacity: 64,
        gpu_copy_bytes: 0,
    };
    {
        let _failure =
            crate::gpu_allocation::inject(|label, _| label == "terrain identity vertex offsets");
        assert!(!begin_arena_migration(
            &mut arena,
            &device,
            ArenaStream::Origins,
            growth,
            crate::gpu_allocation::Requirement::Required,
        ));
    }
    assert!(arena.migration.is_none());
    assert_eq!(arena.origin_buffer.id(), origins);
    assert_eq!(arena.vertex_offset_buffer.id(), offsets);
    assert!(begin_arena_migration(
        &mut arena,
        &device,
        ArenaStream::Origins,
        growth,
        crate::gpu_allocation::Requirement::Required,
    ));
    assert_ne!(arena.vertex_offset_buffer.id(), offsets);
}

/// A backlog-sized buffer the device cannot hold falls back to the minimum growth.
#[test]
fn oversized_backlog_growth_falls_back_to_the_minimum() {
    let offset_bytes = u64::from(crate::chunk::gpu_cull::model::OFFSET_CORNERS)
        * 2
        * std::mem::size_of::<u32>() as u64;
    for (stream, site, item_bytes) in [
        (ArenaStream::Quads, "packed chunk quads", PACKED_QUAD_BYTES),
        (
            ArenaStream::Origins,
            "terrain identity vertex offsets",
            offset_bytes,
        ),
    ] {
        let (device, queue) = noop_device();
        let failures = crate::gpu_allocation::FailureFlag::new(&device);
        let mut arena = ChunkGpuArena::new(&device);
        let minimum = ArenaGrowthPlan {
            new_capacity: 1024,
            gpu_copy_bytes: PACKED_QUAD_BYTES,
        };
        let backlog = ArenaGrowthPlan {
            new_capacity: 1 << 20,
            ..minimum
        };
        let limit = minimum.new_capacity as u64 * item_bytes * 4;
        let attempts = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = attempts.clone();
        let _failure = crate::gpu_allocation::inject(move |label, bytes| {
            let oversized = label == site && bytes > limit;
            if oversized {
                observed.set(observed.get() + 1);
            }
            oversized
        });
        assert!(begin_arena_migration_or_minimum(
            &mut arena, &device, stream, backlog, minimum,
        ));
        let migration = arena.migration.as_ref().expect("minimum growth started");
        assert_eq!(migration.new_capacity, minimum.new_capacity);
        assert!(
            !failures.failed(),
            "optional backlog growth invalidated terrain"
        );
        let uploads = TerrainUploads {
            device: &device,
            queue: &queue,
            staging: None,
        };
        advance_arena_migration(&mut arena, uploads, u64::MAX);
        // An unchanged memory budget still permits the required fallback, without another large try.
        let minimum = ArenaGrowthPlan {
            new_capacity: minimum.new_capacity * 2,
            ..minimum
        };
        assert!(begin_arena_migration_or_minimum(
            &mut arena, &device, stream, backlog, minimum,
        ));
        assert_eq!(attempts.get(), 1);
        advance_arena_migration(&mut arena, uploads, u64::MAX);
        let origin = allocate_origin(&mut arena).unwrap();
        release_origin(&mut arena, origin);
        let minimum = ArenaGrowthPlan {
            new_capacity: minimum.new_capacity * 2,
            ..minimum
        };
        assert!(begin_arena_migration_or_minimum(
            &mut arena, &device, stream, backlog, minimum,
        ));
        assert_eq!(attempts.get(), 2);
        assert!(!failures.failed());
    }
}

/// Indirect command growth without memory keeps the old buffer and writes nothing into it,
/// so the previous publication keeps drawing exactly what it witnessed.
#[test]
fn indirect_growth_without_memory_keeps_the_buffer() {
    let (device, queue) = noop_device();
    let mut arena = ChunkGpuArena::new(&device);
    let buffer = arena.indirect_buffer.id();
    let commands = vec![DrawIndexedIndirectArgs::default(); 4];
    {
        let _failure =
            crate::gpu_allocation::inject(|label, _| label == "packed chunk indirect commands");
        assert!(!reserve_indirect_commands(
            &mut arena,
            &device,
            commands.len()
        ));
        assert!(
            upload_indirect_commands_if_changed(&mut arena, &device, &queue, &commands).is_err()
        );
    }
    assert_eq!(arena.indirect_buffer.id(), buffer);
    assert!(arena.uploaded_indirect_bytes.is_empty());
    assert_eq!(
        upload_indirect_commands_if_changed(&mut arena, &device, &queue, &commands).unwrap(),
        commands.len() as u64 * INDEXED_INDIRECT_BYTES
    );
    assert_ne!(arena.indirect_buffer.id(), buffer);
}
