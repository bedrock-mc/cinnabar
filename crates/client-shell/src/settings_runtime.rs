use client_state::RuntimeSettings;
mod device_render_distance;
pub use device_render_distance::initialize_render_distance;

/// Applies the persisted window mode once per settings revision.
pub fn apply_window_settings(
    settings: bevy::prelude::Res<RuntimeSettings>,
    mut windows: bevy::prelude::Query<
        &mut bevy::window::Window,
        bevy::prelude::With<bevy::window::PrimaryWindow>,
    >,
    mut observed: bevy::prelude::Local<u64>,
) {
    let (generation, user) = settings.user_settings_update();
    if generation <= *observed {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    // A hidden capture window keeps its requested size instead of taking over a display.
    if window.visible {
        window.mode = crate::fullscreen::window_mode(user.video.fullscreen);
    }
    *observed = generation;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        prelude::{App, Update},
        window::{PrimaryWindow, Window, WindowMode},
    };

    #[test]
    fn window_consumes_fullscreen_updates() {
        let mut app = App::new();
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.init_resource::<RuntimeSettings>()
            .add_systems(Update, apply_window_settings);
        let mut user = ui::UserSettings::default();
        user.video.fullscreen = true;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(user.clone());
        app.update();
        assert!(matches!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::BorderlessFullscreen(_)
        ));
        user.video.fullscreen = false;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(user);
        app.update();
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::Windowed
        );
    }
}

pub mod gui_scale;
