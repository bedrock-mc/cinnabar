use std::{path::PathBuf, sync::Arc};

use crate::{FontPixels, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, encode_font_catalog};
use sha2::{Digest, Sha256};

use super::{AssetStartupError, FONT_ASSETS_COMPILE_COMMAND, LoadedFontAssets};

/// Wraps the diagnostic catalog with the failed optional carrier path.
pub(super) fn diagnostic_font_assets(path: PathBuf) -> Result<LoadedFontAssets, AssetStartupError> {
    let runtime =
        diagnostic_font_catalog().map_err(|source| AssetStartupError::FontAssetsDecode {
            path: path.clone(),
            source: Box::new(source),
            rebuild_command: FONT_ASSETS_COMPILE_COMMAND,
        })?;
    Ok(LoadedFontAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
        diagnostic: true,
    })
}

/// Creates the existing diagnostic glyph when no optional font carrier is available.
pub fn diagnostic_font_catalog() -> Result<RuntimeFontCatalog, crate::FontCatalogError> {
    const DIAGNOSTIC_MANIFEST: [u8; 32] = [0xd1; 32];
    let rgba8 = vec![255, 255, 255, 255].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/builtin-diagnostic.png".into(),
        source_bytes: 4,
        source_sha256: Sha256::digest(&rgba8).into(),
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 1,
        height: 1,
        pixels: FontPixels::Rgba8(rgba8.into()),
    };
    let glyph = GlyphMetrics {
        codepoint: '\u{fffd}',
        page: 0,
        uv: [0, 0, 1, 1],
        bearing: [0, 0],
        advance_64: 64,
    };
    let bytes = encode_font_catalog(DIAGNOSTIC_MANIFEST, &[glyph], &[page])?;
    RuntimeFontCatalog::decode(&bytes, DIAGNOSTIC_MANIFEST)
}
