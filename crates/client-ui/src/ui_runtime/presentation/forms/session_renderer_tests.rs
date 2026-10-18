//! The launcher migration leaves the session's existing native screens and JSON popups intact.

use super::super::UiPresentationRuntime;
use crate::ui_runtime::UiRuntime;
use assets::{RuntimeUiAssets, UiFile, encode_ui_catalog};
use launcher::menu::{
    MenuAction, MenuDialog, MenuScreen, MenuView, settings_sections::SettingSection,
};
use std::sync::Arc;

/// Creates a self-contained session carrier whose fallback labels expose an incorrect renderer route.
fn presentation() -> UiPresentationRuntime {
    let mut files = [
        ("ui/_global_variables.json", "{}"),
        ("ui/_ui_defs.json", r#"{"ui_defs":["ui/settings_screen.json","ui/death_screen.json","ui/popup_dialog.json"]}"#),
        ("ui/settings_screen.json", r#"{"namespace":"settings","screen_controls_and_settings":{"type":"screen","controls":[{"marker":{"type":"label","text":"JSON settings fallback","localize":false,"size":[200,20]}}]}}"#),
        ("ui/death_screen.json", r#"{"namespace":"death","death_screen":{"type":"screen","controls":[{"marker":{"type":"label","text":"JSON death fallback","localize":false,"size":[200,20]}}]}}"#),
        ("ui/popup_dialog.json", r#"{"namespace":"popup_dialog","modal_dialog_popup":{"type":"panel","controls":[{"marker":{"type":"label","text":"session popup","localize":false,"size":[200,20]}}]}}"#),
    ].map(|(path, text)| UiFile { path: path.into(), bytes: text.as_bytes().into() });
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let bytes = encode_ui_catalog([1; 32], &[], &[], &[], &files).unwrap();
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation
        .enable_json_ui(Arc::new(RuntimeUiAssets::decode(&bytes).unwrap()))
        .unwrap();
    presentation
}

/// Draws the normal menu route and returns visible labels and active callbacks.
fn paint(presentation: &mut UiPresentationRuntime, view: MenuView) -> (String, Vec<MenuAction>) {
    assert!(!view.out_of_game());
    presentation.set_menu_view(Some(view));
    let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let mut nodes = Vec::new();
    let hits = presentation
        .append_menu(
            &UiRuntime::new(1),
            &mut nodes,
            &mut 1,
            metrics,
            1280.0,
            720.0,
        )
        .unwrap();
    (
        super::pack_harness::drawn_texts(&nodes).join(" "),
        hits.into_iter().map(|(action, _)| action).collect(),
    )
}

#[test]
fn death_keeps_the_native_game_menu_action_and_json_quit_popup() {
    let mut presentation = presentation();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Death;
    view.over_world = true;
    view.death_controls_visible = true;
    let (words, actions) = paint(&mut presentation, view.clone());
    assert!(words.contains("YOU DIED!"), "{words}");
    assert!(!words.contains("JSON death fallback"));
    assert!(actions.contains(&MenuAction::OpenDeathGameMenu));
    assert!(!actions.contains(&MenuAction::OpenDeathQuit));
    view.dialog = Some(MenuDialog::DeathQuit);
    let (words, _) = paint(&mut presentation, view);
    assert!(
        words.contains("YOU DIED!") && words.contains("session popup"),
        "{words}"
    );
}

#[test]
fn settings_from_pause_keep_mods_and_command_macro_editors() {
    let mut presentation = presentation();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.over_world = true;
    view.settings.section = SettingSection::Mods.definition().index;
    view.focused_action = Some(MenuAction::SettingsSection(view.settings.section));
    view.navigation_focus_visible = true;
    let (words, actions) = paint(&mut presentation, view.clone());
    assert!(!words.contains("JSON settings fallback"));
    assert!(actions.contains(&MenuAction::SettingsSection(
        SettingSection::Mods.definition().index
    )));
    view.settings.section = SettingSection::KeyboardAndMouse.definition().index;
    view.focused_action = Some(MenuAction::SettingsMacroCommand(0));
    let (words, actions) = paint(&mut presentation, view.clone());
    assert!(!words.contains("JSON settings fallback"));
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, MenuAction::SettingsMacroCommand(_)))
    );
    view.dialog = Some(MenuDialog::SettingsResetBindings(false));
    let (words, _) = paint(&mut presentation, view);
    assert!(words.contains("session popup"), "{words}");
}
