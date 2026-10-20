//! A full quad arena grows across frames and publishes, and waits when growth runs out of
//! memory.

use super::*;

/// A GPU publication app whose 64 MiB quad arena is full, holding one chunk whose upload
/// needs the arena to grow; also returns its acknowledgements, allowance and the full item
/// count.
fn full_quad_arena_app() -> (
    App,
    ChunkUploadAcknowledgements,
    render_api::PublicationAllowance,
    usize,
) {
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let now = Instant::now();
    let key = SubChunkKey::new(0, 15, 0, 12);
    let mesh = solid_test_mesh();
    let biome = PackedBiomeRecord::fallback();
    let bytes = ChunkRenderQueue::upload_byte_len(&mesh, &biome);
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(
        1,
        1,
        config.maximum_frame_bytes,
        0,
        config.maximum_frame_items,
    );
    let permit = allowance.try_admit_payload(bytes).unwrap();
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let mut handoff_app = App::new();
    handoff_app
        .add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::default())
        .insert_resource(acknowledgements.clone())
        .add_plugins(ChunkRenderPlugin::with_budget(ChunkUploadBudget::new(0, 0)));
    handoff_app
        .world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked_with_biome_identity_permitted(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::default(),
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 1,
                dirty_since: now,
            },
            permit,
        )
        .unwrap();
    handoff_app.update();
    let instance = {
        let world = handoff_app.world_mut();
        let mut query = world.query::<&ChunkRenderInstance>();
        query.single(world).unwrap().clone()
    };
    drop(handoff_app);

    let mut gpu_app =
        noop_gpu_publication_app(acknowledgements.clone(), ChunkGpuRemovalQueue::default());
    let full_items = (config.maximum_frame_bytes / PACKED_QUAD_BYTES) as usize;
    let full_buffer = create_storage_buffer(
        gpu_app.world().resource::<RenderDevice>(),
        "full packed chunk quads",
        config.maximum_frame_bytes,
    );
    {
        let mut arena = gpu_app.world_mut().resource_mut::<ChunkGpuArena>();
        assert!(arena.limits.max_quad_items >= full_items * 2);
        arena.quad_buffer = full_buffer;
        arena.quad_capacity = full_items;
        arena.quad_len = full_items;
    }
    gpu_app.world_mut().spawn(instance);
    (gpu_app, acknowledgements, allowance, full_items)
}

/// A full 64 MiB arena must still grow under the 64 MiB frame ceiling and publish.
#[test]
fn full_64_mib_quad_arena_migrates_across_frames_then_publishes_a_positive_upload() {
    use bevy::ecs::system::RunSystemOnce;

    let config = render_api::PublicationServiceConfig::DEFAULT;
    let (mut gpu_app, acknowledgements, allowance, full_items) = full_quad_arena_app();
    let mut published_frame = None;
    for frame in 0..16 {
        gpu_app
            .world_mut()
            .run_system_once(prepare_gpu_chunks)
            .unwrap();
        let stats = *gpu_app.world().resource::<ChunkGpuUploadStats>();
        assert!(stats.total_bytes <= config.maximum_frame_bytes);
        if !acknowledgements.drain().is_empty() {
            published_frame = Some(frame);
            break;
        }
    }

    let frames = published_frame.expect("a legal arena growth must finish and publish");
    assert!(frames <= 4, "migration took {frames} frames");
    let arena = gpu_app.world().resource::<ChunkGpuArena>();
    // Growing a full stream adds a quarter.
    assert_eq!(arena.quad_capacity, full_items + full_items / 4);
    assert!(arena.migration.is_none());
    assert_eq!(arena.allocations.len(), 1);
    assert_eq!(allowance.live_permits(), 0);
}

/// Running out of memory for a growth buffer leaves the arena as it was and the upload
/// waiting, rather than migrating into an invalid buffer; growth succeeds once memory frees.
#[test]
fn arena_growth_that_runs_out_of_memory_waits_and_retries() {
    use bevy::ecs::system::RunSystemOnce;
    use std::{cell::Cell, rc::Rc};

    let (mut gpu_app, acknowledgements, _allowance, full_items) = full_quad_arena_app();
    let out_of_memory = Rc::new(Cell::new(true));
    let attempts = Rc::new(Cell::new(0));
    let _failures = crate::gpu_allocation::inject({
        let out_of_memory = Rc::clone(&out_of_memory);
        let attempts = Rc::clone(&attempts);
        move |label, _| {
            let quads = label == "packed chunk quads";
            attempts.set(attempts.get() + u32::from(quads));
            quads && out_of_memory.get()
        }
    });
    let full_buffer = gpu_app.world().resource::<ChunkGpuArena>().quad_buffer.id();
    for frame in 1..=3 {
        gpu_app
            .world_mut()
            .run_system_once(prepare_gpu_chunks)
            .unwrap();
        assert!(acknowledgements.drain().is_empty(), "nothing fits yet");
        let stats = *gpu_app.world().resource::<ChunkGpuUploadStats>();
        assert_eq!(stats.uploads_waiting_for_growth, 1);
        assert_eq!(attempts.get(), frame, "one growth attempt per frame");
        let arena = gpu_app.world().resource::<ChunkGpuArena>();
        assert!(arena.migration.is_none());
        assert_eq!(arena.quad_buffer.id(), full_buffer);
        assert_eq!(arena.quad_capacity, full_items);
    }
    out_of_memory.set(false);
    let published = (0..16).any(|_| {
        gpu_app
            .world_mut()
            .run_system_once(prepare_gpu_chunks)
            .unwrap();
        !acknowledgements.drain().is_empty()
    });
    assert!(published, "growth retries and publishes once memory frees");
    assert_eq!(
        gpu_app.world().resource::<ChunkGpuArena>().quad_capacity,
        full_items + full_items / 4
    );
}
