//! Hidden developer clients keep input windows but render only to the capture image.

use bevy::{
    prelude::*,
    render::{
        Render, RenderApp,
        view::window::{ExtractedWindow, create_surfaces},
    },
    window::RawHandleWrapper,
};

/// Prevents native surface creation while the existing offscreen camera owns hidden output.
pub(super) fn configure(app: &mut App, hidden: bool) {
    if hidden && let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.add_systems(Render, suppress_surface_handles.before(create_surfaces));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{app::SubApp, window::WindowWrapper};
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
            let input = app.world_mut().spawn(handle.clone()).id();
            let extracted = app
                .sub_app_mut(RenderApp)
                .world_mut()
                .spawn(ExtractedWindow {
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
                })
                .id();
            configure(&mut app, hidden);
            if !hidden {
                app.sub_app_mut(RenderApp)
                    .add_schedule(Schedule::new(Render));
            }
            for _ in 0..2 {
                let render = app.sub_app_mut(RenderApp).world_mut();
                render.entity_mut(extracted).insert(handle.clone());
                render.run_schedule(Render);
                assert_eq!(
                    render.entity(extracted).contains::<RawHandleWrapper>(),
                    !hidden
                );
                assert!(app.world().entity(input).contains::<RawHandleWrapper>());
            }
        }
    }
}

/// Leaves main-world window handles available to winit and strips only their render copies.
fn suppress_surface_handles(
    mut commands: Commands,
    windows: Query<Entity, (With<ExtractedWindow>, With<RawHandleWrapper>)>,
) {
    for window in &windows {
        commands.entity(window).remove::<RawHandleWrapper>();
    }
}
