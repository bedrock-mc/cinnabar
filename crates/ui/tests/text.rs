use std::sync::Arc;

use assets::{CompiledFontCatalog, FontTexturePage, GlyphMetrics, encode_font_catalog};
use sha2::{Digest, Sha256};
pub use ui::{UiLimits, UiScale};

#[path = "../src/text.rs"]
mod text;

use text::{
    BedrockColor, MAX_GLYPHS_PER_LAYOUT, MAX_TEXT_SPANS, MAX_WRAP_LINES, TextError,
    TextLayoutCache, TextLayoutRequest, TextStyle, parse_bedrock_text,
};

#[test]
fn formatting_codes_change_style_without_emitting_glyphs() {
    let spans = parse_bedrock_text("A§cB§rC", 64).unwrap();
    assert_eq!(spans.plain_text(), "ABC");
    assert_eq!(spans[1].style.color, BedrockColor::Red);
    assert_eq!(spans[2].style, TextStyle::default());
}

#[test]
fn color_codes_preserve_decorations_until_reset() {
    let spans = parse_bedrock_text("§lA§cB§oC§kD§rE", 64).unwrap();
    assert!(spans[0].style.bold);
    assert_eq!(spans[1].style.color, BedrockColor::Red);
    assert!(spans[1].style.bold);
    assert!(spans[2].style.bold && spans[2].style.italic);
    assert!(spans[3].style.bold && spans[3].style.italic && spans[3].style.obfuscated);
    assert_eq!(spans[4].style, TextStyle::default());
}

#[test]
fn pinned_bedrock_color_codes_include_resin() {
    let expected = [
        ('0', BedrockColor::Black),
        ('1', BedrockColor::DarkBlue),
        ('2', BedrockColor::DarkGreen),
        ('3', BedrockColor::DarkAqua),
        ('4', BedrockColor::DarkRed),
        ('5', BedrockColor::DarkPurple),
        ('6', BedrockColor::Gold),
        ('7', BedrockColor::Gray),
        ('8', BedrockColor::DarkGray),
        ('9', BedrockColor::Blue),
        ('a', BedrockColor::Green),
        ('b', BedrockColor::Aqua),
        ('c', BedrockColor::Red),
        ('d', BedrockColor::LightPurple),
        ('e', BedrockColor::Yellow),
        ('f', BedrockColor::White),
        ('g', BedrockColor::MinecoinGold),
        ('h', BedrockColor::MaterialQuartz),
        ('i', BedrockColor::MaterialIron),
        ('j', BedrockColor::MaterialNetherite),
        ('m', BedrockColor::MaterialRedstone),
        ('n', BedrockColor::MaterialCopper),
        ('p', BedrockColor::MaterialGold),
        ('q', BedrockColor::MaterialEmerald),
        ('s', BedrockColor::MaterialDiamond),
        ('t', BedrockColor::MaterialLapis),
        ('u', BedrockColor::MaterialAmethyst),
        ('v', BedrockColor::MaterialResin),
    ];
    for (code, color) in expected {
        let text = format!("§{code}X");
        let spans = parse_bedrock_text(&text, text.len()).unwrap();
        assert_eq!(spans.len(), 1, "code §{code}");
        assert_eq!(spans[0].style.color, color, "code §{code}");
        assert_eq!(spans.plain_text(), "X", "code §{code}");
    }
}

#[test]
fn parser_normalizes_crlf_and_preserves_invalid_section_sequences() {
    let spans = parse_bedrock_text("A\r\nB§zC§", 64).unwrap();
    assert_eq!(spans.plain_text(), "A\nB§zC§");
    let section_before_crlf = parse_bedrock_text("§\r\nA", 64).unwrap();
    assert_eq!(section_before_crlf.plain_text(), "§\nA");

    assert!(matches!(
        parse_bedrock_text("four", 3),
        Err(TextError::TextBytesExceeded {
            actual: 4,
            limit: 3
        })
    ));

    let alternating = "§cA§rB".repeat(MAX_TEXT_SPANS / 2 + 1);
    assert!(matches!(
        parse_bedrock_text(&alternating, alternating.len()),
        Err(TextError::SpanLimitExceeded { .. })
    ));
}

#[test]
fn layout_wraps_in_checked_fixed_point_and_uses_replacement_glyph() {
    let font = font([0x11; 32]);
    let mut cache = TextLayoutCache::new(8, 64 * 1024);
    let layout = cache
        .layout(TextLayoutRequest {
            text: "AB?",
            style: TextStyle::default(),
            width_64: 128,
            scale: UiScale::new(1.0).unwrap(),
            font: &font,
        })
        .unwrap();

    assert_eq!(layout.line_count(), 2);
    assert_eq!(layout.glyphs().len(), 3);
    assert_eq!(layout.glyphs()[0].line, 0);
    assert_eq!(layout.glyphs()[1].line, 0);
    assert_eq!(layout.glyphs()[2].line, 1);
    assert_eq!(layout.glyphs()[2].codepoint, '?');
    assert_eq!(layout.glyphs()[2].resolved_codepoint, '\u{fffd}');
    assert_eq!(layout.glyphs()[2].bounds_64[0], 0);
    assert_eq!(layout.key().width_64, 128);
    assert_eq!(layout.size_64(), [128, 128]);
}

