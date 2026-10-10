use super::super::resource_sorts::ResourceView;
use super::*;
use crate::chunk::transparent::liquid::{
    transparent_frame_draw_for_range, transparent_frame_draws,
};
use bevy::render::renderer::WgpuWrapper;

/// A single transparent face exercises address preparation without external carriers.
fn water(tint: ChunkBiomeTintIdentity) -> ChunkRenderInstance {
    ChunkRenderInstance {
        light_emitters: Arc::from([]),
        cube_layout: CubeQuadLayout::default(),
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

/// Water with a top and a side face, which can overlap on screen and so is sorted.
fn shore(tint: ChunkBiomeTintIdentity) -> ChunkRenderInstance {
    let mut instance = water(tint);
    let side = PackedLiquidQuad::try_pack(
        [0; 3],
        Face::NegativeX,
        [0, 255, 255, 0],
        0,
        1,
        [0; 2],
        false,
    )
    .unwrap();
    instance.liquid_quads = Arc::from([instance.liquid_quads[0], side]);
    instance.liquid_lighting = Arc::from([PackedQuadLighting::new([0; 4]); 2]);
    instance
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
    let instance = shore(tint);
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
            sort_order_independent: false,
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
    assert_eq!(candidate.liquids.state.committed().unwrap().refs().len(), 2);
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
        liquids.state.committed().unwrap().key(),
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

/// The first live manifest after a reload reuses the reload's sort inputs instead of
/// building every order-dependent resident's input in one render frame.
#[test]
fn reload_seeds_the_live_sort_inputs() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let mut app = App::new();
    app.insert_resource(ChunkGpuArena::new(&device));
    let tint = ChunkBiomeTintIdentity::new(4, 7);
    let instances = (0..3)
        .map(|x| {
            let mut instance = shore(tint);
            instance.key = SubChunkKey::new(0, x, 0, 0);
            instance.origin = chunk_origin(instance.key);
            instance
        })
        .collect::<Vec<_>>();
    for instance in &instances {
        app.world_mut().spawn(instance.clone());
    }
    let view = app.world_mut().spawn_empty().id();
    let candidate = PreparedResourceGeometry::build(
        &instances,
        ChunkTextureAssets::default(),
        device,
        queue,
        Some(ResourceView {
            entity: view,
            transform: GlobalTransform::IDENTITY,
            sort_order_independent: false,
        }),
    )
    .unwrap();
    app.insert_resource(Candidate(Some(candidate)));
    app.world_mut().run_system_once(publish).unwrap();
    app.world_mut()
        .resource_scope(|world, mut liquids: Mut<TransparentSortRuntime>| {
            let committed =
                Arc::clone(&liquids.state.committed().unwrap().key().sorted_allocations);
            let manifest = liquids.resident_manifest(
                world.resource::<ChunkGpuArena>(),
                false,
                tint,
                [0; 3],
                |_| panic!("the reload's sort inputs were rebuilt on the render thread"),
                &TransparentSortMetrics::default(),
            );
            assert_eq!(manifest.len(), 3);
            assert_eq!(manifest, committed);
        });
}

/// A view that displaces water sorts flat water after a reload as it does live.
#[test]
fn reload_sorts_flat_water_for_views_that_displace_it() {
    for displaced in [false, true] {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let candidate = PreparedResourceGeometry::build(
            &[water(ChunkBiomeTintIdentity::default())],
            ChunkTextureAssets::default(),
            RenderDevice::from(device),
            RenderQueue(Arc::new(WgpuWrapper::new(queue))),
            Some(ResourceView {
                entity: Entity::PLACEHOLDER,
                transform: GlobalTransform::IDENTITY,
                sort_order_independent: displaced,
            }),
        )
        .unwrap();
        let liquids = &candidate.liquids;
        assert_eq!(
            liquids.state.committed().unwrap().refs().len(),
            usize::from(displaced)
        );
        assert_eq!(liquids.direct_order_independent, !displaced);
    }
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
        order_camera: TransparentFaceMetric::new(Vec3::ZERO).order_camera([instance.key]),
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
        &[shore(ChunkBiomeTintIdentity::default())],
        assets,
        RenderDevice::from(device),
        RenderQueue(Arc::new(WgpuWrapper::new(queue))),
        Some(ResourceView {
            entity: Entity::PLACEHOLDER,
            transform: GlobalTransform::IDENTITY,
            sort_order_independent: false,
        }),
    )
    .unwrap();
    let mut arena = candidate.arena;
    let snapshot = candidate.liquids.state.committed().unwrap();
    for allocation in arena.allocations.values_mut() {
        allocation.gpu.generation += 1;
    }
    assert!(transparent_snapshot_addresses_are_resident(
        snapshot.key(),
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
    arena.geometry_stream_buffer = create_storage_buffer(&device, "test model stream", 1024);
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
        cube_layout: CubeQuadLayout::default(),
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
        order_independent_liquid: false,
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
        order_camera: TransparentFaceMetric::new(Vec3::ZERO).order_camera([instance.key]),
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
                    class: key.order_camera.class(key.address.allocations[0].key),
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
            class: key.order_camera.class(key.address.allocations[0].key),
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

/// A far model order already uploaded for its class commits without a sort or upload.
#[test]
fn far_model_order_in_its_class_commits_without_a_sort_job() {
    let (mut app, view, key) = model_sort_app();
    let identity = key.address.allocations[0].clone();
    let gpu = app
        .world()
        .get::<GpuChunkAllocation>(identity.entity)
        .unwrap()
        .clone();
    app.world_mut()
        .resource_mut::<ChunkGpuArena>()
        .allocations
        .insert(
            identity.entity,
            ArenaAllocation {
                generation: gpu.generation,
                tint_identity: gpu.tint_identity,
                cube_range: None,
                cube_lighting_range: None,
                model_range: gpu.model_range.clone(),
                model_lighting_range: gpu.model_lighting_range.clone(),
                model_draw_range: None,
                transparent_model_draw_range: gpu.transparent_model_draw_range.clone(),
                liquid_range: None,
                liquid_lighting_range: None,
                quad_capacity: 0,
                geometry_stream_range: Some(0..8),
                geometry_stream_capacity: 8,
                biome_range: 0..0,
                biome_capacity: 0,
                gpu,
            },
        );
    let camera = Vec3::new(200.0, 0.0, 0.0);
    app.world_mut()
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view = GlobalTransform::from_translation(camera);
    let class = TransparentFaceMetric::new(camera).class(identity.key);
    app.world_mut()
        .resource_mut::<TransparentModelSortRuntime>()
        .draw_orders
        .publish(
            &key.address,
            TransparentModelSortBatch {
                draw_range: identity.draw_range.clone(),
                class,
                words: Box::new([[0, 0]]),
            },
        );
    for step in 0..3 {
        app.world_mut()
            .get_mut::<ExtractedView>(view)
            .unwrap()
            .world_from_view =
            GlobalTransform::from_translation(camera + Vec3::splat(step as f32 * 0.4));
        app.world_mut()
            .run_system_once(prepare_transparent_model_sorts)
            .unwrap();
        let runtime = app.world().resource::<TransparentModelSortRuntime>();
        assert!(runtime.committed.is_some());
        assert!(runtime.requested.is_none());
        assert_eq!(runtime.next_generation, 0, "no sort job was submitted");
    }
}

/// Resident transparent-model groups with `refs` draw refs each, all visible to one view.
fn model_groups_app(
    groups: &[(SubChunkKey, u32)],
) -> (App, Entity, Vec<TransparentModelAllocationIdentity>) {
    use bevy::render::sync_world::MainEntity;
    let (mut app, view, _) = model_sort_app();
    let mut identities = Vec::new();
    let mut visible = Vec::new();
    for (index, &(key, refs)) in groups.iter().enumerate() {
        let base = index as u32 * 32;
        let mut instance = water(ChunkBiomeTintIdentity::default());
        instance.key = key;
        instance.origin = chunk_origin(key);
        instance.liquid_quads = Arc::from([]);
        instance.liquid_lighting = Arc::from([]);
        instance.has_transparent_liquid = false;
        instance.model_refs = Arc::from([PackedModelRef::new(0, 0, 0, 1)]);
        instance.model_lighting = Arc::from([PackedQuadLighting::new([0; 4])]);
        instance.transparent_model_draw_refs = (0..refs)
            .map(|quad| PackedModelDrawRef::new(0, quad))
            .collect();
        let identity_range = base + 4..base + 4 + 2 * refs;
        let gpu = GpuChunkAllocation {
            cube_layout: CubeQuadLayout::default(),
            key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: Some(base..base + 4),
            model_lighting_range: Some(base + 24..base + 26),
            model_draw_range: None,
            transparent_model_draw_range: Some(identity_range.clone()),
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            order_independent_liquid: false,
            metadata_index: index as u32,
        };
        let entity = app.world_mut().spawn((instance, gpu.clone())).id();
        app.world_mut()
            .resource_mut::<ChunkGpuArena>()
            .allocations
            .insert(
                entity,
                ArenaAllocation {
                    generation: gpu.generation,
                    tint_identity: gpu.tint_identity,
                    cube_range: None,
                    cube_lighting_range: None,
                    model_range: gpu.model_range.clone(),
                    model_lighting_range: gpu.model_lighting_range.clone(),
                    model_draw_range: None,
                    transparent_model_draw_range: Some(identity_range.clone()),
                    liquid_range: None,
                    liquid_lighting_range: None,
                    quad_capacity: 0,
                    geometry_stream_range: Some(base..base + 32),
                    geometry_stream_capacity: 32,
                    biome_range: 0..0,
                    biome_capacity: 0,
                    gpu,
                },
            );
        visible.push((entity, MainEntity::from(entity)));
        identities.push(TransparentModelAllocationIdentity {
            entity,
            key,
            generation: 1,
            model_range: base..base + 4,
            draw_range: identity_range,
        });
    }
    app.world_mut()
        .get_mut::<RenderVisibleEntities>(view)
        .unwrap()
        .entities
        .insert(std::any::TypeId::of::<ChunkRenderInstance>(), visible);
    identities.sort_by_key(|identity| (identity.key, identity.draw_range.start));
    (app, view, identities)
}

/// A queued model job that omitted a group must not commit after an older result re-sorts it.
#[test]
fn queued_model_job_revalidates_groups_an_in_flight_result_overwrites() {
    let moved = SubChunkKey::new(0, 3, 0, 0);
    let stale = SubChunkKey::new(0, -3, 0, 0);
    let (mut app, view, identities) = model_groups_app(&[(moved, 2), (stale, 1)]);
    let (camera_a, camera_b) = (Vec3::new(8.0, 8.0, 8.0), Vec3::new(90.0, 8.0, 8.0));
    let address = TransparentModelAddressIdentity {
        asset_identity: app.world().resource::<ChunkTextureAssets>().identity(),
        allocations: identities.clone().into(),
    };
    let key = |camera: Vec3| TransparentModelSortKey {
        view_entity: view,
        order_camera: TransparentFaceMetric::new(camera).order_camera([moved, stale]),
        address: address.clone(),
    };
    let class = |camera: Vec3| TransparentFaceMetric::new(camera).class(moved);
    assert_ne!(class(camera_a), class(camera_b));
    let moved_identity = identities
        .iter()
        .find(|identity| identity.key == moved)
        .unwrap();
    let candidates = identities
        .iter()
        .flat_map(|identity| {
            let refs = (identity.draw_range.end - identity.draw_range.start) / 2;
            (0..refs).map(move |quad| TransparentModelSortCandidate {
                entity: identity.entity,
                key: identity.key,
                draw_range: identity.draw_range.clone(),
                stable_index: quad,
                centroid: Vec3::from_array(chunk_origin(identity.key).map(|value| value as f32))
                    + Vec3::new(quad as f32 * 4.0 + 1.0, 1.0, 1.0),
                words: [identity.model_range.start / 4, quad],
            })
        })
        .collect::<Arc<[_]>>();
    app.world_mut()
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view = GlobalTransform::from_translation(camera_b);
    {
        let mut runtime = app
            .world_mut()
            .resource_mut::<TransparentModelSortRuntime>();
        // B's order for the moved group is already uploaded, so B omitted it when queued.
        runtime.draw_orders.publish(
            &address,
            TransparentModelSortBatch {
                draw_range: moved_identity.draw_range.clone(),
                class: class(camera_b),
                words: Box::new([[0, 1], [0, 0]]),
            },
        );
        runtime.candidate_cache = Some(TransparentModelCandidateCache {
            address: address.clone(),
            candidates: Arc::clone(&candidates),
        });
        let (a, b) = (ViewSortGeneration(1), ViewSortGeneration(2));
        runtime.next_generation = 2;
        runtime.gate.in_flight = Some(a);
        runtime.gate.pending = Some((
            b,
            TransparentModelSortWork {
                generation: b,
                key: key(camera_b),
                camera_position: camera_b,
                candidates,
            },
        ));
        runtime.requested = Some((b, key(camera_b)));
        // A, sorted for the old camera, lands first and rewrites the moved group.
        runtime
            .result_sender
            .send(TransparentModelWorkerResult {
                generation: a,
                key: key(camera_a),
                batches: vec![TransparentModelSortBatch {
                    draw_range: moved_identity.draw_range.clone(),
                    class: class(camera_a),
                    words: Box::new([[0, 0], [0, 1]]),
                }],
            })
            .unwrap();
    }
    for _ in 0..4 {
        app.world_mut()
            .run_system_once(prepare_transparent_model_sorts)
            .unwrap();
        let runtime = app.world().resource::<TransparentModelSortRuntime>();
        if runtime.committed.as_ref() == Some(&key(camera_b)) {
            break;
        }
        // Hand the worker's result to the next frame, as a real frame interval would.
        if runtime.gate.in_flight_generation().is_some() {
            let result = runtime.result_receiver.lock().unwrap().recv().unwrap();
            runtime.result_sender.send(result).unwrap();
        }
    }
    let runtime = app.world().resource::<TransparentModelSortRuntime>();
    assert_eq!(runtime.committed.as_ref(), Some(&key(camera_b)));
    for identity in &identities {
        assert_eq!(
            runtime
                .draw_orders
                .get(identity)
                .and_then(|order| order.class),
            Some(TransparentFaceMetric::new(camera_b).class(identity.key)),
            "{:?} is sorted for the current camera",
            identity.key
        );
    }
}

/// Runs model-sort frames, handing each worker result to the next frame, until `key` commits.
fn run_model_sorts_until_committed(app: &mut App, key: &TransparentModelSortKey) -> bool {
    for _ in 0..6 {
        app.world_mut()
            .run_system_once(prepare_transparent_model_sorts)
            .unwrap();
        let runtime = app.world().resource::<TransparentModelSortRuntime>();
        if runtime.committed.as_ref() == Some(key) {
            return true;
        }
        if runtime.gate.in_flight_generation().is_some() {
            let result = runtime.result_receiver.lock().unwrap().recv().unwrap();
            runtime.result_sender.send(result).unwrap();
        }
    }
    false
}

/// The witness ceiling limits CPU order copies, never which allocations re-sort.
#[test]
fn unwitnessed_model_order_resorts_when_its_class_changes() {
    let group = SubChunkKey::new(0, 3, 0, 0);
    let (mut app, view, identities) = model_groups_app(&[(group, 2)]);
    let identity = identities[0].clone();
    // A resident witness holding every allowed ref leaves no room for the group's copy.
    let filler = TransparentModelAllocationIdentity {
        entity: app.world_mut().spawn_empty().id(),
        key: SubChunkKey::new(0, 40, 0, 0),
        generation: 1,
        model_range: 0..4,
        draw_range: 0..2 * MAX_TRANSPARENT_DRAW_REFS as u32,
    };
    let mut filler_allocation =
        app.world().resource::<ChunkGpuArena>().allocations[&identity.entity].clone();
    filler_allocation.gpu.key = filler.key;
    filler_allocation.gpu.model_range = Some(filler.model_range.clone());
    filler_allocation.gpu.transparent_model_draw_range = Some(filler.draw_range.clone());
    app.world_mut()
        .resource_mut::<ChunkGpuArena>()
        .allocations
        .insert(filler.entity, filler_allocation);
    let asset_identity = app.world().resource::<ChunkTextureAssets>().identity();
    let address = TransparentModelAddressIdentity {
        asset_identity,
        allocations: Arc::from([identity.clone()]),
    };
    {
        let mut runtime = app
            .world_mut()
            .resource_mut::<TransparentModelSortRuntime>();
        runtime.draw_orders.publish(
            &TransparentModelAddressIdentity {
                asset_identity,
                allocations: Arc::from([filler.clone()]),
            },
            TransparentModelSortBatch {
                draw_range: filler.draw_range.clone(),
                class: FaceOrderClass::Far([1, 0, 0]),
                words: vec![[0, 0]; MAX_TRANSPARENT_DRAW_REFS].into_boxed_slice(),
            },
        );
        runtime.candidate_cache = Some(TransparentModelCandidateCache {
            address: address.clone(),
            candidates: (0..2)
                .map(|quad| TransparentModelSortCandidate {
                    entity: identity.entity,
                    key: group,
                    draw_range: identity.draw_range.clone(),
                    stable_index: quad,
                    centroid: Vec3::new(48.0 + quad as f32 * 4.0 + 1.0, 1.0, 1.0),
                    words: [0, quad],
                })
                .collect(),
        });
    }
    for camera in [Vec3::new(8.0, 8.0, 8.0), Vec3::new(90.0, 8.0, 8.0)] {
        app.world_mut()
            .get_mut::<ExtractedView>(view)
            .unwrap()
            .world_from_view = GlobalTransform::from_translation(camera);
        let metric = TransparentFaceMetric::new(camera);
        let key = TransparentModelSortKey {
            view_entity: view,
            order_camera: metric.order_camera([group]),
            address: address.clone(),
        };
        assert!(run_model_sorts_until_committed(&mut app, &key));
        let orders = &app
            .world()
            .resource::<TransparentModelSortRuntime>()
            .draw_orders;
        assert!(orders.get(&identity).is_none(), "the group has no CPU copy");
        assert_eq!(orders.class(&identity), Some(metric.class(group)));
    }
}
