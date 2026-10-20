//! CPU outline sampling matches the production GPU mask without sampling neighboring atlas art.

use super::Page;

/// Returns white coverage for a one-texel inner outline inside the complete source rectangle.
pub(super) fn sample(texture: &Page, uv: [f32; 2], source: [f32; 4], overlap: [f32; 2]) -> [u8; 4] {
    let mut coverage = coverage(texture, uv, source);
    if overlap[0] > 0. {
        let neighbor = [
            uv[0] - overlap[0] * (source[2] - source[0]),
            uv[1] - overlap[1] * (source[3] - source[1]),
        ];
        coverage = (u32::from(coverage)
            * u32::from(255 - self::coverage(texture, neighbor, source))
            / 255) as u8;
    }
    [255, 255, 255, coverage]
}

/// Samples the inner edge without treating atlas neighbors as part of the sprite.
fn coverage(texture: &Page, uv: [f32; 2], source: [f32; 4]) -> u8 {
    let [x, y] = uv.map(|value| value.floor() as i64);
    let alpha = |x: i64, y: i64| {
        let point = [x as f32 + 0.5, y as f32 + 0.5];
        if x < 0
            || y < 0
            || x >= i64::from(texture.width)
            || y >= i64::from(texture.height)
            || point[0] < source[0]
            || point[1] < source[1]
            || point[0] >= source[2]
            || point[1] >= source[3]
        {
            0
        } else {
            texture.sample(x as f32, y as f32)[3]
        }
    };
    let center = alpha(x, y);
    let mut interior = center;
    for dy in -1..=1 {
        for dx in -1..=1 {
            interior = interior.min(alpha(x + dx, y + dy));
        }
    }
    center.saturating_sub(interior)
}

#[cfg(test)]
mod tests {
    use super::super::{Canvas, Page};
    use ui::{UiBlendMode, UiNode, UiNodeId, UiPoint, UiRect, UiTree, UiVisual};

