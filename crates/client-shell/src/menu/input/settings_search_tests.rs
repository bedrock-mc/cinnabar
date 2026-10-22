//! Search editing yields keyboard input when pointer focus moves to a result control.

use super::*;
use launcher::menu::{MenuAction, MenuScreen, settings_options::SettingId};

/// Mouse and touch result selection preserve the query while transferring input ownership.
#[test]
fn search_result_slider_releases_text_input_before_keyboard_adjustment() {
    for mode in [MenuInputMode::Mouse, MenuInputMode::Touch] {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
        menu.activate(MenuAction::SearchSettings);
        menu.edit_text("volume");
        let id = SettingId::MainVolume;
        let index = id.index() as u16;
        menu.set_option(index, 50);
        let action = MenuAction::SettingsOption(index, 50);
        menu.refresh_settings_focus([MenuAction::SearchSettings, action]);
        menu.input_mode = mode;
        menu.focus_pointer(action);
        assert_eq!(menu.view().field, None);
        menu.edit_text("unexpected");
        assert_eq!(menu.view().settings_query, "volume");
        menu.input_mode = MenuInputMode::Keyboard;
        menu.activate_focused();
        assert_eq!(menu.view().settings.slider_selected, Some(index));
        assert!(!menu.edit_key(KeyCode::ArrowRight, false, false));
        menu.move_horizontal_focus(1);
        let option = id.definition();
        assert_eq!(
            menu.settings_options.value(id),
            (50 + option.step).min(option.max)
        );
        assert_eq!(menu.view().settings_query, "volume");
    }
}