#[test]
fn visual_overhang_drives_wrapping_and_reported_bounds() {
    let negative = font_with_glyph(
        [0x12; 32],
        GlyphMetrics {
            codepoint: 'A',
            page: 0,
            uv: [0, 0, 2, 1],
            bearing: [-1, 0],
            advance_64: 64,
        },
    );
    let mut cache = TextLayoutCache::new(8, 64 * 1024);
    let negative_layout = layout(&mut cache, &negative, "AA", 1.0, 128);
    assert_eq!(negative_layout.line_count(), 2);
    assert_eq!(negative_layout.size_64(), [128, 128]);
    for glyph in negative_layout.glyphs() {
        assert!(glyph.bounds_64[0] >= 0);
        assert!(glyph.bounds_64[2] <= negative_layout.size_64()[0] as i32);
    }

    let positive = font_with_glyph(
        [0x13; 32],
        GlyphMetrics {
            codepoint: 'A',
            page: 0,
            uv: [0, 0, 2, 1],
            bearing: [1, 0],
            advance_64: 64,
        },
    );
    let positive_layout = layout(&mut cache, &positive, "A", 1.0, 192);
    assert_eq!(positive_layout.glyphs()[0].bounds_64, [64, 0, 192, 64]);
    assert_eq!(positive_layout.size_64(), [192, 64]);

    assert!(matches!(
        cache.layout(TextLayoutRequest {
            text: "A",
            style: TextStyle::default(),
            width_64: 128,
            scale: UiScale::default(),
            font: &positive,
        }),
        Err(TextError::VisualWidthExceeded {
            actual_64: 192,
            limit_64: 128,
        })
    ));

    let vertical = font_with_glyph(
        [0x14; 32],
        GlyphMetrics {
            codepoint: 'A',
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, -1],
            advance_64: 64,
        },
    );
    let vertical_layout = layout(&mut cache, &vertical, "A", 1.0, 64);
    assert_eq!(vertical_layout.glyphs()[0].bounds_64, [0, 0, 64, 64]);
    assert_eq!(vertical_layout.size_64(), [64, 128]);
}

#[test]
fn explicit_newlines_and_layout_bounds_fail_closed() {
    let font = font([0x22; 32]);
    let mut cache = TextLayoutCache::new(8, 2 * 1024 * 1024);
    let too_many_lines = "\n".repeat(MAX_WRAP_LINES);
    assert!(matches!(
        cache.layout(TextLayoutRequest {
            text: &too_many_lines,
            style: TextStyle::default(),
            width_64: 64,
            scale: UiScale::default(),
            font: &font,
        }),
        Err(TextError::WrapLineLimitExceeded { .. })
    ));

    let too_many_glyphs = "A".repeat(MAX_GLYPHS_PER_LAYOUT + 1);
    assert!(matches!(
        cache.layout(TextLayoutRequest {
            text: &too_many_glyphs,
            style: TextStyle::default(),
            width_64: u32::MAX,
            scale: UiScale::default(),
            font: &font,
        }),
        Err(TextError::GlyphLimitExceeded { .. }) | Err(TextError::TextBytesExceeded { .. })
    ));
}

#[test]
fn rejected_unbounded_text_does_not_hash_or_advance_cache_identity() {
    let font = font([0x23; 32]);
    let mut cache = TextLayoutCache::new(8, 64 * 1024);
    let oversized = "A".repeat(UiLimits::MAX_TEXT_BYTES + 1);
    assert!(matches!(
        cache.layout(TextLayoutRequest {
            text: &oversized,
            style: TextStyle::default(),
            width_64: 64,
            scale: UiScale::default(),
            font: &font,
        }),
        Err(TextError::TextBytesExceeded { .. })
    ));

    let accepted = layout(&mut cache, &font, "A", 1.0, 64);
    assert_eq!(accepted.id(), 1);
}

#[test]
fn layout_cache_is_identity_scale_and_width_qualified() {
    let primary_font = font([0x33; 32]);
    let other_font = font([0x44; 32]);
    let mut cache = TextLayoutCache::new(8, 64 * 1024);
    let first = layout(&mut cache, &primary_font, "hello", 1.0, 1024);
    let same = layout(&mut cache, &primary_font, "hello", 1.0, 1024);
    let changed_scale = layout(&mut cache, &primary_font, "hello", 2.0, 1024);
    let changed_width = layout(&mut cache, &primary_font, "hello", 1.0, 2048);
    let changed_font = layout(&mut cache, &other_font, "hello", 1.0, 1024);

    assert_eq!(first.id(), same.id());
    assert_ne!(first.id(), changed_scale.id());
    assert_ne!(first.id(), changed_width.id());
    assert_ne!(first.id(), changed_font.id());
    assert!(cache.retained_bytes() <= 64 * 1024);
    assert!(cache.len() <= 8);
}

