use super::super::resource_sorts::ResourceView;
use super::*;
use bevy::render::renderer::WgpuWrapper;

/// A single transparent face exercises address preparation without external carriers.
fn water(tint: ChunkBiomeTintIdentity) -> ChunkRenderInstance {
    ChunkRenderInstance {
        key: SubChunkKey::new(0, 0, 0, 0),
        origin: [0; 3],
        generation: 1,
        cube_quads: Arc::from([]),
        cube_lighting: Arc::from([]),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from([]),
        liquid_quads: Arc::from([PackedLiquidQuad::try_pack(
            [0; 3],
            Face::PositiveY,
            [255; 4],
            0,
            0,
            [0; 2],
            false,
        )
        .unwrap()]),
        liquid_lighting: Arc::from([PackedQuadLighting::new([0; 4])]),
        has_depth_liquid: false,
        has_transparent_liquid: true,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: tint,
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
    }
}

#[derive(Resource)]
struct Candidate(Option<PreparedResourceGeometry>);

/// Calls the production publication boundary, including its deferred component writes.
fn publish(
    mut commands: Commands,
    instances: Query<(Entity, &ChunkRenderInstance)>,
    mut arena: ResMut<ChunkGpuArena>,
    mut candidate: ResMut<Candidate>,
) {
    candidate
        .0
        .take()
        .unwrap()
        .publish(&mut commands, &instances, &mut arena);
}

#[test]
fn publication_keeps_complete_transparent_addresses_and_biome_identity() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let mut app = App::new();
    app.insert_resource(ChunkGpuArena::new(&device));
    let old_buffer = app
        .world()
        .resource::<ChunkGpuArena>()
        .geometry_stream_buffer
        .id();
    let view = app.world_mut().spawn_empty().id();
    let tint = ChunkBiomeTintIdentity::new(4, 7);
    let instance = water(tint);
    let entity = app.world_mut().spawn(instance.clone()).id();
    let assets = ChunkTextureAssets::default();
    let candidate = PreparedResourceGeometry::build(
        std::slice::from_ref(&instance),
        assets.clone(),
        device.clone(),
        queue.clone(),
        Some(ResourceView {
            entity: view,
            transform: GlobalTransform::IDENTITY,
        }),
    )
    .unwrap();
    assert_eq!(
        app.world()
            .resource::<ChunkGpuArena>()
            .geometry_stream_buffer
            .id(),
        old_buffer
    );
    assert_eq!(candidate.liquids.state.committed().unwrap().refs().len(), 1);
    app.insert_resource(Candidate(Some(candidate)));
    app.world_mut().run_system_once(publish).unwrap();
    let arena = app.world().resource::<ChunkGpuArena>();
    assert_ne!(arena.geometry_stream_buffer.id(), old_buffer);
    assert_eq!(
        app.world()
            .get::<GpuChunkAllocation>(entity)
            .unwrap()
            .tint_identity,
        tint
    );
    let liquids = app.world().resource::<TransparentSortRuntime>();
    assert_eq!(liquids.view_entity, Some(view));
    assert!(transparent_snapshot_addresses_are_resident(
        liquids.state.committed().unwrap(),
        arena.allocations.values().map(|allocation| &allocation.gpu),
        std::iter::empty(),
        assets.identity(),
        tint
    ));
    let current = arena.geometry_stream_buffer.id();
    let mut malformed = instance;
    malformed.liquid_lighting = Arc::from([]);
    assert!(PreparedResourceGeometry::build(&[malformed], assets, device, queue, None).is_none());
    assert_eq!(
        app.world()
            .resource::<ChunkGpuArena>()
            .geometry_stream_buffer
            .id(),
        current
    );
}

#[test]
fn review_render_stale_resource_geometry_preserves_active_arena() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let mut app = App::new();
    app.insert_resource(ChunkGpuArena::new(&device));
    let old_buffer = app
        .world()
        .resource::<ChunkGpuArena>()
        .geometry_stream_buffer
        .id();
    let view = app.world_mut().spawn_empty().id();
    let instance = water(ChunkBiomeTintIdentity::default());
    let entity = app.world_mut().spawn(instance.clone()).id();
    let mut candidate = PreparedResourceGeometry::build(
        &[instance.clone()],
        ChunkTextureAssets::default(),
        device,
        queue,
        None,
    )
    .unwrap();
    candidate.models.committed = Some(TransparentModelSortKey {
        view_entity: view,
        rotation_bits: [0; 4],
        address: TransparentModelAddressIdentity {
            asset_identity: ChunkTextureAssets::default().identity(),
            allocations: Arc::from([TransparentModelAllocationIdentity {
                entity,
                key: instance.key,
                generation: instance.generation,
                model_range: 0..4,
                draw_range: 0..2,
            }]),
        },
    });
    app.world_mut().despawn(entity);
    app.insert_resource(Candidate(Some(candidate)));
    app.world_mut().run_system_once(publish).unwrap();
    assert_eq!(
        app.world()
            .resource::<ChunkGpuArena>()
            .geometry_stream_buffer
            .id(),
        old_buffer
    );
}

#[test]
fn review_render_fairness_overflow_keeps_unchanged_uploads_discoverable() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let mut app = App::new();
    app.insert_resource(ChunkGpuArena::new(&device))
        .insert_resource(device)
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
        .insert_resource(ChunkTextureAssets::default())
        .insert_resource(ChunkUploadBudget::new(0, 0))
        .init_resource::<ChunkGpuUploadStats>()
        .init_resource::<ChunkBiomeTints>()
        .init_resource::<ChunkUploadAcknowledgements>()
        .init_resource::<ChunkGpuRemovalQueue>()
        .init_resource::<TransparentRetirementFence>()
        .insert_resource(GpuUpdateFairness::with_limit(2))
        .add_systems(Update, prepare_gpu_chunks);
    for x in 0..3 {
        let mut instance = water(ChunkBiomeTintIdentity::default());
        instance.key.x = x;
        app.world_mut().spawn(instance);
    }
    app.update();
    assert_eq!(
        app.world().resource::<GpuUpdateFairness>().wait_ages.len(),
        2
    );
    app.insert_resource(ChunkUploadBudget::new(3, u64::MAX));
    app.update();
    assert_eq!(app.world().resource::<ChunkGpuArena>().allocations.len(), 3);
}
