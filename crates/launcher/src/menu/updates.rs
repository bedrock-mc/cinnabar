//! Launcher-owned update choices and the small notice shared with presentation.

use update_manifest::channel::Channel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Restart,
    Later,
    Download,
    SwitchNow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ready {
    pub version: String,
    pub writable: bool,
    pub protocol_required: bool,
}

impl Ready {
    /// The primary action always requires the user's click, including protocol-required updates.
    pub fn action(&self) -> Action {
        if self.writable {
            Action::Restart
        } else {
            Action::Download
        }
    }

    /// Uses stronger wording after an outdated-client refusal without starting installation.
    pub fn title(&self) -> &'static str {
        if self.protocol_required {
            "Update required to connect"
        } else {
            "Update available"
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct View {
    /// Only the client holding the updater role exposes update controls and notices.
    pub active: bool,
    pub ready: Option<Ready>,
    pub stable_available: bool,
    pub switch_available: bool,
    pub message: Option<String>,
    /// Publishing tracks known to this view, in the persisted channel order.
    pub published_channels: [bool; Channel::ALL.len()],
}

impl Default for View {
    /// Uses the shipped release metadata until another publication catalog is supplied.
    fn default() -> Self {
        Self::with_publications(Channel::is_published)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Preferences {
    pub checks: bool,
    pub install_on_quit: bool,
    pub channel: Channel,
}

impl Preferences {
    /// Shared defaults for the saved settings and the host's initial session state.
    pub const DEFAULT: Self = Self {
        checks: true,
        install_on_quit: false,
        channel: Channel::Beta,
    };
}

impl Default for Preferences {
    /// Checks default on; installation on exit needs a separate opt-in.
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl View {
    /// Builds a view from the publishing tracks declared by a release catalog.
    pub fn with_publications(published: impl Fn(Channel) -> bool) -> Self {
        Self {
            active: true,
            ready: None,
            stable_available: false,
            switch_available: false,
            message: None,
            published_channels: Channel::ALL.map(published),
        }
    }

    /// Only verified publication of a stable release unlocks its selector entry.
    pub fn channel_visible(&self, choice: usize) -> bool {
        Channel::ALL.get(choice).is_some_and(|channel| {
            self.published_channels[choice]
                && (*channel != Channel::Stable || self.stable_available)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unpublished_stable_stays_hidden_even_with_old_availability() {
        let mut view = View::default();
        assert!(view.channel_visible(Channel::Beta.index()));
        assert!(view.channel_visible(Channel::Nightly.index()));
        assert!(!view.channel_visible(Channel::Stable.index()));
        view.stable_available = true;
        assert!(!view.channel_visible(Channel::Stable.index()));
    }
    #[test]
    fn protocol_requirement_changes_wording_without_authorizing_installation() {
        let notice = Ready {
            version: "2.0.0".into(),
            writable: false,
            protocol_required: true,
        };
        assert!(notice.title().contains("required to connect"));
        assert_eq!(notice.action(), Action::Download);
        assert!(!Preferences::default().install_on_quit);
    }
}

/// Builds the release panel's wording without authorizing an installation.
pub fn popup(ready: Option<&Ready>) -> super::dialogs::Popup {
    let title = ready.map_or("Updates", Ready::title).into();
    super::dialogs::Popup {
        title,
        body: ready.map_or_else(String::new, |ready| format!("Version {}", ready.version)),
        button1: ready
            .map_or("Close", |ready| {
                if ready.writable {
                    "Restart and update"
                } else {
                    "Download update"
                }
            })
            .into(),
        button2: String::new(),
        confirm: ready.map_or(super::MenuAction::DismissDialog, |ready| {
            super::MenuAction::Update(ready.action())
        }),
        dismiss: super::MenuAction::DismissDialog,
    }
}
