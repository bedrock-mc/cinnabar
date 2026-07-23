//! Required pinned item-icon carrier loading, split from the asset-startup
//! root to honor the production line budget.
//!
//! Hotbar and offhand item icons are player-facing pixels, so production
//! startup requires this carrier exactly like the HUD and localization
//! carriers: a missing, oversized, malformed, or stale-provenance carrier is
//! a fatal, actionable error naming the exact path and rebuild command.

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::RuntimeIconCatalog;

use super::{
    AssetStartupError, DEFAULT_ASSET_PATH, canonical_source_manifest_sha256, format_sha256,
};

pub const ICON_ASSETS_FILENAME: &str = "vanilla-v1.mcbeico";
pub const ICON_ASSETS_COMPILE_COMMAND: &str = "make icon-assets";
const ICON_ASSETS_REPORT_FILENAME: &str = "icon-assets.json";
const MAX_ICON_ASSET_BLOB_BYTES: u64 = assets::MAX_ICON_CARRIER_BYTES as u64;

/// Returns a copy-paste recovery command that writes the icon carrier where
/// startup looked for it: the bare make target at the default location, or
/// the same target with `ICON_ASSET_BLOB`/`ICON_ASSET_REPORT` naming the
/// exact custom siblings.
#[must_use]
pub fn icon_assets_rebuild_command(path: &Path) -> String {
    let default_path = icon_asset_path(Path::new(DEFAULT_ASSET_PATH));
    if path == default_path {
        return ICON_ASSETS_COMPILE_COMMAND.to_owned();
    }
    let report_path = path.with_file_name(ICON_ASSETS_REPORT_FILENAME);
    format!(
        "{ICON_ASSETS_COMPILE_COMMAND} ICON_ASSET_BLOB={} ICON_ASSET_REPORT={}",
        super::shell_quote_path(path),
        super::shell_quote_path(&report_path)
    )
}

#[derive(Debug)]
pub struct LoadedIconAssets {
    runtime: Arc<RuntimeIconCatalog>,
    selected_path: PathBuf,
}

impl LoadedIconAssets {
    #[must_use]
    pub fn runtime(&self) -> &Arc<RuntimeIconCatalog> {
        &self.runtime
    }

    pub fn into_runtime(self) -> Arc<RuntimeIconCatalog> {
        self.runtime
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        format!(
            "loaded pinned official Mojang sample item icons from {} ({} sprites, {} entries, source_manifest_sha256={})",
            self.selected_path.display(),
            self.runtime.sprites().len(),
            self.runtime.entries().len(),
            format_sha256(self.runtime.source_manifest_sha256())
        )
    }
}

#[must_use]
pub fn icon_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(ICON_ASSETS_FILENAME)
}

/// Loads and validates the item-icon carrier beside `world_asset_path`,
/// failing closed on absence, size, decode, or stale provenance against the
/// embedded canonical `vanilla-source.json` identity.
pub fn require_icon_assets(
    world_asset_path: &Path,
    vanilla_source_json: &str,
) -> Result<LoadedIconAssets, AssetStartupError> {
    let path = icon_asset_path(world_asset_path);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            let rebuild_command = icon_assets_rebuild_command(&path);
            return Err(AssetStartupError::IconAssetsMissing {
                notice: format!(
                    "required pinned official Mojang sample item-icon carrier was not found at {}; hotbar and offhand icons cannot present, so the client will not start. Build it with `{rebuild_command}`, or refresh every required carrier with `make assets`.",
                    path.display()
                ),
                rebuild_command,
                path,
            });
        }
        Err(source) => {
            return Err(AssetStartupError::IconAssetsRead {
                rebuild_command: icon_assets_rebuild_command(&path),
                path,
                source,
            });
        }
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::IconAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: icon_assets_rebuild_command(&path),
        })?
        .len();
    if length > MAX_ICON_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::IconAssetsTooLarge {
            rebuild_command: icon_assets_rebuild_command(&path),
            path,
            max_bytes: MAX_ICON_ASSET_BLOB_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_ICON_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::IconAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: icon_assets_rebuild_command(&path),
        })?;
    if bytes.len() as u64 > MAX_ICON_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::IconAssetsTooLarge {
            rebuild_command: icon_assets_rebuild_command(&path),
            path,
            max_bytes: MAX_ICON_ASSET_BLOB_BYTES,
        });
    }
    let runtime = RuntimeIconCatalog::decode(&bytes).map_err(|source| {
        AssetStartupError::IconAssetsDecode {
            path: path.clone(),
            source,
            rebuild_command: icon_assets_rebuild_command(&path),
        }
    })?;
    let expected = canonical_source_manifest_sha256(vanilla_source_json);
    if runtime.source_manifest_sha256() != expected {
        return Err(AssetStartupError::IconAssetsProvenance {
            rebuild_command: icon_assets_rebuild_command(&path),
            carrier: format_sha256(runtime.source_manifest_sha256()),
            manifest: format_sha256(expected),
            path,
        });
    }
    Ok(LoadedIconAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
    })
}
