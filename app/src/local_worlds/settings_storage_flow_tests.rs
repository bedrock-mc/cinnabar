//! Storage world rows must retain the core's world identity and deletion workflow.

use super::*;
use crate::menu::{
    LocalWorldAction, MenuAction, MenuRuntime, MenuScreen,
    settings_storage::{SECTION_INDEX, StorageAction},
};

#[test]
fn storage_world_delete_uses_catalog_identity_and_returns_to_settings() {
    use protocol::world_control::{Backend, Difficulty, GameMode, Generator, World};
    let layout = crate::install_layout::scratch("storage-world-flow");
    std::fs::create_dir_all(layout.local_worlds_dir().join("world-id")).unwrap();
    let player_skin = crate::player_skin::LocalPlayerSkin::generated_default("Steve");
    let mut menu =
        MenuRuntime::new_with_layout(true, Some(2), "Steve".into(), layout.clone(), player_skin);
    let mut worlds = LocalWorlds::default();
    worlds.menu.apply(Event::Listed(vec![World {
        id: "world-id".into(),
        name: "My Saved World".into(),
        game_mode: GameMode::Survival,
        generator: Generator::Flat,
        difficulty: Difficulty::Normal,
        allow_cheats: false,
        backend: Backend::Dragonfly,
        seed: 1,
        created_unix: 0,
        last_played_unix: 0,
        size_bytes: 0,
    }]));
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(SECTION_INDEX));
    menu.sync_local_worlds(&mut worlds, false);
    assert_eq!(menu.view().storage.worlds[0].name, "My Saved World");
    menu.activate(MenuAction::SettingsStorage(StorageAction::SelectWorld(0)));
    menu.activate(MenuAction::SettingsStorage(StorageAction::RequestDelete));
    menu.sync_local_worlds(&mut worlds, false);
    assert_eq!(worlds.menu().screen(), Screen::ConfirmDelete);
    assert_eq!(worlds.menu().selected().unwrap().id, "world-id");
    assert!(
        layout.local_worlds_dir().join("world-id").is_dir(),
        "request never removes a world directory"
    );
    menu.activate(MenuAction::LocalWorld(LocalWorldAction::Back));
    menu.sync_local_worlds(&mut worlds, false);
    assert_eq!(worlds.menu().screen(), Screen::Edit);
    menu.activate(MenuAction::LocalWorld(LocalWorldAction::Back));
    menu.sync_local_worlds(&mut worlds, false);
    assert_eq!(menu.view().screen, MenuScreen::Settings);
    assert_eq!(menu.view().settings_section, SECTION_INDEX);
    std::fs::remove_dir_all(layout.user_data_root.parent().unwrap()).unwrap();
}
