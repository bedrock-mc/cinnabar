//! Headless fixtures for recording the same deferred commands as render systems.

#[path = "render_test_support/scene_raster.rs"]
pub(crate) mod scene_raster;

use bevy::{
    ecs::{schedule::ScheduleLabel, system::SystemState},
    prelude::*,
    render::{
        camera::ExtractedCamera,
        renderer::{PendingCommandBuffers, RenderContext, RenderDevice},
    },
};

/// Installs a render owner and its startup systems on `device` without a window or submission.
pub(crate) fn owner_app(device: RenderDevice, queue: wgpu::Queue, plugin: impl Plugin) -> App {
    use bevy::{
        core_pipeline::core_3d::{Opaque3d, Transparent3d},
        render::{
            ExtractSchedule, Render, RenderApp, RenderStartup, render_phase::DrawFunctions,
            render_resource::PipelineCache, renderer::RenderGraph, renderer::RenderQueue,
        },
    };
    let mut render_app = SubApp::new();
    render_app
        .insert_resource(PipelineCache::new(device.clone(), true))
        .insert_resource(device)
        .insert_resource(RenderQueue::new(queue))
        .add_schedule(RenderGraph::base_schedule())
        .init_resource::<DrawFunctions<Opaque3d>>()
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
    app
}

/// As [`owner_app`] on any native adapter with `limits`; `None` skips the test with a named
/// missing-fixture message.
pub(crate) fn fixture_owner_app(
    name: &str,
    limits: wgpu::Limits,
    plugin: impl Plugin,
) -> Option<App> {
    let crate::gpu_snapshot::Gpu { device, queue, .. } =
        crate::gpu_snapshot::Gpu::for_fixture_with_limits(name, wgpu::Features::empty(), limits)?;
    Some(owner_app(RenderDevice::from(device), queue, plugin))
}

/// Fails a headless fixture when the renderer would otherwise stop updating after a GPU error.
pub(crate) fn fail_on_render_error(app: &mut App) {
    app.insert_resource(bevy::render::error_handler::RenderErrorHandler(
        |error, _, _| panic!("headless rendering failed: {error:?}"),
    ));
}

/// Records commands through the render system parameter and flushes them in submission order.
pub(crate) fn record<R>(
    world: &mut World,
    device: &RenderDevice,
    draw: impl FnOnce(&World, &mut RenderContext<'_, '_>) -> R,
) -> (R, Vec<wgpu::CommandBuffer>) {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    world.insert_resource(device.clone());
    world.init_resource::<PendingCommandBuffers>();
    let mut state = SystemState::<RenderContext>::new(world);
    let result = draw(world, &mut state.get(world).unwrap());
    state.apply(world);
    (
        result,
        world
            .resource_mut::<PendingCommandBuffers>()
            .finish()
            .collect::<Vec<_>>(),
    )
}

/// Supplies camera metadata for a headless render view with no presentation surface.
pub(crate) fn camera(hdr: bool) -> ExtractedCamera {
    ExtractedCamera {
        target: None,
        physical_viewport_size: None,
        physical_target_size: None,
        viewport: None,
        schedule: bevy::core_pipeline::Core3d.intern(),
        order: 0,
        output_mode: default(),
        msaa_writeback: default(),
        clear_color: default(),
        sorted_camera_index_for_target: 0,
        exposure: 1.0,
        hdr,
    }
}

/// Creates a camera schedule with no drawable view and a validation device.
pub(crate) fn empty_render_world() -> World {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(bevy::render::renderer::RenderQueue::new(queue));
    world.init_resource::<PendingCommandBuffers>();
    let view = world.spawn_empty().id();
    world.insert_resource(bevy::render::renderer::CurrentView(view));
    let mut schedule = bevy::core_pipeline::Core3d::base_schedule();
    schedule.add_systems(
        (
            bevy::core_pipeline::core_3d::main_opaque_pass_3d,
            bevy::core_pipeline::core_3d::main_transparent_pass_3d,
        )
            .in_set(bevy::core_pipeline::Core3dSystems::MainPass),
    );
    schedule.add_systems(
        bevy::core_pipeline::upscaling::upscaling
            .after(bevy::core_pipeline::Core3dSystems::PostProcess),
    );
    world.add_schedule(schedule);
    world
}

/// Runs an unprepared camera and checks that it submits no graphics work.
pub(crate) fn assert_empty_render(world: &mut World) {
    world.run_schedule(bevy::core_pipeline::Core3d);
    assert!(
        world
            .resource_mut::<PendingCommandBuffers>()
            .finish()
            .collect::<Vec<_>>()
            .is_empty()
    );
}
