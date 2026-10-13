use crate::menu::MenuRuntime;
use launcher::menu::{
    MenuAction,
    settings_options::{SETTINGS_FILE, SettingsOptions, video_presets::VideoPreset},
};

#[test]
fn preset_action_replaces_controlled_session_overrides_and_saves_the_applied_values() {
    let layout = launcher::test_support::scratch("preset-action");
    let path = layout.server_file().with_file_name(SETTINGS_FILE);
    let skin = crate::player_skin::LocalPlayerSkin::generated_default("Presets");
    let mut menu = MenuRuntime::new_with_layout(true, Some(2), "Presets".into(), layout, skin);
    menu.set_named_option("gamma", 70);
    menu.sync_user_settings(None);
    menu.settings_apply = false;
    menu.set_session_option("msaa", Some(8));
    menu.set_session_option("hide_hud", Some(1));
    menu.activate(MenuAction::SettingsVideoPreset(VideoPreset::Quality));
    assert!(menu.settings_dirty);
    assert!(menu.settings_apply);
    assert_eq!(
        menu.settings_options.video_preset(),
        Some(VideoPreset::Quality)
    );
    assert_eq!(menu.settings_options.value("gamma"), 70);
    assert_eq!(menu.gui_scale_preference(), Some(2));
    menu.sync_user_settings(None);
    let loaded = SettingsOptions::load(&path);
    assert_eq!(loaded.video_preset(), Some(VideoPreset::Quality));
    assert_eq!(loaded.value("gamma"), 70);
    assert_eq!(loaded.value("hide_hud"), 0);
    menu.set_session_option("msaa", None);
    assert_eq!(
        menu.settings_options.video_preset(),
        Some(VideoPreset::Quality)
    );
    menu.set_named_option("max_framerate", 60);
    assert_eq!(menu.settings_options.video_preset(), None);
    menu.activate(MenuAction::SettingsVideoPreset(VideoPreset::Balanced));
    assert_eq!(
        menu.settings_options.video_preset(),
        Some(VideoPreset::Balanced)
    );
    assert_eq!(menu.settings_options.value("hide_hud"), 1);
    menu.sync_user_settings(None);
    assert_eq!(
        SettingsOptions::load(&path).video_preset(),
        Some(VideoPreset::Balanced)
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn reapplying_a_preset_replaces_an_override_even_when_effective_values_already_match() {
    let mut menu = MenuRuntime::new(true, 2, "Presets".into());
    menu.activate(MenuAction::SettingsVideoPreset(VideoPreset::Performance));
    menu.set_named_option("msaa", 2);
    menu.set_session_option("msaa", Some(1));
    assert_eq!(
        menu.settings_options.video_preset(),
        Some(VideoPreset::Performance)
    );
    menu.settings_dirty = false;
    menu.settings_apply = false;
    menu.activate(MenuAction::SettingsVideoPreset(VideoPreset::Performance));
    assert!(menu.settings_dirty);
    assert!(menu.settings_apply);
    menu.set_session_option("msaa", None);
    assert_eq!(
        menu.settings_options.video_preset(),
        Some(VideoPreset::Performance)
    );
}
