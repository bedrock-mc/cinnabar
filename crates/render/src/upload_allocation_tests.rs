//! Real owner preparation keeps Metal staging allocations visible until submission.

use bevy::{
    core_pipeline::core_3d::Transparent3d,
    ecs::system::RunSystemOnce,
    prelude::*,
    render::{
        ExtractSchedule, Render, RenderApp, RenderStartup,
        graph::CameraDriverLabel,
        render_graph::{EmptyNode, RenderGraph},
        render_phase::DrawFunctions,
        render_resource::PipelineCache,
        renderer::{RenderAdapter, RenderDevice, RenderQueue, WgpuWrapper},
    },
};
use std::sync::Arc;

/// Installs the real owner and startup systems on Metal without a window or render submission.
pub(crate) fn app(plugin: impl Plugin) -> Option<App> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..Default::default()
    });
    let Ok(adapter) =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("missing fixture: Metal adapter for recurring upload allocation regression");
        return None;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
            ..Default::default()
        },
        ..Default::default()
    }))
    .expect("Metal upload regression device");
    let before = device.get_internal_counters().hal.buffers.read();
    let probe = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("allocation counter probe"),
        size: wgpu::COPY_BUFFER_ALIGNMENT,
        usage: wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    assert_eq!(
        device.get_internal_counters().hal.buffers.read(),
        before + 1,
        "Metal buffer counters must observe allocations"
    );
    drop(probe);
    let device = RenderDevice::from(device);
    let adapter = RenderAdapter(Arc::new(WgpuWrapper::new(adapter)));
    let mut graph = RenderGraph::default();
    graph.add_node(CameraDriverLabel, EmptyNode);
    let mut render_app = SubApp::new();
    render_app
        .insert_resource(PipelineCache::new(device.clone(), adapter, true))
        .insert_resource(device)
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
        .insert_resource(graph)
        .init_resource::<DrawFunctions<Transparent3d>>()
        .add_schedule(Schedule::new(RenderStartup))
        .add_schedule(Render::base_schedule())
        .add_schedule(Schedule::new(ExtractSchedule));
    let mut app = App::new();
    app.insert_resource(Assets::<Shader>::default())
        .insert_sub_app(RenderApp, render_app);
    app.add_plugins(plugin);
    app.finish();
    app.sub_app_mut(RenderApp)
        .world_mut()
        .run_schedule(RenderStartup);
    Some(app)
}

/// Counts all live Metal buffers, including queue staging hidden from destination handles.
pub(crate) fn buffers(world: &World) -> isize {
    world
        .resource::<RenderDevice>()
        .wgpu_device()
        .get_internal_counters()
        .hal
        .buffers
        .read()
}

/// Supplies a valid static UI revision while its viewport uniform changes independently.
fn ui_input() -> render_model::UiRenderInput {
    use render_model::{
        UiRenderBatch, UiRenderInput, UiRenderTextureArray, UiRenderVertex, UiScissor,
        UiTexturePage,
    };
    UiRenderInput {
        revision: 1,
        viewport_size: [64, 64],
        safe_area: [0; 4],
        vertices: Arc::from(
            [UiRenderVertex {
                position: [0.0; 2],
                clip_z: 0.0,
                clip_w: 1.0,
                uv: [0.0; 2],
                color: [255; 4],
                style_flags: 0,
                alpha_cutoff: -1.0,
                model_light: 1.0,
                overlay_color: [0.0; 4],
            }; 4],
        ),
        indices: Arc::from([0, 1, 2, 0, 2, 3]),
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 64, 64),
            0,
            6,
            render_model::UI_BLEND_ALPHA,
        )]),
        textures: Arc::new(
            UiRenderTextureArray::new(
                vec![UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    }
}

#[test]
fn recurring_ui_viewport_updates_allocate_no_gpu_staging_buffers() {
    use crate::ui_render::{
        UiGlintSettings, UiRenderPlugin, UiRenderSceneResource, UiRenderStatsResource,
        prepare_ui_resources,
    };
    let Some(mut app) = app(UiRenderPlugin) else {
        return;
    };
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    let mut scene = UiRenderSceneResource::default();
    scene.publish(ui_input(), &stats).unwrap();
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.insert_resource(scene);
    world.insert_resource(UiGlintSettings {
        strength: 1.0,
        speed: 0.0,
    });
    world.run_system_once(prepare_ui_resources).unwrap();
    assert_eq!(stats.snapshot().accepted_revision, Some(1));
    let initial_buffers = buffers(world);
    for strength in [0.25, 0.5, 0.75] {
        world.resource_mut::<UiGlintSettings>().strength = strength;
        world.run_system_once(prepare_ui_resources).unwrap();
        assert_eq!(stats.snapshot().accepted_revision, Some(1));
        assert_eq!(
            buffers(world),
            initial_buffers,
            "a changed viewport must not allocate another Metal staging buffer"
        );
    }
}
