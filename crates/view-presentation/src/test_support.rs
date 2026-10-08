use std::sync::Arc;
use assets::{RuntimeFontCatalog, FontTexturePage, FontPixels, GlyphMetrics, encode_font_catalog};
use sha2::{Digest, Sha256};

pub fn fixture_font() -> Arc<RuntimeFontCatalog> {
    let pixels = vec![255; 16 * 16 * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/page.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 16,
        height: 16,
        pixels: FontPixels::Rgba8(pixels),
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

pub fn anchor(name: &str) -> crate::nametags::NametagAnchor {
    crate::nametags::NametagAnchor {
        runtime_id: 1, position: bevy::math::Vec3::ZERO, lines: vec![Arc::from(name)],
        depth_tested: false, text_alpha: 1.0, distance: 1.0,
    }
}
