//! Label strokes at fractional display scales, through the path a JSON-UI label paints: the
//! frame's text metrics, the label request, the whole-pixel text origin, the retained draw
//! list, the physical render input and the nearest-sampling rasterizer.

use std::{collections::BTreeMap, sync::Arc};

use assets::{FontPixels, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, encode_font_catalog};
use image::RgbaImage;
use json_ui::{TextAlign, TextOptions};
use render_model::UiRenderTextureArray;
use sha2::{Digest, Sha256};
use ui::{
    DpiScale, SafeArea, TextLayoutCache, TextShadow, UiNode, UiNodeId, UiScale, UiTree, UiVisual,
};

use super::super::pixel_snap::positioned;
use super::painted_label_request;
use crate::ui_runtime::presentation::forms::snapshot::rasterize;
use crate::ui_runtime::presentation::{
    FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationRuntime, rect,
};
use crate::ui_runtime::render_adapter::{UiRenderViewport, adapt_ui_draw_list};

/// Ink rows of every stroke glyph, as tall as Cinnangles Sans capitals.
const INK_ROWS: u16 = 14;

/// Glyphs drawn as full-height vertical strokes one design pixel wide: codepoint, cell width,
/// the first texel column of each stroke, and advance, all in texels as Cinnangles Sans letters
/// of those widths have them.
const STROKE_GLYPHS: [(char, u16, &[u16], u16); 5] = [
    ('H', 10, &[0, 8], 12),
    ('i', 2, &[0], 4),
    ('l', 4, &[2], 6),
    ('m', 10, &[0, 4, 8], 12),
    ('\u{fffd}', 10, &[0, 8], 12),
];

/// Window sizes giving GUI scales 2, 3, 4 and 5.
const WINDOWS: [[u32; 2]; 4] = [[1280, 720], [1600, 900], [1920, 1080], [2560, 1440]];

const DISPLAY_SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

/// A pixel font on the Cinnangles Sans grid: two texels per design pixel, even advances, and
/// every stroke exactly one design pixel wide, so each must cover GUI-scale device pixels.
fn stroke_font() -> Arc<RuntimeFontCatalog> {
    let width = 64usize;
    let height = 16usize;
    let mut rgba8 = vec![0u8; width * height * 4];
    let mut glyphs = Vec::new();
    let mut x = 0u16;
    for (codepoint, cell, strokes, advance) in STROKE_GLYPHS {
        for &stroke in strokes {
            for row in 0..usize::from(INK_ROWS) {
                for column in [stroke, stroke + 1] {
                    let at = (row * width + usize::from(x + column)) * 4;
                    rgba8[at..at + 4].fill(255);
                }
            }
        }
        glyphs.push(GlyphMetrics {
            codepoint,
            page: 0,
            uv: [x, 0, x + cell, INK_ROWS],
            bearing: [0, -(INK_ROWS as i16)],
            advance_64: (advance * 64) as i16,
        });
        x += cell + 2;
    }
    glyphs.push(GlyphMetrics {
        codepoint: ' ',
        page: 0,
        uv: [x, 0, x + 2, 2],
        bearing: [0, 0],
        advance_64: 8 * 64,
    });
    glyphs.sort_by_key(|glyph| glyph.codepoint);
    let rgba8 = rgba8.into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/strokes.png".into(),
        source_bytes: rgba8.len() as u32,
        source_sha256: [5; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: width as u32,
        height: height as u32,
        pixels: FontPixels::Rgba8(rgba8),
    };
    let manifest = [6; 32];
    let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
    Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
}

