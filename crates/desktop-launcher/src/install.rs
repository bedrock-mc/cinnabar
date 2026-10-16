//! The installed release unit's identity, shown without waiting on the network.

use serde::Serialize;
use update_manifest::{build::Build, channel::Channel};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct InstallState {
    pub version: String,
    /// The publication number, or `dev` for an unpublished build.
    pub build: String,
    /// The desired channel.
    pub channel: Channel,
    /// The channel the installed payload was published on.
    pub installed_channel: Channel,
    /// Only channels with a publishing track.
    pub published_channels: Vec<Channel>,
    pub launcher_version: String,
    /// Lets the UI leave room for native window controls.
    pub platform: Platform,
    /// The command surface this launcher offers a web UI bundle.
    pub native_api_version: u32,
}

/// The launcher's own release identity, kept apart from the game's even while they ship together.
pub const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Describes the installed game; the launcher reports its own version separately.
pub fn state(current: &Build, channel: Channel) -> InstallState {
    InstallState {
        version: current.version.clone(),
        build: if current.number == 0 {
            "dev".to_owned()
        } else {
            current.number.to_string()
        },
        channel,
        installed_channel: update_manifest::build::release_channel().unwrap_or_default(),
        published_channels: Channel::ALL
            .into_iter()
            .filter(|channel| channel.is_published())
            .collect(),
        launcher_version: LAUNCHER_VERSION.to_owned(),
        platform: Platform::current(),
        native_api_version: crate::surface::NATIVE_API_VERSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpublished_channels_are_never_offered() {
        let shown = state(
            &Build {
                version: "1.0.0".into(),
                number: 0,
            },
            Channel::Beta,
        );
        assert_eq!(shown.build, "dev");
        for channel in Channel::ALL {
            assert_eq!(
                shown.published_channels.contains(&channel),
                channel.is_published()
            );
        }
    }
}
