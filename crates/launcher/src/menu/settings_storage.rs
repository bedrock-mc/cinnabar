//! Storage screen state and commands; disk work belongs to the host.
use std::path::PathBuf;

pub const SECTION_INDEX: u8 = 25;
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
}

impl StorageView {
    /// Returns the items represented by a vanilla storage category.
    pub fn items(&self, category: &str) -> &[StorageItem] {
        match category {
            "world" => &self.worlds,
            "cache" => &self.cached,
            _ => &[],
        }
    }
}
