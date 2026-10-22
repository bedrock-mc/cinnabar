//! The release pipeline and updater read the same supported platform and artifact names.
use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};

#[derive(Deserialize)]
struct ReleaseAssets {
    repository: String,
    assets: BTreeMap<String, BTreeMap<String, String>>,
    update_assets: BTreeMap<String, BTreeMap<String, String>>,
    update_channels: BTreeMap<String, ChannelRelease>,
    update_payload_prefix: String,
    linux_install: LinuxInstall,
}

/// The per-user Linux installation pointer shared by the installer and client.
#[derive(Deserialize)]
pub struct LinuxInstall {
    pub app_dir: String,
    pub current: String,
}

/// Locates the installer's managed AppDir without depending on the running image's path.
pub fn linux_install() -> &'static LinuxInstall {
    &RELEASE.linux_install
}

/// Where the release pipeline publishes one channel's signed manifest.
#[derive(Deserialize)]
pub struct ChannelRelease {
    pub track: ReleaseTrack,
    pub tag: Option<String>,
    pub manifest: String,
}

/// Tagged releases advance the latest download; rolling releases keep one prerelease tag.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseTrack {
    Tagged,
    Rolling,
}

static RELEASE: LazyLock<ReleaseAssets> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../packaging/release-assets.json"))
        .expect("release asset manifest must be valid")
});

/// Returns a key only for an architecture actually published by the release pipeline.
pub fn platform_key(os: &str, arch: &str) -> Option<String> {
    let arch = if arch == "aarch64" { "arm64" } else { arch };
    RELEASE
        .assets
        .get(os)?
        .contains_key(arch)
        .then(|| format!("{os}-{arch}"))
}

/// The native package filename selected by a manifest publisher for this platform.
pub fn artifact_name(os: &str, arch: &str) -> Option<&'static str> {
    let arch = if arch == "aarch64" { "arm64" } else { arch };
    RELEASE
        .update_assets
        .get(os)
        .and_then(|arches| arches.get(arch))
        .or_else(|| RELEASE.assets.get(os)?.get(arch))
        .map(String::as_str)
}

/// The release track holding `channel`'s manifest, if the pipeline publishes that channel.
pub fn channel_release(channel: &str) -> Option<&'static ChannelRelease> {
    RELEASE.update_channels.get(channel)
}

/// Repository that publishes the installer's channel manifests and native artifacts.
pub fn repository() -> &'static str {
    &RELEASE.repository
}

/// Prefix reserved for immutable update payloads in a channel's release assets.
pub fn payload_prefix() -> &'static str {
    &RELEASE.update_payload_prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_published_platform_has_one_native_update_artifact() {
        for (os, arches) in &RELEASE.assets {
            for arch in arches.keys() {
                assert_eq!(platform_key(os, arch), Some(format!("{os}-{arch}")));
                let name = artifact_name(os, arch).unwrap();
                let extension = match os.as_str() {
                    "windows" => ".msi",
                    "macos" => ".dmg",
                    "linux" => ".AppImage",
                    _ => panic!("unsupported platform"),
                };
                assert!(name.ends_with(extension));
            }
        }
        for (os, arches) in &RELEASE.update_assets {
            for arch in arches.keys() {
                assert!(platform_key(os, arch).is_some());
            }
        }
        assert!(platform_key("windows", "aarch64").is_none());
        assert!(platform_key("freebsd", "x86_64").is_none());
    }

    #[test]
    fn beta_follows_latest_nightly_rolls_and_stable_is_unpublished() {
        use crate::channel::Channel;
        assert_eq!(RELEASE.update_channels.len(), 2);
        let beta = channel_release(Channel::Beta.name()).unwrap();
        assert!(matches!(beta.track, ReleaseTrack::Tagged));
        assert!(beta.tag.is_none());
        assert_eq!(
            Channel::Beta.manifest_url().unwrap(),
            format!(
                "https://github.com/{}/releases/latest/download/{}",
                repository(),
                beta.manifest
            )
        );
        let nightly = channel_release(Channel::Nightly.name()).unwrap();
        assert!(matches!(nightly.track, ReleaseTrack::Rolling));
        assert_eq!(nightly.tag.as_deref(), Some(Channel::Nightly.name()));
        assert_eq!(
            Channel::Nightly.manifest_url().unwrap(),
            format!(
                "https://github.com/{}/releases/download/{}/{}",
                repository(),
                nightly.tag.as_deref().unwrap(),
                nightly.manifest
            )
        );
        assert!(channel_release(Channel::Stable.name()).is_none());
        assert!(Channel::Stable.manifest_url().is_none());
    }
}
