//! Vanilla label wrapping: hyphen chops, overflowing glyphs, per-line
//! alignment, line padding, `...` at a line limit and explicit `§f` white.

use std::sync::Arc;

use assets::{CompiledFontCatalog, FontPixels, FontTexturePage, GlyphMetrics, encode_font_catalog};
use sha2::{Digest, Sha256};
use ui::{
    BedrockColor, TextError, TextLayout, TextLayoutCache, TextLayoutRequest, TextLineAlign,
    TextStyle, TextWrap, UiScale, WordChop,
};

// Every glyph is two texels wide with a two-texel advance.
fn font() -> CompiledFontCatalog {
    let rgba8 = vec![255u8; 32 * 8 * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/page0.png".into(),
        source_bytes: rgba8.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 32,
        height: 8,
        pixels: FontPixels::Rgba8(rgba8),
    };
    let glyphs: Vec<GlyphMetrics> = [' ', '-', '.', 'W', 'a', 'b', 'c', 'd', '\u{fffd}']
        .into_iter()
        .enumerate()
        .map(|(index, codepoint)| GlyphMetrics {
            codepoint,
            page: 0,
            uv: [index as u16 * 2, 0, index as u16 * 2 + 2, 8],
            bearing: [0, 0],
            advance_64: 2 * 64,
        })
        .collect();
    let identity = [9; 32];
    let bytes = encode_font_catalog(identity, &glyphs, &[page]).unwrap();
    CompiledFontCatalog::decode(&bytes, identity).unwrap()
}

fn layout(text: &str, width: u32, wrap: TextWrap) -> Result<Arc<TextLayout>, TextError> {
    let font = font();
    TextLayoutCache::new(4, 64 * 1024).layout(TextLayoutRequest {
        text,
        style: TextStyle::default(),
        width_64: width * 64,
        line_height_64: 8 * 64,
        baseline_64: 0,
        scale: UiScale::default(),
        font: &font,
        wrap,
    })
}

fn lines(layout: &TextLayout) -> Vec<String> {
    let mut out = vec![String::new(); usize::from(layout.line_count())];
    for glyph in layout.glyphs() {
        out[usize::from(glyph.line)].push(glyph.codepoint);
    }
    out
}

fn label(chop: WordChop) -> TextWrap {
    TextWrap {
        chop,
        ..TextWrap::default()
    }
}

#[test]
fn letter_spacing_moves_the_pen_and_changes_wrap_without_changing_ink() {
    let spacing = TextWrap {
        letter_spacing_64: 64,
        ..TextWrap::default()
    };
    let spaced = layout("ab", 6, spacing).unwrap();
    assert_eq!(spaced.size_64()[0], 6 * 64);
    assert_eq!(spaced.glyphs()[1].bounds_64[0], 3 * 64);
    assert_eq!(
        spaced.glyphs()[1].bounds_64[2] - spaced.glyphs()[1].bounds_64[0],
        2 * 64
    );
    assert_eq!(layout("ab", 4, spacing).unwrap().line_count(), 2);
    assert_eq!(
        layout("ab", 4, TextWrap::default()).unwrap().line_count(),
        1
    );
}

#[test]
fn native_pair_advance_shapes_before_letter_spacing_and_restarts_at_line_boundaries() {
    let font = font()
        .with_kerning(std::collections::BTreeMap::from([(('a', 'b'), -64)]))
        .unwrap();
    let request = |text, width: u32| TextLayoutRequest {
        text,
        style: TextStyle::default(),
        width_64: width * 64,
        line_height_64: 8 * 64,
        baseline_64: 0,
        scale: UiScale::default(),
        font: &font,
        wrap: TextWrap {
            letter_spacing_64: 64,
            ..TextWrap::default()
        },
    };
    let mut cache = TextLayoutCache::new(8, 65536);
    let pair = cache.layout(request("ab", 5)).unwrap();
    assert_eq!(pair.line_count(), 1);
    assert_eq!(pair.size_64()[0], 5 * 64);
    assert_eq!(pair.glyphs()[1].bounds_64[0], 2 * 64);
    assert!(Arc::ptr_eq(&pair, &cache.layout(request("ab", 5)).unwrap()));
    let broken = cache.layout(request("a\nb", 5)).unwrap();
    assert!(broken.glyphs().iter().all(|glyph| glyph.bounds_64[0] == 0));
    let wrapped = cache.layout(request("abab", 5)).unwrap();
    assert_eq!(lines(&wrapped), ["ab", "ab"]);
}

// An overlong word chops so its prefix plus `-` fits, then draws the `-`.
#[test]
fn overlong_words_chop_with_a_hyphen() {
    let chopped = layout("abcdabcd", 8, label(WordChop::Hyphen)).unwrap();
    assert_eq!(lines(&chopped), ["abc-", "dab-", "cd"]);
    let bare = layout("abcdabcd", 8, label(WordChop::Bare)).unwrap();
    assert_eq!(lines(&bare), ["abc", "dab", "cd"]);
}

// A glyph wider than the whole line overflows instead of failing the layout.
#[test]
fn a_glyph_wider_than_the_line_overflows() {
    assert!(matches!(
        layout("W", 1, TextWrap::default()),
        Err(TextError::VisualWidthExceeded { .. })
    ));
    let overflow = layout("W", 1, label(WordChop::Hyphen)).unwrap();
    assert_eq!(lines(&overflow), ["W"]);
}

