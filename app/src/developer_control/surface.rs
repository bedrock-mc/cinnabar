//! Hidden developer clients keep input windows but render only to the capture image.

use bevy::{
    prelude::*,
    render::{
        Extract, ExtractSchedule, RenderApp, camera::extract_cameras, sync_world::RenderEntity,
    },
    window::RawHandleWrapper,
};

/// Prevents native surface creation while the existing offscreen camera owns hidden output.
pub(super) fn configure(app: &mut App, hidden: bool) {
    if hidden && let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        // Window extraction queues handles before cameras. Queue their removal after that
        // producer so the extraction flush exposes none to any native surface consumer.
        render_app.add_systems(
            ExtractSchedule,
            suppress_surface_handles.after(extract_cameras),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        app::SubApp,
        render::{MainWorld, view::window::ExtractedWindow},
        window::WindowWrapper,
    };
    use wgpu::rwh::{self, HasDisplayHandle, HasWindowHandle};

    struct InputWindow;

    impl HasWindowHandle for InputWindow {
        fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
            // SAFETY: This integer-only web fixture is never passed to a native API.
            Ok(unsafe { rwh::WindowHandle::borrow_raw(rwh::WebWindowHandle::new(1).into()) })
        }
    }

    impl HasDisplayHandle for InputWindow {
        fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
            Ok(rwh::DisplayHandle::web())
        }
    }

    /// Hidden extraction must never offer a native drawable, including on later frames.
    #[test]
    fn hidden_output_keeps_input_handles_without_creating_native_surfaces() {
        for hidden in [false, true] {
            let mut app = App::new();
            app.insert_sub_app(RenderApp, SubApp::new());
            let handle = RawHandleWrapper::new(&WindowWrapper::new(InputWindow)).unwrap();
            let extracted = app.sub_app_mut(RenderApp).world_mut().spawn_empty().id();
            let mut main = MainWorld::default();
            let input = main
                .spawn((Window::default(), handle, RenderEntity::from(extracted)))
                .id();
            app.sub_app_mut(RenderApp).insert_resource(main);
            let mut extraction = Schedule::new(ExtractSchedule);
            extraction.set_apply_final_deferred(false);
            app.sub_app_mut(RenderApp).add_schedule(extraction);
            app.sub_app_mut(RenderApp).add_systems(
                ExtractSchedule,
                queue_window_handles.before(extract_cameras),
            );
            configure(&mut app, hidden);
            for _ in 0..2 {
                let render = app.sub_app_mut(RenderApp).world_mut();
                render.run_schedule(ExtractSchedule);
                // Consumers must see safe output at the deferred extraction flush,
                // before either capability probing or window surface creation.
                render.resource_scope(|world, mut schedules: Mut<Schedules>| {
                    schedules
                        .get_mut(ExtractSchedule)
                        .unwrap()
                        .apply_deferred(world);
                });
                assert_eq!(
                    render.entity(extracted).contains::<RawHandleWrapper>(),
                    !hidden
                );
                assert!(
                    render
                        .resource::<MainWorld>()
                        .entity(input)
                        .contains::<RawHandleWrapper>()
                );
            }
        }
    }

    /// Reproduces Bevy's deferred handle publication before camera extraction.
    fn queue_window_handles(
        mut commands: Commands,
        windows: Extract<Query<(RenderEntity, &RawHandleWrapper), With<Window>>>,
    ) {
        for (entity, handle) in &windows {
            commands.entity(entity).insert((
                handle.clone(),
                ExtractedWindow {
                    physical_width: 1280,
                    physical_height: 800,
                    present_mode: bevy::window::PresentMode::Immediate,
                    desired_maximum_frame_latency: None,
                    swap_chain_texture_view: None,
                    swap_chain_texture: None,
                    swap_chain_texture_format: None,
                    swap_chain_texture_view_format: None,
                    size_changed: false,
                    present_mode_changed: false,
                    alpha_mode: Default::default(),
                    needs_initial_present: true,
                },
            ));
        }
    }
}

/// Leaves main-world window handles available to winit and strips only their render copies.
fn suppress_surface_handles(
    mut commands: Commands,
    windows: Extract<Query<RenderEntity, With<Window>>>,
) {
    for window in &windows {
        commands.entity(window).remove::<RawHandleWrapper>();
    }
}
