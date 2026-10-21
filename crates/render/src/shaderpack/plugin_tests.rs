//! Headless checks for native-view ownership during shader-pack presentation.

use super::*;
use bevy::camera::CameraMainTextureUsages;
use bevy::ecs::system::RunSystemOnce;
use bevy::render::{
    renderer::{CurrentView, PendingCommandBuffers},
    texture::TextureCache,
    view::{Msaa, RetainedViewEntity, ViewTargetAttachments, prepare_view_targets},
};

#[test]
fn unsupported_presentation_keeps_the_native_target() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions {
                enable: true,
                ..default()
            },
            ..default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&default())).unwrap();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&default())).unwrap();
    let gpu = GpuContext {
        device,
        queue,
        adapter,
    };
    let source = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
        (
            "final.vsh",
            "#version 120\nvoid main() { gl_Position = ftransform(); }",
        ),
        (
            "final.fsh",
            "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }",
        ),
    ]));
    let pack = Arc::new(
        super::super::loader::build(
            &source,
            "fixture",
            JAVA_DIMENSIONS[0],
            &default(),
            None,
            &gpu,
            &default(),
            &default(),
        )
        .unwrap(),
    );
    let state = ViewState::new(&gpu.device, pack, [4, 4]).unwrap();
    let device = RenderDevice::from(gpu.device);
    let mut world = World::new();
    world.insert_resource(device.clone());
    world.insert_resource(RenderQueue::new(gpu.queue));
    world.init_resource::<PendingCommandBuffers>();
    world.init_resource::<ClearColor>();
    world.init_resource::<TextureCache>();
    world.init_resource::<ViewTargetAttachments>();
    let depth = world.resource_mut::<TextureCache>().get(
        &device,
        wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
    );
    let mut camera = crate::render_test_support::camera(false);
    camera.physical_target_size = Some(UVec2::splat(4));
    camera.clear_color = bevy::camera::ClearColorConfig::None;
    let entity = world
        .spawn((
            camera,
            CameraMainTextureUsages::default(),
            Msaa::Off,
            ShaderPackRendering,
            ViewDepthStencilTexture::new(depth, Some(0.0), None),
            ExtractedView {
                retained_view_entity: RetainedViewEntity::new(Entity::PLACEHOLDER.into(), None, 0),
                clip_from_view: Mat4::IDENTITY,
                world_from_view: GlobalTransform::default(),
                clip_from_world: None,
                target_format: wgpu::TextureFormat::Rgba8Unorm,
                viewport: UVec4::new(0, 0, 4, 4),
                color_grading: default(),
                invert_culling: false,
            },
        ))
        .id();
    world.run_system_once(prepare_view_targets).unwrap();
    world.insert_resource(CurrentView(entity));
    let mut views = ShaderPackViews::default();
    views.states.get_mut().unwrap().insert(entity, state);
    world.insert_resource(views);
    let original = world
        .get::<ViewTarget>(entity)
        .unwrap()
        .main_texture_view()
        .id();
    let _: () = world.run_system_once(shaderpack_post).unwrap();
    assert_eq!(
        world
            .get::<ViewTarget>(entity)
            .unwrap()
            .main_texture_view()
            .id(),
        original
    );
    assert!(
        world
            .resource_mut::<PendingCommandBuffers>()
            .finish()
            .next()
            .is_none()
    );
}
