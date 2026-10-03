//! `§k`/`§l`/`§o` reach the draw list: obfuscation swaps same-width rasters
//! per frame, bold redraws an offset copy, italic shears the top edge.

use std::{collections::BTreeSet, sync::Arc};

use assets::{CompiledFontCatalog, FontTexturePage, GlyphMetrics, encode_font_catalog};
use sha2::{Digest, Sha256};
use ui::{
    ObfuscationGlyphs, SafeArea, TextEffects, TextLayout, TextLayoutCache, TextLayoutRequest,
    TextShadow, TextStyle, UiDrawList, UiNode, UiNodeId, UiPoint, UiRect, UiScale, UiTree,
    UiVisual,
};

// Four equal-width (two-texel) rasters with distinct UVs, two atlas pages, so
// the obfuscation pool for that width holds several observable targets.
fn font() -> CompiledFontCatalog {
    let rgba8 = vec![255u8; 8 * 8 * 4].into_boxed_slice();
    let page = |path: &str, tag: u8| FontTexturePage {
        source_path: path.into(),
        source_bytes: rgba8.len() as u32,
        source_sha256: [tag; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 8,
        height: 8,
        rgba8: rgba8.clone(),
    };
    let glyph = |codepoint: char, page: u16, u0: u16| GlyphMetrics {
        codepoint,
        page,
        uv: [u0, 0, u0 + 2, 8],
        bearing: [0, 0],
        advance_64: 2 * 64,
    };
    let glyphs = [
        glyph('A', 0, 0),
        glyph('B', 1, 2),
        glyph('C', 0, 4),
        glyph('\u{fffd}', 0, 6),
    ];
    let identity = [7; 32];
    let bytes = encode_font_catalog(
        identity,
        &glyphs,
        &[page("font/page0.png", 1), page("font/page1.png", 2)],
    )
    .unwrap();
    CompiledFontCatalog::decode(&bytes, identity).unwrap()
}

fn layout(text: &str, style: TextStyle, font: &CompiledFontCatalog) -> Arc<TextLayout> {
    TextLayoutCache::new(4, 64 * 1024)
        .layout(TextLayoutRequest {
            text,
            style,
            width_64: 64 * 64,
            line_height_64: 64,
            baseline_64: 0,
            scale: UiScale::default(),
            font,
            wrap: Default::default(),
        })
        .unwrap()
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> UiRect {
    UiRect::new(
        UiPoint::new(left, top).unwrap(),
        UiPoint::new(right, bottom).unwrap(),
    )
    .unwrap()
}

fn draw_with(layout: Arc<TextLayout>, effects: TextEffects<'_>) -> UiDrawList {
    let visual = UiVisual::Text {
        layout,
        color: [255; 4],
        shadow: TextShadow::None,
    };
    let mut tree = UiTree::new(vec![
        UiNode::new(UiNodeId::new(1), None, rect(0.0, 0.0, 200.0, 40.0)).with_visual(visual),
    ])
    .unwrap();
    tree.layout(
        rect(0.0, 0.0, 200.0, 100.0),
        UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();
    tree.build_draw_list_with(effects).unwrap()
}

#[test]
fn bold_glyph_emits_a_second_offset_copy() {
    let font = font();
    let plain = draw_with(
        layout("AB", TextStyle::default(), &font),
        TextEffects::default(),
    );
    let bold = draw_with(
        layout(
            "AB",
            TextStyle {
                bold: true,
                ..TextStyle::default()
            },
            &font,
        ),
        TextEffects::default(),
    );
    assert_eq!(plain.vertices.len(), 2 * 4);
    // Two glyphs, each drawn twice.
    assert_eq!(bold.vertices.len(), 2 * 2 * 4);
    // The emboldening copy of the first glyph sits one design pixel right.
    let offset = bold.vertices[4].position[0] - bold.vertices[0].position[0];
    assert!((offset - 1.0).abs() < 1e-4, "bold offset was {offset}");
}

// Styled glyphs never set the glint bit, so bold text draws no enchantment sheen.
#[test]
fn styled_glyphs_carry_no_vertex_style_bits() {
    let font = font();
    let styled = draw_with(
        layout("\u{a7}l\u{a7}oA\u{a7}kB", TextStyle::default(), &font),
        TextEffects::default(),
    );
    assert!(!styled.vertices.is_empty());
    assert!(styled.vertices.iter().all(|vertex| vertex.style_flags == 0));
}

#[test]
fn italic_glyph_shears_its_top_edge_right() {
    let font = font();
    let dl = draw_with(
        layout(
            "A",
            TextStyle {
                italic: true,
                ..TextStyle::default()
            },
            &font,
        ),
        TextEffects::default(),
    );
    // Corner order is top-left, top-right, bottom-right, bottom-left.
    let top_left = dl.vertices[0].position;
    let bottom_left = dl.vertices[3].position;
    assert!(
        top_left[0] > bottom_left[0],
        "top {top_left:?} should lean right of bottom {bottom_left:?}"
    );
    // The shear is horizontal only.
    assert_eq!(top_left[1], dl.vertices[1].position[1]);
    assert_eq!(bottom_left[0], dl.vertices[2].position[0] - 2.0);
}

#[test]
fn obfuscation_swaps_a_same_width_raster_and_animates_across_frames() {
    let font = font();
    let pool = ObfuscationGlyphs::from_catalog(&font);
    let text = layout("\u{a7}kA", TextStyle::default(), &font);

    let mut seen = BTreeSet::new();
    for seed in 0..64u64 {
        let dl = draw_with(
            Arc::clone(&text),
            TextEffects {
                obfuscation_seed: seed,
                obfuscation: Some(&pool),
                ..Default::default()
            },
        );
        // The scrambled cell keeps its two-texel width whichever raster is picked.
        let width = dl.vertices[1].uv[0] - dl.vertices[0].uv[0];
        assert_eq!(width, 2.0, "swap must preserve the cell width");
        seen.insert((
            dl.batches[0].texture_page,
            dl.vertices[0].uv.map(f32::to_bits),
        ));
    }
    assert!(seen.len() > 1, "obfuscation must animate across frames");

    // Both shadow and glyph passes resolve the same raster within a frame.
    let shadowed = {
        let mut tree = UiTree::new(vec![
            UiNode::new(UiNodeId::new(1), None, rect(0.0, 0.0, 200.0, 40.0)).with_visual(
                UiVisual::Text {
                    layout: Arc::clone(&text),
                    color: [255; 4],
                    shadow: TextShadow::Offset64(64),
                },
            ),
        ])
        .unwrap();
        tree.layout(
            rect(0.0, 0.0, 200.0, 100.0),
            UiScale::default(),
            SafeArea::ZERO,
        )
        .unwrap();
        tree.build_draw_list_with(TextEffects {
            obfuscation_seed: 5,
            obfuscation: Some(&pool),
            ..Default::default()
        })
        .unwrap()
    };
    assert_eq!(shadowed.vertices[0].uv, shadowed.vertices[4].uv);

    // Without a pool the obfuscated glyph renders itself unchanged.
    let plain = draw_with(Arc::clone(&text), TextEffects::default());
    assert_eq!(plain.vertices.len(), 4);
    assert_eq!(plain.vertices[0].uv, [0.0, 0.0]);
}

// Rotated text turns each glyph quad about the node's centre.
#[test]
fn rotated_text_turns_glyphs_about_the_node_centre() {
    let font = font();
    let text = layout("A", TextStyle::default(), &font);
    let draw = |angle_radians: f32| {
        let mut tree = UiTree::new(vec![
            UiNode::new(UiNodeId::new(1), None, rect(0.0, 0.0, 20.0, 20.0)).with_visual(
                UiVisual::RotatedText {
                    layout: Arc::clone(&text),
                    color: [255; 4],
                    shadow: TextShadow::None,
                    angle_radians,
                },
            ),
        ])
        .unwrap();
        tree.layout(
            rect(0.0, 0.0, 200.0, 100.0),
            UiScale::default(),
            SafeArea::ZERO,
        )
        .unwrap();
        tree.build_draw_list_with(TextEffects::default()).unwrap()
    };
    let flat = draw(0.0);
    let turned = draw(std::f32::consts::PI);
    for (flat, turned) in flat.vertices.iter().zip(&turned.vertices) {
        let [x, y] = turned.position;
        assert!((x - (20.0 - flat.position[0])).abs() < 1e-4);
        assert!((y - (20.0 - flat.position[1])).abs() < 1e-4);
    }
}

// Formatting palette changes must reuse geometry and still preserve the label alpha.
#[test]
fn pack_palette_changes_cached_text_tints() {
    let font = font();
    let text = layout("§2A§rB§wC", TextStyle::default(), &font);
    let palette = ui::FormattingPalette::from_globals(|name| match name {
        "$2_color_format" => Some([0.976, 0.859, 0.427]),
        "$party_blue_color" => Some([0.549, 0.702, 1.0]),
        _ => None,
    });
    let draw = draw_with(
        text,
        TextEffects {
            palette: Some(&palette),
            ..Default::default()
        },
    );
    assert_eq!(draw.vertices[0].color, [249, 219, 109, 255]);
    assert_eq!(draw.vertices[4].color, [255; 4]);
    assert_eq!(draw.vertices[8].color, [140, 179, 255, 255]);
}
