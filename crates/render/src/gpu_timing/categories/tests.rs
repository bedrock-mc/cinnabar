use super::*;
use bevy::{
    asset::AssetId,
    core_pipeline::core_3d::{Opaque3dBatchSetKey, Opaque3dBinKey},
    prelude::{Mesh, default},
    render::{
        batching::gpu_preprocessing::GpuPreprocessingMode,
        render_phase::{
            BinnedRenderPhaseType, Draw, DrawFunctionId, InputUniformIndex, ViewBinnedRenderPhases,
        },
        render_resource::CachedRenderPipelineId,
        sync_world::MainEntity,
        view::RetainedViewEntity,
    },
};
use std::sync::Mutex;

/// Creates an empty phase through Bevy's public per-view constructor.
fn phase<P: BinnedPhaseItem>() -> BinnedRenderPhase<P> {
    let view = RetainedViewEntity::new(MainEntity::from(Entity::PLACEHOLDER), None, 0);
    let mut phases = ViewBinnedRenderPhases::<P>::default();
    phases.prepare_for_new_frame(view, GpuPreprocessingMode::None);
    phases.remove(&view).unwrap()
}

#[test]
fn splitting_preserves_nonadjacent_category_order_and_empty_clear() {
    use RuntimeStage::{
        GpuOpaqueOther as Other, GpuTerrainModel as Model, GpuTerrainSolid as Solid,
    };
    let original = [Model, Model, Solid, Other, Model];
    let ranges = category_ranges(original);
    assert_eq!(
        ranges,
        [(Model, 0..2), (Solid, 2..3), (Other, 3..4), (Model, 4..5)]
    );
    assert_eq!(
        ranges
            .iter()
            .flat_map(|(_, range)| range.clone())
            .collect::<Vec<_>>(),
        (0..original.len()).collect::<Vec<_>>()
    );
    assert_eq!(category_ranges([]), [(Other, 0..0)]);
}

#[derive(Resource, Default)]
struct DrawLog(Mutex<Vec<(usize, Entity)>>);

struct RecordDraw(usize);

impl Draw<Opaque3d> for RecordDraw {
    fn draw<'w>(
        &mut self,
        world: &'w World,
        _: &mut TrackedRenderPass<'w>,
        _: Entity,
        item: &Opaque3d,
    ) -> Result<(), DrawError> {
        assert_eq!(item.batch_range, 0..1);
        assert!(matches!(item.extra_index, PhaseItemExtraIndex::None));
        world
            .resource::<DrawLog>()
            .0
            .lock()
            .unwrap()
            .push((self.0, item.entity()));
        Ok(())
    }
}

/// Adds a non-mesh item without normalizing either its category or entity order.
fn add(
    phase: &mut BinnedRenderPhase<Opaque3d>,
    draw: DrawFunctionId,
    entity: Entity,
    kind: BinnedRenderPhaseType,
) {
    phase.add(
        Opaque3dBatchSetKey {
            draw_function: draw,
            pipeline: CachedRenderPipelineId::INVALID,
            material_bind_group_index: None,
            lightmap_slab: None,
            slabs: default(),
        },
        Opaque3dBinKey {
            asset_id: AssetId::<Mesh>::default().untyped(),
        },
        (entity, MainEntity::from(entity)),
        InputUniformIndex::default(),
        kind,
    );
}

#[test]
fn non_mesh_execution_keeps_every_existing_bin_and_entity_order() {
    let (device, _queue) = super::super::tests::noop_device(wgpu::Features::empty());
    let mut world = World::new();
    world.init_resource::<DrawLog>();
    let functions = DrawFunctions::<Opaque3d>::default();
    let ids: Vec<_> = (0..3)
        .map(|index| functions.write().add(RecordDraw(index)))
        .collect();
    let mut phase = phase::<Opaque3d>();
    for index in [0, 1, 2, 0, 1, 0] {
        add(
            &mut phase,
            ids[index],
            world.spawn_empty().id(),
            BinnedRenderPhaseType::NonMesh,
        );
    }
    let expected: Vec<_> = phase
        .non_mesh_items
        .iter()
        .flat_map(|((batch, _), bin)| {
            let index = ids
                .iter()
                .position(|&id| id == batch.draw_function)
                .unwrap();
            bin.entities.values().map(move |&entity| (index, entity))
        })
        .collect();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = texture.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut functions = functions.write();
    functions.prepare(&world);
    for (_, range) in category_ranges([
        RuntimeStage::GpuTerrainModel,
        RuntimeStage::GpuTerrainSolid,
        RuntimeStage::GpuTerrainModel,
    ]) {
        let colors = [Some(wgpu::RenderPassColorAttachment {
            view: &target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })];
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &colors,
            ..Default::default()
        });
        let mut pass = TrackedRenderPass::new(&device, pass);
        draw_bins(
            &phase,
            range,
            &mut functions,
            &world,
            Entity::PLACEHOLDER,
            &mut pass,
        )
        .unwrap();
    }
    assert_eq!(*world.resource::<DrawLog>().0.lock().unwrap(), expected);
}

#[test]
fn unsupported_phases_keep_the_original_node() {
    let mut opaque = phase::<Opaque3d>();
    let alpha = phase::<AlphaMask3d>();
    assert!(admitted(&opaque, &alpha, false));
    assert!(!admitted(&opaque, &alpha, true));
    let functions = DrawFunctions::<Opaque3d>::default();
    let draw = functions.write().add(RecordDraw(0));
    add(
        &mut opaque,
        draw,
        Entity::PLACEHOLDER,
        BinnedRenderPhaseType::BatchableMesh,
    );
    assert!(!admitted(&opaque, &alpha, false));
}

#[test]
fn category_splitting_requires_opt_in_and_timestamp_support() {
    for enabled in [false, true] {
        for supported in [false, true] {
            let features = if supported {
                wgpu::Features::TIMESTAMP_QUERY
            } else {
                wgpu::Features::empty()
            };
            let (device, _queue) = super::super::tests::noop_device(features);
            let mut world = World::new();
            world.init_resource::<DrawFunctions<Opaque3d>>();
            if enabled {
                world.insert_resource(CategoryProfiling);
            }
            let color = crate::scene_sampling::tests::texture(
                &device,
                wgpu::TextureFormat::Rgba8Unorm,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let depth = crate::scene_sampling::tests::texture(
                &device,
                wgpu::TextureFormat::Depth32Float,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let color = color.create_view(&default());
            let depth = depth.create_view(&default());
            let (split, buffers) =
                crate::render_test_support::record(&mut world, &device, |world, context| {
                    draw_categories(
                        world,
                        context,
                        Entity::PLACEHOLDER,
                        &phase(),
                        &phase(),
                        false,
                        wgpu::RenderPassColorAttachment {
                            view: &color,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        },
                        wgpu::RenderPassDepthStencilAttachment {
                            view: &depth,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(0.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        },
                        None,
                    )
                });
            assert_eq!(split, enabled && supported);
            assert_eq!(buffers.is_empty(), !split);
        }
    }
}

#[test]
fn category_profiling_disables_overlapping_in_pass_draw_spans() {
    use bevy::ecs::system::RunSystemOnce;
    let (device, queue) = super::super::tests::noop_device(
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES,
    );
    let mut world = World::new();
    world.insert_resource(device);
    world.insert_resource(queue);
    world.insert_resource(crate::RuntimeStageProfiler::new(true));
    world.insert_resource(CategoryProfiling);
    world
        .run_system_once(super::super::init_gpu_timestamps)
        .unwrap();
    assert!(!world.resource::<GpuTimestamps>().draw_spans);
}
