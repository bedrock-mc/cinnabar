use bevy::{
    input::keyboard::{Key, NativeKey},
    prelude::{App, IntoScheduleConfigs, KeyCode, MinimalPlugins, Update},
};

use super::*;
use crate::present_mode::{PresentModeRuntime, apply_present_mode};
use launcher::menu::MenuAction;

fn app(visible: bool) -> (App, Entity) {
    let menu = MenuRuntime::new(visible, 2, "Steve".to_owned());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .init_resource::<RuntimeSettings>()
        .insert_resource(menu)
        .add_systems(
            Update,
            (toggle_fullscreen_hotkey, apply_runtime_fullscreen_setting).chain(),
        );
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    (app, window)
}

fn key(app: &mut App, window: Entity, state: ButtonState, repeat: bool) {
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::F11,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat,
        window,
    });
    app.update();
}

fn assert_fullscreen(app: &App, window: Entity, fullscreen: bool) {
    assert_eq!(
        is_fullscreen(app.world().get::<Window>(window).unwrap()),
        fullscreen
    );
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().fullscreen,
        fullscreen
    );
    assert_eq!(
        app.world()
            .resource::<RuntimeSettings>()
            .user_settings_update()
            .1
            .video
            .fullscreen,
        fullscreen,
    );
}

#[test]
fn video_checkbox_enters_and_leaves_fullscreen() {
    let (mut app, window) = app(true);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsFullscreen(true));
    app.update();
    assert_fullscreen(&app, window, true);
    assert_eq!(
        app.world().get::<Window>(window).unwrap().mode,
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    );

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsFullscreen(false));
    app.update();
    assert_fullscreen(&app, window, false);
}

#[test]
fn f11_toggles_from_menus_and_gameplay_without_repeating_while_held() {
    for visible in [true, false] {
        let (mut app, window) = app(visible);
        key(&mut app, window, ButtonState::Pressed, false);
        assert_fullscreen(&app, window, true);
        let generation = app
            .world()
            .resource::<RuntimeSettings>()
            .user_settings_update()
            .0;
        key(&mut app, window, ButtonState::Pressed, true);
        key(&mut app, window, ButtonState::Released, false);
        app.update();
        assert_fullscreen(&app, window, true);
        assert_eq!(
            app.world()
                .resource::<RuntimeSettings>()
                .user_settings_update()
                .0,
            generation
        );
        key(&mut app, window, ButtonState::Pressed, false);
        assert_fullscreen(&app, window, false);
    }
}

#[test]
fn unfocused_and_other_window_keys_do_not_toggle() {
    let (mut app, window) = app(true);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    key(&mut app, window, ButtonState::Pressed, false);
    assert_fullscreen(&app, window, false);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    let other = app.world_mut().spawn_empty().id();
    key(&mut app, other, ButtonState::Pressed, false);
    assert_fullscreen(&app, window, false);
}

#[test]
fn retained_video_settings_apply_and_hotkeys_preserve_other_settings() {
    let (mut app, window) = app(false);
    let mut settings = ui::UserSettings::default();
    settings.video.horizontal_fov_degrees = 75.0;
    settings.video.fullscreen = true;
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(settings);
    app.update();
    assert_fullscreen(&app, window, true);
    key(&mut app, window, ButtonState::Pressed, false);
    assert_fullscreen(&app, window, false);
    assert_eq!(
        app.world()
            .resource::<RuntimeSettings>()
            .user_settings_update()
            .1
            .video
            .horizontal_fov_degrees,
        75.0
    );
}

#[test]
fn fullscreen_changes_preserve_the_automatic_vsync_policy() {
    for from_hotkey in [true, false] {
        let (mut app, window) = app(true);
        let initial_mode = app.world().get::<Window>(window).unwrap().present_mode;
        app.insert_resource(PresentModeRuntime::from_startup(false, false, false, false))
            .add_systems(
                Update,
                apply_present_mode.after(apply_runtime_fullscreen_setting),
            );
        if from_hotkey {
            key(&mut app, window, ButtonState::Pressed, false);
        } else {
            app.world_mut()
                .resource_mut::<MenuRuntime>()
                .activate(MenuAction::SettingsFullscreen(true));
            app.update();
        }
        assert_fullscreen(&app, window, true);
        assert_eq!(
            app.world()
                .resource::<PresentModeRuntime>()
                .policy()
                .preference(),
            render::PresentModePreference::Auto,
        );
        assert_eq!(
            app.world().get::<Window>(window).unwrap().present_mode,
            initial_mode
        );
        assert_eq!(
            app.world()
                .resource::<RuntimeSettings>()
                .user_settings_update()
                .0,
            0
        );
    }
}

#[test]
fn menu_press_waits_for_a_primary_window() {
    let (mut app, window) = app(true);
    app.world_mut().despawn(window);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsFullscreen(true));
    app.update();
    let replacement = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    app.update();
    assert_fullscreen(&app, replacement, true);
}

#[test]
fn a_hidden_capture_window_ignores_the_saved_fullscreen_setting() {
    let layout = launcher::test_support::scratch("hidden-fullscreen");
    let skin = crate::player_skin::LocalPlayerSkin::generated_default("Hidden");
    let menu = MenuRuntime::new_with_layout(false, Some(2), "Hidden".into(), layout, skin);
    let mut settings = RuntimeSettings::default();
    let mut user = settings.user_settings_update().1.clone();
    user.video.fullscreen = true;
    settings.replace_user_settings(user);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .insert_resource(settings)
        .insert_resource(menu)
        .add_systems(
            Update,
            (toggle_fullscreen_hotkey, apply_runtime_fullscreen_setting).chain(),
        );
    let window = app
        .world_mut()
        .spawn((
            Window {
                visible: false,
                ..Window::default()
            },
            PrimaryWindow,
        ))
        .id();
    app.update();
    app.update();
    assert_eq!(
        app.world().get::<Window>(window).unwrap().mode,
        WindowMode::Windowed
    );
    assert!(
        app.world()
            .resource::<RuntimeSettings>()
            .user_settings_update()
            .1
            .video
            .fullscreen,
        "the player's saved preference is left alone"
    );
}
