//! Opt-in component spike. The default client registers no extension runtime.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use bevy::{prelude::*, window::PrimaryWindow};
use mod_host::{ModGrants, ModHost};

use crate::environment::VisualTimeOverride;

use crate::{
    app::ClientFrameSet,
    menu::MenuRuntime,
    ui_runtime::{UiRuntime, presentation::UiPresentationRuntime},
};

const COMPONENT_ENV: &str = "CINNABAR_MOD_COMPONENT";
const DEMO_KEY: KeyCode = KeyCode::F8;
const RELOAD_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Resource)]
struct ModRuntime {
    host: ModHost,
    last_reload: Instant,
}

/// Installs the developer extension only when its component path is explicit.
pub(crate) fn configure_from_environment(app: &mut App) {
    let path = std::env::var_os(COMPONENT_ENV);
    configure(app, path.as_deref().map(Path::new));
}

/// Loads one optional component without changing the vanilla schedule on absence.
fn configure(app: &mut App, path: Option<&Path>) {
    let Some(path) = path else { return };
    match ModHost::load_with_grants(path, ModGrants { environment: true }) {
        Ok(host) => {
            app.insert_resource(VisualTimeOverride(host.time_override()))
                .insert_resource(ModRuntime {
                    host,
                    last_reload: Instant::now(),
                })
                .add_systems(
                    Update,
                    drive_mod
                        .after(ClientFrameSet::SemanticFinalize)
                        .before(ClientFrameSet::UiPublication)
                        .before(crate::environment::update_atmosphere_frame),
                );
        }
        Err(error) => eprintln!("Cinnabar extension {} disabled: {error:#}", path.display()),
    }
}

/// Runs the bounded guest and publishes only its validated presentation output.
fn drive_mod(
    mut extension: ResMut<ModRuntime>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ui: Res<UiRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut time_override: ResMut<VisualTimeOverride>,
) {
    if extension.last_reload.elapsed() >= RELOAD_INTERVAL {
        extension.last_reload = Instant::now();
        if let Err(error) = extension.host.reload_if_changed() {
            eprintln!("Cinnabar extension reload rejected: {error:#}");
        }
    }
    let focused = windows.single().is_ok_and(|window| window.focused);
    let pressed = keybind_allowed(
        focused,
        crate::screen_policy::absorbs_input(Some(&ui), menu.as_deref(), Some(&presentation)),
    ) && keys.just_pressed(DEMO_KEY);
    if extension.host.is_active()
        && let Err(error) = extension.host.frame(pressed)
    {
        eprintln!("Cinnabar extension callback disabled: {error:#}");
    }
    time_override.0 = extension.host.time_override();
    if let Err(error) = presentation.set_mod_label(extension.host.label()) {
        eprintln!("Cinnabar extension HUD rejected: {error}");
    }
}

/// A mod keybind is unavailable while another UI or an unfocused window owns input.
fn keybind_allowed(window_focused: bool, input_absorbed: bool) -> bool {
    window_focused && !input_absorbed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_mod_path_registers_no_runtime_or_systems() {
        let mut app = App::new();
        configure(&mut app, None);
        assert!(!app.world().contains_resource::<ModRuntime>());
        assert!(!app.world().contains_resource::<VisualTimeOverride>());
        // Any extension system would fail here: none of its required resources exist.
        app.update();
    }

    #[test]
    fn keybind_respects_existing_input_authority() {
        assert!(keybind_allowed(true, false));
        assert!(!keybind_allowed(false, false));
        assert!(!keybind_allowed(true, true));
    }

    #[test]
    #[ignore = "requires CINNABAR_MOD_COMPONENT and installed UI carrier (make assets)"]
    fn configured_sample_drives_the_app_adapter_offline() {
        std::env::var_os(COMPONENT_ENV)
            .expect("CINNABAR_MOD_COMPONENT must name a compiled sample");
        let presentation =
            crate::ui_runtime::presentation::forms::pack_harness::engine_presentation()
                .expect("real UI carrier required for the selected sample");
        let mut app = App::new();
        app.insert_resource(presentation)
            .insert_resource(UiRuntime::new(1))
            .insert_resource(ButtonInput::<KeyCode>::default());
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let vanilla = sample_frame(&mut app);
        configure_from_environment(&mut app);
        assert!(app.world().contains_resource::<ModRuntime>());
        app.update();
        let initial = sample_frame(&mut app);
        assert_ne!(vanilla, initial);

        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(DEMO_KEY);
        app.update();
        assert_eq!(initial, sample_frame(&mut app));
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert_ne!(initial, sample_frame(&mut app));
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(DEMO_KEY)
        );
    }

    /// Renders the adapter's retained state without a window or network session.
    fn sample_frame(app: &mut App) -> render::UiRenderInput {
        app.world_mut()
            .resource_mut::<UiPresentationRuntime>()
            .build(
                &UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    }
}

#[cfg(test)]
mod time_changer_tests;
