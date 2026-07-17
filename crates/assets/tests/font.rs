#[path = "../src/font.rs"]
mod font;

use font::{
    FontCatalogError, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, encode_font_catalog,
};
use sha2::{Digest, Sha256};

const SOURCE_MANIFEST_SHA256: [u8; 32] = [0x42; 32];

#[test]
fn runtime_decodes_exact_provenance_and_unmodified_rgba8() {
    let pixels = vec![1, 2, 3, 128, 9, 8, 7, 64].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/default8.png".into(),
        source_bytes: 17,
        source_sha256: [0x24; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 2,
        height: 1,
        rgba8: pixels.clone(),
    };
    let glyph = GlyphMetrics {
        codepoint: 'A',
        page: 0,
        uv: [0, 0, 1, 1],
        bearing: [0, -1],
        advance_64: 512,
    };
    let bytes = encode_font_catalog(SOURCE_MANIFEST_SHA256, &[glyph], &[page]).unwrap();
    let catalog = RuntimeFontCatalog::decode(&bytes, SOURCE_MANIFEST_SHA256).unwrap();

    assert_eq!(catalog.identity().schema, 1);
    assert_eq!(
        catalog.identity().source_manifest_sha256,
        SOURCE_MANIFEST_SHA256
    );
    let carrier_sha256: [u8; 32] = Sha256::digest(&bytes[..bytes.len() - 32]).into();
    assert_eq!(catalog.identity().carrier_sha256, carrier_sha256);
    assert_eq!(catalog.glyphs(), &[glyph]);
    assert_eq!(catalog.glyph('A'), Some(&glyph));
    assert_eq!(catalog.pages()[0].rgba8.as_ref(), pixels.as_ref());
}

#[test]
fn runtime_rejects_wrong_provenance_hash_and_offsets_before_payload_use() {
    let bytes = carrier();
    assert!(matches!(
        RuntimeFontCatalog::decode(&bytes, [0x99; 32]),
        Err(FontCatalogError::SourceManifestMismatch)
    ));

    let mut corrupt_hash = bytes.to_vec();
    corrupt_hash[96] ^= 1;
    assert!(matches!(
        RuntimeFontCatalog::decode(&corrupt_hash, SOURCE_MANIFEST_SHA256),
        Err(FontCatalogError::CarrierHashMismatch)
    ));

    let mut corrupt_offset = bytes.to_vec();
    corrupt_offset[53..61].copy_from_slice(&u64::MAX.to_le_bytes());
    resign(&mut corrupt_offset);
    assert!(matches!(
        RuntimeFontCatalog::decode(&corrupt_offset, SOURCE_MANIFEST_SHA256),
        Err(FontCatalogError::InvalidCarrier { .. })
    ));
}

#[test]
fn encoder_rejects_duplicate_glyphs_and_invalid_page_references() {
    let page = page();
    let glyph = GlyphMetrics {
        codepoint: 'A',
        page: 0,
        uv: [0, 0, 1, 1],
        bearing: [0, 0],
        advance_64: 64,
    };
    assert!(matches!(
        encode_font_catalog(
            SOURCE_MANIFEST_SHA256,
            &[glyph, glyph],
            std::slice::from_ref(&page)
        ),
        Err(FontCatalogError::InvalidCatalog { .. })
    ));
    assert!(matches!(
        encode_font_catalog(
            SOURCE_MANIFEST_SHA256,
            &[GlyphMetrics { page: 1, ..glyph }],
            &[page]
        ),
        Err(FontCatalogError::InvalidCatalog { .. })
    ));
}

fn carrier() -> Box<[u8]> {
    let glyph = GlyphMetrics {
        codepoint: 'A',
        page: 0,
        uv: [0, 0, 1, 1],
        bearing: [0, 0],
        advance_64: 64,
    };
    encode_font_catalog(SOURCE_MANIFEST_SHA256, &[glyph], &[page()]).unwrap()
}

fn page() -> FontTexturePage {
    let rgba8 = vec![255, 255, 255, 255].into_boxed_slice();
    FontTexturePage {
        source_path: "font/default8.png".into(),
        source_bytes: 8,
        source_sha256: [0x11; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 1,
        height: 1,
        rgba8,
    }
}

fn resign(bytes: &mut [u8]) {
    let hash_offset = bytes.len() - 32;
    let digest = Sha256::digest(&bytes[..hash_offset]);
    bytes[hash_offset..].copy_from_slice(&digest);
}
