use super::super::resource_sorts::ResourceView;
use super::*;
use crate::chunk::transparent::model::camera_position_bits;
use bevy::render::renderer::WgpuWrapper;

/// A single transparent face exercises address preparation without external carriers.
fn water(tint: ChunkBiomeTintIdentity) -> ChunkRenderInstance {
    ChunkRenderInstance {
        light_emitters: Arc::from([]),
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
        std::slice::from_ref(&instance),
        ChunkTextureAssets::default(),
        device,
        queue,
        None,
    )
    .unwrap();
    candidate.models.committed = Some(TransparentModelSortKey {
        view_entity: view,
        camera_position_bits: camera_position_bits(Vec3::ZERO).unwrap(),
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

#[test]
fn review_render_retained_liquid_snapshot_resolves_updated_active_generation() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let assets = ChunkTextureAssets::default();
    let identity = assets.identity();
    let candidate = PreparedResourceGeometry::build(
        &[water(ChunkBiomeTintIdentity::default())],
        assets,
        RenderDevice::from(device),
        RenderQueue(Arc::new(WgpuWrapper::new(queue))),
        Some(ResourceView {
            entity: Entity::PLACEHOLDER,
            transform: GlobalTransform::IDENTITY,
        }),
    )
    .unwrap();
    let mut arena = candidate.arena;
    let snapshot = candidate.liquids.state.committed().unwrap();
    for allocation in arena.allocations.values_mut() {
        allocation.gpu.generation += 1;
    }
    assert!(transparent_snapshot_addresses_are_resident(
        snapshot,
        arena.allocations.values().map(|allocation| &allocation.gpu),
        std::iter::empty(),
        identity,
        ChunkBiomeTintIdentity::default()
    ));
    assert_eq!(transparent_frame_draws(snapshot, &arena).len(), 1);
    assert!(transparent_frame_draw_for_range(snapshot, &arena, 0..1).is_some());
}

/// Builds a resident model sort with a writable stream and one matching view.
fn model_sort_app() -> (App, Entity, TransparentModelSortKey) {
    use bevy::{
        core_pipeline::core_3d::graph::Core3d,
        render::{render_graph::RenderSubGraph, sync_world::MainEntity, view::RetainedViewEntity},
    };
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let mut app = App::new();
    let mut arena = ChunkGpuArena::new(&device);
    arena.geometry_stream_buffer = create_storage_buffer(&device, "test model stream", 64);
    app.insert_resource(arena)
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
        .init_resource::<ChunkTextureAssets>()
        .init_resource::<TransparentSortRuntime>()
        .init_resource::<TransparentUploadBudget>()
        .init_resource::<TransparentModelSortRuntime>();
    let view = app.world_mut().spawn_empty().id();
    let mut instance = water(ChunkBiomeTintIdentity::default());
    instance.model_refs = Arc::from([PackedModelRef::new(0, 0, 0, 1)]);
    instance.model_lighting = Arc::from([PackedQuadLighting::new([0; 4])]);
    instance.transparent_model_draw_refs = Arc::from([PackedModelDrawRef::new(0, 0)]);
    let entity = app.world_mut().spawn(instance.clone()).id();
    let allocation = GpuChunkAllocation {
        key: instance.key,
        generation: instance.generation,
        tint_identity: instance.tint_identity,
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: Some(0..4),
        model_lighting_range: Some(6..8),
        model_draw_range: None,
        transparent_model_draw_range: Some(4..6),
        liquid_range: None,
        liquid_lighting_range: None,
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_range: None,
        metadata_index: 0,
    };
    app.world_mut().entity_mut(entity).insert(allocation);
    let mut visible = RenderVisibleEntities::default();
    visible.entities.insert(
        std::any::TypeId::of::<ChunkRenderInstance>(),
        vec![(entity, MainEntity::from(entity))],
    );
    app.world_mut().entity_mut(view).insert((
        ExtractedView {
            retained_view_entity: RetainedViewEntity::new(view.into(), None, 0),
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            hdr: false,
            viewport: UVec4::new(0, 0, 1, 1),
            color_grading: default(),
            invert_culling: false,
        },
        ExtractedCamera {
            target: None,
            physical_viewport_size: None,
            physical_target_size: None,
            viewport: None,
            render_graph: Core3d.intern(),
            order: 0,
            output_mode: default(),
            msaa_writeback: default(),
            clear_color: default(),
            sorted_camera_index_for_target: 0,
            exposure: 1.0,
            hdr: false,
        },
        visible,
    ));
    app.world_mut()
        .resource_mut::<TransparentSortRuntime>()
        .view_entity = Some(view);
    let key = TransparentModelSortKey {
        view_entity: view,
        camera_position_bits: camera_position_bits(Vec3::ZERO).unwrap(),
        address: TransparentModelAddressIdentity {
            asset_identity: app.world().resource::<ChunkTextureAssets>().identity(),
            allocations: Arc::from([TransparentModelAllocationIdentity {
                entity,
                key: instance.key,
                generation: instance.generation,
                model_range: 0..4,
                draw_range: 4..6,
            }]),
        },
    };
    (app, view, key)
}

#[test]
fn review_render_model_result_survives_camera_rotation() {
    let (mut app, view, key) = model_sort_app();
    let generation = ViewSortGeneration(1);
    {
        let mut runtime = app
            .world_mut()
            .resource_mut::<TransparentModelSortRuntime>();
        runtime.requested = Some((generation, key.clone()));
        runtime
            .result_sender
            .send(TransparentModelWorkerResult {
                generation,
                key: key.clone(),
                batches: vec![TransparentModelSortBatch {
                    draw_range: 4..6,
                    words: Box::new([[0, 0]]),
                }],
            })
            .unwrap();
    }
    app.world_mut()
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view =
        GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(0.5)));
    app.world_mut()
        .run_system_once(prepare_transparent_model_sorts)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<TransparentModelSortRuntime>()
            .committed
            .as_ref(),
        Some(&key)
    );
}

