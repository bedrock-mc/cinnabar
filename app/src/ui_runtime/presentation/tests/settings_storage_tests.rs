//! Storage presentation reads the actual app installation layout.
use client_ui::test_support::{draw_menu_actions as draw, settings_view as settings};
use launcher::menu::{
    MenuAction, MenuDialog, settings_options::SettingId, settings_storage::StorageAction,
};
use std::sync::Arc;

#[test]
fn settings_storage_has_real_categories_and_confirmed_cache_delete() {
    let player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

    let mut presentation = client_ui::ui_runtime::presentation::UiPresentationRuntime::new(
        client_ui::test_support::pack_harness::font(),
    )
    .unwrap();
    let layout = launcher::test_support::scratch("storage-ui");
    std::fs::create_dir_all(layout.resource_pack_cache_dir()).unwrap();
    std::fs::write(
        layout.resource_pack_cache_dir().join("fixture-pack"),
        [0; 1024],
    )
    .unwrap();
    let mut view = settings();
    Arc::make_mut(&mut view.settings_options).set(SettingId::ScreenAnimations.index(), 0);
    view.settings.section = launcher::menu::settings_storage::SECTION_INDEX;
    view.storage = Arc::new(launcher_host::settings_storage::read_storage(&layout));
    assert_eq!(view.storage.cached[0].bytes, 1024);
    let actions = draw(&player_runtime, &mut presentation, &view);
    for (index, _) in launcher::menu::settings_storage::CATEGORIES
        .iter()
        .enumerate()
    {
        assert!(
            actions.contains(&MenuAction::SettingsStorage(StorageAction::Toggle(
                index as u8
            ))),
            "{actions:?}"
        );
    }
    let category = client_ui::test_support::menu_hit_targets(&presentation)
        .iter()
        .find(|(action, _)| *action == MenuAction::SettingsStorage(StorageAction::Toggle(0)))
        .unwrap()
        .1;
    assert!(
        presentation.scroll_menu(
            ui::UiPoint::new(
                (category.min().x() + category.max().x()) / 2.0,
                (category.min().y() + category.max().y()) / 2.0,
            )
            .unwrap(),
            -4096.0,
            true,
        )
    );
    let actions = draw(&player_runtime, &mut presentation, &view);
    assert!(
        actions.contains(&MenuAction::SettingsStorage(StorageAction::RequestClear)),
        "{actions:?}"
    );
    assert!(
        actions.contains(&MenuAction::SettingsStorage(
            StorageAction::RequestScreenshots
        )),
        "{actions:?}"
    );
    client_ui::test_support::snapshot_menu(&player_runtime, &view, "settings-storage-measured");
    view.dialog = Some(MenuDialog::StorageDelete);
    let actions = draw(&player_runtime, &mut presentation, &view);
    assert!(
        actions.contains(&MenuAction::SettingsStorage(StorageAction::ConfirmDelete)),
        "{actions:?}"
    );
    assert!(actions.contains(&MenuAction::DismissDialog));
    client_ui::test_support::snapshot_menu(&player_runtime, &view, "settings-storage-delete");
    std::fs::remove_dir_all(layout.user_data_root.parent().unwrap()).unwrap();
}

#[test]
fn settings_storage() {
    if ui_engine::test_support::carrier().is_none() {
        return;
    }
    let player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut menu = client_shell::menu::MenuRuntime::new(true, 2, "Steve".to_owned());
    menu.activate(MenuAction::SettingsSection(
        launcher::menu::settings_storage::SECTION_INDEX,
    ));
    let mut view = menu.view();
    let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local");
    view.language_choices =
        launcher::menu::settings_options::SettingsOptions::language_choices(&local);
    view.screen = launcher::menu::MenuScreen::Settings;
    view.settings.section = launcher::menu::settings_storage::SECTION_INDEX;
    client_ui::test_support::snapshot_menu(
        &player_runtime,
        &view,
        "settings-storage_management_forced_index",
    );
}
