use super::*;
use bevy::{
    core_pipeline::core_3d::Transparent3d,
    ecs::system::SystemState,
    render::render_phase::{RenderCommand, RenderCommandResult, TrackedRenderPass},
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
        .init_resource::<materials::Materials>()
        .init_resource::<clock::AnimationClock>()
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

    let source = materials::source(
        &mod_render::material::module(mod_render::material::BUILTIN),
        include_str!("drawing.wgsl"),
    );
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

    app.world_mut().run_system_once(pipeline::bindings).unwrap();
    let stages = crate::gpu_timing::tests::draw_stages(app.world_mut(), |world| {
        let mut state =
            SystemState::<<pipeline::Draw as RenderCommand<Transparent3d>>::Param>::new(world);
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
                pipeline::Draw::render(item, ((), ()), None, state.get(world).unwrap(), &mut pass),
                RenderCommandResult::Success
            ));
        }
        drop(pass);
        queue.submit([encoder.finish()]);
    });
    assert_eq!(stages, [render_scene::RuntimeStage::GpuModPrimitives; 2]);
}
