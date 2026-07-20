//! Required pinned HUD carrier loading, split from the asset-startup root to
//! honor the production module line budget.

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::RuntimeHudCatalog;

use super::{
    AssetStartupError, DEFAULT_ASSET_PATH, HUD_ASSETS_COMPILE_COMMAND, HUD_ASSETS_FILENAME,
    HUD_ASSETS_REPORT_FILENAME, MAX_HUD_ASSET_BLOB_BYTES, format_sha256,
};

pub struct LoadedHudAssets {
    runtime: Arc<RuntimeHudCatalog>,
    selected_path: PathBuf,
}

impl LoadedHudAssets {
    #[must_use]
    pub fn runtime(&self) -> &Arc<RuntimeHudCatalog> {
        &self.runtime
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        format!(
            "loaded pinned official Mojang sample HUD assets from {} (source_manifest_sha256={})",
            self.selected_path.display(),
            format_sha256(self.runtime.source_manifest_sha256())
        )
    }

    pub fn into_runtime(self) -> Arc<RuntimeHudCatalog> {
        self.runtime
    }
}

#[must_use]
pub fn hud_assets_missing_notice(path: &Path) -> String {
    let rebuild_command = hud_assets_rebuild_command(path);
    let recovery = if rebuild_command == HUD_ASSETS_COMPILE_COMMAND {
        format!(
            "Build only this carrier with `{HUD_ASSETS_COMPILE_COMMAND}`, or refresh every required carrier with `make assets`."
        )
    } else {
        format!("Build the carrier at that exact custom location with `{rebuild_command}`.")
    };
    format!(
        "required pinned official Mojang sample HUD carrier was not found at {}; the survival HUD cannot render, so the client will not start. {recovery}",
        path.display()
    )
}

/// Returns a copy-paste recovery command that writes the carrier where startup looked for it.
#[must_use]
pub fn hud_assets_rebuild_command(path: &Path) -> String {
    let default_path = hud_asset_path(Path::new(DEFAULT_ASSET_PATH));
    if path == default_path {
        return HUD_ASSETS_COMPILE_COMMAND.to_owned();
    }

    let report_path = path.with_file_name(HUD_ASSETS_REPORT_FILENAME);
    format!(
        "{HUD_ASSETS_COMPILE_COMMAND} HUD_ASSET_BLOB={} HUD_ASSET_REPORT={}",
        shell_quote_path(path),
        shell_quote_path(&report_path)
    )
}

#[cfg(windows)]
fn shell_quote_path(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("'{}'", path.replace('\'', "''"))
}

#[cfg(not(windows))]
fn shell_quote_path(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}

#[must_use]
pub fn hud_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(HUD_ASSETS_FILENAME)
}

/// Probes for and validates the HUD carrier adjacent to `world_asset_path`.
///
/// This low-level API returns `Ok(None)` only to let tooling inspect whether a carrier exists. The
/// production client must call [`require_hud_assets`] so absence is a fatal, actionable startup
/// error. A present but malformed, oversized, or stale carrier always fails closed here.
pub fn load_hud_assets(
    world_asset_path: &Path,
) -> Result<Option<LoadedHudAssets>, AssetStartupError> {
    let path = hud_asset_path(world_asset_path);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(AssetStartupError::HudAssetsRead {
                path,
                source,
                rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
            });
        }
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::HudAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        })?
        .len();
    if length > MAX_HUD_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::HudAssetsTooLarge {
            path,
            max_bytes: MAX_HUD_ASSET_BLOB_BYTES,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_HUD_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::HudAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        })?;
    if bytes.len() as u64 > MAX_HUD_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::HudAssetsTooLarge {
            path,
            max_bytes: MAX_HUD_ASSET_BLOB_BYTES,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        });
    }
    let runtime =
        RuntimeHudCatalog::decode(&bytes).map_err(|source| AssetStartupError::HudAssetsDecode {
            path: path.clone(),
            source,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        })?;
    Ok(Some(LoadedHudAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
    }))
}

/// Loads the pinned HUD carrier, failing closed when it is absent.
///
/// The renderer treats the native survival HUD as required art. A missing carrier is a fatal
/// startup error rather than a silent degrade: without it the survival HUD would be invisible
/// with no on-screen indication of why. Malformed or stale carriers already fail through
/// [`load_hud_assets`]; this wrapper additionally rejects the absent case with the shared
/// [`hud_assets_missing_notice`] guidance.
pub fn require_hud_assets(world_asset_path: &Path) -> Result<LoadedHudAssets, AssetStartupError> {
    load_hud_assets(world_asset_path)?.ok_or_else(|| {
        let path = hud_asset_path(world_asset_path);
        AssetStartupError::HudAssetsMissing {
            notice: hud_assets_missing_notice(&path),
            rebuild_command: hud_assets_rebuild_command(&path),
            path,
        }
    })
}
