use std::sync::Arc;

use assets::{CompiledFontCatalog, FontTexturePage, GlyphMetrics, encode_font_catalog};
use sha2::{Digest, Sha256};
pub use ui::{
    BedrockColor, PointerPhase, SafeArea, TextLayout, TextLayoutCache, TextLayoutRequest,
    TextStyle, UiAction, UiDrawBatch, UiDrawList, UiError, UiLimits, UiNode, UiNodeId, UiPoint,
    UiRect, UiScale, UiTree, UiVertex, UiVisual,
};

#[test]
fn safe_area_scale_and_focus_order_are_deterministic() {
    let mut tree = fixture_menu();
    let frame = tree
        .layout(
            rect(0.0, 0.0, 1920.0, 1080.0),
            UiScale::new(2.0).unwrap(),
            SafeArea::new(20.0, 20.0, 20.0, 20.0).unwrap(),
        )
        .unwrap();

    assert_eq!(frame.focus_order(), &[node(10), node(20), node(30)],);
    assert!(frame.bounds(node(10)).unwrap().min().x() >= 20.0);
    assert_eq!(
        frame.bounds(node(10)).unwrap(),
        rect(20.0, 20.0, 220.0, 100.0)
    );
}

#[test]
fn focus_change_releases_pointer_capture_and_navigation_wraps() {
    let mut tree = fixture_menu();
    let frame = tree
        .layout(
            rect(0.0, 0.0, 640.0, 480.0),
            UiScale::default(),
            SafeArea::ZERO,
        )
        .unwrap();
    assert_eq!(tree.focus_mut().set_focused(Some(node(10))), None);
    tree.focus_mut().capture_pointer(node(10)).unwrap();
    assert_eq!(tree.focus_mut().set_focused(Some(node(20))), Some(node(10)));
    assert_eq!(tree.focus().pointer_capture(), None);

    tree.focus_mut().set_focused(Some(node(30)));
    let transition = tree.handle_action(&frame, UiAction::TabNext).unwrap();
    assert_eq!(transition.focused, Some(node(10)));
    let transition = tree.handle_action(&frame, UiAction::TabPrevious).unwrap();
    assert_eq!(transition.focused, Some(node(30)));
}

#[test]
fn pointer_ignores_controls_outside_effective_clip_and_safe_viewport() {
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(0.0, 0.0, 50.0, 50.0)).with_clip_children(true),
        UiNode::new(node(2), Some(node(1)), rect(75.0, 0.0, 125.0, 50.0)).with_focusable(true),
        UiNode::new(node(3), None, rect(175.0, 0.0, 225.0, 50.0)).with_focusable(true),
    ])
    .unwrap();
    let frame = tree
        .layout(
            rect(0.0, 0.0, 200.0, 100.0),
            UiScale::default(),
            SafeArea::new(10.0, 0.0, 40.0, 0.0).unwrap(),
        )
        .unwrap();

    for position in [[100.0, 25.0], [190.0, 25.0]] {
        let transition = tree
            .handle_action(
                &frame,
                UiAction::PointerPrimary {
                    position: UiPoint::new(position[0], position[1]).unwrap(),
                    phase: PointerPhase::Pressed,
                },
            )
            .unwrap();
        assert_eq!(transition.focused, None);
        assert_eq!(tree.focus().pointer_capture(), None);
    }
}

#[test]
fn pointer_uses_render_order_when_navigation_order_differs() {
    let mut tree = UiTree::new(vec![
        UiNode::new(node(20), None, rect(0.0, 0.0, 50.0, 50.0))
            .with_focusable(true)
            .with_navigation_order(1)
            .with_visual(UiVisual::Solid {
                texture_page: 0,
                color: [20, 0, 0, 255],
            }),
        UiNode::new(node(30), None, rect(0.0, 0.0, 50.0, 50.0))
            .with_focusable(true)
            .with_navigation_order(0)
            .with_visual(UiVisual::Solid {
                texture_page: 0,
                color: [30, 0, 0, 255],
            }),
    ])
    .unwrap();
    let frame = tree
        .layout(
            rect(0.0, 0.0, 100.0, 100.0),
            UiScale::default(),
            SafeArea::ZERO,
        )
        .unwrap();
    assert_eq!(frame.focus_order(), &[node(30), node(20)]);

    let transition = tree
        .handle_action(
            &frame,
            UiAction::PointerPrimary {
                position: UiPoint::new(25.0, 25.0).unwrap(),
                phase: PointerPhase::Pressed,
            },
        )
        .unwrap();

    assert_eq!(transition.focused, Some(node(30)));
    assert_eq!(tree.focus().pointer_capture(), Some(node(30)));
}

