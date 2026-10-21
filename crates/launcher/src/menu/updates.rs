//! Launcher-owned update choices and the small notice shared with presentation.

use update_manifest::channel::Channel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    CheckNow,
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
    pub fn title(&self) -> String {
        if self.protocol_required {
            format!("Update required to connect ({})", self.version)
        } else {
            format!("Update ready ({})", self.version)
        }
    }
}

/// Progress from the worker, independent of a previously downloaded update.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Status {
    #[default]
    Idle,
    Checking,
    Downloading,
    UpToDate,
    Failed,
}

impl Status {
    /// A running check or download cannot accept another manual check.
    pub fn is_busy(self) -> bool {
        matches!(self, Self::Checking | Self::Downloading)
    }
}

/// Identifies the running client using the shared product version and publication number.
pub fn installed_version() -> String {
    let version = crate::PRODUCT_VERSION;
    let build = update_manifest::build_number();
    if build == 0 {
        format!("Version {version}")
    } else {
        format!("Version {version} · Build {build}")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct View {
    /// Only the client holding the updater role exposes update controls and notices.
    pub active: bool,
    pub ready: Option<Ready>,
    pub status: Status,
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
    /// Gives busy and failed checks priority while retaining any verified update offer.
    pub fn status_text(&self) -> String {
        if !self.status.is_busy()
            && self.status != Status::Failed
            && let Some(ready) = &self.ready
        {
            return ready.title();
        }
        match self.status {
            Status::Checking => "Checking for updates…".into(),
            Status::Downloading => "Downloading update…".into(),
            Status::Failed => "Could not check or download updates. Try again.".into(),
            Status::UpToDate => "You're up to date.".into(),
            Status::Idle => "Check for the latest release.".into(),
        }
    }

    /// Builds a view from the publishing tracks declared by a release catalog.
    pub fn with_publications(published: impl Fn(Channel) -> bool) -> Self {
        Self {
            active: true,
            ready: None,
            status: Status::Idle,
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
    /// Refreshing a retained offer reports current work and restores the offer afterward.
    #[test]
    fn update_status_keeps_downloaded_offers_separate_from_checks() {
        let mut view = View {
            ready: Some(Ready {
                version: crate::PRODUCT_VERSION.into(),
                writable: true,
                protocol_required: false,
            }),
            ..Default::default()
        };
        let ready = view.status_text();
        assert_eq!(ready, view.ready.as_ref().unwrap().title());
        for status in [Status::Checking, Status::Downloading, Status::Failed] {
            view.status = status;
            assert_ne!(view.status_text(), ready);
            assert!(view.ready.is_some());
        }
        view.status = Status::UpToDate;
        assert_eq!(view.status_text(), ready);
        view.ready = None;
        assert!(view.status_text().contains("up to date"));
    }
}
