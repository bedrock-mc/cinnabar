//! Storage inventory reads the installed data roots; deletion never follows links.

use std::{fs, io, path::Path, sync::Arc};

mod worlds;

use super::{MenuDialog, MenuRuntime};
use crate::install_layout::InstallLayout;

pub(crate) use launcher::menu::settings_storage::*;

/// Measures real world and downloaded-pack data once when Storage opens.
pub(crate) fn read_storage(layout: &InstallLayout) -> StorageView {
    let mut view = StorageView::default();
    match entries(&layout.local_worlds_dir()) {
        Ok(items) => view.worlds = items,
        Err(error) => view.error = Some(error.to_string()),
    }
    match entries(&layout.resource_pack_cache_dir()) {
        Ok(items) => view.cached = items,
        Err(error) => view.error = Some(error.to_string()),
    }
    match entries(&layout.screenshots_dir()) {
        Ok(items) => {
            view.screenshots = items
                .into_iter()
                .filter(|item| {
                    item.path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
                        && fs::symlink_metadata(&item.path).is_ok_and(|metadata| metadata.is_file())
                })
                .collect()
        }
        Err(error) => view.error = Some(error.to_string()),
    }
    view
}

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

/// Counts ordinary entries without traversing links outside the requested directory.
fn entries(root: &Path) -> io::Result<Vec<StorageItem>> {
    let reader = match fs::read_dir(root) {
        Ok(reader) => reader,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut items = Vec::new();
    for entry in reader {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        items.push(StorageItem {
            name: entry.file_name().to_string_lossy().into_owned(),
            bytes: tree_bytes(&entry.path())?,
            path: entry.path(),
            ..Default::default()
        });
    }
    items.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(items)
}

/// Measures file contents recursively while excluding symlink targets.
fn tree_bytes(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Ok(0);
    }
    if !metadata.is_dir() {
        return Ok(metadata.len());
    }
    fs::read_dir(path)?.try_fold(0_u64, |total, entry| {
        Ok(total.saturating_add(tree_bytes(&entry?.path())?))
    })
}

/// Removes one inventoried cache entry, refusing linked roots and path escapes.
fn remove_entry(root: &Path, path: &Path) -> io::Result<()> {
    if path.parent() != Some(root)
        || root
            .ancestors()
            .any(|parent| fs::symlink_metadata(parent).is_ok_and(|metadata| metadata.is_symlink()))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cache path is linked or outside its data directory",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cache entry is a symbolic link",
        ));
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
#[path = "settings_storage_tests.rs"]
mod tests;
