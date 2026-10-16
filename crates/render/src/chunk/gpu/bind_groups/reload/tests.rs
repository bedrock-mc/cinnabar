use super::*;
use crate::chunk::tests::solid_test_mesh;
use bevy::ecs::system::RunSystemOnce;
use render_scene::{ChunkRenderQueue, ChunkUploadAcknowledgements, ChunkUploadPriority};

/// Creates real main-world queue instances and uploads them through the ordinary GPU owner.
fn publication_app() -> (App, Vec<(Entity, ChunkRenderInstance)>) {
    let mut main = App::new();
    main.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(8));
    for x in 0..2 {
        main.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_insert(
                SubChunkKey::new(0, x, 0, 0),
                solid_test_mesh(),
                ChunkUploadPriority::new(0.0),
            )
            .unwrap();
    }
    main.update();
    let instances = {
        let world = main.world_mut();
        let mut query = world.query::<&ChunkRenderInstance>();
        let mut instances: Vec<_> = query.iter(world).cloned().collect();
        instances.sort_by_key(|instance| instance.key);
        instances
    };
    let mut app = crate::chunk::tests::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(ChunkUploadBudget::new(8, u64::MAX))
        .init_resource::<ChunkTextureReload>()
        .init_resource::<ChunkGpuTextureAssets>()
        .init_resource::<ChunkTextureUploadStats>();
    app.world_mut()
        .run_system_once(prepare_chunk_texture_assets)
        .unwrap();
    let instances = instances
        .into_iter()
        .map(|instance| (app.world_mut().spawn(instance.clone()).id(), instance))
        .collect();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    (app, instances)
}

/// Sends a synchronous build through the same completion mailbox used by the atlas worker.
fn complete_worker(
    app: &mut App,
    candidate: &ChunkTextureAssets,
    instances: Vec<ChunkRenderInstance>,
) {
    let device = app.world().resource::<RenderDevice>().clone();
    let queue = app.world().resource::<RenderQueue>().clone();
    app.world()
        .resource::<ChunkTextureReload>()
        .request_geometry(
            app.world()
                .resource::<ChunkTextureReload>()
                .geometry_epoch(),
            candidate.clone(),
            instances.clone().into(),
        );
    let geometry = PreparedResourceGeometry::build(
        &instances,
        candidate.clone(),
        device.clone(),
        queue.clone(),
        None,
    )
    .unwrap();
    let (prepared, stats) = build_chunk_texture_assets(candidate, &device, &queue).unwrap();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let request = app
        .world()
        .resource::<ChunkTextureReload>()
        .preparation()
        .unwrap();
    assert!(sender.send(Some((prepared, stats, Some(geometry)))).is_ok());
    let mut gpu = app.world_mut().resource_mut::<ChunkGpuTextureAssets>();
    gpu.pending = Some(PendingTextures {
        request,
        result: std::sync::Mutex::new(receiver),
    });
}

/// Returns the actual material-table and geometry identities used by terrain draw preparation.
fn published(app: &App) -> (ChunkTextureAssetIdentity, BufferId, BufferId) {
    let gpu = app.world().resource::<ChunkGpuTextureAssets>();
    let prepared = gpu.prepared.as_ref().unwrap();
    (
        prepared.identity,
        prepared.material_buffer.id(),
        app.world().resource::<ChunkGpuArena>().quad_buffer.id(),
    )
}

/// Arms the production retirement fence without acknowledging its GPU completion.
fn arm_retirements(app: &mut App) -> (TransparentRetirementFence, u64) {
    let fence = app.world().resource::<TransparentRetirementFence>().clone();
    let epoch = app
        .world_mut()
        .resource_scope(|world, mut arena: Mut<ChunkGpuArena>| {
            let runtime = world.resource::<TransparentSortRuntime>();
            arm_transparent_retirements(&mut arena, &runtime.state, &fence).unwrap()
        });
    (fence, epoch)
}

