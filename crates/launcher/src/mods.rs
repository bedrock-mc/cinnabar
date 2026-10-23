//! Mod-management commands and immutable Settings snapshots, independent of guest execution.

/// The Mods tab's settings selector variable.
pub const SECTION: &str = crate::menu::settings_sections::SettingSection::Mods
    .definition()
    .name;
/// File extension of an importable mod component.
pub const COMPONENT_EXTENSION: &str = "wasm";
/// Release tags share the installed metadata version's display bound.
pub const MAX_UPDATE_VERSION_BYTES: usize = ui::mod_metadata::MAX_VERSION_BYTES;

/// Commands name the card's own identity so list changes cannot retarget a press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Marketplace(crate::marketplace::Action),
    ReadMore(u64),
    ToggleActive,
    ToggleAvailable,
    Enable(u64),
    Disable(u64),
    Remove(u64),
    Import,
    ViewUpdates,
    OpenUpdate(u64),
    Update(u64),
    Reload(u64),
    ReloadAll,
    MoveUp(u64),
    MoveDown(u64),
    Permission(u64, usize, bool),
    AllowRequested(u64),
    RevokeAll(u64),
    Customize(u64),
    ShowAllPermissions(u64),
}

/// One permission supplied by the host's grant declaration, rather than a UI capability list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permission {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub requested: Option<bool>,
    /// Includes every declared value, such as all requested command names.
    pub requested_allowed: bool,
    pub sensitive: bool,
}

impl Permission {
    /// Adds the gameplay warning to the short name when the capability requires it.
    pub fn label(&self) -> String {
        if self.sensitive {
            format!("⚠ {}", self.name)
        } else {
            self.name.clone()
        }
    }
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
    /// Whether a valid permission declaration exists, including an empty request.
    pub fn declares_permissions(&self) -> bool {
        self.permissions
            .iter()
            .any(|permission| permission.requested.is_some())
    }
    /// Counts allowed requests and all requests without counting extra grants.
    pub fn requested_counts(&self) -> (usize, usize) {
        self.permissions
            .iter()
            .filter(|permission| permission.requested == Some(true))
            .fold((0, 0), |(allowed, total), permission| {
                (
                    allowed + usize::from(permission.requested_allowed),
                    total + 1,
                )
            })
    }
    /// Supplies stable host capability indices, with requested permissions first.
    pub fn permission_order(&self, show_all: bool) -> Vec<usize> {
        let mut indices: Vec<_> = self
            .permissions
            .iter()
            .enumerate()
            .filter(|(_, permission)| show_all || permission.requested == Some(true))
            .map(|(index, _)| index)
            .collect();
        indices.sort_by_key(|index| self.permissions[*index].requested != Some(true));
        indices
    }
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
    NextJoin,
    Failed(String),
    ReloadFailed(String),
}

/// An active entry, in precedence order, and its own host status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveMod {
    pub id: u64,
    pub status: Status,
}

/// A newer GitHub release for one installed file revision; installation stays manual.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub id: u64,
    pub version: String,
    pub release_url: String,
}

/// Directory contents and the ordered Active list, published off-frame by the app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    /// Live update operations, retained through the runtime reload acknowledgement.
    pub update_progress: Vec<UpdateProgress>,
    pub installed: Vec<InstalledMod>,
    pub active: Vec<ActiveMod>,
    pub busy: bool,
    pub message: String,
    pub updates: Vec<AvailableUpdate>,
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
    pub marketplace: crate::marketplace::State,
    pub library: Library,
    pub details_expanded: Option<u64>,
    pub active_expanded: bool,
    pub available_expanded: bool,
    pub customizing: Option<u64>,
    pub show_all_permissions: bool,
    pub consent: Option<u64>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            marketplace: Default::default(),
            library: Library::default(),
            details_expanded: None,
            active_expanded: true,
            available_expanded: true,
            customizing: None,
            show_all_permissions: false,
            consent: None,
        }
    }
}
impl Snapshot {
    /// Keeps disclosure changes local and forwards work only while the library is idle.
    pub fn apply(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::Marketplace(action) => {
                self.marketplace.apply(action);
                None
            }
            Action::Customize(id) => {
                self.customizing = (self.customizing != Some(id)).then_some(id);
                self.show_all_permissions = false;
                if self.consent == Some(id) {
                    self.consent = None;
                }
                None
            }
            Action::ShowAllPermissions(id) => {
                if self.customizing == Some(id) {
                    self.show_all_permissions = !self.show_all_permissions;
                }
                None
            }
            Action::Enable(id) if !self.library.busy => {
                if self
                    .library
                    .installed
                    .iter()
                    .find(|item| item.id == id)
                    .is_some_and(|item| {
                        let (allowed, total) = item.requested_counts();
                        !item.permissions.iter().any(|permission| permission.enabled)
                            || allowed < total
                            || !item.declares_permissions()
                    })
                {
                    self.consent = Some(id);
                }
                Some(action)
            }
            Action::AllowRequested(id) if !self.library.busy => {
                if !self
                    .library
                    .installed
                    .iter()
                    .any(|item| item.id == id && item.declares_permissions())
                {
                    return None;
                }
                if self.consent == Some(id) {
                    self.consent = None;
                }
                Some(action)
            }
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
    /// Retires disclosures for removed components and consent for disabled components.
    pub fn publish(&mut self, library: Library) {
        if self
            .details_expanded
            .is_some_and(|id| !library.installed.iter().any(|item| item.id == id))
        {
            self.details_expanded = None;
        }
        if self
            .customizing
            .is_some_and(|id| !library.installed.iter().any(|item| item.id == id))
        {
            self.customizing = None;
            self.show_all_permissions = false;
        }
        if self
            .consent
            .is_some_and(|id| !library.active.iter().any(|item| item.id == id))
        {
            self.consent = None;
        }
        self.library = library;
    }
}
/// The operation currently owning a mod update, without pretending to know later work's fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdatePhase {
    Downloading,
    Verifying,
    Installing,
    Reloading,
}

