//! Required pinned localization carrier loading, split from the asset-startup
//! root to honor the production line budget.
//!
//! Chat translation keys and item display names are player-facing text, so
//! production startup requires this carrier exactly like the HUD carrier: a
//! missing, oversized, malformed, or stale-provenance carrier is a fatal,
//! actionable error naming the rebuild command.

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::RuntimeLangCatalog;

use super::{AssetStartupError, canonical_source_manifest_sha256, format_sha256};

pub const LANG_ASSETS_FILENAME: &str = "vanilla-v1.mcbelang";
pub const LANG_ASSETS_COMPILE_COMMAND: &str = "make lang-assets";
const MAX_LANG_ASSET_BLOB_BYTES: u64 = 8 * 1024 * 1024;

pub struct LoadedLangAssets {
    runtime: Arc<RuntimeLangCatalog>,
    selected_path: PathBuf,
}

impl LoadedLangAssets {
    #[must_use]
    pub fn runtime(&self) -> &Arc<RuntimeLangCatalog> {
        &self.runtime
    }

    pub fn into_runtime(self) -> Arc<RuntimeLangCatalog> {
        self.runtime
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        format!(
            "loaded pinned official Mojang sample localization from {} ({} entries, source_manifest_sha256={})",
            self.selected_path.display(),
            self.runtime.len(),
            format_sha256(self.runtime.source_manifest_sha256())
        )
    }
}

#[must_use]
pub fn lang_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(LANG_ASSETS_FILENAME)
}

/// Loads and validates the localization carrier beside `world_asset_path`,
/// failing closed on absence, size, decode, or stale provenance against the
/// embedded canonical `vanilla-source.json` identity.
pub fn require_lang_assets(
    world_asset_path: &Path,
    vanilla_source_json: &str,
) -> Result<LoadedLangAssets, AssetStartupError> {
    let path = lang_asset_path(world_asset_path);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Err(AssetStartupError::LangAssetsMissing {
                notice: format!(
                    "required pinned official Mojang sample localization carrier was not found at {}; chat translation and item names cannot present, so the client will not start. Build it with `{LANG_ASSETS_COMPILE_COMMAND}`, or refresh every required carrier with `make assets`.",
                    path.display()
                ),
                rebuild_command: LANG_ASSETS_COMPILE_COMMAND,
                path,
            });
        }
        Err(source) => {
            return Err(AssetStartupError::LangAssetsRead {
                path,
                source,
                rebuild_command: LANG_ASSETS_COMPILE_COMMAND,
            });
        }
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::LangAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: LANG_ASSETS_COMPILE_COMMAND,
        })?
        .len();
    if length > MAX_LANG_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::LangAssetsTooLarge {
            path,
            max_bytes: MAX_LANG_ASSET_BLOB_BYTES,
            rebuild_command: LANG_ASSETS_COMPILE_COMMAND,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_LANG_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::LangAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: LANG_ASSETS_COMPILE_COMMAND,
        })?;
    if bytes.len() as u64 > MAX_LANG_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::LangAssetsTooLarge {
            path,
            max_bytes: MAX_LANG_ASSET_BLOB_BYTES,
            rebuild_command: LANG_ASSETS_COMPILE_COMMAND,
        });
    }
    let runtime = RuntimeLangCatalog::decode(&bytes).map_err(|source| {
        AssetStartupError::LangAssetsDecode {
            path: path.clone(),
            source,
            rebuild_command: LANG_ASSETS_COMPILE_COMMAND,
        }
    })?;
    let expected = canonical_source_manifest_sha256(vanilla_source_json);
    if runtime.source_manifest_sha256() != expected {
        return Err(AssetStartupError::LangAssetsProvenance {
            path,
            carrier: format_sha256(runtime.source_manifest_sha256()),
            manifest: format_sha256(expected),
            rebuild_command: LANG_ASSETS_COMPILE_COMMAND,
        });
    }
    Ok(LoadedLangAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
    })
}
