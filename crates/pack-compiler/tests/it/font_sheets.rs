use std::{fs, path::Path};

use assets::{
    FONT_CARRIER_SCHEMA, FontPixels, FontTexturePage, GlyphMetrics, GlyphSheet, RuntimeFontCatalog,
    encode_font_catalog, extract_cells,
};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use pack_compiler::{
    CompiledFontCarrier, FontCompileReport, compact_font_pages, overlay_font_glyph_sheets,
};
use sha2::{Digest, Sha256};

fn base() -> CompiledFontCarrier {
    let pixels = vec![255; 64 * 64 * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/base.png".into(),
        source_bytes: 1,
        source_sha256: [2; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 64,
        height: 64,
        pixels: FontPixels::Rgba8(pixels),
    };
    let glyphs = ['A', '中'].map(|codepoint| GlyphMetrics {
        codepoint,
        page: 0,
        uv: [2, 2, 10, 10],
        bearing: [1, -10],
        advance_64: 9 * 64,
    });
    let bytes = encode_font_catalog([1; 32], &glyphs, &[page]).unwrap();
    CompiledFontCarrier {
        report: FontCompileReport {
            schema: FONT_CARRIER_SCHEMA,
            glyphs: glyphs.len(),
            pages: 1,
            source_bytes: 1,
            decoded_bytes: 64 * 64 * 4,
            source_manifest_sha256: [1; 32],
            carrier_sha256: bytes[bytes.len() - 32..].try_into().unwrap(),
        },
        bytes,
    }
}

fn write_png(path: &Path, size: [u32; 2], pixels: &[u8]) {
    PngEncoder::new(fs::File::create(path).unwrap())
        .write_image(pixels, size[0], size[1], ExtendedColorType::Rgba8)
        .unwrap();
}

#[test]
fn server_private_glyph_uses_native_metrics_and_pixels_without_losing_fallback() {
    let pack = tempfile::tempdir().unwrap();
    fs::create_dir(pack.path().join("font")).unwrap();
    let mut pixels = vec![0; 128 * 128 * 4];
    for y in 1..7 {
        let pixel = (y * 128 + 3) * 4;
        pixels[pixel..pixel + 4].copy_from_slice(&[255, 255, 85, 255]);
    }
    write_png(&pack.path().join("font/glyph_E1.png"), [128, 128], &pixels);
    let sheet = GlyphSheet {
        high_byte: 0xe1,
        width: 128,
        height: 128,
        rgba8: pixels.into(),
    };
    let cells = extract_cells(&sheet);
    let native = &cells[0];
    let original = base();
    let before = RuntimeFontCatalog::decode(&original.bytes, [1; 32]).unwrap();
    let compiled = overlay_font_glyph_sheets(original.clone(), pack.path()).unwrap();
    assert_eq!(
        compiled.bytes,
        overlay_font_glyph_sheets(original, pack.path())
            .unwrap()
            .bytes
    );
    let font = RuntimeFontCatalog::decode(&compiled.bytes, [1; 32]).unwrap();
    for codepoint in ['A', '中'] {
        assert_eq!(font.glyph(codepoint), before.glyph(codepoint));
    }
    let glyph = font.glyph('\u{e100}').unwrap();
    assert_eq!(glyph.bearing, native.bearing);
    assert_eq!(glyph.advance_64, native.advance_64);
    assert_eq!(
        [glyph.uv[2] - glyph.uv[0], glyph.uv[3] - glyph.uv[1]].map(u32::from),
        native.draw_size_64.map(|value| value / 64),
    );
    let page = &font.pages()[usize::from(glyph.page)];
    for y in glyph.uv[1]..glyph.uv[3] {
        for x in glyph.uv[0]..glyph.uv[2] {
            let pixel = ((u32::from(y) * page.width + u32::from(x)) * 4) as usize;
            assert_eq!(&page.pixels.bytes()[pixel..pixel + 4], &[255, 255, 85, 255]);
        }
    }
    let blank = font.glyph('\u{e101}').unwrap();
    assert_eq!(blank.advance_64, 0);
    let page = &font.pages()[usize::from(blank.page)];
    let pixel = ((u32::from(blank.uv[1]) * page.width + u32::from(blank.uv[0])) * 4) as usize;
    assert_eq!(&page.pixels.bytes()[pixel..pixel + 4], &[0; 4]);
    assert_ne!(
        font.identity().carrier_sha256,
        before.identity().carrier_sha256
    );
}

#[test]
fn compact_pages_preserves_all_metrics_ink_and_sampling_gutters() {
    let original = base();
    let before = RuntimeFontCatalog::decode(&original.bytes, [1; 32]).unwrap();
    let compacted = compact_font_pages(original).unwrap();
    let after = RuntimeFontCatalog::decode(&compacted.bytes, [1; 32]).unwrap();
    assert_eq!(before.glyphs(), after.glyphs());
    assert_eq!([after.pages()[0].width, after.pages()[0].height], [16, 16]);
    for y in 0..16 {
        assert_eq!(
            &before.pages()[0].pixels.bytes()[y * 64 * 4..y * 64 * 4 + 16 * 4],
            &after.pages()[0].pixels.bytes()[y * 16 * 4..(y + 1) * 16 * 4]
        );
    }
    assert_eq!(
        after.pages()[0].source_sha256,
        before.pages()[0].source_sha256
    );
    assert_eq!(compacted.report.decoded_bytes, 16 * 16 * 4);
    assert_eq!(compact_font_pages(compacted.clone()).unwrap(), compacted);
}

#[test]
fn malformed_optional_glyph_pack_fails_before_publication() {
    let pack = tempfile::tempdir().unwrap();
    fs::create_dir(pack.path().join("font")).unwrap();
    assert!(overlay_font_glyph_sheets(base(), pack.path()).is_err());
    write_png(
        &pack.path().join("font/glyph_E1.png"),
        [17, 16],
        &vec![0; 17 * 16 * 4],
    );
    assert!(overlay_font_glyph_sheets(base(), pack.path()).is_err());
}
