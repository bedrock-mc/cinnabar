use super::SessionGlyphSheets;

/// Builds distinct small rasters, so the atlas cannot hide insufficient capacity by deduplication.
fn cell(codepoint: char, index: usize) -> assets::CellGlyph {
    let mut rgba8 = vec![255; 16 * 16 * 4];
    rgba8[..8].copy_from_slice(&(index as u64).to_le_bytes());
    assets::CellGlyph {
        codepoint,
        size: [16, 16],
        rgba8: rgba8.into(),
        bearing: [0, 0],
        advance_64: 16 * 64,
        draw_size_64: [16 * 64; 2],
    }
}

#[test]
fn complete_item_font_keeps_its_small_spacing_glyphs() {
    let mut cells: Vec<_> = (0..2000)
        .map(|index| cell(char::from_u32(0xe000 + index as u32).unwrap(), index))
        .collect();
    cells.push(assets::CellGlyph {
        codepoint: '\u{f800}',
        size: [1, 1],
        rgba8: vec![255; 4].into(),
        bearing: [0, 0],
        advance_64: 16 * 64,
        draw_size_64: [64; 2],
    });
    let sheets = SessionGlyphSheets::with_named(cells, Default::default());
    assert_eq!(sheets.prepared().glyphs.len(), 2001);
    assert!(
        sheets
            .prepared()
            .glyphs
            .iter()
            .any(|glyph| glyph.metrics.codepoint == '\u{f800}')
    );
}

#[test]
fn admitted_item_font_retains_its_spacing_and_count_glyphs() {
    let Some(sheets) = crate::ui_runtime::presentation::forms::pack_harness::env_glyphs() else {
        eprintln!(
            "skipping admitted_item_font_retains_its_spacing_and_count_glyphs: missing CINNABAR_FORM_PACK_DIR fixture"
        );
        return;
    };
    let prepared = sheets.prepared();
    for codepoint in ['\u{e5ff}', '\u{f800}', '\u{f820}'] {
        assert!(
            prepared
                .glyphs
                .iter()
                .any(|glyph| glyph.metrics.codepoint == codepoint),
            "missing {codepoint:?} from {} packed glyphs",
            prepared.glyphs.len()
        );
    }
}
