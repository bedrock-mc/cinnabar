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

/// A hash-checked carrier and its required, decoded item glint artwork.
#[derive(Debug)]
pub struct LoadedUiAssets {
    pub assets: Arc<RuntimeUiAssets>,
    pub item_glint: Arc<render_model::UiTexturePage>,
}

/// The carrier path beside the selected world carrier.
pub fn ui_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(UI_ASSETS_FILENAME)
}

/// The decoded carrier and required artwork, or an error naming its path and rebuild command.
pub fn require_ui_assets(world_asset_path: &Path) -> anyhow::Result<LoadedUiAssets> {
    let path = ui_asset_path(world_asset_path);
    let loaded = read_carrier(&path).map_err(|reason| {
        anyhow::anyhow!(
            "required JSON-UI carrier at {} is unusable ({reason}); build it with `{UI_ASSETS_COMPILE_COMMAND}`, or refresh every required carrier with `make assets`",
            path.display()
        )
    })?;
    tracing::info!(
        "loaded JSON-UI carrier from {} ({} atlas pages, {} textures, {} ui files)",
        path.display(),
        loaded.assets.atlas_pages().len(),
        loaded.assets.textures().len(),
        loaded.assets.ui_files().len()
    );
    Ok(loaded)
}

/// Verifies the carrier envelope, pinned source and required item glint before startup accepts it.
fn read_carrier(path: &Path) -> Result<LoadedUiAssets, String> {
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
    let item_glint =
        super::presentation::item_glint::carrier_texture(&assets).ok_or_else(|| {
            format!(
                "missing or invalid required item glint raster {}.png",
                assets::entity::ITEM_GLINT_TEXTURE_IDENTIFIER
            )
        })?;
    Ok(LoadedUiAssets {
        assets: Arc::new(assets),
        item_glint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Writes a valid, pinned carrier with the supplied synthetic glint file.
    fn write_carrier(world: &Path, glint: Option<&[u8]>) {
        let mut files = vec![
            assets::UiFile {
                path: "ui/_global_variables.json".into(),
                bytes: b"{}".as_slice().into(),
            },
            assets::UiFile {
                path: "ui/_ui_defs.json".into(),
                bytes: br#"{"ui_defs":[]}"#.as_slice().into(),
            },
        ];
        if let Some(bytes) = glint {
            files.push(assets::UiFile {
                path: format!("{}.png", assets::entity::ITEM_GLINT_TEXTURE_IDENTIFIER).into(),
                bytes: bytes.into(),
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let manifest =
            assets::canonical_source_manifest_sha256(assets::VANILLA_SOURCE_MANIFEST.as_bytes());
        let bytes = assets::encode_ui_catalog(manifest, &[], &[], &[], &files).unwrap();
        std::fs::write(ui_asset_path(world), bytes).unwrap();
    }

    #[test]
    fn missing_or_invalid_item_glint_fails_startup_with_path_and_rebuild_hint() {
        let directory = tempfile::tempdir().unwrap();
        let world = directory.path().join(assets::carriers::WORLD.output);
        for glint in [None, Some(b"invalid PNG".as_slice())] {
            write_carrier(&world, glint);
            let error = require_ui_assets(&world).unwrap_err().to_string();
            assert!(error.contains("missing or invalid required item glint raster"));
            assert!(error.contains(&ui_asset_path(&world).display().to_string()));
            assert!(error.contains("make assets"));
        }
    }

    #[test]
    fn previous_ui_carrier_version_fails_startup_with_rebuild_hint() {
        let directory = tempfile::tempdir().unwrap();
        let world = directory.path().join(assets::carriers::WORLD.output);
        write_carrier(&world, None);
        let path = ui_asset_path(&world);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..12].copy_from_slice(&(assets::UI_CARRIER_VERSION - 1).to_le_bytes());
        let payload_end = bytes.len() - Sha256::output_size();
        let hash = Sha256::digest(&bytes[..payload_end]);
        bytes[payload_end..].copy_from_slice(&hash);
        std::fs::write(&path, bytes).unwrap();
        let error = require_ui_assets(&world).unwrap_err().to_string();
        assert!(error.contains("unsupported ui carrier header"));
        assert!(error.contains(&path.display().to_string()));
        assert!(error.contains("make assets"));
    }

    #[test]
    fn startup_retains_the_validated_item_glint_pixels() {
        let directory = tempfile::tempdir().unwrap();
        let world = directory.path().join(assets::carriers::WORLD.output);
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(3, 2, image::Rgba([30, 90, 150, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        write_carrier(&world, Some(png.get_ref()));
        let loaded = require_ui_assets(&world).unwrap();
        assert_eq!(loaded.item_glint.dimensions(), [3, 2]);
        assert_eq!(loaded.item_glint.pixels(), [30, 90, 150, 255].repeat(6));
        let glint = loaded.item_glint.clone();
        let prepared = super::super::presentation::PreparedJsonUi::from_loaded(loaded).unwrap();
        let mut presentation = super::super::presentation::UiPresentationRuntime::new(
            ui_engine::test_support::fixture_font(),
        )
        .unwrap();
        presentation.enable_prepared_json_ui(prepared).unwrap();
        assert!(Arc::ptr_eq(
            presentation.item_glint_texture().unwrap(),
            &glint
        ));
    }
}
