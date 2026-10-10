use assets::{
    FontCatalogError, FontPixels, FontTexturePage, GlyphMetrics, RuntimeFontCatalog,
    encode_font_catalog,
};
use sha2::{Digest, Sha256};

const SOURCE_MANIFEST_SHA256: [u8; 32] = [0x42; 32];

#[test]
fn runtime_uses_existing_multi_page_glyph_routes_without_provider_changes() {
    let page = |name: &str, alpha| {
        let pixels = vec![255, 255, 255, alpha].into_boxed_slice();
        FontTexturePage {
            source_path: name.into(),
            source_bytes: 1,
            source_sha256: [0x24; 32],
            pixels_sha256: Sha256::digest(&pixels).into(),
            width: 1,
            height: 1,
            pixels: FontPixels::Rgba8(pixels),
        }
    };
    let glyph = |codepoint, page| GlyphMetrics {
        codepoint,
        page,
        uv: [0, 0, 1, 1],
        bearing: [0, -1],
        advance_64: 512,
    };
    let glyphs = [glyph('A', 0), glyph('世', 1)];
    let bytes = encode_font_catalog(
        SOURCE_MANIFEST_SHA256,
        &glyphs,
        &[
            page("font/primary.png", 255),
            page("font/secondary.png", 128),
        ],
    )
    .unwrap();
    let catalog = RuntimeFontCatalog::decode(&bytes, SOURCE_MANIFEST_SHA256).unwrap();
    assert_eq!(catalog.glyph('A').unwrap().page, 0);
    assert_eq!(catalog.glyph('世').unwrap().page, 1);
    assert_eq!(catalog.pages()[1].pixels.bytes()[3], 128);
}

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
        pixels: FontPixels::Rgba8(pixels.clone()),
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

    assert_eq!(catalog.identity().schema, assets::FONT_CARRIER_SCHEMA);
    assert_eq!(
        catalog.identity().source_manifest_sha256,
        SOURCE_MANIFEST_SHA256
    );
    let carrier_sha256: [u8; 32] = Sha256::digest(&bytes[..bytes.len() - 32]).into();
    assert_eq!(catalog.identity().carrier_sha256, carrier_sha256);
    assert_eq!(catalog.glyphs(), &[glyph]);
    assert_eq!(catalog.glyph('A'), Some(&glyph));
    assert_eq!(catalog.pages()[0].pixels.bytes(), pixels.as_ref());
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
        pixels: FontPixels::Rgba8(rgba8),
    }
}

fn resign(bytes: &mut [u8]) {
    let hash_offset = bytes.len() - 32;
    let digest = Sha256::digest(&bytes[..hash_offset]);
    bytes[hash_offset..].copy_from_slice(&digest);
}

#[test]
fn review_glyph_identity_includes_bearings_and_exact_draw_sizes() {
    let catalog = RuntimeFontCatalog::decode(&carrier(), SOURCE_MANIFEST_SHA256).unwrap();
    let glyph = assets::SheetGlyph {
        metrics: GlyphMetrics {
            codepoint: 'A',
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        },
        draw_size_64: [64, 64],
    };
    let first = catalog.with_glyphs(&[glyph], |_| true).identity();
    let mut moved = glyph;
    moved.metrics.bearing[0] = 1;
    assert_ne!(first, catalog.with_glyphs(&[moved], |_| true).identity());
    let mut resized = glyph;
    resized.draw_size_64[0] += 1;
    assert_ne!(first, catalog.with_glyphs(&[resized], |_| true).identity());
}