#[test]
fn same_revision_frame_from_another_tree_is_rejected() {
    let mut first = UiTree::new(vec![
        UiNode::new(node(1), None, rect(0.0, 0.0, 10.0, 10.0)).with_focusable(true),
    ])
    .unwrap();
    let mut second = UiTree::new(vec![
        UiNode::new(node(2), None, rect(0.0, 0.0, 10.0, 10.0)).with_focusable(true),
    ])
    .unwrap();
    let foreign_frame = first
        .layout(
            rect(0.0, 0.0, 100.0, 100.0),
            UiScale::default(),
            SafeArea::ZERO,
        )
        .unwrap();
    second
        .layout(
            rect(0.0, 0.0, 100.0, 100.0),
            UiScale::default(),
            SafeArea::ZERO,
        )
        .unwrap();

    assert!(
        second
            .handle_action(&foreign_frame, UiAction::TabNext)
            .is_err()
    );
    assert_eq!(second.focus().focused(), None);
}

#[test]
fn invalid_or_nonfocusable_ids_cannot_enter_focus_or_capture_state() {
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(0.0, 0.0, 10.0, 10.0)),
        UiNode::new(node(2), None, rect(0.0, 0.0, 10.0, 10.0)).with_focusable(true),
    ])
    .unwrap();

    for invalid in [node(1), node(999)] {
        let _ = tree.focus_mut().set_focused(Some(invalid));
        assert_eq!(tree.focus().focused(), None);
        assert!(tree.focus_mut().capture_pointer(invalid).is_err());
        assert_eq!(tree.focus().pointer_capture(), None);
    }
}

#[test]
fn clip_depth_duplicate_ids_and_parent_cycles_fail_closed() {
    let tree = deeply_clipped_tree(UiLimits::MAX_CLIP_DEPTH + 1).unwrap();
    assert!(matches!(
        tree.build_draw_list(),
        Err(UiError::ClipDepthExceeded { .. })
    ));

    assert!(matches!(
        UiTree::new(vec![solid_node(1, None), solid_node(1, None)]),
        Err(UiError::DuplicateNodeId { id }) if id == node(1)
    ));

    assert!(matches!(
        UiTree::new(vec![solid_node(1, Some(2)), solid_node(2, Some(1))]),
        Err(UiError::ParentCycle { .. })
    ));
}

#[test]
fn draw_list_uses_stable_tree_order_intersected_clips_and_no_empty_batches() {
    let mut tree = UiTree::new(vec![
        UiNode::new(node(30), Some(node(10)), rect(25.0, 25.0, 75.0, 75.0)).with_visual(
            UiVisual::Solid {
                texture_page: 7,
                color: [30, 0, 0, 255],
            },
        ),
        UiNode::new(node(20), Some(node(10)), rect(0.0, 0.0, 0.0, 10.0)).with_visual(
            UiVisual::Solid {
                texture_page: 7,
                color: [20, 0, 0, 255],
            },
        ),
        UiNode::new(node(10), None, rect(0.0, 0.0, 50.0, 50.0)).with_clip_children(true),
    ])
    .unwrap();
    tree.layout(
        rect(0.0, 0.0, 100.0, 100.0),
        UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();

    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.vertices.len(), 4);
    assert_eq!(draw.indices.as_slice(), &[0, 1, 2, 0, 2, 3]);
    assert_eq!(draw.batches.len(), 1);
    assert_eq!(draw.batches[0].texture_page, 7);
    assert_eq!(draw.batches[0].clip, rect(0.0, 0.0, 50.0, 50.0));
    assert_eq!(draw.batches[0].index_range, 0..6);
    assert_eq!(draw.vertices[0].color, [30, 0, 0, 255]);
    assert!(
        draw.batches
            .iter()
            .all(|batch| !batch.index_range.is_empty())
    );
}

#[test]
fn image_visual_emits_exact_texture_uvs() {
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(10.0, 20.0, 19.0, 29.0)).with_visual(
            UiVisual::Image {
                texture_page: 3,
                uv: [0, 0, 9, 9],
                color: [255; 4],
            },
        ),
    ])
    .unwrap();
    tree.layout(
        rect(0.0, 0.0, 100.0, 100.0),
        UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();

    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.batches[0].texture_page, 3);
    assert_eq!(
        draw.vertices.iter().map(|vertex| vertex.uv).collect::<Vec<_>>(),
        [[0, 0], [9, 0], [9, 9], [0, 9]]
    );
}

#[test]
fn cached_text_layout_emits_glyph_quads_by_texture_page() {
    let layout = text_layout();
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(4.0, 8.0, 100.0, 40.0)).with_visual(UiVisual::Text {
            layout,
            color: [255, 255, 255, 255],
        }),
    ])
    .unwrap();
    tree.layout(
        rect(0.0, 0.0, 200.0, 100.0),
        UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();

    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.vertices.len(), 8);
    assert_eq!(draw.indices.len(), 12);
    assert_eq!(draw.batches.len(), 2);
    assert_eq!(draw.batches[0].texture_page, 0);
    assert_eq!(draw.batches[1].texture_page, 1);
    assert_eq!(draw.vertices[0].position, [4.0, 8.0]);
}