#[test]
fn cache_evicts_the_least_recently_used_entry_within_both_caps() {
    let font = font([0x55; 32]);
    let mut cache = TextLayoutCache::new(2, 64 * 1024);
    assert!(cache.is_empty());
    let a = layout(&mut cache, &font, "A", 1.0, 1024);
    let b = layout(&mut cache, &font, "B", 1.0, 1024);
    let a_again = layout(&mut cache, &font, "A", 1.0, 1024);
    assert_eq!(a.id(), a_again.id());
    let _c = layout(&mut cache, &font, "C", 1.0, 1024);
    let a_still_cached = layout(&mut cache, &font, "A", 1.0, 1024);
    let b_after_eviction = layout(&mut cache, &font, "B", 1.0, 1024);
    assert_eq!(a.id(), a_still_cached.id());
    assert_ne!(b.id(), b_after_eviction.id());
    assert_eq!(cache.len(), 2);
    assert!(cache.retained_bytes() <= 64 * 1024);

    let mut byte_capped = TextLayoutCache::new(8, 1);
    let first_uncached = layout(&mut byte_capped, &font, "A", 1.0, 1024);
    let second_uncached = layout(&mut byte_capped, &font, "A", 1.0, 1024);
    assert_ne!(first_uncached.id(), second_uncached.id());
    assert!(byte_capped.is_empty());
    assert_eq!(byte_capped.retained_bytes(), 0);

    let mut probe = TextLayoutCache::new(1, usize::MAX);
    let _probe_layout = layout(&mut probe, &font, "A", 1.0, 1024);
    let conservative_entry_bytes = probe.retained_bytes();
    assert!(conservative_entry_bytes >= 4_096);

    let mut exact_cap = TextLayoutCache::new(1, conservative_entry_bytes);
    let exact_first = layout(&mut exact_cap, &font, "A", 1.0, 1024);
    let exact_second = layout(&mut exact_cap, &font, "A", 1.0, 1024);
    assert_eq!(exact_first.id(), exact_second.id());
    assert_eq!(exact_cap.retained_bytes(), conservative_entry_bytes);

    let mut below_cap = TextLayoutCache::new(1, conservative_entry_bytes - 1);
    let below_first = layout(&mut below_cap, &font, "A", 1.0, 1024);
    let below_second = layout(&mut below_cap, &font, "A", 1.0, 1024);
    assert_ne!(below_first.id(), below_second.id());
    assert_eq!(below_cap.retained_bytes(), 0);
}

fn layout(
    cache: &mut TextLayoutCache,
    font: &CompiledFontCatalog,
    content: &str,
    scale: f32,
    width_64: u32,
) -> Arc<text::TextLayout> {
    cache
        .layout(TextLayoutRequest {
            text: content,
            style: TextStyle::default(),
            width_64,
            scale: UiScale::new(scale).unwrap(),
            font,
        })
        .unwrap()
}

fn font(source_manifest_sha256: [u8; 32]) -> CompiledFontCatalog {
    let rgba8 = vec![255, 255, 255, 255].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/default8.png".into(),
        source_bytes: 4,
        source_sha256: [0x66; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 1,
        height: 1,
        rgba8,
    };
    let glyphs = [' ', 'A', 'B', 'C', 'e', 'h', 'l', 'o', '\u{fffd}']
        .into_iter()
        .map(|codepoint| GlyphMetrics {
            codepoint,
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        })
        .collect::<Vec<_>>();
    let bytes = encode_font_catalog(source_manifest_sha256, &glyphs, &[page]).unwrap();
    CompiledFontCatalog::decode(&bytes, source_manifest_sha256).unwrap()
}

fn font_with_glyph(source_manifest_sha256: [u8; 32], glyph: GlyphMetrics) -> CompiledFontCatalog {
    let rgba8 = vec![255; 4 * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/overhang.png".into(),
        source_bytes: 16,
        source_sha256: [0x77; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 4,
        height: 1,
        rgba8,
    };
    let replacement = GlyphMetrics {
        codepoint: '\u{fffd}',
        page: 0,
        uv: [2, 0, 3, 1],
        bearing: [0, 0],
        advance_64: 64,
    };
    let bytes =
        encode_font_catalog(source_manifest_sha256, &[glyph, replacement], &[page]).unwrap();
    CompiledFontCatalog::decode(&bytes, source_manifest_sha256).unwrap()
}
