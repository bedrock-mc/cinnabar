//! Headless fixtures for recording the same deferred commands as render systems.

use bevy::{
    ecs::{schedule::ScheduleLabel, system::SystemState},
    prelude::*,
    render::{
        camera::ExtractedCamera,
        renderer::{PendingCommandBuffers, RenderContext, RenderDevice},
    },
};

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
    world.insert_resource(device.clone());
    world.init_resource::<PendingCommandBuffers>();
    let mut state = SystemState::<RenderContext>::new(world);
    let result = draw(world, &mut state.get(world).unwrap());
    state.apply(world);
    (result, world.resource_mut::<PendingCommandBuffers>().take())
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
        compositing_space: None,
    }
}

/// Creates a camera schedule with no drawable view and a validation device.
pub(crate) fn empty_render_world() -> World {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(bevy::render::renderer::RenderQueue(std::sync::Arc::new(
        bevy::render::renderer::WgpuWrapper::new(queue),
    )));
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
            .take()
            .is_empty()
    );
}
