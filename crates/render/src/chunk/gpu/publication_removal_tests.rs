use super::*;
use bevy::ecs::system::RunSystemOnce;

use render_scene::{
    ChunkRenderInstance, ChunkUploadAcknowledgements, ChunkUploadToken, chunk::chunk_origin,
};

fn cube_instance(x: i32, one_quad: bool) -> ChunkRenderInstance {
    let mesh = solid_test_mesh();
    let count = if one_quad { 1 } else { mesh.quads().len() };
    let key = SubChunkKey::new(0, x, 0, 0);
    ChunkRenderInstance {
        cube_layout: CubeQuadLayout::default(),
        key,
        cube_quads: Arc::from(&mesh.quads()[..count]),
        cube_lighting: Arc::from(&mesh.cube_lighting()[..count]),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from([]),
        liquid_quads: Arc::from([]),
        liquid_lighting: Arc::from([]),
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::default(),
        generation: 1,
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
        origin: chunk_origin(key),
    }
}

fn publication_app() -> App {
    let mut app = noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(ChunkUploadBudget::new(8, u64::MAX));
    app
}

#[test]
fn stalled_removal_completion_pauses_fresh_chunks_but_preserves_replacements_and_retry() {
    let mut app = publication_app();
    let removed_first = app.world_mut().spawn(cube_instance(0, false)).id();
    let removed_second = app.world_mut().spawn(cube_instance(1, false)).id();
    let replacing = app.world_mut().spawn(cube_instance(2, true)).id();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();

    {
        let mut arena = app.world_mut().resource_mut::<ChunkGpuArena>();
        assert_eq!(arena.allocations.len(), 3);
        let first_bytes =
            RetiredArenaAllocation::full(removed_first, arena.allocations[&removed_first].clone())
                .owned_bytes();
        let second_bytes = RetiredArenaAllocation::full(
            removed_second,
            arena.allocations[&removed_second].clone(),
        )
        .owned_bytes();
        arena.retirement_budget =
            TransparentRetirementBudget::with_limits(8, first_bytes + second_bytes - 1);
    }
    app.world_mut()
        .entity_mut(removed_first)
        .remove::<ChunkRenderInstance>();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();

    app.world_mut()
        .entity_mut(removed_second)
        .remove::<ChunkRenderInstance>();
    app.world_mut()
        .get_mut::<ChunkRenderInstance>(replacing)
        .unwrap()
        .generation = 2;
    let mut fresh_instance = cube_instance(3, false);
    fresh_instance.token = Some(ChunkUploadToken {
        generation: fresh_instance.generation,
        dirty_since: Instant::now(),
    });
    let fresh_key = fresh_instance.key;
    let fresh = app.world_mut().spawn(fresh_instance).id();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    {
        let arena = app.world().resource::<ChunkGpuArena>();
        assert!(arena.pending_removals.contains(&removed_second));
        assert!(arena.allocations.contains_key(&removed_second));
        assert_eq!(arena.allocations[&replacing].generation, 2);
        assert!(!arena.allocations.contains_key(&fresh));
        assert_eq!(arena.retired_allocations.len(), 2);
    }
    assert!(
        app.world()
            .resource::<ChunkUploadAcknowledgements>()
            .drain()
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<GpuUpdateFairness>()
            .wait_ages
            .contains_key(&fresh)
    );

    let fence = app.world().resource::<TransparentRetirementFence>().clone();
    let epoch = fence.try_reserve().unwrap();
    for retirement in &mut app
        .world_mut()
        .resource_mut::<ChunkGpuArena>()
        .retired_allocations
    {
        retirement.release_epoch = Some(epoch);
    }
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    assert!(
        !app.world()
            .resource::<ChunkGpuArena>()
            .allocations
            .contains_key(&fresh),
        "arming a fence must not release memory before GPU completion"
    );

    assert!(fence.complete(epoch));
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    let arena = app.world().resource::<ChunkGpuArena>();
    assert!(!arena.pending_removals.contains(&removed_second));
    assert!(!arena.allocations.contains_key(&removed_second));
    assert!(arena.allocations.contains_key(&fresh));
    let acknowledgements = app
        .world()
        .resource::<ChunkUploadAcknowledgements>()
        .drain();
    assert_eq!(acknowledgements.len(), 1);
    assert_eq!(acknowledgements[0].key, fresh_key);
}

#[test]
fn removal_operation_allowance_does_not_masquerade_as_stalled_retirement() {
    let mut app = publication_app();
    let removed = app.world_mut().spawn(cube_instance(0, false)).id();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    app.world_mut()
        .entity_mut(removed)
        .remove::<ChunkRenderInstance>();
    app.insert_resource(ChunkUploadBudget::new(8, u64::MAX).with_zero_byte_operations_per_frame(0));
    let fresh = app.world_mut().spawn(cube_instance(1, false)).id();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    let arena = app.world().resource::<ChunkGpuArena>();
    assert!(arena.pending_removals.contains(&removed));
    assert!(arena.allocations.contains_key(&fresh));
    assert_eq!(arena.retirement_budget.bytes, 0);
}

/// Empty receipts can finish beside a resident whose GPU ranges cannot retire yet.
#[test]
fn absent_air_receipts_finish_while_a_live_removal_waits_for_retirement() {
    let mut app = publication_app();
    let resident = app.world_mut().spawn(cube_instance(0, false)).id();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    app.world_mut()
        .resource_mut::<ChunkGpuArena>()
        .retirement_budget = TransparentRetirementBudget::with_limits(0, 0);
    app.world_mut()
        .entity_mut(resident)
        .remove::<ChunkRenderInstance>();

    let keys = [0, 1, 2].map(|x| SubChunkKey::new(0, x, 0, 0));
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(1, keys.len(), 0, keys.len(), config.maximum_frame_items);
    let removals = app.world().resource::<ChunkGpuRemovalQueue>();
    for key in keys {
        removals
            .push(PendingGpuRemoval {
                publications: None,
                kind: render_api::TerrainRemovalKind::Empty,
                key,
                token: Some(ChunkUploadToken {
                    generation: 2,
                    dirty_since: Instant::now(),
                }),
                priority: ChunkUploadPriority::new(0.0),
                permit: allowance
                    .try_admit_zero_byte()
                    .unwrap()
                    .into_handoff()
                    .unwrap()
                    .into_render_entity()
                    .unwrap(),
            })
            .unwrap_or_else(|_| panic!("the removal mailbox has room"));
    }

    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();

    let mut completed = app
        .world()
        .resource::<ChunkUploadAcknowledgements>()
        .drain()
        .into_iter()
        .map(|ack| ack.key)
        .collect::<Vec<_>>();
    completed.sort_unstable();
    assert_eq!(completed, keys[1..]);
    assert_eq!(allowance.live_permits(), 1);
    assert_eq!(
        app.world().resource::<ChunkGpuRemovalQueue>().pending_len(),
        1
    );
    assert!(
        app.world()
            .resource::<ChunkGpuArena>()
            .allocations
            .contains_key(&resident)
    );

    app.world_mut()
        .resource_mut::<ChunkGpuArena>()
        .retirement_budget = TransparentRetirementBudget::with_limits(8, u64::MAX);
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();

    let completed = app
        .world()
        .resource::<ChunkUploadAcknowledgements>()
        .drain();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].key, keys[0]);
    assert_eq!(allowance.live_permits(), 0);
    assert_eq!(
        app.world().resource::<ChunkGpuRemovalQueue>().pending_len(),
        0
    );
}
