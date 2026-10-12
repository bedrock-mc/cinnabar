//! Fixtures shared with tests in the crates that paint through the engine.

use std::sync::Arc;

use assets::{
    FontPixels, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, RuntimeUiAssets,
    encode_font_catalog,
};
use sha2::{Digest, Sha256};

/// Models the shipped atlas: ink 16 texels tall sitting 14 above the baseline
/// with a 12-texel advance, so layout geometry in these tests matches what the
/// client actually lays out. A fixture whose glyphs hang below the baseline
/// instead makes every row report far more height than it occupies.
pub fn fixture_font() -> Arc<RuntimeFontCatalog> {
    let pixels = vec![255; 16 * 16 * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/page.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 16,
        height: 16,
        pixels: FontPixels::Rgba8(pixels.into()),
    };
    let glyphs = ['/', '0', '2', '\u{fffd}'].map(|codepoint| GlyphMetrics {
        codepoint,
        page: 0,
        uv: [0, 0, 12, 16],
        bearing: [0, -14],
        advance_64: 12 * 64,
    });
    let manifest = [7; 32];
    let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
    Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
}

/// Loads the installed UI carrier, reporting an absent fixture and rejecting malformed data.
pub fn carrier() -> Option<Arc<RuntimeUiAssets>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("../.local")
        .join("assets/compiled/vanilla-v1.mcbeui");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping local UI fixture test: missing {}; make assets",
                path.display()
            );
            return None;
        }
        Err(error) => panic!("read UI fixture {}: {error}", path.display()),
    };
    Some(Arc::new(
        RuntimeUiAssets::decode(&bytes).expect("decode installed UI fixture"),
    ))
}
