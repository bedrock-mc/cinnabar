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

use super::{
    AssetStartupError, DEFAULT_ASSET_PATH, canonical_source_manifest_sha256, format_sha256,
};

pub const LANG_ASSETS_FILENAME: &str = "vanilla-v1.mcbelang";
pub const LANG_ASSETS_COMPILE_COMMAND: &str = "make lang-assets";
const LANG_ASSETS_REPORT_FILENAME: &str = "lang-assets.json";
const MAX_LANG_ASSET_BLOB_BYTES: u64 = 8 * 1024 * 1024;

/// Returns a copy-paste recovery command that writes the localization
/// carrier where startup looked for it: the bare make target at the default
/// location, or the same target with `LANG_ASSET_BLOB`/`LANG_ASSET_REPORT`
/// naming the exact custom siblings.
#[must_use]
pub fn lang_assets_rebuild_command(path: &Path) -> String {
    let default_path = lang_asset_path(Path::new(DEFAULT_ASSET_PATH));
    if path == default_path {
        return LANG_ASSETS_COMPILE_COMMAND.to_owned();
    }
    let report_path = path.with_file_name(LANG_ASSETS_REPORT_FILENAME);
    format!(
        "{LANG_ASSETS_COMPILE_COMMAND} LANG_ASSET_BLOB={} LANG_ASSET_REPORT={}",
        super::shell_quote_path(path),
        super::shell_quote_path(&report_path)
    )
}

#[derive(Debug)]
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
            "loaded pinned official Mojang sample localization from {} ({} entries, source_manifest_sha256={}, lang_source_sha256={})",
            self.selected_path.display(),
            self.runtime.len(),
            format_sha256(self.runtime.source_manifest_sha256()),
            format_sha256(self.runtime.lang_source_sha256())
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
            let rebuild_command = lang_assets_rebuild_command(&path);
            return Err(AssetStartupError::LangAssetsMissing {
                notice: format!(
                    "required pinned official Mojang sample localization carrier was not found at {}; chat translation and item names cannot present, so the client will not start. Build it with `{rebuild_command}`, or refresh every required carrier with `make assets`.",
                    path.display()
                ),
                rebuild_command,
                path,
            });
        }
        Err(source) => {
            return Err(AssetStartupError::LangAssetsRead {
                rebuild_command: lang_assets_rebuild_command(&path),
                path,
                source,
            });
        }
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::LangAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: lang_assets_rebuild_command(&path),
        })?
        .len();
    if length > MAX_LANG_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::LangAssetsTooLarge {
            rebuild_command: lang_assets_rebuild_command(&path),
            path,
            max_bytes: MAX_LANG_ASSET_BLOB_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_LANG_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::LangAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: lang_assets_rebuild_command(&path),
        })?;
    if bytes.len() as u64 > MAX_LANG_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::LangAssetsTooLarge {
            rebuild_command: lang_assets_rebuild_command(&path),
            path,
            max_bytes: MAX_LANG_ASSET_BLOB_BYTES,
        });
    }
    let runtime = RuntimeLangCatalog::decode(&bytes).map_err(|source| {
        AssetStartupError::LangAssetsDecode {
            path: path.clone(),
            source,
            rebuild_command: lang_assets_rebuild_command(&path),
        }
    })?;
    let expected = canonical_source_manifest_sha256(vanilla_source_json);
    if runtime.source_manifest_sha256() != expected {
        return Err(AssetStartupError::LangAssetsProvenance {
            rebuild_command: lang_assets_rebuild_command(&path),
            carrier: format_sha256(runtime.source_manifest_sha256()),
            manifest: format_sha256(expected),
            path,
        });
    }
    // The carrier must also have been compiled from the exact pinned
    // `texts/en_US.lang` bytes: a tampered language file beside the
    // canonical manifest fails closed here.
    if runtime.lang_source_sha256() != assets::VANILLA_EN_US_LANG_SHA256 {
        return Err(AssetStartupError::LangAssetsSourceProvenance {
            rebuild_command: lang_assets_rebuild_command(&path),
            carrier: format_sha256(runtime.lang_source_sha256()),
            pinned: format_sha256(assets::VANILLA_EN_US_LANG_SHA256),
            path,
        });
    }
    Ok(LoadedLangAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
    })
}
