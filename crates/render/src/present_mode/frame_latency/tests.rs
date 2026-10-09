use super::*;
use bevy::{
    camera::{NormalizedRenderTarget, RenderTarget},
    ecs::system::RunSystemOnce,
    render::{MainWorld, texture::OutputColorAttachment, view::window::ExtractedWindow},
    window::{RawHandleWrapper, WindowRef, WindowWrapper},
};
use wgpu::rwh::{self, HasDisplayHandle, HasWindowHandle};

struct TestWindow;

impl HasWindowHandle for TestWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        // SAFETY: Web handles contain an integer, not a borrowed native pointer. This fixture
        // only tests extraction and invalidation; it never creates a native surface.
        Ok(unsafe { rwh::WindowHandle::borrow_raw(rwh::WebWindowHandle::new(1).into()) })
    }
}

impl HasDisplayHandle for TestWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::web())
    }
}

/// Creates matching main/render windows and a retained output attachment on the NOOP device.
fn worlds(vsync: bool) -> (World, Entity, NormalizedRenderTarget) {
    let mut main = MainWorld::default();
    let latency = Some(frame_latency_for_vsync(vsync));
    let entity = main
        .spawn(Window {
            desired_maximum_frame_latency: latency,
            ..Default::default()
        })
        .id();
    let target = RenderTarget::Window(WindowRef::Entity(entity))
        .normalize(None)
        .unwrap();
    let mut windows = ExtractedWindows::default();
    windows.windows.insert(
        entity,
        ExtractedWindow {
            entity,
            handle: RawHandleWrapper::new(&WindowWrapper::new(TestWindow)).unwrap(),
            physical_width: 4,
            physical_height: 4,
            present_mode: bevy::window::PresentMode::Fifo,
            desired_maximum_frame_latency: latency,
            swap_chain_texture_view: None,
            swap_chain_texture: None,
            swap_chain_texture_format: None,
            swap_chain_texture_view_format: None,
            size_changed: false,
            present_mode_changed: false,
            alpha_mode: Default::default(),
            needs_initial_present: false,
        },
    );
    let (app, _) = crate::queue_review_support::app();
    let device = app
        .world()
        .resource::<bevy::render::renderer::RenderDevice>();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("retained window output"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    windows
        .windows
        .get_mut(&entity)
        .unwrap()
        .swap_chain_texture_view = Some(view.clone());
    let mut attachments = ViewTargetAttachments::default();
    attachments.insert(
        target.clone(),
        OutputColorAttachment::new(view, texture.format()),
    );
    let mut world = World::new();
    world.insert_resource(main);
    world.insert_resource(windows);
    world.insert_resource(attachments);
    world.init_resource::<ChangedFrameLatency>();
    world.init_resource::<WindowSurfaces>();
    (world, entity, target)
}

/// Both toggle directions invalidate retained outputs even when the present mode is unchanged.
#[test]
fn live_frame_latency_changes_release_cached_surface_outputs() {
    for initial_vsync in [false, true] {
        let (mut world, entity, target) = worlds(initial_vsync);
        world.run_system_once(extract_frame_latency).unwrap();
        world.run_system_once(recreate_surfaces).unwrap();
        assert!(
            world
                .resource::<ViewTargetAttachments>()
                .contains_key(&target)
        );

        world
            .resource_mut::<MainWorld>()
            .entity_mut(entity)
            .get_mut::<Window>()
            .unwrap()
            .desired_maximum_frame_latency = Some(frame_latency_for_vsync(!initial_vsync));
        world.run_system_once(extract_frame_latency).unwrap();
        world.run_system_once(recreate_surfaces).unwrap();
        let extracted = &world.resource::<ExtractedWindows>().windows[&entity];
        assert_eq!(
            extracted.desired_maximum_frame_latency,
            Some(frame_latency_for_vsync(!initial_vsync))
        );
        assert!(extracted.swap_chain_texture_view.is_none());
        assert!(
            !world
                .resource::<ViewTargetAttachments>()
                .contains_key(&target)
        );
        assert!(!world.resource::<ChangedFrameLatency>().0);
    }
}
