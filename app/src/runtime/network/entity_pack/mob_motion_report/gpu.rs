//! Production actor draw and native GPU texture readback for the offline motion report.
use bevy::{
    asset::AssetPlugin,
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::{CorePipelinePlugin, tonemapping::Tonemapping},
    mesh::MeshPlugin,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{CachedPipelineState, PipelineCache, TextureFormat, TextureUsages},
        renderer::{RenderAdapterInfo, RenderDevice},
    },
    window::WindowPlugin,
};

pub(super) const VIEWPORT: [u32; 2] = [768, 512];

#[derive(Resource, Default)]
struct Captured {
    revision: u64,
    pixels: Vec<u8>,
}

pub(super) fn app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins(AssetPlugin::default())
        .add_plugins(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            TransformPlugin,
            CorePipelinePlugin,
            render::ActorRenderPlugin,
        ));
    let mut target = Image::new_target_texture(
        VIEWPORT[0],
        VIEWPORT[1],
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(target);
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Camera {
                clear_color: Color::srgb(0.38, 0.49, 0.58).into(),
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
            Msaa::Off,
            Tonemapping::None,
            camera_transform(0.0),
        ))
        .id();
    app.init_resource::<Captured>();
    app.world_mut().spawn(Readback::texture(target)).observe(
        |event: On<ReadbackComplete>, mut capture: ResMut<Captured>| {
            capture.revision += 1;
            capture.pixels.clone_from(&event.data);
        },
    );
    app.finish();
    app.cleanup();
    let info = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderAdapterInfo>();
    eprintln!("mob replay adapter: {:?}", info.0);
    #[cfg(target_os = "macos")]
    assert_eq!(info.0.backend, wgpu::Backend::Metal);
    assert_ne!(info.0.device_type, wgpu::DeviceType::Cpu);
    (app, camera)
}

pub(super) fn camera_transform(travel: f32) -> Transform {
    Transform::from_xyz(5.0 + travel, 2.8, 6.0).looking_at(Vec3::new(travel, 0.9, 0.0), Vec3::Y)
}

pub(super) fn update(app: &mut App) {
    app.update();
    app.sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
}

pub(super) fn capture(app: &mut App) -> image::RgbaImage {
    let previous = app.world().resource::<Captured>().revision;
    // Let extraction, draw, readback mapping and main-world event delivery finish.
    for _ in 0..4 {
        update(app);
    }
    let capture = app.world().resource::<Captured>();
    assert!(capture.revision > previous, "fresh native GPU readback");
    image::RgbaImage::from_raw(VIEWPORT[0], VIEWPORT[1], capture.pixels.clone())
        .expect("tightly packed RGBA native GPU target")
}

pub(super) fn verify(app: &App) {
    let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
    for pipeline in cache.pipelines() {
        if let CachedPipelineState::Err(error) = &pipeline.state {
            panic!("mob replay pipeline failed: {error}");
        }
    }
    assert!(
        !app.world()
            .resource::<render::ActorPresentationGate>()
            .drain()
            .is_empty(),
        "the production actor draw must execute and complete on the GPU"
    );
}