    /// Builds a colored pack page with an alpha cutout and opaque art outside the mask bounds.
    fn page() -> Page {
        let mut rgba = vec![0u8; 8 * 8 * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[17, 99, 180, 255]);
        }
        rgba[(2 * 8 + 2) * 4 + 3] = 0;
        Page {
            width: 8,
            height: 8,
            rgba: rgba.into(),
            repeat_content: None,
        }
    }

    /// Uses actual outline nodes and the editor's full draw-list rasterization path.
    fn raster(inverse: bool, cropped: bool, opacity: u8) -> Vec<u8> {
        let rect =
            UiRect::new(UiPoint::new(0., 0.).unwrap(), UiPoint::new(4., 4.).unwrap()).unwrap();
        let mut nodes = Vec::new();
        if inverse {
            nodes.push(
                UiNode::new(UiNodeId::new(1), None, rect).with_visual(UiVisual::Solid {
                    texture_page: 0,
                    color: [17, 99, 180, 255],
                }),
            );
        }
        nodes.push(UiNode::new(UiNodeId::new(2), None, rect).with_visual(
            UiVisual::SpriteOutline {
                texture_page: 1,
                uv: [if cropped { 4. } else { 2. }, 2., 6., 6.],
                overlap_offset: [0.; 2],
                source: [2., 2., 6., 6.],
                color: if inverse {
                    [255, 255, 255, opacity]
                } else {
                    [240, 80, 20, opacity]
                },
                blend: if inverse {
                    UiBlendMode::Invert
                } else {
                    UiBlendMode::Alpha
                },
            },
        ));
        let list = UiTree::new(nodes).unwrap().build_draw_list().unwrap();
        let white = Page::white();
        let texture = page();
        let mut canvas = Canvas::new(4, 4);
        canvas.draw(&list, |page| {
            Some(if page == 0 { &white } else { &texture })
        });
        canvas.into_rgba()
    }

    /// Composites two nine-pixel masks at their actual eight-pixel pitch.
    fn adjacent(inverse: bool, both_edges: bool, dy: u32, opacity: u8) -> Vec<u8> {
        let rect = |x, y, w, h| {
            UiRect::new(
                UiPoint::new(x, y).unwrap(),
                UiPoint::new(x + w, y + h).unwrap(),
            )
            .unwrap()
        };
        let mut nodes = vec![
            UiNode::new(UiNodeId::new(0), None, rect(0., 0., 17., 10.)).with_visual(
                UiVisual::Solid {
                    texture_page: 0,
                    color: [17, 99, 180, 255],
                },
            ),
        ];
        for (id, x, y, overlap_offset) in [
            (1, 8., dy as f32, [0.; 2]),
            (2, 0., 0., [8. / 9., dy as f32 / 9.]),
        ] {
            nodes.push(
                UiNode::new(UiNodeId::new(id), None, rect(x, y, 9., 9.)).with_visual(
                    UiVisual::SpriteOutline {
                        texture_page: 1,
                        uv: [0., 0., 9., 9.],
                        source: [0., 0., 9., 9.],
                        overlap_offset,
                        color: if inverse {
                            [255, 255, 255, opacity]
                        } else {
                            [240, 80, 20, opacity]
                        },
                        blend: if inverse {
                            UiBlendMode::Invert
                        } else {
                            UiBlendMode::Alpha
                        },
                    },
                ),
            );
        }
        let texture = Page {
            width: 9,
            height: 9,
            rgba: (0..81)
                .flat_map(|at| {
                    [
                        30,
                        50,
                        70,
                        if at % 9 == 8 || both_edges && at % 9 == 0 {
                            255
                        } else {
                            0
                        },
                    ]
                })
                .collect(),
            repeat_content: None,
        };
        let white = Page::white();
        let draw = UiTree::new(nodes).unwrap().build_draw_list().unwrap();
        let mut canvas = Canvas::new(17, 10);
        canvas.draw(&draw, |page| {
            Some(if page == 0 { &white } else { &texture })
        });
        canvas.into_rgba()
    }

    /// Sparse right-edge art survives while real overlap paints only once in either blend mode.
    #[test]
    fn adjacent_outlines_preserve_sparse_edges_and_suppress_only_shared_coverage() {
        for inverse in [false, true] {
            for both_edges in [false, true] {
                for dy in [0, 1] {
                    for opacity in [110, 255] {
                        let pixels = adjacent(inverse, both_edges, dy, opacity);
                        for y in 0..10 {
                            for x in 0..17 {
                                let covered = (y < 9 && (x == 8 || both_edges && x == 0))
                                    || ((dy..dy + 9).contains(&y)
                                        && (x == 16 || both_edges && x == 8));
                                for (channel, background) in
                                    [17., 99., 180.].into_iter().enumerate()
                                {
                                    let alpha = if covered {
                                        f32::from(opacity) / 255.
                                    } else {
                                        0.
                                    };
                                    let target = if inverse {
                                        255. - background
                                    } else {
                                        [240., 80., 20.][channel]
                                    };
                                    let expected = background + alpha * (target - background);
                                    let actual =
                                        f32::from(pixels[((y * 17 + x) * 4) as usize + channel]);
                                    assert!(
                                        (actual - expected).abs() <= 1.5,
                                        "({x}, {y}) inverse={inverse} both_edges={both_edges} dy={dy} opacity={opacity}: {actual} != {expected}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Atlas neighbors, cropped seams and pack RGB never turn the outline into a filled food icon.
    #[test]
    fn sprite_outline_preserves_inner_transparency_and_authored_tint() {
        for cropped in [false, true] {
            let pixels = raster(false, cropped, 128);
            for (x, y, painted) in if cropped {
                vec![(0, 2, false), (3, 3, true)]
            } else {
                vec![(0, 0, false), (1, 1, true), (2, 2, false), (3, 3, true)]
            } {
                let at = (y * 4 + x) * 4;
                let expected = if painted { [240, 80, 20, 128] } else { [0; 4] };
                for channel in 0..4 {
                    assert!(
                        (i16::from(pixels[at + channel]) - i16::from(expected[channel])).abs() <= 1
                    );
                }
            }
        }
    }

    /// Inversion reads the background through the outline, including translucent held-food previews.
    #[test]
    fn inverse_sprite_outline_preserves_uncovered_background() {
        for opacity in [255, 110] {
            let pixels = raster(true, false, opacity);
            for (x, y, inverted) in [(0, 0, false), (1, 1, true), (2, 2, false), (3, 3, true)] {
                let at = (y * 4 + x) * 4;
                for (channel, background) in [17., 99., 180.].into_iter().enumerate() {
                    let alpha = if inverted {
                        f32::from(opacity) / 255.
                    } else {
                        0.
                    };
                    let expected = background + alpha * (255. - 2. * background);
                    assert!((f32::from(pixels[at + channel]) - expected).abs() <= 1.5);
                }
                assert_eq!(pixels[at + 3], 255);
            }
        }
    }
}
