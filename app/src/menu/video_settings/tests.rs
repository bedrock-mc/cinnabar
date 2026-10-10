use launcher_host::video_settings::{FILE_NAME, MAX_FILE_BYTES, load};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use bevy::prelude::{App, IntoScheduleConfigs, MinimalPlugins, Update};

use super::*;
use {
    crate::{
        fullscreen::{apply_runtime_fullscreen_setting, toggle_fullscreen_hotkey},
        settings_runtime::RuntimeSettings,
    },
    launcher::menu::MenuAction,
};

struct ConfigRoot(PathBuf);

impl ConfigRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "cinnabar-video-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn menu(&self) -> MenuRuntime {
        let mut layout = launcher::test_support::checkout();
        layout.user_config_root = self.0.clone();
        let skin = crate::player_skin::LocalPlayerSkin::generated_default("Steve");
        MenuRuntime::new_with_layout(true, Some(2), "Steve".to_owned(), layout, skin)
    }
}

impl Drop for ConfigRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn missing_preferences_default_and_saved_values_reload() {
    let root = ConfigRoot::new();
    assert_eq!(load(&root.0).unwrap(), SavedVideoSettings::default());
    let expected = SavedVideoSettings {
        fullscreen: true,
        gui_scale_offset: -4,
    };
    save(&root.0, expected).unwrap();
    assert_eq!(load(&root.0).unwrap(), expected);
    let menu = root.menu();
    assert!(menu.view().fullscreen);
    assert_eq!(menu.gui_scale_offset(), expected.gui_scale_offset);
    assert!(menu.fullscreen_change.unwrap());
    assert!(
        !root
            .0
            .join(format!("{FILE_NAME}.tmp-{}", std::process::id()))
            .exists()
    );
}

#[test]
fn changed_values_persist_without_saving_viewport_clamps_or_cli_overrides() {
    let root = ConfigRoot::new();
    let expected = SavedVideoSettings {
        fullscreen: false,
        gui_scale_offset: -4,
    };
    save(&root.0, expected).unwrap();
    let mut menu = root.menu();
    menu.set_gui_scale_preference(Some(4));
    menu.sync_gui_scale(
        -1,
        ui::DesktopGuiScale::for_window([1280, 720])
            .choices()
            .collect(),
    );
    let mut app = App::new();
    app.insert_resource(menu)
        .add_systems(Update, persist_video_settings);
    // An unchanged tuple must not cause a write, including after a resize or
    // a fixed capture override. Removing its file makes that observable.
    fs::remove_file(root.0.join(FILE_NAME)).unwrap();
    app.update();
    assert!(!root.0.join(FILE_NAME).exists());
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsFullscreen(true));
    app.update();
    wait_for_save(&mut app);
    let loaded = load(&root.0).unwrap();
    assert!(loaded.fullscreen);
    assert_eq!(loaded.gui_scale_offset, expected.gui_scale_offset);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsScale(-1));
    app.update();
    wait_for_save(&mut app);
    assert_eq!(load(&root.0).unwrap().gui_scale_offset, -1);
}

#[test]
fn a_loaded_fullscreen_preference_applies_to_the_new_window() {
    let root = ConfigRoot::new();
    save(
        &root.0,
        SavedVideoSettings {
            fullscreen: true,
            gui_scale_offset: -2,
        },
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<RuntimeSettings>()
        .insert_resource(root.menu())
        .add_systems(Update, apply_runtime_fullscreen_setting);
    let window = app
        .world_mut()
        .spawn((bevy::window::Window::default(), bevy::window::PrimaryWindow))
        .id();
    app.update();
    assert_eq!(
        app.world()
            .get::<bevy::window::Window>(window)
            .unwrap()
            .mode,
        bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
    );
    assert!(
        app.world()
            .resource::<RuntimeSettings>()
            .user_settings_update()
            .1
            .video
            .fullscreen
    );
    assert_eq!(app.world().resource::<MenuRuntime>().gui_scale_offset(), -2);
}

#[test]
fn f11_can_disable_a_loaded_fullscreen_preference_and_saves_it() {
    use bevy::{
        input::{
            ButtonState,
            keyboard::{Key, KeyboardInput, NativeKey},
        },
        prelude::KeyCode,
    };
    let root = ConfigRoot::new();
    save(
        &root.0,
        SavedVideoSettings {
            fullscreen: true,
            gui_scale_offset: -2,
        },
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .init_resource::<RuntimeSettings>()
        .insert_resource(root.menu())
        .add_systems(
            Update,
            (
                toggle_fullscreen_hotkey,
                apply_runtime_fullscreen_setting,
                persist_video_settings,
            )
                .chain(),
        );
    let window = app
        .world_mut()
        .spawn((bevy::window::Window::default(), bevy::window::PrimaryWindow))
        .id();
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::F11,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
    wait_for_save(&mut app);
    assert!(!load(&root.0).unwrap().fullscreen);
    let restarted = root.menu();
    assert!(!restarted.view().fullscreen);
    assert_eq!(restarted.gui_scale_offset(), -2);
    assert_eq!(
        app.world()
            .get::<bevy::window::Window>(window)
            .unwrap()
            .mode,
        bevy::window::WindowMode::Windowed
    );
}

#[test]
fn malformed_and_oversized_files_return_errors_without_overwriting_them() {
    let root = ConfigRoot::new();
    fs::create_dir_all(&root.0).unwrap();
    let path = root.0.join(FILE_NAME);
    fs::write(&path, br#"{"fullscreen": "yes"}"#).unwrap();
    assert!(load(&root.0).is_err());
    assert!(root.menu().view().message.is_some());
    assert_eq!(fs::read(&path).unwrap(), br#"{"fullscreen": "yes"}"#);
    fs::write(&path, vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap();
    assert!(load(&root.0).is_err());
}

/// Waits for the worker's acknowledgment while driving the normal persistence system.
fn wait_for_save(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        app.update();
        let menu = app.world().resource::<MenuRuntime>();
        let current = SavedVideoSettings {
            fullscreen: menu.fullscreen,
            gui_scale_offset: menu.gui_scale_offset,
        };
        if menu.last_saved_video_settings == current {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "video settings were not acknowledged"
        );
        std::thread::yield_now();
    }
}

#[test]
fn a_pending_video_write_can_return_to_the_last_saved_value() {
    use std::{sync::mpsc, time::Duration};

    let root = ConfigRoot::new();
    let mut menu = root.menu();
    let original = menu.last_saved_video_settings;
    let (saved_tx, saved_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut first = true;
    menu.video_settings_writer = Some(
        writer::Writer::new(move |settings| {
            saved_tx.send(settings).unwrap();
            if first {
                first = false;
                release_rx.recv().unwrap();
            }
            Ok(())
        })
        .unwrap(),
    );
    menu.gui_scale_offset = original.gui_scale_offset + 1;
    let mut app = App::new();
    app.insert_resource(menu)
        .add_systems(Update, persist_video_settings);
    app.update();
    let first = saved_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .gui_scale_offset = original.gui_scale_offset;
    app.update();
    release_tx.send(()).unwrap();
    assert_ne!(first, original);
    assert_eq!(
        saved_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        original
    );
}