#[test]
fn draw_batch_limit_is_centralized_and_enforced() {
    let nodes = (0..=UiLimits::MAX_DRAW_BATCHES)
        .map(|index| {
            UiNode::new(
                node(u32::try_from(index + 1).unwrap()),
                None,
                rect(index as f32, 0.0, index as f32 + 1.0, 1.0),
            )
            .with_visual(UiVisual::Solid {
                texture_page: u16::try_from(index % 2).unwrap(),
                color: [255; 4],
            })
        })
        .collect();
    let tree = UiTree::new(nodes).unwrap();
    assert!(matches!(
        tree.build_draw_list(),
        Err(UiError::DrawBatchLimitExceeded { actual, limit })
            if actual == UiLimits::MAX_DRAW_BATCHES + 1
                && limit == UiLimits::MAX_DRAW_BATCHES
    ));
}

#[test]
fn draw_caps_are_fixed_and_share_one_checked_byte_ceiling() {
    assert_eq!(UiLimits::MAX_UI_VERTICES, 262_144);
    assert_eq!(UiLimits::MAX_UI_INDICES, 393_216);
    assert_eq!(UiLimits::MAX_DRAW_BATCHES, 8_192);
    assert_eq!(UiLimits::MAX_DRAW_LIST_BYTES, 16 * 1024 * 1024);
    assert!(
        UiLimits::MAX_UI_VERTICES * std::mem::size_of::<UiVertex>()
            + UiLimits::MAX_UI_INDICES * std::mem::size_of::<u32>()
            + UiLimits::MAX_DRAW_BATCHES * std::mem::size_of::<UiDrawBatch>()
            <= UiLimits::MAX_DRAW_LIST_BYTES
    );
}

fn fixture_menu() -> UiTree {
    UiTree::new(vec![
        UiNode::new(node(30), None, rect(0.0, 100.0, 100.0, 140.0)).with_focusable(true),
        UiNode::new(node(20), None, rect(0.0, 50.0, 100.0, 90.0))
            .with_focusable(true)
            .with_navigation_order(1),
        UiNode::new(node(10), None, rect(0.0, 0.0, 100.0, 40.0))
            .with_focusable(true)
            .with_navigation_order(0),
    ])
    .unwrap()
}

fn deeply_clipped_tree(depth: usize) -> Result<UiTree, UiError> {
    let nodes = (0..depth)
        .map(|index| {
            UiNode::new(
                node(u32::try_from(index + 1).unwrap()),
                (index > 0).then(|| node(u32::try_from(index).unwrap())),
                rect(0.0, 0.0, 100.0, 100.0),
            )
            .with_clip_children(true)
        })
        .collect();
    UiTree::new(nodes)
}

fn solid_node(id: u32, parent: Option<u32>) -> UiNode {
    UiNode::new(node(id), parent.map(node), rect(0.0, 0.0, 1.0, 1.0)).with_visual(UiVisual::Solid {
        texture_page: 0,
        color: [255; 4],
    })
}

fn text_layout() -> Arc<TextLayout> {
    let rgba8 = vec![255; 8].into_boxed_slice();
    let pages = [
        FontTexturePage {
            source_path: "font/page0.png".into(),
            source_bytes: 4,
            source_sha256: [1; 32],
            pixels_sha256: Sha256::digest(&rgba8[..4]).into(),
            width: 1,
            height: 1,
            rgba8: rgba8[..4].to_vec().into_boxed_slice(),
        },
        FontTexturePage {
            source_path: "font/page1.png".into(),
            source_bytes: 4,
            source_sha256: [2; 32],
            pixels_sha256: Sha256::digest(&rgba8[4..]).into(),
            width: 1,
            height: 1,
            rgba8: rgba8[4..].to_vec().into_boxed_slice(),
        },
    ];
    let glyphs = [
        GlyphMetrics {
            codepoint: 'A',
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        },
        GlyphMetrics {
            codepoint: 'B',
            page: 1,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        },
        GlyphMetrics {
            codepoint: '\u{fffd}',
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        },
    ];
    let identity = [9; 32];
    let bytes = encode_font_catalog(identity, &glyphs, &pages).unwrap();
    let font = CompiledFontCatalog::decode(&bytes, identity).unwrap();
    TextLayoutCache::new(1, 64 * 1024)
        .layout(TextLayoutRequest {
            text: "AB",
            style: TextStyle::default(),
            width_64: 128,
            scale: UiScale::default(),
            font: &font,
        })
        .unwrap()
}

fn node(id: u32) -> UiNodeId {
    UiNodeId::new(id)
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> UiRect {
    UiRect::new(
        UiPoint::new(left, top).unwrap(),
        UiPoint::new(right, bottom).unwrap(),
    )
    .unwrap()
}

#[allow(dead_code)]
fn _assert_public_draw_contract(_: UiDrawList, _: UiVertex) {}
