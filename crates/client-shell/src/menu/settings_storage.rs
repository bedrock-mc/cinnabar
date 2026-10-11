//! Routes Storage menu actions to the host and local-world controller.

use super::MenuRuntime;
use launcher::menu::{MenuDialog, settings_storage::*};
use launcher_host::settings_storage::{read_storage, remove_entry};
use std::sync::Arc;
mod worlds;

impl MenuRuntime {
    /// Refreshes disk measurements only on navigation, never during a draw.
    pub(super) fn refresh_storage(&mut self) {
        self.storage = Arc::new(read_storage(&self.layout));
        if self.storage.error.is_some() {
            self.dialog = Some(MenuDialog::StorageError);
        }
    }

    /// Handles cache selection and confirms destructive cache operations.
    pub(super) fn activate_storage(&mut self, action: StorageAction) {
        match action {
            StorageAction::Toggle(index) => {
                if let Some(expanded) = Arc::make_mut(&mut self.storage)
                    .expanded
                    .get_mut(usize::from(index))
                {
                    *expanded = !*expanded;
                }
            }
            StorageAction::SelectWorld(index) => {
                let view = Arc::make_mut(&mut self.storage);
                view.selected_world = (view.selected_world != Some(index)
                    && index < view.worlds.len())
                .then_some(index);
                view.selected = None;
            }
            StorageAction::Select(index) => {
                let view = Arc::make_mut(&mut self.storage);
                view.selected_world = None;
                view.selected =
                    (view.selected != Some(index) && index < view.cached.len()).then_some(index);
            }
            StorageAction::RequestClear
            | StorageAction::RequestDelete
            | StorageAction::RequestScreenshots => {
                if self.over_world() || self.is_connecting() || self.session.owns_directory {
                    Arc::make_mut(&mut self.storage).error =
                        Some("Leave the world before clearing downloaded packs.".into());
                    self.dialog = Some(MenuDialog::StorageError);
                    return;
                }
                if action == StorageAction::RequestDelete && self.storage.selected_world.is_some() {
                    self.request_storage_world_delete();
                    return;
                }
                let (root, paths) = match action {
                    StorageAction::RequestDelete => {
                        let Some(item) = self
                            .storage
                            .selected
                            .and_then(|index| self.storage.cached.get(index))
                        else {
                            return;
                        };
                        (
                            self.layout.resource_pack_cache_dir(),
                            vec![item.path.clone()],
                        )
                    }
                    StorageAction::RequestScreenshots => (
                        self.layout.screenshots_dir(),
                        self.storage
                            .screenshots
                            .iter()
                            .map(|item| item.path.clone())
                            .collect(),
                    ),
                    _ => (
                        self.layout.resource_pack_cache_dir(),
                        self.storage
                            .cached
                            .iter()
                            .map(|item| item.path.clone())
                            .collect(),
                    ),
                };
                Arc::make_mut(&mut self.storage).pending_delete = Some((root, paths));
                Arc::make_mut(&mut self.storage).deleting_screenshots =
                    action == StorageAction::RequestScreenshots;
                if action != StorageAction::RequestDelete {
                    Arc::make_mut(&mut self.storage).selected = None;
                }
                self.dialog = Some(MenuDialog::StorageDelete);
            }
            StorageAction::ConfirmDelete => {
                if self.dialog != Some(MenuDialog::StorageDelete) {
                    return;
                }
                self.dialog = None;
                if self.over_world() || self.is_connecting() || self.session.owns_directory {
                    return;
                }
                let Some((root, paths)) = Arc::make_mut(&mut self.storage).pending_delete.take()
                else {
                    return;
                };
                let result = paths.iter().try_for_each(|path| remove_entry(&root, path));
                self.refresh_storage();
                if let Err(error) = result {
                    Arc::make_mut(&mut self.storage).error =
                        Some(format!("Could not delete stored content: {error}"));
                    self.dialog = Some(MenuDialog::StorageError);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "settings_storage_tests.rs"]
mod tests;
