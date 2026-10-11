//! Actionable carrier read, decode and identity failures.
use crate::{AssetError, FontCatalogError, HudCatalogError};
use std::{io, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AssetStartupError {
    #[error(
        "invalid local finite PCM carrier at {path}: {detail}; rebuild with make audio-pcm-assets"
    )]
    AudioPcm { path: PathBuf, detail: String },
    #[error(
        "required neutral actor carrier at {path} is unavailable or invalid: {detail}\nrebuild with: {rebuild_command}"
    )]
    ActorAssets {
        path: PathBuf,
        detail: Box<str>,
        rebuild_command: String,
    },
    #[error("could not read compiled asset blob at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("compiled asset blob at {path} exceeds the {max_bytes}-byte startup limit")]
    TooLarge { path: PathBuf, max_bytes: u64 },

    #[error(
        "could not decode compiled asset blob at {path}: {source}\nrebuild stale local assets with: {rebuild_command}"
    )]
    Decode {
        path: PathBuf,
        #[source]
        source: Box<AssetError>,
        rebuild_command: &'static str,
    },

    #[error("could not parse the checked-in vanilla source manifest: {0}")]
    SourceManifest(#[from] serde_json::Error),

    #[error(
        "could not read required atmosphere asset carrier at {path}: {source}\nrebuild local atmosphere assets with: {rebuild_command}"
    )]
    AtmosphereRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: &'static str,
    },

    #[error(
        "required atmosphere asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local atmosphere assets with: {rebuild_command}"
    )]
    AtmosphereTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: &'static str,
    },

    #[error(
        "could not decode required atmosphere asset carrier at {path}: {source}\nrebuild local atmosphere assets with: {rebuild_command}"
    )]
    AtmosphereDecode {
        path: PathBuf,
        #[source]
        source: Box<AssetError>,
        rebuild_command: &'static str,
    },

    #[error(
        "could not read required entity asset carrier at {path}: {source}\nrebuild local entity assets with: {rebuild_command}"
    )]
    EntityAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: &'static str,
    },

    #[error(
        "required entity asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local entity assets with: {rebuild_command}"
    )]
    EntityAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: &'static str,
    },

    #[error(
        "could not decode required entity asset carrier at {path}: {source}\nrebuild local entity assets with: {rebuild_command}"
    )]
    EntityAssetsDecode {
        path: PathBuf,
        #[source]
        source: Box<AssetError>,
        rebuild_command: &'static str,
    },

    #[error(
        "required entity asset carrier at {path} has stale provenance (expected source manifest SHA-256 {expected}, found {actual})\nrebuild local entity assets with: {rebuild_command}"
    )]
    EntityAssetsProvenance {
        path: PathBuf,
        expected: String,
        actual: String,
        rebuild_command: &'static str,
    },

    #[error(
        "compiled asset carrier at {path} has stale provenance ({component}: expected SHA-256 {expected}, found {actual})\nrebuild stale local assets with: {rebuild_command}"
    )]
    WorldAssetsProvenance {
        path: PathBuf,
        component: &'static str,
        expected: String,
        actual: String,
        rebuild_command: &'static str,
    },

    #[error(
        "checkout conflict: the pinned block registry stamps wire protocol {actual} but the active content authority binds protocol {expected}; regenerate the world and physics registry inputs together"
    )]
    PinnedRegistryProtocolMismatch { expected: u32, actual: u32 },

    #[error("could not read the wire protocol stamp from the pinned block registry: {source}")]
    PinnedRegistryHeader {
        #[source]
        source: Box<AssetError>,
    },

    #[error(
        "required atmosphere asset carrier at {path} has stale provenance (expected source manifest SHA-256 {expected}, found {actual})\nrebuild local atmosphere assets with: {rebuild_command}"
    )]
    AtmosphereAssetsProvenance {
        path: PathBuf,
        expected: String,
        actual: String,
        rebuild_command: &'static str,
    },

    #[error(
        "could not read required font asset carrier at {path}: {source}\nrebuild local font assets with: {rebuild_command}"
    )]
    FontAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: &'static str,
    },

    #[error(
        "required font asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local font assets with: {rebuild_command}"
    )]
    FontAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: &'static str,
    },

    #[error(
        "could not decode required font asset carrier at {path}: {source}\nrebuild local font assets with: {rebuild_command}"
    )]
    FontAssetsDecode {
        path: PathBuf,
        #[source]
        source: Box<FontCatalogError>,
        rebuild_command: &'static str,
    },

    #[error(
        "could not read local HUD asset carrier at {path}: {source}\nrebuild local HUD assets with: {rebuild_command}"
    )]
    HudAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: String,
    },

    #[error(
        "local HUD asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local HUD assets with: {rebuild_command}"
    )]
    HudAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: String,
    },

    #[error(
        "could not decode local HUD asset carrier at {path}: {source}\nrebuild local HUD assets with: {rebuild_command}"
    )]
    HudAssetsDecode {
        path: PathBuf,
        #[source]
        source: HudCatalogError,
        rebuild_command: String,
    },

    #[error("{notice}")]
    HudAssetsMissing {
        path: PathBuf,
        rebuild_command: String,
        notice: String,
    },

    #[error(
        "could not read required localization carrier at {path}: {source}\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: String,
    },

    #[error(
        "local localization carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: String,
    },

    #[error(
        "could not decode local localization carrier at {path}: {source}\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsDecode {
        path: PathBuf,
        #[source]
        source: crate::LangCatalogError,
        rebuild_command: String,
    },

    #[error(
        "local localization carrier at {path} was compiled from manifest {carrier} but the checkout pins {manifest}\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsProvenance {
        path: PathBuf,
        carrier: String,
        manifest: String,
        rebuild_command: String,
    },

    #[error(
        "local localization carrier at {path} was compiled from texts/en_US.lang bytes {carrier} but the checkout pins {pinned}\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsSourceProvenance {
        path: PathBuf,
        carrier: String,
        pinned: String,
        rebuild_command: String,
    },

    #[error("{notice}")]
    LangAssetsMissing {
        path: PathBuf,
        rebuild_command: String,
        notice: String,
    },

    #[error(
        "could not read local sound-definition carrier at {path}: {source}
