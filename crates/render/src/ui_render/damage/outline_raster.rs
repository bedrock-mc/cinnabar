//! CPU-prepared masks exercise the ordinary sprite shader and both production blend modes.

use super::*;
use render_api::sprite_mask::{MaskLayout, MaskPatch, SpriteMask};

/// Draws a single prepared white mask over an unrelated colored background.
fn mask_scene(size: [u32; 2], rgba: Vec<u8>, inverse: bool, opacity: u8) -> UiRenderInput {
    let mut input = scene();
    input.vertices = Arc::from([]);
    input.indices = Arc::from([]);
    input.batches = Arc::from([]);
    let pixels = (size[0] * size[1]) as usize;
    let pages = vec![
        UiTexturePage::owned(size, [17, 99, 180, 255].repeat(pixels).into()).unwrap(),
        UiTexturePage::owned(size, rgba.into()).unwrap(),
    ];
    let dynamic_start = pages.len();
    input.textures = Arc::new(UiTextureCatalog::new(pages, dynamic_start).unwrap());
    quad(
        &mut input,
        [0., 0., SIDE as f32, SIDE as f32],
        [255; 4],
        0,
        None,
    );
    let start = input.vertices.len();
    quad(
        &mut input,
        [0., 0., size[0] as f32, size[1] as f32],
        if inverse {
            [255, 255, 255, opacity]
        } else {
            [240, 80, 20, opacity]
        },
        1,
        None,
    );
    for vertex in &mut Arc::make_mut(&mut input.vertices)[start..] {
        vertex.uv = vertex.position[..2].try_into().unwrap();
    }
    if inverse {
        Arc::make_mut(&mut input.batches)[1].blend_mode = render_model::UI_BLEND_INVERT;
    }
    input
}

/// Uses two repeated source masks at their real eight-pixel pitch, combining before compositing.
fn adjacent_mask(left: u8, right: u8, dy: u32) -> Vec<u8> {
    MaskLayout {
        source: Arc::new(SpriteMask::outline([9, 9], [9, 9], |x, _| {
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
    }
    .rasterize()
    .unwrap()
}

#[test]
fn adjacent_sprite_outlines_preserve_sparse_and_unequal_edges_in_both_blend_modes() {
    for inverse in [false, true] {
        for (left, right) in [(0, 255), (64, 64), (128, 128), (255, 255), (128, 255)] {
            for dy in [0, 1] {
                for opacity in [110, 255] {
                    let input =
                        mask_scene([17, 10], adjacent_mask(left, right, dy), inverse, opacity);
                    let Some(raster) = Raster::new(&input) else {
                        return;
                    };
                    let output = target(&raster.gpu, false);
                    raster.draw(&input, &output, None);
                    let pixels = raster.pixels(&output);
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
                                (y * SIDE + x) as usize * 4,
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

/// Compares the rendered single paint against alpha-over or one inversion of the background.
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
            "inverse={inverse} source={coverage} opacity={opacity} channel={channel}: {} != {expected}",
            pixels[at + channel]
        );
    }
}

#[test]
fn high_resolution_outline_keeps_coverage_when_minified_to_eighteen_pixels() {
    let layout = MaskLayout {
        source: Arc::new(SpriteMask::outline([72, 72], [18, 18], |_, _| 255)),
        extent: [72., 72.],
        size: [18, 18],
        patches: vec![MaskPatch {
            bounds: [0., 0., 72., 72.],
            uv: [0., 0., 1., 1.],
            opacity: 255,
        }],
    };
    for inverse in [false, true] {
        let input = mask_scene(layout.size, layout.rasterize().unwrap(), inverse, 255);
        let Some(raster) = Raster::new(&input) else {
            return;
        };
        let output = target(&raster.gpu, false);
        raster.draw(&input, &output, None);
        let pixels = raster.pixels(&output);
        for (x, y) in [(0, 8), (17, 8), (8, 0), (8, 17)] {
            assert_color(&pixels, ((y * SIDE + x) * 4) as usize, 64, 255, inverse);
        }
        assert_color(&pixels, ((8 * SIDE + 8) * 4) as usize, 0, 255, inverse);
    }
}

#[test]
fn source_cutouts_and_cropped_bands_do_not_create_filled_icons_or_new_edges() {
    let source = Arc::new(SpriteMask::outline([4, 4], [4, 4], |x, y| {
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
    let input = mask_scene(layout.size, layout.rasterize().unwrap(), false, 128);
    let Some(raster) = Raster::new(&input) else {
        return;
    };
    let output = target(&raster.gpu, false);
    raster.draw(&input, &output, None);
    let pixels = raster.pixels(&output);
    assert_color(&pixels, (2 * SIDE * 4) as usize, 0, 128, false);
    assert_color(&pixels, ((3 * SIDE + 1) * 4) as usize, 255, 128, false);
}
