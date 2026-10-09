//! A retained draw list re-emits only changed nodes and always equals a full build.

use std::sync::Arc;

use assets::{CompiledFontCatalog, FontPixels, FontTexturePage, GlyphMetrics, encode_font_catalog};
use sha2::{Digest, Sha256};
use ui::{
    RetainedDraw, SafeArea, TextEffects, TextLayout, TextLayoutCache, TextLayoutRequest,
    TextShadow, TextStyle, UiBlendMode, UiDrawList, UiMesh, UiMeshBatch, UiMeshVertex, UiNode,
    UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual,
};

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> UiRect {
    UiRect::new(
        UiPoint::new(left, top).unwrap(),
        UiPoint::new(right, bottom).unwrap(),
    )
    .unwrap()
}

fn id(value: u32) -> UiNodeId {
    UiNodeId::new(value)
}

/// A two-page font: `A` on page 0, `B` on page 1.
fn font() -> CompiledFontCatalog {
    let page = |index: u8| {
        let pixels = vec![255; 4].into_boxed_slice();
        FontTexturePage {
            source_path: format!("font/page{index}.png").into(),
            source_bytes: 4,
            source_sha256: [index + 1; 32],
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
        bearing: [0, 0],
        advance_64: 64,
    };
    let glyphs = [glyph('A', 0), glyph('B', 1), glyph('\u{fffd}', 0)];
    let bytes = encode_font_catalog([9; 32], &glyphs, &[page(0), page(1)]).unwrap();
    CompiledFontCatalog::decode(&bytes, [9; 32]).unwrap()
}

fn text(font: &CompiledFontCatalog, value: &str) -> Arc<TextLayout> {
    TextLayoutCache::new(4, 64 * 1024)
        .layout(TextLayoutRequest {
            text: value,
            style: TextStyle::default(),
            width_64: 64 * 64,
            line_height_64: 64,
            baseline_64: 0,
            scale: UiScale::default(),
            font,
            wrap: Default::default(),
        })
        .unwrap()
}

fn mesh(offset: f32) -> Arc<UiMesh> {
    let vertex = |position: [f32; 2]| UiMeshVertex {
        position,
        clip_z: 0.5,
        clip_w: 1.0,
        uv: [0.5, 0.5],
        color: [255; 4],
        style_flags: 0,
        alpha_test: false,
        model_light: 1.0,
        overlay_color: [0.0; 4],
    };
    Arc::new(
        UiMesh::new(
            vec![
                vertex([offset, 0.0]),
                vertex([1.0, offset]),
                vertex([0.0, 1.0]),
            ]
            .into(),
            vec![0, 1, 2].into(),
            vec![UiMeshBatch {
                texture_page: 2,
                index_range: 0..3,
                blend: UiBlendMode::Alpha,
                depth_test: true,
                depth_write: true,
                alpha_cutoff: Some(0.5),
            }]
            .into(),
        )
        .unwrap(),
    )
}

fn sprite(page: u16, color: [u8; 4]) -> UiVisual {
    UiVisual::Sprite {
        texture_page: page,
        uv: [0, 0, 1, 1],
        color,
    }
}

/// A clipped panel of sprites, shadowed text, a model and a nested clipped group.
fn hud(font: &CompiledFontCatalog) -> Vec<UiNode> {
    vec![
        UiNode::new(id(1), None, rect(0.0, 0.0, 200.0, 100.0)).with_clip_children(true),
        UiNode::new(id(2), Some(id(1)), rect(0.0, 0.0, 10.0, 10.0))
            .with_visual(sprite(0, [255; 4])),
        UiNode::new(id(3), Some(id(1)), rect(10.0, 0.0, 20.0, 10.0)).with_visual(sprite(0, [9; 4])),
        UiNode::new(id(4), Some(id(1)), rect(0.0, 20.0, 40.0, 30.0)).with_visual(UiVisual::Text {
            layout: text(font, "AB"),
            color: [255; 4],
            shadow: TextShadow::Offset64(64),
        }),
        UiNode::new(id(5), Some(id(1)), rect(50.0, 50.0, 80.0, 90.0))
            .with_visual(UiVisual::Mesh(mesh(0.0))),
        UiNode::new(id(6), Some(id(1)), rect(100.0, 0.0, 150.0, 40.0)).with_clip_children(true),
        UiNode::new(id(7), Some(id(6)), rect(0.0, 0.0, 30.0, 30.0))
            .with_visual(sprite(1, [255; 4])),
        UiNode::new(id(8), Some(id(6)), rect(20.0, 20.0, 80.0, 30.0))
            .with_visual(sprite(0, [255; 4])),
        UiNode::new(id(9), None, rect(150.0, 150.0, 160.0, 160.0)).with_visual(sprite(0, [1; 4])),
    ]
}

const VIEWPORT: [f32; 4] = [0.0, 0.0, 400.0, 300.0];

fn viewport() -> UiRect {
    rect(VIEWPORT[0], VIEWPORT[1], VIEWPORT[2], VIEWPORT[3])
}

fn full(nodes: &[UiNode]) -> UiDrawList {
    let mut tree = UiTree::new(nodes.to_vec()).unwrap();
    tree.layout(viewport(), UiScale::new(2.0).unwrap(), SafeArea::ZERO)
        .unwrap();
    tree.build_draw_list_with(TextEffects::default()).unwrap()
}

/// A retained list and the nodes it was drawn from.
struct Retained {
    draw: RetainedDraw,
    last: Vec<UiNode>,
}

fn retained(nodes: &[UiNode]) -> Retained {
    let draw = RetainedDraw::build(
        nodes,
        viewport(),
        UiScale::new(2.0).unwrap(),
        SafeArea::ZERO,
        TextEffects::default(),
    )
    .unwrap();
    Retained {
        draw,
        last: nodes.to_vec(),
    }
}

fn redraw(retained: &mut Retained, nodes: &[UiNode]) -> Option<ui::DrawUpdate> {
    retained.draw.update(
        &mut retained.last,
        nodes,
        viewport(),
        UiScale::new(2.0).unwrap(),
        SafeArea::ZERO,
        TextEffects::default(),
    )
}

fn assert_equal(retained: &Retained, nodes: &[UiNode]) {
    assert_eq!(retained.last, nodes);
    let expected = full(nodes);
    let actual = retained.draw.draw_list();
    assert_eq!(actual.vertices, expected.vertices);
    assert_eq!(actual.indices, expected.indices);
    assert_eq!(actual.batches, expected.batches);
}

#[test]
fn a_fresh_retained_list_equals_the_full_build() {
    let nodes = hud(&font());
    assert_equal(&retained(&nodes), &nodes);
}

#[test]
fn recoloring_one_sprite_rewrites_only_its_vertices() {
    let mut nodes = hud(&font());
    let mut draw = retained(&nodes);
    nodes[2] = nodes[2].clone().with_visual(sprite(0, [200, 10, 10, 255]));
    let update = redraw(&mut draw, &nodes).expect("same shape");
    assert_eq!(update.emitted, 1);
    assert!(!update.rebuilt);
    assert_eq!(update.vertices.len(), 1);
    assert_eq!(update.vertices[0].len(), 4);
    assert_equal(&draw, &nodes);
}

#[test]
fn a_posed_model_rewrites_its_vertices_without_touching_batches() {
    let mut nodes = hud(&font());
    let mut draw = retained(&nodes);
    for step in 1..4 {
        nodes[4] = nodes[4]
            .clone()
            .with_visual(UiVisual::Mesh(mesh(step as f32 * 0.1)));
        let update = redraw(&mut draw, &nodes).expect("same shape");
        assert_eq!(update.emitted, 1);
        assert!(!update.rebuilt);
        assert_equal(&draw, &nodes);
    }
}

#[test]
fn longer_text_splices_its_glyphs_and_keeps_later_nodes() {
    let font = font();
    let mut nodes = hud(&font);
    let mut draw = retained(&nodes);
    for value in ["ABAB", "B", "", "AAB"] {
        nodes[3] = nodes[3].clone().with_visual(UiVisual::Text {
            layout: text(&font, value),
            color: [255; 4],
            shadow: TextShadow::Offset64(64),
        });
        let update = redraw(&mut draw, &nodes).expect("same shape");
        assert_eq!(update.emitted, 1);
        assert_equal(&draw, &nodes);
    }
}

#[test]
fn a_page_change_merges_and_splits_batches_like_a_full_build() {
    let mut nodes = hud(&font());
    let mut draw = retained(&nodes);
    for page in [1, 0, 3, 0] {
        nodes[1] = nodes[1].clone().with_visual(sprite(page, [255; 4]));
        let update = redraw(&mut draw, &nodes).expect("same shape");
        assert_eq!(update.emitted, 1);
        assert_equal(&draw, &nodes);
    }
}

#[test]
fn a_moved_group_emits_its_subtree_again() {
    let mut nodes = hud(&font());
    let mut draw = retained(&nodes);
    nodes[5] = nodes[5].clone().with_bounds(rect(90.0, 10.0, 120.0, 30.0));
    let update = redraw(&mut draw, &nodes).expect("same shape");
    assert_eq!(update.emitted, 3, "the group and its two children");
    assert_equal(&draw, &nodes);
    // Moving a node out of its parent's clip drops its quads entirely.
    nodes[6] = nodes[6]
        .clone()
        .with_bounds(rect(500.0, 500.0, 530.0, 530.0));
    redraw(&mut draw, &nodes).expect("same shape");
    assert_equal(&draw, &nodes);
}

#[test]
fn several_changes_in_one_frame_still_equal_the_full_build() {
    let font = font();
    let mut nodes = hud(&font);
    let mut draw = retained(&nodes);
    nodes[1] = nodes[1].clone().with_visual(sprite(1, [255; 4]));
    nodes[3] = nodes[3].clone().with_visual(UiVisual::Text {
        layout: text(&font, "BBA"),
        color: [3, 4, 5, 255],
        shadow: TextShadow::None,
    });
    nodes[8] = nodes[8].clone().with_visual(UiVisual::None);
    let update = redraw(&mut draw, &nodes).expect("same shape");
    assert_eq!(update.emitted, 3);
    assert!(update.rebuilt);
    assert_equal(&draw, &nodes);
}

#[test]
fn an_unchanged_frame_emits_nothing() {
    let nodes = hud(&font());
    let mut draw = retained(&nodes);
    let update = redraw(&mut draw, &nodes).expect("same shape");
    assert_eq!(update, ui::DrawUpdate::default());
}

#[test]
fn a_new_node_or_viewport_needs_a_full_build() {
    let nodes = hud(&font());
    let mut draw = retained(&nodes);
    let mut grown = nodes.clone();
    grown.push(UiNode::new(id(10), None, rect(0.0, 0.0, 1.0, 1.0)));
    assert!(redraw(&mut draw, &grown).is_none());
    let mut draw = retained(&nodes);
    let reparented: Vec<_> = nodes
        .iter()
        .map(|node| {
            if node.id() == id(8) {
                node.clone().with_identity(id(8), Some(id(1)))
            } else {
                node.clone()
            }
        })
        .collect();
    assert!(redraw(&mut draw, &reparented).is_none());
    let mut draw = retained(&nodes);
    assert!(
        draw.draw
            .update(
                &mut draw.last,
                &nodes,
                viewport(),
                UiScale::new(1.0).unwrap(),
                SafeArea::ZERO,
                TextEffects::default(),
            )
            .is_none()
    );
}