/// A rotation-only camera change must not re-sort or re-upload committed model order.
#[test]
fn committed_model_sort_is_reused_for_rotation_only_camera_change() {
    let (mut app, view, key) = model_sort_app();
    app.world_mut()
        .resource_mut::<TransparentModelSortRuntime>()
        .committed = Some(key.clone());
    for yaw in [0.5, 1.5, -2.0] {
        app.world_mut()
            .get_mut::<ExtractedView>(view)
            .unwrap()
            .world_from_view = GlobalTransform::from(Transform::from_rotation(
            Quat::from_rotation_y(yaw) * Quat::from_rotation_x(0.3),
        ));
        app.world_mut()
            .run_system_once(prepare_transparent_model_sorts)
            .unwrap();
        let runtime = app.world().resource::<TransparentModelSortRuntime>();
        assert_eq!(runtime.committed.as_ref(), Some(&key));
        assert!(runtime.requested.is_none());
        assert_eq!(runtime.next_generation, 0);
    }
}

#[test]
fn review_render_model_staged_upload_survives_camera_rotation() {
    let (mut app, view, key) = model_sort_app();
    app.world_mut()
        .resource_mut::<TransparentModelSortRuntime>()
        .staged = Some(TransparentModelStagedSort {
        key: key.clone(),
        batches: VecDeque::from([TransparentModelSortBatch {
            draw_range: 4..6,
            words: Box::new([[0, 0]]),
        }]),
    });
    app.world_mut()
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view =
        GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(0.5)));
    app.world_mut()
        .run_system_once(prepare_transparent_model_sorts)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<TransparentModelSortRuntime>()
            .committed
            .as_ref(),
        Some(&key)
    );
}

#[test]
fn review_render_model_view_loss_clears_abandoned_request() {
    let (mut app, view, key) = model_sort_app();
    app.world_mut()
        .resource_mut::<TransparentModelSortRuntime>()
        .requested = Some((ViewSortGeneration(1), key));
    app.world_mut().entity_mut(view).remove::<ExtractedView>();
    app.world_mut()
        .run_system_once(prepare_transparent_model_sorts)
        .unwrap();
    assert!(
        app.world()
            .resource::<TransparentModelSortRuntime>()
            .requested
            .is_none()
    );
}
