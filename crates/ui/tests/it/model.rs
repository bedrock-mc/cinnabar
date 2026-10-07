use std::sync::Arc;

use assets::{CompiledFontCatalog, FontPixels, FontTexturePage, GlyphMetrics, encode_font_catalog};
use sha2::{Digest, Sha256};
pub use ui::{
    PointerPhase, SafeArea, TextLayout, TextLayoutCache, TextLayoutRequest, TextShadow, TextStyle,
    UiAction, UiDrawBatch, UiDrawList, UiError, UiLimits, UiNode, UiNodeId, UiPoint, UiRect,
    UiScale, UiTree, UiVertex, UiVisual, UiWorldProjection,
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
fn cached_text_layout_emits_glyph_quads_by_texture_page() {
    let layout = text_layout();
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(4.0, 8.0, 100.0, 40.0)).with_visual(UiVisual::Text {
            layout,
            color: [255, 255, 255, 255],
            shadow: TextShadow::None,
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

// A glint sprite draws like a sprite but flags every vertex for the glint pass.
#[test]
fn glint_sprite_flags_its_vertices() {
    let sprite = |glint: bool| {
        let (texture_page, uv, color) = (3, [17, 23, 26, 32], [255; 4]);
        let visual = if glint {
            UiVisual::GlintSprite {
                texture_page,
                uv,
                color,
            }
        } else {
            UiVisual::Sprite {
                texture_page,
                uv,
                color,
            }
        };
        let mut tree = UiTree::new(vec![
            UiNode::new(node(1), None, rect(10.0, 20.0, 19.0, 29.0)).with_visual(visual),
        ])
        .unwrap();
        tree.layout(
            rect(0.0, 0.0, 100.0, 100.0),
            UiScale::default(),
            SafeArea::ZERO,
        )
        .unwrap();
        tree.build_draw_list().unwrap().vertices
    };
    let (plain, glint) = (sprite(false), sprite(true));
    assert!(plain.iter().all(|vertex| vertex.style_flags == 0));
    assert!(
        glint
            .iter()
            .all(|vertex| vertex.style_flags == ui::UI_STYLE_GLINT)
    );
    assert_eq!(
        plain.iter().map(|vertex| vertex.uv).collect::<Vec<_>>(),
        glint.iter().map(|vertex| vertex.uv).collect::<Vec<_>>()
    );
}

#[test]
fn sprite_visual_preserves_atlas_texel_bounds() {
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(10.0, 20.0, 19.0, 29.0)).with_visual(UiVisual::Sprite {
            texture_page: 3,
            uv: [17, 23, 26, 32],
            color: [255; 4],
        }),
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
        draw.vertices
            .iter()
            .map(|vertex| vertex.uv)
            .collect::<Vec<_>>(),
        [[17.0, 23.0], [26.0, 23.0], [26.0, 32.0], [17.0, 32.0]]
    );
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
    text_layout_sampling(false)
}

fn text_layout_sampling(linear: bool) -> Arc<TextLayout> {
    text_layout_rendering(linear, assets::FontRendering::Coverage)
}

fn text_layout_rendering(linear: bool, rendering: assets::FontRendering) -> Arc<TextLayout> {
    let rgba8 = vec![255; 8].into_boxed_slice();
    let pages = [
        FontTexturePage {
            source_path: "font/page0.png".into(),
            source_bytes: 4,
            source_sha256: [1; 32],
            pixels_sha256: Sha256::digest(&rgba8[..4]).into(),
            width: 1,
            height: 1,
            pixels: FontPixels::Rgba8(rgba8[..4].to_vec().into_boxed_slice()),
        },
        FontTexturePage {
            source_path: "font/page1.png".into(),
            source_bytes: 4,
            source_sha256: [2; 32],
            pixels_sha256: Sha256::digest(&rgba8[4..]).into(),
            width: 1,
            height: 1,
            pixels: FontPixels::Rgba8(rgba8[4..].to_vec().into_boxed_slice()),
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
    let font = if linear {
        font.with_linear_sampling()
    } else {
        font
    }
    .with_rendering(rendering);
    TextLayoutCache::new(1, 64 * 1024)
        .layout(TextLayoutRequest {
            text: "AB",
            style: TextStyle::default(),
            width_64: 128,
            line_height_64: 64,
            baseline_64: 0,
            scale: UiScale::default(),
            font: &font,
            wrap: Default::default(),
        })
        .unwrap()
}

#[test]
fn outline_text_selects_linear_sampler_without_changing_default_text_geometry() {
    let draw = |linear| {
        draw_list(UiVisual::Text {
            layout: text_layout_sampling(linear),
            color: [255; 4],
            shadow: TextShadow::None,
        })
    };
    let nearest = draw(false);
    let linear = draw(true);
    assert!(
        nearest
            .vertices
            .iter()
            .all(|vertex| vertex.style_flags == 0)
    );
    assert!(
        linear
            .vertices
            .iter()
            .all(|vertex| vertex.style_flags == ui::UI_STYLE_BILINEAR)
    );
    assert_eq!(nearest.indices, linear.indices);
    for (nearest, linear) in nearest.vertices.iter().zip(&linear.vertices) {
        assert_eq!(nearest.position, linear.position);
        assert_eq!(nearest.uv, linear.uv);
    }
}

#[test]
fn native_text_modes_emit_gamma_and_distance_flags_with_their_sampler() {
    use assets::{FONT_STYLE_COVERAGE_GAMMA, FONT_STYLE_SDF, FontRendering};
    for (rendering, flags) in [
        (FontRendering::NativeCoverage, FONT_STYLE_COVERAGE_GAMMA),
        (
            FontRendering::NativeSdf,
            FONT_STYLE_COVERAGE_GAMMA | FONT_STYLE_SDF | ui::UI_STYLE_BILINEAR,
        ),
    ] {
        let draw = draw_list(UiVisual::Text {
            layout: text_layout_rendering(true, rendering),
            color: [255; 4],
            shadow: TextShadow::None,
        });
        assert!(
            draw.vertices
                .iter()
                .all(|vertex| vertex.style_flags == flags)
        );
        assert_eq!(draw.vertices.len(), 8);
    }
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

#[test]
fn world_projection_keeps_local_geometry_independent_of_hud_scale_and_safe_area() {
    let projection = UiWorldProjection {
        clip_from_local: [
            [0.1, 0.0, 0.0, 0.0],
            [0.0, -0.1, 0.0, 0.0],
            [0.0; 4],
            [0.0, 0.0, 1.0, 2.0],
        ],
        viewport_size: [100.0, 80.0],
        depth_test: true,
        depth_write: true,
        alpha_test: true,
    };
    let solid = UiVisual::Solid {
        texture_page: 0,
        color: [255; 4],
    };
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(-4.0, -2.0, 4.0, 2.0))
            .with_visual(UiVisual::Gradient {
                texture_page: 0,
                colors: [[255, 0, 0, 255], [0, 0, 255, 128]],
                horizontal: false,
            })
            .with_world_projection(projection),
        UiNode::new(node(2), None, rect(4.0, 8.0, 8.0, 12.0)).with_visual(solid),
    ])
    .unwrap();
    tree.layout(
        rect(0.0, 0.0, 100.0, 80.0),
        UiScale::new(2.0).unwrap(),
        SafeArea::new(7.0, 9.0, 3.0, 2.0).unwrap(),
    )
    .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.vertices[0].position, [80.0, 72.0]);
    assert_eq!(draw.vertices[0].color, [255, 0, 0, 255]);
    assert_eq!(draw.vertices[2].color, [0, 0, 255, 128]);
    assert_eq!(draw.vertices[0].clip_z, 1.0);
    assert_eq!(draw.vertices[0].clip_w, 2.0);
    assert_eq!(draw.vertices[4].position, [15.0, 25.0]);
    assert_eq!(draw.vertices[4].clip_z, 0.0);
    assert_eq!(draw.vertices[4].clip_w, 1.0);
    assert!(draw.vertices[..4].iter().all(|vertex| vertex.alpha_test));
    assert!(draw.vertices[4..].iter().all(|vertex| !vertex.alpha_test));
    assert_eq!(draw.batches.len(), 2);
    assert!(draw.batches[0].depth_test && draw.batches[0].world_projection);
    assert!(draw.batches[0].depth_write);
    assert!(!draw.batches[1].depth_write);
    assert!(!draw.batches[1].depth_test && !draw.batches[1].world_projection);
    assert_eq!(draw.batches[0].clip, rect(0.0, 0.0, 100.0, 80.0));
}

