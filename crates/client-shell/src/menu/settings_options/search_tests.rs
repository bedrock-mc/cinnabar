//! Settings search drafts remain transient and follow ordinary text input.

use crate::menu::MenuRuntime;
use launcher::menu::{MenuAction, MenuField, MenuScreen, settings_sections::SettingSection};

#[test]
fn query_edits_clear_and_category_selection_preserve_saved_options() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    let saved = menu.view().settings_options;
    menu.activate(MenuAction::SearchSettings);
    menu.edit_text("volume");
    assert_eq!(menu.view().settings_query, "volume");
    assert_eq!(menu.view().field, Some(MenuField::SettingsQuery));
    menu.activate(MenuAction::ClearSettingsSearch);
    assert!(menu.view().settings_query.is_empty());
    assert_eq!(menu.view().field, Some(MenuField::SettingsQuery));
    menu.edit_text("language");
    let section = SettingSection::Language.definition().index;
    menu.activate(MenuAction::SettingsSearchSection(section));
    assert_eq!(menu.view().settings.section, section);
    assert!(menu.view().settings_query.is_empty());
    assert_eq!(menu.view().settings_options, saved);
    assert!(!menu.settings_dirty);
}

#[test]
fn escape_clears_search_before_leaving_settings_and_reopening_starts_empty() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SearchSettings);
    menu.edit_text("vsync");
    menu.go_back_from_input();
    assert_eq!(menu.view().screen, MenuScreen::Settings);
    assert!(menu.view().settings_query.is_empty());
    menu.edit_text("gamma");
    menu.activate(MenuAction::AddBack);
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    assert!(menu.view().settings_query.is_empty());
}