// Each wrapped line centres on its own, not the block as a whole.
#[test]
fn alignment_places_each_line() {
    let wrap = TextWrap {
        align: TextLineAlign::Center,
        ..label(WordChop::Hyphen)
    };
    let centred = layout("abcd ab", 8, wrap).unwrap();
    let first = centred.glyphs().iter().find(|g| g.line == 0).unwrap();
    let second = centred.glyphs().iter().find(|g| g.line == 1).unwrap();
    assert_eq!(first.bounds_64[0], 0);
    assert_eq!(second.bounds_64[0], 2 * 64);
}

// Alignment offsets truncate onto the pixel grid, each line on its own: `ab` and `abc` centred
// in 9 sit at 2.5 and 1.5, which a 1-pixel grid truncates to 2 and 1 and a 2-pixel grid to 2
// and 0. A 0.8-pixel grid (DPI 1.25) takes 2.5 to three steps, 2.4 (153.6/64, rounded).
#[test]
fn alignment_offsets_truncate_onto_the_pixel_grid() {
    let starts = |grid: u32| {
        let wrap = TextWrap {
            align: TextLineAlign::Center,
            align_grid_65536: grid,
            ..TextWrap::default()
        };
        let centred = layout(
            "ab
abc", 9, wrap,
        )
        .unwrap();
        [0, 1].map(|line| {
            centred
                .glyphs()
                .iter()
                .find(|g| g.line == line)
                .unwrap()
                .bounds_64[0]
        })
    };
    assert_eq!(starts(0), [160, 96]);
    assert_eq!(starts(65_536), [128, 64]);
    assert_eq!(starts(2 * 65_536), [128, 0]);
    assert_eq!(starts(52_429)[0], 154);
}

// `line_padding` adds to the pitch between lines, not after the last.
#[test]
fn line_padding_spaces_lines() {
    let wrap = TextWrap {
        line_padding_64: 4 * 64,
        ..TextWrap::default()
    };
    let padded = layout("a\nb", 64, wrap).unwrap();
    assert_eq!(padded.size_64()[1], (8 + 4 + 8) * 64);
    let b = padded.glyphs().iter().find(|g| g.codepoint == 'b').unwrap();
    assert_eq!(b.bounds_64[1], 12 * 64);
}

// Past the line limit the text stops and the last line ends in `...`.
#[test]
fn line_limit_ends_in_an_ellipsis() {
    let wrap = TextWrap {
        max_lines: Some(1),
        ..label(WordChop::Hyphen)
    };
    let cut = layout("abcd abcd", 10, wrap).unwrap();
    assert_eq!(lines(&cut), ["ab..."]);
    assert!(cut.ellipsized());
    let whole = layout("abcd", 10, wrap).unwrap();
    assert!(!whole.ellipsized());
}

/// A saved vanilla line ends in a newline; ellipsis must not remove a fitting glyph.
#[test]
fn ellipsis_keeps_the_last_visible_glyph_when_it_fits() {
    let cut = layout(
        "abcd\nab",
        20,
        TextWrap {
            max_lines: Some(1),
            ..label(WordChop::Hyphen)
        },
    )
    .unwrap();
    assert_eq!(lines(&cut), ["abcd..."]);
    assert!(cut.ellipsized());
}

// `§f` is white, not the label's own colour; `§` then a newline is one token.
#[test]
fn explicit_white_and_formatting_newlines() {
    let spans = ui::parse_bedrock_text("§fX", 64).unwrap();
    assert_eq!(spans[0].style.color, BedrockColor::White);
    assert_eq!(
        ui::parse_bedrock_text("X", 64).unwrap()[0].style.color,
        BedrockColor::Base
    );
    let joined = layout("a§\nb", 64, TextWrap::default()).unwrap();
    assert_eq!(lines(&joined), ["ab"]);
}

#[test]
fn glyph_sources_survive_format_codes_wrapping_and_generated_punctuation() {
    let text = "a§bbcd§r abcd";
    let plain = ui::parse_bedrock_text(text, ui::UiLimits::MAX_TEXT_BYTES)
        .unwrap()
        .plain_text()
        .chars()
        .collect::<Vec<_>>();
    let wrapped = layout(
        text,
        5,
        TextWrap {
            chop: WordChop::Hyphen,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(wrapped.line_count() > 1);
    assert!(wrapped.glyph_source_indices().contains(&None));
    for (glyph, index) in wrapped.glyphs().iter().zip(wrapped.glyph_source_indices()) {
        if let Some(index) = index {
            assert_eq!(glyph.codepoint, plain[*index]);
        } else {
            assert_eq!(glyph.codepoint, '-');
        }
    }
    let cut = layout(
        text,
        5,
        TextWrap {
            chop: WordChop::Hyphen,
            max_lines: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(cut.ellipsized());
    assert!(
        cut.glyphs()
            .iter()
            .zip(cut.glyph_source_indices())
            .rev()
            .take(3)
            .all(|(glyph, index)| glyph.codepoint == '.' && index.is_none())
    );
}
