//! A hidden window without a native surface still produces original camera pixels.

use std::sync::{Arc, Mutex};

use bevy::{
    asset::AssetPlugin,
    camera::CameraPlugin,
    core_pipeline::{CorePipelinePlugin, tonemapping::Tonemapping},
    mesh::MeshPlugin,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
        settings::RenderCreation,
    },
    window::{PrimaryWindow, WindowPlugin, WindowResolution},
};

/// Uses a native offscreen device and names the missing fixture when none is available.
fn renderer() -> Option<RenderPlugin> {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = match bevy::tasks::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!(
                "skipping hidden_camera_pixels_reach_shared_capture: missing native GPU fixture"
            );
            return None;
        }
        Err(error) => panic!("capture fixture adapter: {error}"),
    };
    let info = adapter.get_info();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..default()
    }))
    .expect("capture fixture device");
    Some(RenderPlugin {
        render_creation: RenderCreation::manual(
            RenderDevice::from(device),
            RenderQueue::new(queue),
            RenderAdapterInfo::new(info),
            RenderAdapter::new(adapter),
            RenderInstance::new(instance),
        ),
        synchronous_pipeline_compilation: true,
        ..default()
    })
}

/// Captures through the same readback shared by screenshots and recordings.
fn capture(app: &mut App) -> Image {
    let result = Arc::new(Mutex::new(None));
    let received = result.clone();
    app.world_mut()
        .resource_mut::<super::FrameCapture>()
        .request(move |image| {
            *received.lock().unwrap() = Some(image.clone());
        });
    test_time::eventually("hidden camera pixel readback", || {
        app.update();
        app.sub_app(RenderApp)
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        result.lock().unwrap().is_some()
    });
    result.lock().unwrap().take().unwrap()
}

/// Checks rendered color and dimensions without relying on camera or schedule internals.
fn assert_blue(image: Image, expected_size: (u32, u32)) {
    let pixels = image.try_into_dynamic().unwrap().to_rgb8();
    assert_eq!(pixels.dimensions(), expected_size);
    let pixel = pixels.get_pixel(expected_size.0 / 2, expected_size.1 / 2).0;
    assert!(
        pixel[2] > 100 && pixel[2] > pixel[1] && pixel[1] > pixel[0],
        "blue camera output, got {pixel:?}"
    );
}

#[test]
fn hidden_camera_pixels_reach_shared_capture() {
    let Some(renderer) = renderer() else { return };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: Some(Window {
                visible: false,
                focused: false,
                resolution: WindowResolution::new(64, 32).with_scale_factor_override(1.0),
                ..default()
            }),
            ..default()
        })
        .add_plugins(AssetPlugin::default())
        .add_plugins(renderer)
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ));
    super::configure_frame_capture(&mut app);
    app.world_mut().spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.2, 0.4, 0.8)),
            ..default()
        },
        client_presentation::camera::FlyCamera::default(),
        Msaa::Off,
        Tonemapping::Linear,
    ));
    app.finish();
    app.cleanup();
    for _ in 0..8 {
        app.update();
    }
    assert_blue(capture(&mut app), (64, 32));
    {
        let mut windows = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows
            .single_mut(app.world_mut())
            .unwrap()
            .resolution
            .set_physical_resolution(32, 16);
    }
    for _ in 0..4 {
        app.update();
    }
    assert_blue(capture(&mut app), (32, 16));
}
