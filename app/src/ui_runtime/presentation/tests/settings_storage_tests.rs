//! Storage presentation reads the actual app installation layout.
use crate::menu::{MenuAction, MenuDialog, settings_storage::StorageAction};
use client_ui::test_support::{draw_menu_actions as draw, settings_view as settings};
use std::sync::Arc;

#[test]
fn settings_storage_has_real_categories_and_confirmed_cache_delete() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping settings_storage_has_real_categories_and_confirmed_cache_delete: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let layout = crate::install_layout::scratch("storage-ui");
    std::fs::create_dir_all(layout.resource_pack_cache_dir()).unwrap();
    std::fs::write(
        layout.resource_pack_cache_dir().join("fixture-pack"),
        [0; 1024],
    )
    .unwrap();
    let mut view = settings();
    view.settings_section = crate::menu::settings_storage::SECTION_INDEX;
    view.storage = Arc::new(crate::menu::settings_storage::read_storage(&layout));
    assert_eq!(view.storage.cached[0].bytes, 1024);
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
    if super::super::forms::pack_harness::carrier().is_none() {
        return;
    }
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Steve".to_owned());
    menu.activate(MenuAction::SettingsSection(
        crate::menu::settings_storage::SECTION_INDEX,
    ));
    let mut view = menu.view();
    let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local");
    view.language_choices =
        crate::menu::settings_options::SettingsOptions::language_choices(&local);
    view.screen = crate::menu::MenuScreen::Settings;
    view.settings_section = crate::menu::settings_storage::SECTION_INDEX;
    client_ui::test_support::snapshot_menu(
        &player_runtime,
        &view,
        "settings-storage_management_forced_index",
    );
}
