use super::*;

/// Reads a shipped face from its pinned manifest for coverage and raster regressions.
fn source(carrier: assets::carriers::Carrier) -> (std::path::PathBuf, Vec<u8>, Font) {
    let profile = carrier.font_face.unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(profile.manifest).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/fonts")
        .join(manifest["font_file"].as_str().unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let font = source::parse(&path, &bytes).unwrap();
    (path, bytes, font)
}

#[test]
fn shipped_heading_and_body_pixels_have_solid_coverage_and_equal_stems() {
    for carrier in [assets::carriers::FONT_SEVEN, assets::carriers::FONT_TEN] {
        let (_, _, font) = source(carrier);
        let em = carrier.font_face.unwrap().line_metrics().em_64 / 64;
        for codepoint in '!'..='~' {
            let glyph = rasterize(&font, codepoint, em, GlyphAdvances::Source).unwrap();
            assert!(
                glyph.alpha.iter().all(|&alpha| alpha == 0 || alpha == 255),
                "{} {codepoint}: fractional coverage at {em} px/em",
                carrier.name
            );
        }
        let glyph = rasterize(&font, 'H', em, GlyphAdvances::Source).unwrap();
        let row = &glyph.alpha[glyph.width as usize..2 * glyph.width as usize];
        let left = row.iter().take_while(|&&alpha| alpha == 255).count();
        let right = row.iter().rev().take_while(|&&alpha| alpha == 255).count();
        assert!(left > 0);
        assert_eq!(left, right, "{} has unequal H stems", carrier.name);
    }
}

#[test]
fn complete_carriers_keep_every_mapped_scalar_and_styled_motd_letters() {
    for carrier in assets::carriers::CARRIERS
        .iter()
        .filter(|carrier| carrier.font_face.is_some())
    {
        let (path, bytes, font) = source(*carrier);
        let face = carrier.font_face.unwrap();
        let manifest = assets::canonical_source_manifest_sha256(face.manifest);
        let compiled = compile_outline_font(
            &path,
            &bytes,
            manifest,
            OutlineFontConfig {
                pixel_height: face.raster_em_pixels(),
                atlas_side: 2048,
                synthesize_mathematical_letters: true,
                space_advance_64: face.space_advance_64(),
                ascii_bearing: face.ascii_bearing,
                ..OutlineFontConfig::default()
            },
        )
        .unwrap();
        let catalog = assets::RuntimeFontCatalog::decode(&compiled.bytes, manifest).unwrap();
        for &codepoint in font.chars().keys() {
            assert!(
                catalog.glyph(codepoint).is_some(),
                "{} missing U+{:04X}",
                face.name,
                codepoint as u32
            );
        }
        for codepoint in ['\u{2b50}', '\u{a000}', '\u{ac00}', '𝑩', '𝗦', '𝟭'] {
            assert!(
                catalog.glyph(codepoint).is_some(),
                "{} missing U+{:04X}",
                face.name,
                codepoint as u32
            );
        }
        if let Some(advance) = face.space_advance_64() {
            assert_eq!(catalog.glyph(' ').unwrap().advance_64, advance);
            assert_eq!(catalog.glyph('\u{a0}').unwrap().advance_64, advance);
        }
        if let Some(bearing) = face.ascii_bearing {
            for codepoint in ['A', 'I', ')', '}'] {
                let original = rasterize(
                    &font,
                    codepoint,
                    face.raster_em_pixels(),
                    GlyphAdvances::Source,
                )
                .unwrap();
                let adjusted = catalog.glyph(codepoint).unwrap();
                assert_eq!(
                    adjusted.bearing[0],
                    original.bearing[0] + bearing.offset(codepoint)
                );
                assert_eq!(adjusted.advance_64, original.advance_64);
            }
        }
        assert!(catalog.glyphs().len() >= font.chars().len());
        assert!(catalog.pages().len() > 1);
        for page in catalog.pages() {
            assert!(
                page.pixels
                    .bytes()
                    .iter()
                    .all(|&alpha| alpha == 0 || alpha == 255),
                "{} has off-grid source pixels",
                face.name
            );
        }
    }
}

#[test]
fn high_glyph_indices_remain_mapped_in_shipped_faces() {
    for carrier in [assets::carriers::FONT, assets::carriers::FONT_TEN] {
        let (_, _, font) = source(carrier);
        for codepoint in ['\u{a95f}', '\u{aa5c}', '\u{aa5f}', '\u{a640}'] {
            assert_ne!(
                font.lookup_glyph_index(codepoint),
                0,
                "{} lost mapped U+{:04X}",
                carrier.name,
                codepoint as u32
            );
        }
    }
}
