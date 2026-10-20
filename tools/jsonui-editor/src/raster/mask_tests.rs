//! Prepared masks follow the normal draw-list raster path without an outline-specific sampler.

use super::{Canvas, Page};
use render_api::sprite_mask::{MaskLayout, MaskPatch, SpriteMask};
use std::sync::Arc;
use ui::{UiBlendMode, UiMesh, UiNode, UiNodeId, UiPoint, UiRect, UiTree, UiVisual};

/// Draws one prepared mask through the same ordinary textured geometry used by native painters.
fn raster(layout: &MaskLayout, inverse: bool, opacity: u8) -> Vec<u8> {
    let [w, h] = layout.size;
    let rect = UiRect::new(
        UiPoint::new(0., 0.).unwrap(),
        UiPoint::new(w as f32, h as f32).unwrap(),
    )
    .unwrap();
    let draw = UiTree::new(vec![
        UiNode::new(UiNodeId::new(0), None, rect).with_visual(UiVisual::Solid {
            texture_page: 0,
            color: [17, 99, 180, 255],
        }),
        UiNode::new(UiNodeId::new(1), None, rect).with_visual(UiVisual::Mesh(Arc::new(
            UiMesh::sprite(
                1,
                [0., 0., w as f32, h as f32],
                if inverse {
                    [255, 255, 255, opacity]
                } else {
                    [240, 80, 20, opacity]
                },
                if inverse {
                    UiBlendMode::Invert
                } else {
                    UiBlendMode::Alpha
                },
            )
            .unwrap(),
        ))),
    ])
    .unwrap()
    .build_draw_list()
    .unwrap();
    let page = Page {
        width: w,
        height: h,
        rgba: layout.rasterize().unwrap().into(),
        repeat_content: None,
    };
    let white = Page::white();
    let mut canvas = Canvas::new(w, h);
    canvas.draw(&draw, |index| Some(if index == 0 { &white } else { &page }));
    canvas.into_rgba()
}

/// Checks coverage after exactly one alpha-over or inverse operation.
fn assert_color(pixels: &[u8], at: usize, coverage: u8, opacity: u8, inverse: bool) {
    let alpha = f32::from(coverage) * f32::from(opacity) / (255. * 255.);
    for (channel, background) in [17., 99., 180.].into_iter().enumerate() {
        let target = if inverse {
            255. - background
        } else {
            [240., 80., 20.][channel]
        };
        let expected = background + alpha * (target - background);
        assert!(
            (f32::from(pixels[at + channel]) - expected).abs() <= 1.5,
            "inverse={inverse} source={coverage}: {} != {expected}",
            pixels[at + channel]
        );
    }
}

#[test]
fn adjacent_outlines_preserve_sparse_edges_and_suppress_all_shared_coverage() {
    for inverse in [false, true] {
        for (left, right) in [(0, 255), (64, 64), (128, 128), (255, 255), (128, 255)] {
            for dy in [0, 1] {
                for opacity in [110, 255] {
                    let layout = MaskLayout {
                        source: Arc::new(SpriteMask::outline([9, 9], |x, _| {
                            if x == 0 {
                                left
                            } else if x == 8 {
                                right
                            } else {
                                0
                            }
                        })),
                        extent: [17., 10.],
                        size: [17, 10],
                        patches: [[8., dy as f32, 17., dy as f32 + 9.], [0., 0., 9., 9.]]
                            .map(|bounds| MaskPatch {
                                bounds,
                                uv: [0., 0., 1., 1.],
                                opacity: 255,
                            })
                            .into(),
                    };
                    let pixels = raster(&layout, inverse, opacity);
                    for y in 0..10 {
                        for x in 0..17 {
                            let edge = |x, y| {
                                if y < 9 {
                                    if x == 0 {
                                        left
                                    } else if x == 8 {
                                        right
                                    } else {
                                        0
                                    }
                                } else {
                                    0
                                }
                            };
                            let coverage = edge(x, y).max(if x >= 8 && y >= dy {
                                edge(x - 8, y - dy)
                            } else {
                                0
                            });
                            assert_color(
                                &pixels,
                                ((y * 17 + x) * 4) as usize,
                                coverage,
                                opacity,
                                inverse,
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn high_resolution_outline_survives_minification_in_both_blend_modes() {
    let layout = MaskLayout {
        source: Arc::new(SpriteMask::outline([72, 72], |_, _| 255)),
        extent: [72., 72.],
        size: [18, 18],
        patches: vec![MaskPatch {
            bounds: [0., 0., 72., 72.],
            uv: [0., 0., 1., 1.],
            opacity: 255,
        }],
    };
    for inverse in [false, true] {
        let pixels = raster(&layout, inverse, 255);
        for (x, y) in [(0, 8), (17, 8), (8, 0), (8, 17)] {
            assert_color(&pixels, (y * 18 + x) * 4, 64, 255, inverse);
        }
        assert_color(&pixels, (8 * 18 + 8) * 4, 0, 255, inverse);
    }
}

#[test]
fn cropped_masks_preserve_the_original_inner_transparency() {
    let source = Arc::new(SpriteMask::outline([4, 4], |x, y| {
        if x == 0 && y == 0 { 0 } else { 255 }
    }));
    let layout = MaskLayout {
        source,
        extent: [2., 4.],
        size: [2, 4],
        patches: vec![MaskPatch {
            bounds: [0., 0., 2., 4.],
            uv: [0.5, 0., 1., 1.],
            opacity: 255,
        }],
    };
    let pixels = raster(&layout, false, 128);
    assert_color(&pixels, (2 * 2) * 4, 0, 128, false);
    assert_color(&pixels, (3 * 2 + 1) * 4, 255, 128, false);
}
