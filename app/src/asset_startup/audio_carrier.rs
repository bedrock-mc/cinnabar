//! Optional pinned sound-definition carrier loading.
//!
//! Per the VPA-017 owner decision this carrier binds optionally: absence
//! selects a bounded empty-catalog fallback with a one-time startup notice,
//! while a present-but-oversized, malformed, or stale-provenance carrier is a
//! fatal, actionable error naming the exact path and rebuild command. It is
//! deliberately not added to the required-at-startup carrier set.

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::RuntimeAudioCatalog;

use super::{
    AssetStartupError, DEFAULT_ASSET_PATH, VANILLA_SOURCE_JSON, canonical_source_manifest_sha256,
    format_sha256,
};

pub const AUDIO_ASSETS_FILENAME: &str = "vanilla-v1.mcbeaud";
pub const AUDIO_ASSETS_COMPILE_COMMAND: &str = "make audio-assets";
const AUDIO_ASSETS_REPORT_FILENAME: &str = "audio-assets.json";
const MAX_AUDIO_ASSET_BLOB_BYTES: u64 = assets::MAX_AUDIO_CARRIER_BYTES as u64;

/// Returns a copy-paste recovery command that writes the sound-definition
/// carrier where startup looked for it: the bare make target at the default
/// location, or the same target with `AUDIO_ASSET_BLOB`/`AUDIO_ASSET_REPORT`
/// naming the exact custom siblings.
#[must_use]
pub fn audio_assets_rebuild_command(path: &Path) -> String {
    let default_path = audio_asset_path(Path::new(DEFAULT_ASSET_PATH));
    if path == default_path {
        return AUDIO_ASSETS_COMPILE_COMMAND.to_owned();
    }
    let report_path = path.with_file_name(AUDIO_ASSETS_REPORT_FILENAME);
    format!(
        "{AUDIO_ASSETS_COMPILE_COMMAND} AUDIO_ASSET_BLOB={} AUDIO_ASSET_REPORT={}",
        super::shell_quote_path(path),
        super::shell_quote_path(&report_path)
    )
}

/// One-time startup notice for an absent optional carrier.
#[must_use]
pub fn audio_assets_missing_notice(path: &Path) -> String {
    let rebuild_command = audio_assets_rebuild_command(path);
    let recovery = if rebuild_command == AUDIO_ASSETS_COMPILE_COMMAND {
        format!(
            "Build only this carrier with `{AUDIO_ASSETS_COMPILE_COMMAND}`, or refresh every carrier with `make assets`."
        )
    } else {
        format!("Build the carrier at that exact custom location with `{rebuild_command}`.")
    };
    format!(
        "optional vanilla sound-definition carrier was not found at {}; named PlaySound lookups count as unresolved skips until it exists. {recovery}",
        path.display()
    )
}

#[derive(Debug)]
pub struct LoadedAudioAssets {
    runtime: Arc<RuntimeAudioCatalog>,
    selected_path: PathBuf,
}

impl LoadedAudioAssets {
    #[must_use]
    pub fn runtime(&self) -> &Arc<RuntimeAudioCatalog> {
        &self.runtime
    }

    pub fn into_runtime(self) -> Arc<RuntimeAudioCatalog> {
        self.runtime
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        format!(
            "loaded pinned official Mojang sample sound definitions from {} ({} definitions, source_manifest_sha256={}, sound_definitions_sha256={})",
            self.selected_path.display(),
            self.runtime.definitions().len(),
            format_sha256(self.runtime.source_manifest_sha256()),
            format_sha256(self.runtime.sound_definitions_sha256())
        )
    }
}

#[must_use]
pub fn audio_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(AUDIO_ASSETS_FILENAME)
}

/// Probes for and validates the sound-definition carrier adjacent to
/// `world_asset_path`.
///
/// Returns `Ok(None)` only when the carrier is absent, so production falls
/// back to the bounded empty catalog; every other failure — oversized,
/// malformed, or stale provenance against the embedded canonical
/// `vanilla-source.json` identity — fails closed here.
pub fn load_audio_assets(
    world_asset_path: &Path,
) -> Result<Option<LoadedAudioAssets>, AssetStartupError> {
    let path = audio_asset_path(world_asset_path);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(AssetStartupError::AudioAssetsRead {
                path: path.clone(),
                source,
                rebuild_command: audio_assets_rebuild_command(&path),
            });
        }
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::AudioAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: audio_assets_rebuild_command(&path),
        })?
        .len();
    if length > MAX_AUDIO_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::AudioAssetsTooLarge {
            path: path.clone(),
            max_bytes: MAX_AUDIO_ASSET_BLOB_BYTES,
            rebuild_command: audio_assets_rebuild_command(&path),
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_AUDIO_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::AudioAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: audio_assets_rebuild_command(&path),
        })?;
    if bytes.len() as u64 > MAX_AUDIO_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::AudioAssetsTooLarge {
            path: path.clone(),
            max_bytes: MAX_AUDIO_ASSET_BLOB_BYTES,
            rebuild_command: audio_assets_rebuild_command(&path),
        });
    }
    let runtime = RuntimeAudioCatalog::decode(&bytes).map_err(|source| {
        AssetStartupError::AudioAssetsDecode {
            path: path.clone(),
            source: Box::new(source),
            rebuild_command: audio_assets_rebuild_command(&path),
        }
    })?;
    // The carrier must have been compiled from the checkout's pinned canonical
    // `vanilla-source.json`; a rebuilt manifest beside a stale carrier fails
    // closed exactly like the localization and item-icon carriers.
    let expected = canonical_source_manifest_sha256(VANILLA_SOURCE_JSON);
    if runtime.source_manifest_sha256() != expected {
        return Err(AssetStartupError::AudioAssetsProvenance {
            rebuild_command: audio_assets_rebuild_command(&path),
            carrier: format_sha256(runtime.source_manifest_sha256()),
            manifest: format_sha256(expected),
            path,
        });
    }
    Ok(Some(LoadedAudioAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
    }))
}
