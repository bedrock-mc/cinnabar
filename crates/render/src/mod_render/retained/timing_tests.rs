use super::*;
use bevy::{
    core_pipeline::core_3d::Transparent3d,
    ecs::system::SystemState,
    render::{
        render_phase::{RenderCommand, RenderCommandResult, TrackedRenderPass},
        view::ViewUniformOffset,
    },
};

#[test]
fn retained_draws_record_mod_gpu_timing() {
    let features = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    let (device, queue) = crate::gpu_timing::tests::noop_device(features);
    let (mut app, view) = crate::queue_review_support::app();
    app.insert_resource(PipelineCache::new(device.clone(), true))
        .insert_resource(device.clone())
        .insert_resource(queue.clone())
        .init_resource::<gpu::Gpu>()
        .init_resource::<pipeline::Pipeline>()
        .insert_resource(DrawingFrame {
            connection: 1,
            ..default()
        })
        .init_resource::<render_scene::ModRenderScene>()
        .add_render_command::<Transparent3d, pipeline::Draw>();
    let (mut store, _, mut instance) = drawing();
    store.begin();
    instance.geometry = store
        .create_geometry(GeometrySpec::Polyline {
            vertices: [[0.; 3], [1., 0., 0.]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            closed: false,
        })
        .unwrap();
    store
        .create(instance, mod_api::drawing::DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    app.world_mut().resource_mut::<DrawingFrame>().live_owners = store
        .snapshot()
        .geometry
        .keys()
        .map(|h| (h.owner, h.generation))
        .collect();
    app.world_mut()
        .resource_mut::<render_scene::ModRenderScene>()
        .retained = vec![store.snapshot().clone()];
    app.world_mut().run_system_once(gpu::queue).unwrap();

    let source = crate::shader_source::standalone(include_str!("drawing.wesl"), &[]);
    let mut cache = app.world_mut().resource_mut::<PipelineCache>();
    cache.set_shader(
        SHADER.id(),
        crate::shader_safety::from_wgsl(source, "retained-timing.wgsl"),
    );
    cache.process_queue();
    let items = crate::queue_review_support::items(&app, view);
    assert_eq!(
        items.len(),
        2,
        "triangle and polyline batches must both draw"
    );
    for item in &items {
        assert!(
            app.world()
                .resource::<PipelineCache>()
                .get_render_pipeline(item.pipeline)
                .is_some()
        );
    }

    let uniform = device.create_buffer(&BufferDescriptor {
        label: Some("retained timing view"),
        size: bevy::render::view::ViewUniform::min_size().get(),
        usage: BufferUsages::UNIFORM,
        mapped_at_creation: false,
    });
    let layout = app
        .world()
        .resource::<PipelineCache>()
        .get_bind_group_layout(&pipeline::layout());
    app.world_mut().resource_scope(|_, mut gpu: Mut<gpu::Gpu>| {
        let gpu::Gpu { meshes, views, .. } = &mut *gpu;
        for prepared in views.values_mut() {
            for batch in &prepared.batches {
                prepared.bindings.insert(
                    batch.geometry,
                    device.create_bind_group(
                        "retained timing bindings",
                        &layout,
                        &[
                            BindGroupEntry {
                                binding: 0,
                                resource: uniform.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 1,
                                resource: meshes[&batch.geometry].buffer.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 2,
                                resource: prepared.buffer.as_entire_binding(),
                            },
                        ],
                    ),
                );
            }
        }
    });
    let stages = crate::gpu_timing::tests::draw_stages(app.world_mut(), |world| {
        let mut state =
            SystemState::<<pipeline::Draw as RenderCommand<Transparent3d>>::Param>::new(world);
        let offset = ViewUniformOffset { offset: 0 };
        let textures = [
            render_scene::SCENE_COLOR_FORMAT,
            bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
        ]
        .map(|format| {
            device
                .create_texture(&TextureDescriptor {
                    label: Some("retained timing target"),
                    size: Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 4,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&default())
        });
        let colors = [Some(RenderPassColorAttachment {
            view: &textures[0],
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })];
        let mut encoder = device.create_command_encoder(&default());
        let pass = encoder.begin_render_pass(&RenderPassDescriptor {
            color_attachments: &colors,
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &textures[1],
                depth_ops: Some(Operations::default()),
                stencil_ops: None,
            }),
            ..default()
        });
        let mut pass = TrackedRenderPass::new(&device, pass);
        let items = world
            .resource::<bevy::render::render_phase::ViewSortedRenderPhases<Transparent3d>>()
            .get(&view)
            .unwrap();
        for item in items.items.values() {
            assert!(matches!(
                pipeline::Draw::render(
                    item,
                    ((), &offset),
                    None,
                    state.get(world).unwrap(),
                    &mut pass
                ),
                RenderCommandResult::Success
            ));
        }
        drop(pass);
        queue.submit([encoder.finish()]);
    });
    assert_eq!(stages, [render_scene::RuntimeStage::GpuModPrimitives; 2]);
}
