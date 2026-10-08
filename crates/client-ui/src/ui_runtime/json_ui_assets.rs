//! The required JSON-UI carrier (`make ui-assets`): the gameplay HUD, forms, and
//! engine screens draw from it, so startup fails closed when it is missing,
//! unreadable, malformed, or compiled from another pinned source manifest.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{MAX_UI_CARRIER_BYTES, RuntimeUiAssets};

pub const UI_ASSETS_FILENAME: &str = assets::carriers::UI.output;
const UI_ASSETS_COMPILE_COMMAND: &str = "make ui-assets";

/// The carrier path beside the selected world carrier.
pub fn ui_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(UI_ASSETS_FILENAME)
}

/// The decoded carrier, or an error naming its path and rebuild command.
pub fn require_ui_assets(world_asset_path: &Path) -> anyhow::Result<Arc<RuntimeUiAssets>> {
    let path = ui_asset_path(world_asset_path);
    let assets = read_carrier(&path).map_err(|reason| {
        anyhow::anyhow!(
            "required JSON-UI carrier at {} is unusable ({reason}); build it with `{UI_ASSETS_COMPILE_COMMAND}`, or refresh every required carrier with `make assets`",
            path.display()
        )
    })?;
    eprintln!(
        "loaded JSON-UI carrier from {} ({} atlas pages, {} textures, {} ui files)",
        path.display(),
        assets.atlas_pages().len(),
        assets.textures().len(),
        assets.ui_files().len()
    );
    Ok(Arc::new(assets))
}

fn read_carrier(path: &Path) -> Result<RuntimeUiAssets, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_UI_CARRIER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_UI_CARRIER_BYTES {
        return Err(format!("exceeds {MAX_UI_CARRIER_BYTES} bytes"));
    }
    let assets = RuntimeUiAssets::decode(&bytes).map_err(|error| error.to_string())?;
    let expected =
        assets::canonical_source_manifest_sha256(assets::VANILLA_SOURCE_MANIFEST.as_bytes());
    if assets.source_manifest_sha256() != expected {
        return Err("compiled from a different pinned source manifest".to_owned());
    }
    Ok(assets)
}
