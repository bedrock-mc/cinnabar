//! Storage world deletion goes through the existing local-world control channel.

use std::sync::Arc;
use {
    crate::{local_worlds::LocalWorlds, menu::MenuRuntime},
    launcher::{
        local_worlds::model::{Input, Screen},
        menu::{MenuDialog, MenuScreen},
    },
};

impl MenuRuntime {
    /// Resolves a storage directory against the core's catalog instead of deleting files directly.
    pub(in crate::menu::settings_storage) fn request_storage_world_delete(&mut self) {
        let id = self
            .storage
            .selected_world
            .and_then(|index| self.storage.worlds.get(index))
            .and_then(|item| item.path.file_name())
            .map(|id| id.to_string_lossy().into_owned());
        Arc::make_mut(&mut self.storage).world_request = id;
    }

    /// Loads world names from the core catalog and forwards a pending delete into its native prompt.
    pub(in crate::menu) fn sync_storage_worlds(&mut self, worlds: &mut LocalWorlds) {
        let changed = self.storage.worlds.iter().any(|item| {
            worlds.menu().worlds().iter().any(|world| {
                item.path
                    .file_name()
                    .is_some_and(|id| id == world.id.as_str())
                    && (item.name != world.name
                        || item.game_type
                            != launcher::local_worlds::form::game_mode_label(world.game_mode)
                        || item.last_played != world.last_played_unix.max(world.created_unix))
            })
        });
        if changed {
            for item in &mut Arc::make_mut(&mut self.storage).worlds {
                if let Some(world) = worlds.menu().worlds().iter().find(|world| {
                    item.path
                        .file_name()
                        .is_some_and(|id| id == world.id.as_str())
                }) {
                    let card = launcher::menu::worlds_tab::world_card(world);
                    item.name = card.name;
                    item.date = card.date;
                    item.game_type = card.game_mode;
                    item.last_played = world.last_played_unix.max(world.created_unix);
                }
            }
        }
        if self.storage.world_request.is_none() {
            return;
        }
        let Some(id) = Arc::make_mut(&mut self.storage).world_request.take() else {
            return;
        };
        let index = worlds
            .menu()
            .worlds()
            .iter()
            .position(|world| world.id == id);
        if let Some(index) = index.filter(|_| worlds.menu().screen() == Screen::List) {
            worlds.input(Input::BeginEdit(index));
            worlds.input(Input::RequestDelete);
            Arc::make_mut(&mut self.storage).return_from_world = true;
            self.screen = MenuScreen::Play;
        } else {
            Arc::make_mut(&mut self.storage).error = Some("This world is not available in the local world catalog. Reopen Storage after the world list loads.".into());
            self.dialog = Some(MenuDialog::StorageError);
        }
    }

    /// Returns to Storage once the core's edit/delete flow returns to its list.
    pub(in crate::menu) fn finish_storage_world(&mut self, screen: Screen) {
        if self.storage.return_from_world && screen == Screen::List {
            self.screen = MenuScreen::Settings;
            self.refresh_storage();
        }
    }
}
