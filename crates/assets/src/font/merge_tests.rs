//! Adding glyphs merges them into the sorted table exactly as a full rebuild would.

use std::collections::BTreeMap;

use super::{GlyphMetrics, merge_glyphs};

fn glyph(codepoint: u32, page: u16) -> GlyphMetrics {
    GlyphMetrics {
        codepoint: char::from_u32(codepoint).unwrap(),
        page,
        uv: [0, 0, 1, 1],
        bearing: [0, 0],
        advance_64: 64,
    }
}

/// The table a map of every glyph produces, the added ones winning.
fn rebuilt(base: &[GlyphMetrics], added: &BTreeMap<char, GlyphMetrics>) -> Vec<GlyphMetrics> {
    let mut all: BTreeMap<char, GlyphMetrics> =
        base.iter().map(|glyph| (glyph.codepoint, *glyph)).collect();
    all.extend(added.iter().map(|(codepoint, glyph)| (*codepoint, *glyph)));
    all.into_values().collect()
}

#[test]
fn merged_tables_equal_rebuilt_ones() {
    let base: Vec<_> = (0x20..0x7f)
        .step_by(3)
        .map(|codepoint| glyph(codepoint, 0))
        .collect();
    for added in [
        Vec::new(),
        vec![glyph(0x10, 1)],
        vec![glyph(0x7e, 1), glyph(0x7f, 1), glyph(0xe000, 1)],
        vec![
            glyph(0x20, 2),
            glyph(0x23, 2),
            glyph(0x24, 2),
            glyph(0x21, 2),
        ],
        (0x18..0x90).map(|codepoint| glyph(codepoint, 3)).collect(),
    ] {
        let added: BTreeMap<_, _> = added
            .into_iter()
            .map(|glyph| (glyph.codepoint, glyph))
            .collect();
        assert_eq!(
            merge_glyphs(&base, added.clone()).to_vec(),
            rebuilt(&base, &added)
        );
    }
}
