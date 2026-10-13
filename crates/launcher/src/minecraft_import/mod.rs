//! Explicit, previewed migration of player-owned settings and server entries.

mod keyboard;
pub mod options;
pub mod servers;
pub mod summary;

/// Commands shared by the launcher controls and its serial import worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Begin,
    Select(usize),
    Apply,
    ConfirmApply,
    NotNow,
    Close,
    Revert,
    ConfirmRevert,
}

/// The visible step of the import flow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Phase {
    #[default]
    Idle,
    Offered,
    Profiles,
    Preview,
    ConfirmImport,
    Busy,
    Result,
    Revert,
}

/// Immutable presentation data; source paths and file payloads stay in the worker.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub phase: Phase,
    pub profiles: Vec<String>,
    pub body: String,
    pub pack_status: String,
    pub can_revert: bool,
}

impl Snapshot {
    /// Lists exactly the controls offered by the current modal.
    pub fn actions(&self) -> Vec<Action> {
        match self.phase {
            Phase::Offered => vec![Action::Begin, Action::NotNow],
            Phase::Profiles => (0..self.profiles.len())
                .map(Action::Select)
                .chain([Action::Close])
                .collect(),
            Phase::Preview => vec![Action::Apply, Action::Begin, Action::Close],
            Phase::ConfirmImport => vec![Action::ConfirmApply, Action::Close],
            Phase::Revert => vec![Action::ConfirmRevert, Action::Close],
            Phase::Busy => Vec::new(),
            Phase::Result if self.can_revert => vec![Action::Revert, Action::Close],
            Phase::Idle | Phase::Result => vec![Action::Close],
        }
    }

    /// Supplies the modal title without exposing filesystem details.
    pub fn title(&self) -> &'static str {
        match self.phase {
            Phase::Offered => "Import from Minecraft?",
            Phase::Profiles => "Choose a Minecraft profile",
            Phase::Preview => "Review Minecraft import",
            Phase::ConfirmImport => "Import these preferences?",
            Phase::Revert => "Revert Minecraft import?",
            _ => "Import from Minecraft",
        }
    }

    /// Explains the first offer's scope or returns the current operation's details.
    pub fn body(&self) -> &str {
        if self.phase == Phase::ConfirmImport {
            "Import these settings, servers and packs? You can revert this later in Settings."
        } else if self.phase == Phase::Offered {
            "Preview and import your Minecraft settings, saved servers and resource packs."
        } else {
            &self.body
        }
    }

    /// Supplies a readable label for a command offered by this snapshot.
    pub fn label(&self, action: Action) -> &str {
        match action {
            Action::Begin if self.phase == Phase::Preview => "Choose another profile",
            Action::Begin | Action::Apply | Action::ConfirmApply => "Import",
            Action::Select(index) => self.profiles.get(index).map_or("Profile", String::as_str),
            Action::NotNow => "Not now",
            Action::Revert | Action::ConfirmRevert => "Revert",
            Action::Close if matches!(self.phase, Phase::Preview | Phase::ConfirmImport) => {
                "Cancel"
            }
            Action::Close => "Close",
        }
    }
}