impl UpdatePhase {
    /// The actual operation shown beside its progress indicator.
    pub fn label(self) -> &'static str {
        match self {
            Self::Downloading => "Downloading…",
            Self::Verifying => "Verifying…",
            Self::Installing => "Installing…",
            Self::Reloading => "Reloading…",
        }
    }
}

/// Bounded download counters and stage information for one file revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateProgress {
    pub id: u64,
    pub phase: UpdatePhase,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_sec: Option<u64>,
    pub eta_secs: Option<u64>,
}

impl UpdateProgress {
    /// Starts a stage without carrying misleading download estimates into installation or reload.
    pub fn stage(id: u64, phase: UpdatePhase) -> Self {
        Self {
            id,
            phase,
            received_bytes: 0,
            total_bytes: 0,
            bytes_per_sec: None,
            eta_secs: None,
        }
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
    fn permission_order_and_summary_keep_the_host_indices() {
        let mut item = installed(1);
        item.permissions = [false, true, false, true]
            .into_iter()
            .enumerate()
            .map(|(index, requested)| Permission {
                name: format!("Permission {index}"),
                description: String::new(),
                enabled: index < 2,
                requested: Some(requested),
                requested_allowed: index < 2,
                sensitive: index == 1,
            })
            .collect();
        assert_eq!(item.permission_order(false), [1, 3]);
        assert_eq!(item.permission_order(true), [1, 3, 0, 2]);
        assert_eq!(item.requested_counts(), (1, 2));
        assert_eq!(item.permissions[1].label(), "⚠ Permission 1");
        assert_eq!(item.permissions[0].label(), "Permission 0");
    }

    #[test]
    fn undeclared_cards_cannot_forward_one_click_grants() {
        let mut snapshot = Snapshot::default();
        snapshot.library.installed.push(installed(1));
        assert_eq!(snapshot.apply(Action::AllowRequested(1)), None);
        assert_eq!(snapshot.apply(Action::Customize(1)), None);
        assert_eq!(snapshot.customizing, Some(1));
        assert_eq!(snapshot.apply(Action::ShowAllPermissions(1)), None);
        assert!(snapshot.show_all_permissions);
        snapshot.publish(Library::default());
        assert_eq!(snapshot.customizing, None);
        assert!(!snapshot.show_all_permissions);
    }

    #[test]
    fn first_enable_keeps_empty_declaration_consent_until_an_explicit_choice() {
        let mut item = installed(1);
        item.permissions.push(Permission {
            name: "HUD".into(),
            description: String::new(),
            enabled: false,
            requested: Some(false),
            requested_allowed: true,
            sensitive: false,
        });
        let mut snapshot = Snapshot::default();
        snapshot.library.installed.push(item);
        assert_eq!(snapshot.apply(Action::Enable(1)), Some(Action::Enable(1)));
        let mut active = snapshot.library.clone();
        active.active.push(ActiveMod {
            id: 1,
            status: Status::Running,
        });
        snapshot.publish(active);
        assert_eq!(snapshot.consent, Some(1));
        assert_eq!(
            snapshot.apply(Action::AllowRequested(1)),
            Some(Action::AllowRequested(1))
        );
        assert_eq!(snapshot.consent, None);
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
