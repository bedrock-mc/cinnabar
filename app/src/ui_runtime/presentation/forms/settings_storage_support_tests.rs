//! Real-carrier storage and support input checks; PNGs use the offline gallery path.

use super::super::UiPresentationRuntime;
use crate::menu::{
    MenuAction, MenuDialog, MenuRuntime, MenuScreen, MenuView,
    settings_storage::{StorageAction, StorageView},
    settings_support::{SupportAction, SupportDialog, SupportLink},
};
use crate::ui_runtime::UiRuntime;
use std::sync::Arc;

/// Draws the retained menu twice and returns only its active hit actions.
fn draw(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    presentation: &mut UiPresentationRuntime,
    view: &MenuView,
) -> Vec<MenuAction> {
    for _ in 0..2 {
        presentation.set_menu_view(Some(view.clone()));
        presentation
            .build(
                player_runtime,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    presentation
        .menu_hit_targets
        .iter()
        .map(|(action, _)| *action)
        .collect()
}

/// Creates a Settings view without starting a session.
fn settings() -> MenuView {
    let mut view = MenuRuntime::new(true, 2, "Steve".into()).view();
    view.screen = MenuScreen::Settings;
    view
}

#[test]
fn settings_help_uses_rating_prompt_and_licenses_scroll() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping settings_help_uses_rating_prompt_and_licenses_scroll: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut view = settings();
    view.dialog = Some(MenuDialog::SettingsSupport(SupportDialog::Help));
    let actions = draw(&player_runtime, &mut presentation, &view);
    assert!(
        actions.contains(&MenuAction::SettingsSupport(SupportAction::Open(
            SupportLink::Help
        ))),
        "{actions:?}"
    );
    assert!(actions.contains(&MenuAction::DismissDialog));
    assert_eq!(actions.len(), 2, "modal must own input");
    super::play_flow_snapshots::snapshot(&player_runtime, &view, "settings-help-center");
    view.dialog = Some(MenuDialog::SettingsSupport(SupportDialog::FontLicense));
    assert!(
        draw(&player_runtime, &mut presentation, &view)
            .iter()
            .all(|action| *action == MenuAction::DismissDialog)
    );
    assert!(
        presentation.scroll_menu(ui::UiPoint::new(640.0, 360.0).unwrap(), -8.0, false),
        "font license body must scroll"
    );
    assert!(
        presentation
            .menu_scrolls
            .offsets()
            .values()
            .any(|offset| *offset > 0.0)
    );
    super::play_flow_snapshots::snapshot(&player_runtime, &view, "settings-font-license");
}

#[test]
fn settings_storage_has_real_categories_and_confirmed_cache_delete() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping settings_storage_has_real_categories_and_confirmed_cache_delete: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let layout = crate::install_layout::InstallLayout::scratch("storage-ui");
    std::fs::create_dir_all(layout.resource_pack_cache_dir()).unwrap();
    std::fs::write(
        layout.resource_pack_cache_dir().join("fixture-pack"),
        [0; 1024],
    )
    .unwrap();
    let mut view = settings();
    view.settings_section = crate::menu::settings_storage::SECTION_INDEX;
    view.storage = Arc::new(StorageView::read(&layout));
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
    super::play_flow_snapshots::snapshot(&player_runtime, &view, "settings-storage-measured");
    view.dialog = Some(MenuDialog::StorageDelete);
    let actions = draw(&player_runtime, &mut presentation, &view);
    assert!(
        actions.contains(&MenuAction::SettingsStorage(StorageAction::ConfirmDelete)),
        "{actions:?}"
    );
    assert!(actions.contains(&MenuAction::DismissDialog));
    super::play_flow_snapshots::snapshot(&player_runtime, &view, "settings-storage-delete");
    std::fs::remove_dir_all(layout.user_data_root.parent().unwrap()).unwrap();
}