rebuild sound-definition assets with: {rebuild_command}"
    )]
    AudioAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: String,
    },

    #[error(
        "local sound-definition carrier at {path} exceeds the {max_bytes}-byte startup limit
rebuild sound-definition assets with: {rebuild_command}"
    )]
    AudioAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: String,
    },

    #[error(
        "could not decode local sound-definition carrier at {path}: {source}
rebuild sound-definition assets with: {rebuild_command}"
    )]
    AudioAssetsDecode {
        path: PathBuf,
        #[source]
        source: Box<crate::AudioCatalogError>,
        rebuild_command: String,
    },

    #[error(
        "local sound-definition carrier at {path} was compiled from manifest {carrier} but the checkout pins {manifest}
rebuild sound-definition assets with: {rebuild_command}"
    )]
    AudioAssetsProvenance {
        path: PathBuf,
        carrier: String,
        manifest: String,
        rebuild_command: String,
    },

    #[error(
        "could not read required item-icon carrier at {path}: {source}
rebuild item-icon assets with: {rebuild_command}"
    )]
    IconAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: String,
    },

    #[error(
        "local item-icon carrier at {path} exceeds the {max_bytes}-byte startup limit
rebuild item-icon assets with: {rebuild_command}"
    )]
    IconAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: String,
    },

    #[error(
        "could not decode local item-icon carrier at {path}: {source}
rebuild item-icon assets with: {rebuild_command}"
    )]
    IconAssetsDecode {
        path: PathBuf,
        #[source]
        source: Box<crate::AssetError>,
        rebuild_command: String,
    },

    #[error(
        "local item-icon carrier at {path} was compiled from manifest {carrier} but the checkout pins {manifest}
rebuild item-icon assets with: {rebuild_command}"
    )]
    IconAssetsProvenance {
        path: PathBuf,
        carrier: String,
        manifest: String,
        rebuild_command: String,
    },

    #[error("{notice}")]
    IconAssetsMissing {
        path: PathBuf,
        rebuild_command: String,
        notice: String,
    },
}
