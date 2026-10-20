//! CPU outline sampling matches the production GPU mask without sampling neighboring atlas art.

use super::Page;

/// Returns white coverage for a one-texel inner outline inside the complete source rectangle.
pub(super) fn sample(texture: &Page, uv: [f32; 2], source: [f32; 4]) -> [u8; 4] {
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
    [255, 255, 255, center.saturating_sub(interior)]
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
