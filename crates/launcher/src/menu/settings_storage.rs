//! Storage screen state and commands; disk work belongs to the host.
use std::path::PathBuf;

use crate::install_layout::InstallLayout;

pub const SECTION_INDEX: u8 = super::settings_sections::SettingSection::StorageManagement
    .definition()
    .index;
pub const CATEGORIES: [&str; 6] = [
    "world",
    "world_template",
    "resource",
    "behavior",
    "skin",
    "cache",
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StorageItem {
    pub name: String,
    pub bytes: u64,
    pub date: String,
    pub game_type: String,
    pub last_played: i64,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StorageView {
    pub worlds: Vec<StorageItem>,
    pub cached: Vec<StorageItem>,
    pub screenshots: Vec<StorageItem>,
    pub deleting_screenshots: bool,
    pub expanded: [bool; CATEGORIES.len()],
    pub selected: Option<usize>,
    pub selected_world: Option<usize>,
    pub world_request: Option<String>,
    pub pending_delete: Option<(PathBuf, Vec<PathBuf>)>,
    pub return_from_world: bool,
    pub error: Option<String>,
    pub opens_folders: bool, // the host can show a `StorageFolder` in a file manager
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageAction {
    Toggle(u8),
    Select(usize),
    SelectWorld(usize),
    RequestClear,
    RequestDelete,
    RequestScreenshots,
    ConfirmDelete,
    OpenFolder(StorageFolder),
}

/// A user data folder the host can reveal in the OS file manager.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageFolder {
    Worlds,
    ResourcePacks,
    /// Java shader packs, offered in the Shaders tab.
    ShaderPacks,
}

impl StorageFolder {
    /// Returns the directory this folder names in `layout`.
    pub fn dir(self, layout: &InstallLayout) -> PathBuf {
        match self {
            Self::Worlds => layout.local_worlds_dir(),
            Self::ResourcePacks => layout.global_resource_packs_dir(),
            Self::ShaderPacks => layout.shader_packs_dir(),
        }
    }
}

impl StorageView {
    /// Returns the items of a vanilla storage category, or `None` for one the client does not
    /// keep (world templates, resource, behavior and skin packs).
    pub fn items(&self, category: &str) -> Option<&[StorageItem]> {
        match category {
            "world" => Some(&self.worlds),
            "cache" => Some(&self.cached),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_open_the_layout_dirs_that_hold_worlds_and_packs() {
        let layout = crate::test_support::scratch("storage-folders");
        assert_eq!(
            StorageFolder::Worlds.dir(&layout),
            layout.local_worlds_dir()
        );
        assert_eq!(
            StorageFolder::ResourcePacks.dir(&layout),
            layout.global_resource_packs_dir()
        );
        assert_eq!(
            StorageFolder::ShaderPacks.dir(&layout),
            layout.shader_packs_dir()
        );
        for folder in [
            StorageFolder::Worlds,
            StorageFolder::ResourcePacks,
            StorageFolder::ShaderPacks,
        ] {
            assert!(folder.dir(&layout).starts_with(&layout.user_data_root));
        }
    }
}