/// Lines long enough for a fractional scale's per-glyph rounding to reach whole pixels.
fn paragraph() -> String {
    ["Him", "lHm", "iHl", "mHi", "Hll", "mimH", "HiH"]
        .iter()
        .cycle()
        .take(120)
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text` painted as a left-aligned label filling a `physical` window at display scale `dpi`,
/// from a half-unit GUI origin as centred controls have; returns the frame and its GUI scale.
fn paint(
    font: &RuntimeFontCatalog,
    textures: &Arc<UiRenderTextureArray>,
    text: &str,
    physical: [u32; 2],
    dpi: f32,
) -> (RgbaImage, u32) {
    let dpi = DpiScale::new(dpi).unwrap();
    let metrics = TextMetrics::for_viewport(physical, dpi, None);
    let gui_scale = metrics.gui_scale;
    let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let gui = physical.map(|side| f64::from(side) / f64::from(gui_scale));
    let dest = positioned([3.5, 2.5, gui[0] - 7.0, gui[1] - 5.0], gui_scale, px);
    let request = painted_label_request(
        metrics,
        text,
        dest,
        font,
        1.0,
        &TextOptions::default(),
        TextAlign::Left,
        px,
    );
    let layout = TextLayoutCache::new(4, 1 << 20).layout(request).unwrap();
    assert!(
        layout.line_count() > 1 && !layout.ellipsized(),
        "the paragraph wraps onto whole lines"
    );
    let [width, height] = layout.size_64().map(|size| size as f32 / 64.0);
    let node = UiNode::new(
        UiNodeId::new(1),
        None,
        rect(dest[0], dest[1], dest[0] + width, dest[1] + height).unwrap(),
    )
    .with_visual(UiVisual::Text {
        layout,
        color: [255; 4],
        shadow: TextShadow::None,
    });
    let logical = physical.map(|side| side as f32 / dpi.get());
    let mut tree = UiTree::new(vec![node]).unwrap();
    tree.layout(
        rect(0.0, 0.0, logical[0], logical[1]).unwrap(),
        UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();
    let input = adapt_ui_draw_list(
        &tree.build_draw_list().unwrap(),
        Arc::clone(textures),
        UiRenderViewport {
            physical_size: physical,
            dpi_scale: dpi,
            safe_area: SafeArea::ZERO,
        },
    )
    .unwrap();
    (rasterize(&input), gui_scale as u32)
}

/// Device-pixel lengths of every horizontal run of white ink, row by row.
fn stroke_widths(image: &RgbaImage) -> Vec<u32> {
    let mut widths = Vec::new();
    for row in image.rows() {
        let mut run = 0;
        for pixel in row {
            if pixel[0] > 200 {
                run += 1;
            } else if run > 0 {
                widths.push(run);
                run = 0;
            }
        }
        if run > 0 {
            widths.push(run);
        }
    }
    widths
}

/// At an integer GUI scale every design pixel spans that many device pixels, as vanilla's
/// font pixels do, whatever the platform's display scale. A fractional display scale
/// (Windows 125%, 150%, 175%) used to drift glyphs off the device grid along a line, giving
/// some letters' strokes one device pixel more or less than their neighbours'.
#[test]
fn label_strokes_cover_whole_gui_pixels_at_fractional_display_scales() {
    let font = stroke_font();
    let textures = Arc::clone(
        &UiPresentationRuntime::new(Arc::clone(&font))
            .unwrap()
            .textures,
    );
    let text = paragraph();
    for physical in WINDOWS {
        let mut uneven = BTreeMap::new();
        for dpi in DISPLAY_SCALES {
            let (image, gui_scale) = paint(&font, &textures, &text, physical, dpi);
            let widths = stroke_widths(&image);
            assert!(
                widths.len() > 500,
                "{physical:?} at {dpi}: the paragraph is drawn"
            );
            let mut counts = BTreeMap::new();
            for width in widths.into_iter().filter(|width| *width != gui_scale) {
                *counts.entry(width).or_insert(0usize) += 1;
            }
            if !counts.is_empty() {
                uneven.insert(dpi.to_string(), (gui_scale, counts));
            }
        }
        assert!(
            uneven.is_empty(),
            "{physical:?}: display scale -> (GUI scale, stroke width -> count of strokes not \
             spanning the GUI scale): {uneven:?}"
        );
    }
}

/// The display scale only changes logical coordinates: a window renders the same device
/// pixels at 1.25, 1.5, 1.75 and 2.0 as at 1.0, so text at display scale 2 keeps its pixels.
#[test]
fn labels_render_the_same_device_pixels_at_every_display_scale() {
    let font = stroke_font();
    let textures = Arc::clone(
        &UiPresentationRuntime::new(Arc::clone(&font))
            .unwrap()
            .textures,
    );
    let text = paragraph();
    for physical in WINDOWS {
        let (reference, _) = paint(&font, &textures, &text, physical, 1.0);
        let differing: Vec<f32> = DISPLAY_SCALES
            .into_iter()
            .filter(|dpi| paint(&font, &textures, &text, physical, *dpi).0 != reference)
            .collect();
        assert!(
            differing.is_empty(),
            "{physical:?} renders differently from display scale 1 at {differing:?}"
        );
    }
}
