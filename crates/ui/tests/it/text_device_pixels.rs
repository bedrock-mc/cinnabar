//! Layout on the device pixel grid: a request's device scale keeps glyph edges on whole
//! device pixels at fractional display scales and leaves whole display scales unchanged.

use std::sync::Arc;

use assets::{CompiledFontCatalog, FontPixels, FontTexturePage, GlyphMetrics, encode_font_catalog};
use sha2::{Digest, Sha256};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, TEXT_BASELINE_64, TEXT_LINE_HEIGHT_64, TextLayout, TextLayoutCache,
    TextLayoutRequest, TextLineAlign, TextStyle, TextWrap, UiScale, WordChop,
};

/// Cinnangles Sans-shaped glyphs: codepoint, texel width, height, top bearing and advance.
const GLYPHS: [(char, u16, u16, i16, i16); 8] = [
    (' ', 1, 1, 0, 8),
    ('-', 10, 2, -8, 12),
    ('.', 2, 4, -4, 4),
    ('H', 10, 14, -14, 12),
    ('i', 2, 14, -14, 4),
    ('l', 4, 14, -14, 6),
    ('m', 10, 10, -10, 12),
    ('\u{fffd}', 10, 14, -14, 12),
];

/// Creates a small fixed-cell font fixture with varied glyph advances.
fn font() -> CompiledFontCatalog {
    let rgba8 = vec![255u8; 128 * 16 * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/page0.png".into(),
        source_bytes: rgba8.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 128,
        height: 16,
        pixels: FontPixels::Rgba8(rgba8),
    };
    let mut x = 0;
    let glyphs: Vec<GlyphMetrics> = GLYPHS
        .into_iter()
        .map(|(codepoint, width, height, top, advance)| {
            let glyph = GlyphMetrics {
                codepoint,
                page: 0,
                uv: [x, 0, x + width, height],
                bearing: [0, top],
                advance_64: advance * 64,
            };
            x += width + 2;
            glyph
        })
        .collect();
    let identity = [4; 32];
    let bytes = encode_font_catalog(identity, &glyphs, &[page]).unwrap();
    CompiledFontCatalog::decode(&bytes, identity).unwrap()
}

/// `text` at `scale` within `width` output pixels, on the device grid of `device_scale`.
fn layout(
    font: &CompiledFontCatalog,
    text: &str,
    scale: f32,
    width: f32,
    wrap: TextWrap,
    device_scale: Option<f32>,
) -> Arc<TextLayout> {
    TextLayoutCache::new(4, 1 << 20)
        .layout(TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64: (width * 64.0) as u32,
            line_height_64: TEXT_LINE_HEIGHT_64,
            baseline_64: TEXT_BASELINE_64,
            scale: UiScale::new_display(scale).unwrap(),
            font,
            wrap: TextWrap {
                device_scale_65536: device_scale.map_or(0, |dpi| (dpi * 65_536.0).round() as u32),
                ..wrap
            },
        })
        .unwrap()
}

/// The text scale an integer GUI scale gives at a display scale, as the frame's metrics derive it.
fn gui_text_scale(gui_scale: u32, dpi: f32) -> f32 {
    gui_scale as f32 / (FONT_DESIGN_PIXEL_TEXELS as f32 * dpi)
}

/// Creates a long mixed-width line that exposes cumulative device-grid drift.
fn paragraph() -> String {
    ["Him", "lHm", "iHl", "mHi", "Hll", "mimH", "H.i-l"]
        .iter()
        .cycle()
        .take(60)
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// At display scales 1 and 2 output pixels already land on the device grid, so device units
/// change no glyph, line or size, whatever the wrapping, alignment, padding or style.
#[test]
fn whole_display_scales_lay_out_as_before() {
    let font = font();
    let text = format!("§l{}§r {}", paragraph(), paragraph());
    let wraps = [
        TextWrap::default(),
        TextWrap {
            align: TextLineAlign::Center,
            chop: WordChop::Hyphen,
            line_padding_64: 3 * 64,
            ..TextWrap::default()
        },
        TextWrap {
            align: TextLineAlign::Right,
            chop: WordChop::Bare,
            max_lines: Some(3),
            align_grid_65536: 65_536,
            ..TextWrap::default()
        },
    ];
    for dpi in [1.0, 2.0] {
        for gui_scale in 1..=8 {
            for factor in [0.5, 1.0, 2.0] {
                let scale = gui_text_scale(gui_scale, dpi) * factor;
                for wrap in wraps {
                    let width = 300.0 * scale;
                    let plain = layout(&font, &text, scale, width, wrap, None);
                    let device = layout(&font, &text, scale, width, wrap, Some(dpi));
                    assert_eq!(
                        (device.glyphs(), device.size_64(), device.line_count()),
                        (plain.glyphs(), plain.size_64(), plain.line_count()),
                        "display scale {dpi}, GUI scale {gui_scale}, factor {factor}, {wrap:?}"
                    );
                }
            }
        }
    }
}

/// Checks long lines at fractional display scales against the device grid and final edge-rounding tolerance.
#[test]
fn fractional_display_scales_keep_glyph_edges_on_whole_device_pixels() {
    let font = font();
    let text = paragraph();
    for dpi in [1.25, 1.5, 1.75] {
        for gui_scale in 1..=8 {
            let scale = gui_text_scale(gui_scale, dpi);
            let laid = layout(
                &font,
                &text,
                scale,
                60_000.0,
                TextWrap::default(),
                Some(dpi),
            );
            assert_eq!(laid.line_count(), 1);
            let device_per_texel = gui_scale as f32 / FONT_DESIGN_PIXEL_TEXELS as f32;
            let tolerance = dpi / 128.0 + 1e-4;
            let mut pen = 0.0;
            for (glyph, codepoint) in laid.glyphs().iter().zip(text.chars()) {
                let (_, width, _, _, advance) = GLYPHS
                    .into_iter()
                    .find(|metrics| metrics.0 == codepoint)
                    .unwrap();
                let edges =
                    [glyph.bounds_64[0], glyph.bounds_64[2]].map(|edge| edge as f32 / 64.0 * dpi);
                let expected = [pen, pen + f32::from(width) * device_per_texel];
                assert!(
                    edges
                        .iter()
                        .zip(expected)
                        .all(|(edge, expected)| (edge - expected).abs() <= tolerance),
                    "display scale {dpi}, GUI scale {gui_scale}: {codepoint:?} spans {edges:?} \
                     device pixels, not {expected:?}"
                );
                pen += f32::from(advance) * device_per_texel;
            }
        }
    }
}

/// A font scale whose texels fall between half device pixels has no device grid to keep, so
/// its layout stays in output pixels.
#[test]
fn fractional_text_scales_keep_output_pixels() {
    let font = font();
    let text = paragraph();
    for dpi in [1.0, 1.25, 2.0] {
        let scale = gui_text_scale(4, dpi) * 0.8;
        let plain = layout(&font, &text, scale, 400.0, TextWrap::default(), None);
        let device = layout(&font, &text, scale, 400.0, TextWrap::default(), Some(dpi));
        assert_eq!(device.glyphs(), plain.glyphs(), "display scale {dpi}");
        assert_eq!(device.size_64(), plain.size_64(), "display scale {dpi}");
    }
}
