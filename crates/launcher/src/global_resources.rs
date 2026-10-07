//! Pack-management commands and immutable launcher snapshots.
use resource_pack::{ActivePack, InstalledPack};

/// Commands the settings host can submit without owning pack storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    SelectAvailable(usize),
    SelectActive(usize),
    ReadMore(bool, usize),
    Activate(usize),
    Deactivate(usize),
    MoveUp(usize),
    MoveDown(usize),
    Settings(usize),
    CloseSettings,
    Subpack(usize),
    ToggleAvailable,
    ToggleActive,
    Import,
    Apply,
}

/// Immutable view of the staged selection and worker status.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Generation of the indexed pack lists, independent of presentation updates.
    pub revision: u64,
    pub memory_tier: u32,
    pub icons: std::collections::BTreeMap<(String, u64), String>,
    pub available: Vec<InstalledPack>,
    pub active: Vec<InstalledPack>,
    pub selection: Vec<ActivePack>,
    /// Exact last runtime acknowledgement; absent until initial pack preparation completes.
    pub applied_selection: Option<Vec<ActivePack>>,
    pub selected: Option<(bool, usize)>,
    pub details_expanded: Option<(bool, usize)>,
    pub settings: Option<usize>,
    pub available_expanded: bool,
    pub active_expanded: bool,
    pub busy: bool,
    pub message: String,
}

impl Snapshot {
    /// Reports edits relative to the last acknowledged runtime stack.
    pub fn has_pending_changes(&self) -> bool {
        self.applied_selection
            .as_ref()
            .is_some_and(|applied| *applied != self.selection)
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            memory_tier: 0,
            icons: Default::default(),
            available: Vec::new(),
            active: Vec::new(),
            selection: Vec::new(),
            applied_selection: None,
            selected: None,
            details_expanded: None,
            settings: None,
            available_expanded: true,
            active_expanded: true,
            busy: false,
            message: String::new(),
        }
    }
}