#[test]
fn world_projection_preserves_homogeneous_w_at_and_behind_the_camera() {
    let projection = UiWorldProjection {
        clip_from_local: [
            [0.1, 0.0, 0.0, 0.25],
            [0.0, -0.1, 0.0, 0.0],
            [0.0; 4],
            [0.0, 0.0, 0.5, 1.0],
        ],
        viewport_size: [100.0, 80.0],
        depth_test: false,
        depth_write: false,
        alpha_test: false,
    };
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(-8.0, -2.0, 4.0, 2.0))
            .with_visual(UiVisual::Solid {
                texture_page: 0,
                color: [255; 4],
            })
            .with_world_projection(projection),
    ])
    .unwrap();
    tree.layout(
        rect(0.0, 0.0, 100.0, 80.0),
        UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.vertices[0].clip_w, -1.0);
    assert_eq!(draw.vertices[1].clip_w, 2.0);
    assert!(
        draw.vertices
            .iter()
            .all(|vertex| vertex.position.iter().all(|x| x.is_finite()))
    );
    assert_eq!(
        draw.indices.len(),
        6,
        "GPU must clip a partially visible quad, not a CPU bound"
    );
    let mut at_camera = projection;
    at_camera.clip_from_local[3][3] = 2.0;
    let tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(-8.0, -2.0, 4.0, 2.0))
            .with_visual(UiVisual::Solid {
                texture_page: 0,
                color: [255; 4],
            })
            .with_world_projection(at_camera),
    ])
    .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.vertices[0].clip_w, 0.0);
    assert!(draw.vertices[0].position.iter().all(|x| x.is_finite()));
}

