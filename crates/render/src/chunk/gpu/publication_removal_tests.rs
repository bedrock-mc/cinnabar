use super::*;
use bevy::ecs::system::RunSystemOnce;

fn cube_instance(x: i32, one_quad: bool) -> ChunkRenderInstance {
    let mesh = solid_test_mesh();
    let count = if one_quad { 1 } else { mesh.quads().len() };
    let key = SubChunkKey::new(0, x, 0, 0);
    ChunkRenderInstance {
        light_emitters: Arc::from([]),
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
        origin: queue::chunk_origin(key),
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

#[test]
fn invisible_emitters_survive_empty_geometry_publication_and_clear_on_replacement() {
    let assets = test_runtime_assets(true);
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets,
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &source,
    );
    assert!(mesh.is_empty());
    assert_eq!(mesh.light_emitters().len(), world::BLOCKS_PER_SUB_CHUNK);
    let expected = mesh.light_emitters().to_vec();
    let key = SubChunkKey::new(0, 2, 3, 4);
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(acknowledgements.clone())
        .add_plugins(ChunkRenderPlugin::with_budget(ChunkUploadBudget::new(
            8,
            u64::MAX,
        )));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked(
            key,
            mesh,
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 1,
                dirty_since: Instant::now(),
            },
        )
        .unwrap();
    app.update();
    let instance = {
        let world = app.world_mut();
        world
            .query::<&ChunkRenderInstance>()
            .single(world)
            .expect("invisible emitters must reach the render world")
            .clone()
    };
    assert_eq!(instance.light_emitters(), expected);
    let mut gpu_app = publication_app();
    gpu_app.insert_resource(acknowledgements.clone());
    gpu_app.world_mut().spawn(instance);
    gpu_app
        .world_mut()
        .run_system_once(prepare_gpu_chunks)
        .unwrap();
    assert_eq!(acknowledgements.drain().len(), 1);
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update(key, ChunkMesh::default(), ChunkUploadPriority::new(0.0))
        .unwrap();
    app.update();
    let world = app.world_mut();
    assert_eq!(world.query::<&ChunkRenderInstance>().iter(world).count(), 0);
}
