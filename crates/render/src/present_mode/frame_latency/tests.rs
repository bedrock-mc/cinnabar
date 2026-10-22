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
fn worlds(initial: NonZeroU32) -> (World, Entity, NormalizedRenderTarget) {
    let mut main = MainWorld::default();
    let latency = Some(initial);
    let entity = main
        .spawn(Window {
            desired_maximum_frame_latency: latency,
            ..Default::default()
        })
        .id();
    let target = RenderTarget::Window(WindowRef::Entity(entity))
        .normalize(None)
        .unwrap();
    let handle = RawHandleWrapper::new(&WindowWrapper::new(TestWindow)).unwrap();
    let mut extracted = ExtractedWindow {
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
    };
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
    extracted.swap_chain_texture_view = Some(view.clone());
    let mut attachments = ViewTargetAttachments::default();
    attachments.insert(
        target.clone(),
        OutputColorAttachment::new(view, texture.format()),
    );
    let mut world = World::new();
    world.insert_resource(main);
    let render_entity = world
        .spawn((
            bevy::render::sync_world::MainEntity::from(entity),
            extracted,
            handle,
        ))
        .id();
    world.insert_resource(attachments);
    world.init_resource::<ChangedFrameLatency>();
    (world, render_entity, target)
}

/// Only a drawable held by presentation reserves a second VSync frame; other paths keep one.
#[test]
fn vsync_reserves_a_slot_only_for_a_drawable_held_by_presentation() {
    assert_eq!(frame_latency(true, false).get(), 1);
    assert_eq!(frame_latency(true, true).get(), 2);
    assert_eq!(frame_latency(false, false).get(), 2);
    assert_eq!(frame_latency(false, true).get(), 2);
}

/// Windows starts with overlap until headroom is measured; other platforms retain their default.
#[test]
fn vsync_latency_follows_the_backend_delivery_path() {
    let expected = if cfg!(target_os = "windows") || presentation_holds_a_drawable() {
        2
    } else {
        1
    };
    assert_eq!(frame_latency_for_vsync(true).get(), expected);
    assert_eq!(frame_latency_for_vsync(false).get(), 2);
    #[cfg(target_os = "ios")]
    assert!(presentation_holds_a_drawable());
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    assert!(!presentation_holds_a_drawable());
}

/// Adaptive decisions cannot consume the slot reserved for prefetched Metal drawables.
#[test]
fn adaptive_feedback_preserves_display_link_and_unsynchronized_latency() {
    for measured in [1, 2] {
        assert_eq!(resolve_latency(true, false, measured).get(), measured);
        assert_eq!(resolve_latency(true, true, measured).get(), 2);
        assert_eq!(resolve_latency(false, false, measured).get(), 2);
        assert_eq!(resolve_latency(false, true, measured).get(), 2);
    }
}

/// Both latency change directions invalidate retained outputs with an unchanged present mode.
#[test]
fn live_frame_latency_changes_release_cached_surface_outputs() {
    for (initial, next) in [(1, 2), (2, 1)] {
        let (initial, next) = (
            NonZeroU32::new(initial).unwrap(),
            NonZeroU32::new(next).unwrap(),
        );
        let (mut world, entity, target) = worlds(initial);
        world.run_system_once(extract_frame_latency).unwrap();
        world.run_system_once(recreate_surfaces).unwrap();
        assert!(
            world
                .resource::<ViewTargetAttachments>()
                .contains_key(&target)
        );

        let main_entity = world
            .get::<bevy::render::sync_world::MainEntity>(entity)
            .unwrap()
            .id();
        world
            .resource_mut::<MainWorld>()
            .entity_mut(main_entity)
            .get_mut::<Window>()
            .unwrap()
            .desired_maximum_frame_latency = Some(next);
        world.run_system_once(extract_frame_latency).unwrap();
        world.run_system_once(recreate_surfaces).unwrap();
        let extracted = world.get::<ExtractedWindow>(entity).unwrap();
        assert_eq!(extracted.desired_maximum_frame_latency, Some(next));
        assert!(extracted.swap_chain_texture_view.is_none());
        assert!(
            !world
                .resource::<ViewTargetAttachments>()
                .contains_key(&target)
        );
        assert!(!world.resource::<ChangedFrameLatency>().0);
    }
}
