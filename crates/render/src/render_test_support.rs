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

/// Reads a buffer range after submitted uploads finish, including on the validating NOOP device.
pub(crate) fn read_buffer(
    device: &RenderDevice,
    queue: &bevy::render::renderer::RenderQueue,
    buffer: &wgpu::Buffer,
    offset: u64,
    bytes: u64,
) -> Vec<u8> {
    if bytes == 0 {
        return Vec::new();
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("terrain publication readback"),
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, offset, &readback, 0, bytes);
    encoder.map_buffer_on_submit(&readback, wgpu::MapMode::Read, .., |result| {
        result.expect("terrain publication readback maps");
    });
    queue.submit([encoder.finish()]);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    readback.slice(..).get_mapped_range().unwrap().to_vec()
}

/// Reads one RGBA8 texture layer, removing the GPU copy's required row padding.
pub(crate) fn read_rgba8_layer(
    device: &RenderDevice,
    queue: &bevy::render::renderer::RenderQueue,
    texture: &wgpu::Texture,
    layer: u32,
) -> Vec<u8> {
    let width = texture.width() * 4;
    let row =
        width.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let bytes = u64::from(row) * u64::from(texture.height());
    let scratch = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("terrain colour layer readback"),
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: 0,
                y: 0,
                z: layer,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &scratch,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(texture.height()),
            },
        },
        wgpu::Extent3d {
            width: texture.width(),
            height: texture.height(),
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    read_buffer(device, queue, &scratch, 0, bytes)
        .chunks(row as usize)
        .flat_map(|row| row[..width as usize].iter().copied())
        .collect()
}