#[test]
fn world_projection_applies_to_glyphs_and_shadows_without_rescaling_cached_layouts() {
    let layout = text_layout();
    let visual = UiVisual::Text {
        layout,
        color: [255; 4],
        shadow: TextShadow::Offset64(64),
    };
    let local = draw_list(visual.clone());
    let projection = UiWorldProjection {
        clip_from_local: [
            [0.01, 0.0, 0.0, 0.0],
            [0.0, -0.02, 0.0, 0.0],
            [0.0; 4],
            [0.1, 0.2, 0.3, 2.0],
        ],
        viewport_size: [200.0, 100.0],
        depth_test: true,
        depth_write: false,
        alpha_test: true,
    };
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(4.0, 8.0, 100.0, 40.0))
            .with_visual(visual)
            .with_world_projection(projection),
    ])
    .unwrap();
    tree.layout(
        rect(0.0, 0.0, 200.0, 100.0),
        UiScale::new(3.0).unwrap(),
        SafeArea::new(15.0, 10.0, 0.0, 0.0).unwrap(),
    )
    .unwrap();
    let projected = tree.build_draw_list().unwrap();
    assert_eq!(projected.vertices.len(), local.vertices.len());
    for (local, projected) in local.vertices.iter().zip(projected.vertices.iter()) {
        let expected = [
            (0.01 * local.position[0] + 2.1) * 100.0,
            (1.8 + 0.02 * local.position[1]) * 50.0,
        ];
        assert!((projected.position[0] - expected[0]).abs() < 0.0001);
        assert!((projected.position[1] - expected[1]).abs() < 0.0001);
        assert_eq!(projected.clip_z, 0.3);
        assert_eq!(projected.clip_w, 2.0);
        assert_eq!(projected.uv, local.uv);
        assert_eq!(projected.color, local.color);
        assert!(projected.alpha_test);
        assert!(!local.alpha_test);
    }
}

