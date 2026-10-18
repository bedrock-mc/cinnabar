//! Release channels share their names, URLs, and ordering with launcher preferences.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Beta,
    Nightly,
    Stable,
}

impl Channel {
    /// Order used by the saved channel setting; stable stays last so it can be hidden initially.
    pub const ALL: [Self; 3] = [Self::Beta, Self::Nightly, Self::Stable];

    /// The signed channel name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Beta => "beta",
            Self::Nightly => "nightly",
            Self::Stable => "stable",
        }
    }

    /// The user-facing channel name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Beta => "Beta",
            Self::Nightly => "Nightly (Unstable)",
            Self::Stable => "Stable",
        }
    }

    /// Nightly also follows beta releases while keeping the saved Nightly preference.
    pub const fn additional_channel(self) -> Option<Self> {
        match self {
            Self::Nightly => Some(Self::Beta),
            Self::Beta | Self::Stable => None,
        }
    }

    /// Accepts only releases published for this choice or its additional channel.
    pub fn accepts_release(self, release: &str) -> bool {
        self.name() == release
            || self
                .additional_channel()
                .is_some_and(|channel| channel.name() == release)
    }

    /// The radio binding used by the existing settings picker.
    pub const fn binding(self) -> &'static str {
        match self {
            Self::Beta => "update_channel_beta",
            Self::Nightly => "update_channel_nightly",
            Self::Stable => "update_channel_stable",
        }
    }

    /// The persisted setting's index, derived from its declared order.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|channel| *channel == self)
            .expect("declared channel")
    }

    /// Resolves known channel names, including the reserved stable channel.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|channel| channel.name() == name)
    }

    /// Whether the shared release metadata assigns this channel a publishing track.
    pub fn is_published(self) -> bool {
        super::platforms::channel_release(self.name()).is_some()
    }

    /// The pipeline's manifest URL, or none for a channel without a publishing track.
    pub fn manifest_url(self) -> Option<String> {
        let release = super::platforms::channel_release(self.name())?;
        let path = match release.track {
            super::platforms::ReleaseTrack::Tagged => "latest/download".to_owned(),
            super::platforms::ReleaseTrack::Rolling => {
                format!("download/{}", release.tag.as_ref()?)
            }
        };
        Some(format!(
            "https://github.com/{}/releases/{}/{}",
            super::platforms::repository(),
            path,
            release.manifest
        ))
    }
}