/// A despawned section can block a reload before or after CPU extraction selects its atlas.
#[test]
fn pending_physical_removals_defer_the_entire_resource_swap_and_retry_after_its_fence() {
    for selected_early in [false, true] {
        let (mut app, instances) = publication_app();
        let before = published(&app);
        let candidate = ChunkTextureAssets::with_revision(Arc::new(RuntimeAssets::diagnostic()), 7);
        let (live, mut replacement) = instances[0].clone();
        replacement.generation += 1;
        let removed = instances[1].0;
        app.world_mut()
            .entity_mut(removed)
            .remove::<ChunkRenderInstance>();
        app.insert_resource(
            ChunkUploadBudget::new(8, u64::MAX).with_zero_byte_operations_per_frame(0),
        );
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        assert!(
            app.world()
                .resource::<ChunkGpuArena>()
                .pending_removals
                .contains(&removed)
        );
        complete_worker(&mut app, &candidate, vec![replacement.clone()]);
        if selected_early {
            app.insert_resource(candidate.clone());
            app.world_mut().entity_mut(live).insert(replacement.clone());
        }
        let reload = app.world().resource::<ChunkTextureReload>().clone();
        for _ in 0..2 {
            app.world_mut()
                .run_system_once(prepare_chunk_texture_assets)
                .unwrap();
            app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
            assert_eq!(published(&app), before);
            assert!(reload.status(candidate.identity()).is_none());
            assert!(reload.geometry_pending());
            assert!(
                app.world()
                    .resource::<ChunkGpuTextureAssets>()
                    .staged
                    .is_some()
            );
            assert_eq!(
                app.world().resource::<ChunkGpuArena>().allocations[&live].generation,
                instances[0].1.generation
            );
        }
        app.insert_resource(
            ChunkUploadBudget::new(8, u64::MAX).with_zero_byte_operations_per_frame(1),
        );
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        assert!(
            !app.world()
                .resource::<ChunkGpuArena>()
                .pending_removals
                .contains(&removed)
        );
        assert_eq!(
            app.world()
                .resource::<ChunkGpuArena>()
                .retired_allocations
                .len(),
            1
        );
        let (fence, epoch) = arm_retirements(&mut app);
        app.world_mut()
            .run_system_once(prepare_chunk_texture_assets)
            .unwrap();
        assert_eq!(
            published(&app),
            before,
            "arming a fence does not complete a resource swap"
        );
        assert!(reload.status(candidate.identity()).is_none());
        assert!(fence.complete(epoch));
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        app.world_mut()
            .run_system_once(prepare_chunk_texture_assets)
            .unwrap();
        if !selected_early {
            assert_eq!(reload.status(candidate.identity()), Some(Ok(())));
            assert_eq!(
                published(&app),
                before,
                "GPU readiness precedes CPU atlas selection"
            );
            app.insert_resource(candidate.clone());
            app.world_mut().entity_mut(live).insert(replacement.clone());
            app.world_mut()
                .run_system_once(prepare_chunk_texture_assets)
                .unwrap();
        }
        let after = published(&app);
        assert_eq!(after.0, candidate.identity());
        assert_ne!(after.1, before.1);
        assert_ne!(after.2, before.2);
        assert_eq!(
            app.world()
                .get::<GpuChunkAllocation>(live)
                .unwrap()
                .generation,
            replacement.generation
        );
        let arena = app.world().resource::<ChunkGpuArena>();
        assert_eq!(arena.allocations.len(), 1);
        assert!(arena.retired_allocations.is_empty());
        assert_eq!(arena.retirement_budget.bytes, 0);
        assert!(!reload.geometry_pending());
        assert!(reload.requested().is_none());
        let stats = *app.world().resource::<ChunkTextureUploadStats>();
        app.world_mut()
            .run_system_once(prepare_chunk_texture_assets)
            .unwrap();
        assert_eq!(published(&app), after);
        assert_eq!(*app.world().resource::<ChunkTextureUploadStats>(), stats);
    }
}

/// A normal completed replacement retains its old draw until CPU selection, then swaps once.
#[test]
fn an_unblocked_worker_keeps_the_existing_two_world_publication_order() {
    let (mut app, instances) = publication_app();
    let before = published(&app);
    let candidate = ChunkTextureAssets::with_revision(Arc::new(RuntimeAssets::diagnostic()), 8);
    complete_worker(
        &mut app,
        &candidate,
        instances
            .iter()
            .map(|(_, instance)| instance.clone())
            .collect(),
    );
    app.world_mut()
        .run_system_once(prepare_chunk_texture_assets)
        .unwrap();
    assert_eq!(published(&app), before);
    let reload = app.world().resource::<ChunkTextureReload>().clone();
    assert_eq!(reload.status(candidate.identity()), Some(Ok(())));
    app.insert_resource(candidate.clone());
    app.world_mut()
        .run_system_once(prepare_chunk_texture_assets)
        .unwrap();
    assert_eq!(published(&app).0, candidate.identity());
    assert_ne!(published(&app).2, before.2);
    assert!(!reload.geometry_pending());
    assert!(
        app.world()
            .resource::<ChunkGpuTextureAssets>()
            .staged
            .is_none()
    );
}

/// A reset cancels an unselected snapshot; a selected atlas waits for old fences and resumes.
#[test]
fn session_reset_never_installs_retired_geometry_or_stalls_the_new_session() {
    for selected in [false, true] {
        let (mut app, instances) = publication_app();
        let before = published(&app);
        let candidate = ChunkTextureAssets::with_revision(Arc::new(RuntimeAssets::diagnostic()), 9);
        complete_worker(
            &mut app,
            &candidate,
            instances
                .iter()
                .map(|(_, instance)| instance.clone())
                .collect(),
        );
        app.world_mut()
            .run_system_once(prepare_chunk_texture_assets)
            .unwrap();
        let reload = app.world().resource::<ChunkTextureReload>().clone();
        if selected {
            app.insert_resource(candidate.clone());
        }
        reload.discard_geometry();
        for (entity, _) in &instances {
            app.world_mut().despawn(*entity);
        }
        app.world_mut()
            .run_system_once(prepare_chunk_texture_assets)
            .unwrap();
        assert_eq!(published(&app), before);
        if !selected {
            assert!(reload.status(candidate.identity()).is_none());
            assert!(
                app.world()
                    .resource::<ChunkGpuTextureAssets>()
                    .staged
                    .is_none()
            );
        }
        let mut next = instances[0].1.clone();
        next.key.x = 7;
        next.origin = render_scene::chunk::chunk_origin(next.key);
        let next = app.world_mut().spawn(next).id();
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        assert_eq!(
            app.world().get::<GpuChunkAllocation>(next).is_some(),
            !selected
        );
        let (fence, epoch) = arm_retirements(&mut app);
        assert!(fence.complete(epoch));
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        app.world_mut()
            .run_system_once(prepare_chunk_texture_assets)
            .unwrap();
        if selected {
            assert_eq!(published(&app).0, candidate.identity());
            assert!(reload.requested().is_none());
            app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
        }
        assert!(app.world().get::<GpuChunkAllocation>(next).is_some());
        assert!(!reload.geometry_pending());
    }
}