#[test]
fn world_projection_keeps_depth_writing_text_separate_from_its_read_only_plate() {
    let projection = UiWorldProjection {
        clip_from_local: [
            [0.01, 0.0, 0.0, 0.0],
            [0.0, -0.01, 0.0, 0.0],
            [0.0; 4],
            [0.0, 0.0, 0.5, 1.0],
        ],
        viewport_size: [100.0, 80.0],
        depth_test: true,
        depth_write: false,
        alpha_test: false,
    };
    let node_with_mode = |id, depth_write| {
        UiNode::new(node(id), None, rect(0.0, 0.0, 10.0, 10.0))
            .with_visual(UiVisual::Solid {
                texture_page: 0,
                color: [255, 255, 255, 32],
            })
            .with_world_projection(UiWorldProjection {
                depth_write,
                alpha_test: depth_write,
                ..projection
            })
    };
    let tree = UiTree::new(vec![
        node_with_mode(1, false),
        node_with_mode(2, true),
        node_with_mode(3, false),
    ])
    .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.batches.len(), 3);
    assert_eq!(
        draw.batches
            .iter()
            .map(|batch| batch.depth_write)
            .collect::<Vec<_>>(),
        [false, true, false]
    );
    assert!(
        draw.vertices[4..8]
            .iter()
            .all(|vertex| vertex.alpha_test && vertex.color[3] == 32)
    );
    assert!(
        draw.vertices[..4]
            .iter()
            .chain(draw.vertices[8..].iter())
            .all(|vertex| !vertex.alpha_test)
    );
}

#[test]
fn world_projection_rejects_non_finite_matrix_or_invalid_viewport() {
    let mut projection = UiWorldProjection {
        clip_from_local: [[0.0; 4]; 4],
        viewport_size: [100.0, 80.0],
        depth_test: false,
        depth_write: false,
        alpha_test: false,
    };
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        projection.clip_from_local[0][1] = invalid;
        assert!(matches!(
            UiTree::new(vec![
                UiNode::new(node(1), None, rect(0.0, 0.0, 1.0, 1.0))
                    .with_world_projection(projection)
            ]),
            Err(UiError::InvalidWorldProjection { .. })
        ));
    }
    projection.clip_from_local[0][1] = 0.0;
    projection.viewport_size[0] = 0.0;
    assert!(matches!(
        UiTree::new(vec![
            UiNode::new(node(1), None, rect(0.0, 0.0, 1.0, 1.0)).with_world_projection(projection)
        ]),
        Err(UiError::InvalidWorldProjection { .. })
    ));
}

#[test]
fn text_shadow_draws_a_darkened_offset_pass_before_the_glyph_pass() {
    let layout = text_layout();
    let glyphs = layout.glyphs().len();
    let unshadowed = draw_list(UiVisual::Text {
        layout: Arc::clone(&layout),
        color: [255, 255, 255, 255],
        shadow: TextShadow::None,
    });
    // One design pixel at the layout's scale.
    let shadowed = draw_list(UiVisual::Text {
        layout: Arc::clone(&layout),
        color: [200, 100, 60, 255],
        shadow: TextShadow::Offset64(2 * 64),
    });

    assert_eq!(unshadowed.vertices.len(), glyphs * 4);
    assert_eq!(shadowed.vertices.len(), glyphs * 8);

    // The shadow pass comes first, each channel quartered, alpha preserved.
    assert_eq!(
        shadowed.vertices[0].color,
        [200 >> 2, 100 >> 2, 60 >> 2, 255]
    );
    assert_eq!(shadowed.vertices[glyphs * 4].color, [200, 100, 60, 255]);

    // ...and it sits exactly one design pixel down-right of the glyph itself.
    let shadow_origin = shadowed.vertices[0].position;
    let glyph_origin = shadowed.vertices[glyphs * 4].position;
    assert_eq!(shadow_origin[0] - glyph_origin[0], 2.0);
    assert_eq!(shadow_origin[1] - glyph_origin[1], 2.0);
}

fn draw_list(visual: UiVisual) -> UiDrawList {
    let mut tree = UiTree::new(vec![
        UiNode::new(node(1), None, rect(4.0, 8.0, 100.0, 40.0)).with_visual(visual),
    ])
    .unwrap();
    tree.layout(
        rect(0.0, 0.0, 200.0, 100.0),
        UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();
    tree.build_draw_list().unwrap()
}
