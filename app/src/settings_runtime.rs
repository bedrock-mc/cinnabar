mod device_render_distance;
mod render_distance;
pub(crate) use device_render_distance::initialize_render_distance;
pub(crate) use render_distance::apply_render_distance;

use bevy::prelude::Resource;
use ui::UserSettings;

/// App-owned retained settings handoff used by menus and live subsystem
/// adapters. Every replacement is complete and monotonically versioned.
#[derive(Resource, Clone, Debug, Default)]
pub struct RuntimeSettings {
    generation: u64,
    user_settings: UserSettings,
}

impl RuntimeSettings {
    /// Write back the fullscreen adapter's already-applied window state without
    /// publishing a complete settings replacement. A window toggle must not
    /// apply unrelated camera or VSync defaults through their generation readers.
    pub(crate) fn set_fullscreen(&mut self, fullscreen: bool) {
        self.user_settings.video.fullscreen = fullscreen;
    }

    pub fn replace_user_settings(&mut self, settings: UserSettings) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.user_settings = settings;
        self.generation
    }

    #[must_use]
    pub const fn user_settings_update(&self) -> (u64, &UserSettings) {
        (self.generation, &self.user_settings)
    }
}

/// Applies the persisted window mode once per settings revision.
pub(crate) fn apply_window_settings(
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
        window.mode = if user.video.fullscreen {
            bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
        } else {
            bevy::window::WindowMode::Windowed
        };
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
