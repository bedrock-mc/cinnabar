//! Mod-management commands and immutable Settings snapshots, independent of guest execution.

/// The Mods tab's settings selector variable.
pub const SECTION: &str = crate::menu::settings_sections::SettingSection::Mods
    .definition()
    .name;
/// File extension of an importable mod component.
pub const COMPONENT_EXTENSION: &str = "wasm";

/// Commands name the card's own identity so list changes cannot retarget a press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ReadMore(u64),
    ToggleActive,
    ToggleAvailable,
    Enable(u64),
    Disable(u64),
    Remove(u64),
    Import,
    Reload(u64),
    ReloadAll,
    MoveUp(u64),
    MoveDown(u64),
    Permission(u64, usize, bool),
    RequestedPermissions(u64, bool),
}

/// One permission supplied by the host's grant declaration, rather than a UI capability list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permission {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub requested: Option<bool>,
}

/// A directory component or an enabled registration whose source is unavailable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledMod {
    /// Identifies the file revision, or the registration path while its source is unavailable.
    pub id: u64,
    pub file: String,
    pub bytes: u64,
    pub permissions: Vec<Permission>,
    pub metadata: ui::mod_metadata::ModMetadata,
}
impl InstalledMod {
    /// The component's declared name, else its file name without the extension.
    pub fn name(&self) -> &str {
        self.metadata.name.as_deref().unwrap_or_else(|| {
            std::path::Path::new(&self.file)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(&self.file)
        })
    }
    /// The menu artwork key its icon draws under; a new file revision gets a new key.
    pub fn icon_key(&self) -> Option<String> {
        self.metadata
            .icon
            .as_ref()
            .map(|_| format!("mod-icon:{}", self.id))
    }
}

/// The host's recorded outcome, including a reload which kept a running instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Starting,
    Running,
    Failed(String),
    ReloadFailed(String),
}

/// An active entry, in precedence order, and its own host status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveMod {
    pub id: u64,
    pub status: Status,
}

/// Directory contents and the ordered Active list, published off-frame by the app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub installed: Vec<InstalledMod>,
    pub active: Vec<ActiveMod>,
    pub busy: bool,
    pub message: String,
}
impl Library {
    /// Preserves Active-list precedence and directory order for available components.
    pub fn groups(&self) -> (Vec<InstalledMod>, Vec<InstalledMod>) {
        let active = self
            .active
            .iter()
            .filter_map(|entry| self.installed.iter().find(|item| item.id == entry.id))
            .cloned()
            .collect();
        let available = self
            .installed
            .iter()
            .filter(|item| !self.active.iter().any(|entry| entry.id == item.id))
            .cloned()
            .collect();
        (active, available)
    }
}

/// The published library and local disclosure state.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub library: Library,
    pub details_expanded: Option<u64>,
    pub active_expanded: bool,
    pub available_expanded: bool,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            library: Library::default(),
            details_expanded: None,
            active_expanded: true,
            available_expanded: true,
        }
    }
}
impl Snapshot {
    /// Keeps disclosure changes local and forwards work only while the library is idle.
    pub fn apply(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::ReadMore(id) => {
                self.details_expanded = (self.details_expanded != Some(id)).then_some(id);
                None
            }
            Action::ToggleActive => {
                self.active_expanded = !self.active_expanded;
                None
            }
            Action::ToggleAvailable => {
                self.available_expanded = !self.available_expanded;
                None
            }
            _ => (!self.library.busy).then_some(action),
        }
    }
    /// Closes details when their component leaves the library.
    pub fn publish(&mut self, library: Library) {
        if self
            .details_expanded
            .is_some_and(|id| !library.installed.iter().any(|item| item.id == id))
        {
            self.details_expanded = None;
        }
        self.library = library;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Creates a directory entry with no requested or granted permissions.
    fn installed(id: u64) -> InstalledMod {
        InstalledMod {
            id,
            file: format!("mod-{id}.wasm"),
            bytes: 1,
            permissions: Vec::new(),
            metadata: Default::default(),
        }
    }
    #[test]
    fn declared_names_override_the_file_stem() {
        let mut item = installed(7);
        assert_eq!(item.name(), "mod-7");
        assert_eq!(item.icon_key(), None);
        item.metadata.name = Some("Zoom".into());
        item.metadata.icon = Some(std::sync::Arc::from([1u8].as_slice()));
        assert_eq!(item.name(), "Zoom");
        assert!(item.icon_key().is_some());
    }
    #[test]
    fn active_groups_keep_precedence_and_disclosures_stay_local() {
        let mut snapshot = Snapshot::default();
        assert_eq!(snapshot.apply(Action::ReadMore(2)), None);
        snapshot.publish(Library {
            installed: vec![installed(1), installed(2), installed(3)],
            active: vec![
                ActiveMod {
                    id: 2,
                    status: Status::Running,
                },
                ActiveMod {
                    id: 1,
                    status: Status::Running,
                },
            ],
            ..Default::default()
        });
        let (active, available) = snapshot.library.groups();
        assert_eq!(active, [installed(2), installed(1)]);
        assert_eq!(available, [installed(3)]);
        snapshot.library.busy = true;
        assert_eq!(snapshot.apply(Action::ReloadAll), None);
        snapshot.publish(Library::default());
        assert_eq!(snapshot.details_expanded, None);
    }
}