#[test]
fn attached_named_font_keeps_default_metrics_and_rebases_private_pages() {
    let base = RuntimeFontCatalog::decode(&carrier(), SOURCE_MANIFEST_SHA256).unwrap();
    let mut private_page = page();
    let FontPixels::Rgba8(pixels) = &mut private_page.pixels else {
        unreachable!("authored pages are RGBA");
    };
    pixels[3] = 128;
    private_page.pixels_sha256 = Sha256::digest(&pixels[..]).into();
    let glyph = GlyphMetrics {
        advance_64: 192,
        ..*base.glyph('A').unwrap()
    };
    let private_bytes =
        encode_font_catalog(SOURCE_MANIFEST_SHA256, &[glyph], &[private_page]).unwrap();
    let private = RuntimeFontCatalog::decode(&private_bytes, SOURCE_MANIFEST_SHA256)
        .unwrap()
        .with_linear_sampling();
    let combined = base.with_named_font("private_controls", &private).unwrap();
    assert_eq!(combined.glyphs(), base.glyphs());
    assert_eq!(combined.pages()[0], base.pages()[0]);
    assert_eq!(combined.pages().len(), 2);
    let alias = combined.font_named("private_controls");
    assert!(!combined.linear_sampling());
    assert!(alias.linear_sampling());
    assert_eq!(alias.glyph('A').unwrap().page, 1);
    assert_eq!(alias.glyph('A').unwrap().advance_64, 192);
    assert_eq!(combined.pages()[1].pixels.bytes()[3], 128);
    assert_eq!(combined.font_named("unknown").glyph('A'), base.glyph('A'));
    let next = combined
        .with_named_font("second_controls", &private)
        .unwrap();
    assert_eq!(
        next.font_named("second_controls").glyph('A').unwrap().page,
        2
    );
    assert_ne!(
        alias.identity(),
        next.font_named("second_controls").identity()
    );
    assert!(base.with_named_font("", &private).is_err());
}

/// Coverage storage samples like the RGBA page wherever a glyph is visible, at a quarter of the bytes.
#[test]
fn coverage_pages_match_rgba_alpha_on_sample_glyphs_and_drop_the_rgba_copy() {
    // A white glyph edge over transparent texels whose colour nearest sampling never shows.
    let rgba: Vec<u8> = [
        [255, 255, 255, 255],
        [255, 255, 255, 128],
        [0, 0, 0, 0],
        [7, 9, 3, 0],
    ]
    .concat();
    let white = FontTexturePage {
        source_path: "font/glyph.png".into(),
        source_bytes: 1,
        source_sha256: [0x24; 32],
        pixels_sha256: Sha256::digest(&rgba).into(),
        width: 2,
        height: 2,
        pixels: FontPixels::Rgba8(rgba.clone().into()),
    };
    let mut tinted = vec![255; 16];
    tinted[0] = 200;
    let colour = FontTexturePage {
        source_path: "font/tinted.png".into(),
        pixels_sha256: Sha256::digest(&tinted).into(),
        pixels: FontPixels::Rgba8(tinted.into()),
        ..white.clone()
    };
    let glyph = |codepoint, page| GlyphMetrics {
        codepoint,
        page,
        uv: [0, 0, 2, 2],
        bearing: [0, -2],
        advance_64: 128,
    };
    let bytes = encode_font_catalog(
        SOURCE_MANIFEST_SHA256,
        &[glyph('A', 0), glyph('B', 1)],
        &[white, colour],
    )
    .unwrap();
    let decoded = RuntimeFontCatalog::decode(&bytes, SOURCE_MANIFEST_SHA256).unwrap();
    let coverage = decoded.clone().with_coverage_pages();
    let (original, stored) = (&decoded.pages()[0].pixels, &coverage.pages()[0].pixels);
    assert!(matches!(stored, FontPixels::Coverage(bytes) if bytes.len() == 4));
    assert!(stored.rgba8().is_none(), "the RGBA copy is gone");
    for index in 0..4 {
        let (rgba, sampled) = (original.texel(index).unwrap(), stored.texel(index).unwrap());
        assert_eq!(sampled[3], rgba[3], "texel {index} alpha");
        if rgba[3] != 0 {
            assert_eq!(sampled, rgba, "visible texel {index}");
        }
    }
    assert!(
        matches!(coverage.pages()[1].pixels, FontPixels::Rgba8(_)),
        "coloured pages stay RGBA"
    );
    assert!(matches!(
        decoded.with_linear_sampling().with_coverage_pages().pages()[0].pixels,
        FontPixels::Rgba8(_)
    ));
}
