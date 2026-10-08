//! Render queue fixtures using a validated local NOOP device.
use bevy::{
    core_pipeline::core_3d::Transparent3d,
    prelude::*,
    render::{
        render_phase::{DrawFunctions, ViewSortedRenderPhases},
        render_resource::{CachedRenderPipelineId, PipelineCache, RenderPipelineDescriptor},
        renderer::{RenderAdapter, RenderDevice, RenderQueue, WgpuWrapper},
        sync_world::MainEntity,
        view::{ExtractedView, RetainedViewEntity},
    },
};
use std::sync::Arc;

/// Processes queued descriptors before reading one, without requiring loaded shader assets.
pub(crate) fn queued_descriptor(
    cache: &mut PipelineCache,
    id: CachedRenderPipelineId,
) -> &RenderPipelineDescriptor {
    // Linux processes the cache on Bevy's asynchronous pool, even on the NOOP backend.
    bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    cache.process_queue();
    cache.get_render_pipeline_descriptor(id)
}

/// Creates a render view with an empty transparent phase and a real pipeline cache.
pub(crate) fn app() -> (App, RetainedViewEntity) {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..Default::default()
    });
    let adapter =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
            ..Default::default()
        },
        ..Default::default()
    }))
    .unwrap();
    let device = RenderDevice::from(device);
    let adapter = RenderAdapter(Arc::new(WgpuWrapper::new(adapter)));
    let mut app = App::new();
    app.insert_resource(PipelineCache::new(device.clone(), adapter, false))
        .insert_resource(device)
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
        .init_resource::<DrawFunctions<Transparent3d>>()
        .init_resource::<ViewSortedRenderPhases<Transparent3d>>();
    let view = app.world_mut().spawn_empty().id();
    let retained = RetainedViewEntity::new(view.into(), None, 0);
    app.world_mut().entity_mut(view).insert((
        MainEntity::from(view),
        Msaa::Sample4,
        ExtractedView {
            retained_view_entity: retained,
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            hdr: false,
            viewport: UVec4::new(0, 0, 1, 1),
            color_grading: default(),
            invert_culling: false,
        },
    ));
    app.world_mut()
        .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
        .insert_or_clear(retained);
    (app, retained)
}

/// Returns the queued items for the fixture's sole view.
pub(crate) fn items(app: &App, view: RetainedViewEntity) -> &[Transparent3d] {
    &app.world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>()
        .get(&view)
        .unwrap()
        .items
}

/// Clears only phase items, retaining the same view across a frame transition.
pub(crate) fn clear(app: &mut App, view: RetainedViewEntity) {
    app.world_mut()
        .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
        .insert_or_clear(view);
}
