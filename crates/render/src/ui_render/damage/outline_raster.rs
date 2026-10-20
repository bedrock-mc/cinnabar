//! Sprite outlines use the production shader and synthetic, deliberately unrelated pack colors.

use super::*;

/// A four-texel mask surrounded by opaque unrelated atlas art, with one transparent cutout.
fn outline_scene(cropped: bool) -> UiRenderInput {
    let mut input = scene();
    input.vertices = Arc::from([]);
    input.indices = Arc::from([]);
    input.batches = Arc::from([]);
    let mut texels = vec![0u8; 8 * 8 * 4];
    for pixel in texels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[17, 99, 180, 255]);
    }
    texels[(2 * 8 + 2) * 4 + 3] = 0;
    input.textures = Arc::new(
        UiTextureCatalog::new(
            vec![UiTexturePage::owned([8, 8], texels.into()).unwrap()],
            1,
        )
        .unwrap(),
    );
    quad(
        &mut input,
        [0., 0., SIDE as f32, SIDE as f32],
        [240, 80, 20, 128],
        0,
        None,
    );
    for vertex in Arc::make_mut(&mut input.vertices) {
        vertex.style_flags = u32::from(render_api::UI_STYLE_SPRITE_OUTLINE);
        vertex.model_light = 0.;
        vertex.alpha_cutoff = 0.;
        vertex.overlay_color = [2., 2., 6., 6.];
        vertex.uv = [
            2. + vertex.position[0] / SIDE as f32 * 4.,
            2. + vertex.position[1] / SIDE as f32 * 4.,
        ];
        if cropped {
            vertex.uv[0] = 4. + vertex.position[0] / SIDE as f32 * 2.;
        }
    }
    input
}

/// Real pixels preserve tint, transparency, sprite boundaries and a cropped preview's seam.
#[test]
fn sprite_outline_raster_ignores_texture_rgb_and_neighboring_atlas_art() {
    for cropped in [false, true] {
        let input = outline_scene(cropped);
        let Some(raster) = Raster::new(&input) else {
            return;
        };
        let output = target(&raster.gpu, false);
        raster.draw(&input, &output, None);
        let pixels = raster.pixels(&output);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let source_x = if cropped { 4 + x / 32 } else { 2 + x / 16 };
                let source_y = 2 + y / 16;
                let opaque = |x: i32, y: i32| {
                    (2..6).contains(&x) && (2..6).contains(&y) && (x != 2 || y != 2)
                };
                let center = opaque(source_x as i32, source_y as i32);
                let interior = (-1..=1).all(|dy| {
                    (-1..=1).all(|dx| opaque(source_x as i32 + dx, source_y as i32 + dy))
                });
                let expected: [u8; 4] = if center && !interior {
                    [120, 40, 10, 128]
                } else {
                    [0; 4]
                };
                let at = ((y * SIDE + x) * 4) as usize;
                for channel in 0..4 {
                    assert!(
                        (i16::from(pixels[at + channel]) - i16::from(expected[channel])).abs() <= 1,
                        "pixel ({x}, {y}), cropped={cropped}, expected={expected:?}, actual={:?}",
                        &pixels[at..at + 4]
                    );
                }
            }
        }
    }
}

/// The actual invert pipeline changes the background only at the alpha outline, with faded preview support.
#[test]
fn inverse_sprite_outline_changes_the_background_without_changing_the_mask() {
    for opacity in [255u8, 110] {
        let mut input = outline_scene(false);
        let mut outline_vertices = input.vertices.to_vec();
        for vertex in &mut outline_vertices {
            vertex.color = [255, 255, 255, opacity];
        }
        input.vertices = Arc::from([]);
        input.indices = Arc::from([]);
        input.batches = Arc::from([]);
        quad(
            &mut input,
            [0., 0., SIDE as f32, SIDE as f32],
            [255; 4],
            0,
            None,
        );
        for vertex in Arc::make_mut(&mut input.vertices) {
            vertex.uv = [0.5, 0.5];
        }
        let mut vertices = input.vertices.to_vec();
        vertices.extend(outline_vertices);
        input.vertices = vertices.into();
        let mut indices = input.indices.to_vec();
        indices.extend([4, 5, 6, 4, 6, 7]);
        input.indices = indices.into();
        let mut batches = input.batches.to_vec();
        batches.push(UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, SIDE, SIDE),
            6,
            6,
            render_model::UI_BLEND_INVERT,
        ));
        input.batches = batches.into();
        let Some(raster) = Raster::new(&input) else {
            return;
        };
        let output = target(&raster.gpu, false);
        raster.draw(&input, &output, None);
        let pixels = raster.pixels(&output);
        for (x, y, inverted) in [
            (8, 8, false),
            (24, 24, true),
            (40, 40, false),
            (56, 56, true),
        ] {
            let offset = ((y * SIDE + x) * 4) as usize;
            for (channel, background) in [17., 99., 180.].into_iter().enumerate() {
                let alpha = if inverted {
                    f32::from(opacity) / 255.
                } else {
                    0.
                };
                let expected = background + alpha * (255. - 2. * background);
                assert!(
                    (f32::from(pixels[offset + channel]) - expected).abs() <= 1.5,
                    "inverse pixel ({x}, {y}) opacity={opacity}, RGB={:?}",
                    &pixels[offset..offset + 3]
                );
            }
        }
    }
}

/// Sparse nine-pixel art retains its last column; coincident opaque edges blend only once.
#[test]
fn adjacent_sprite_outlines_preserve_sparse_edges_in_alpha_and_inverse_modes() {
    for inverse in [false, true] {
        for both_edges in [false, true] {
            for dy in [0, 1] {
                let mut input = scene();
                input.vertices = Arc::from([]);
                input.indices = Arc::from([]);
                input.batches = Arc::from([]);
                let mask: Vec<u8> = (0..81)
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
                    .collect();
                input.textures = Arc::new(
                    UiTextureCatalog::new(
                        vec![
                            UiTexturePage::owned([9, 9], [17, 99, 180, 255].repeat(81).into())
                                .unwrap(),
                            UiTexturePage::owned([9, 9], mask.into()).unwrap(),
                        ],
                        1,
                    )
                    .unwrap(),
                );
                quad(
                    &mut input,
                    [0., 0., SIDE as f32, SIDE as f32],
                    [255; 4],
                    0,
                    None,
                );
                let opacity = 110;
                for (x, y, overlap) in [
                    (8., dy as f32, [0.; 2]),
                    (0., 0., [8. / 9., dy as f32 / 9.]),
                ] {
                    let start = input.vertices.len();
                    quad(
                        &mut input,
                        [x, y, x + 9., y + 9.],
                        if inverse {
                            [255, 255, 255, opacity]
                        } else {
                            [240, 80, 20, opacity]
                        },
                        1,
                        None,
                    );
                    for vertex in &mut Arc::make_mut(&mut input.vertices)[start..] {
                        vertex.style_flags = u32::from(render_api::UI_STYLE_SPRITE_OUTLINE);
                        vertex.overlay_color = [0., 0., 9., 9.];
                        vertex.model_light = overlap[0];
                        vertex.alpha_cutoff = overlap[1];
                        vertex.uv = [vertex.position[0] - x, vertex.position[1] - y];
                    }
                }
                if inverse {
                    for batch in &mut Arc::make_mut(&mut input.batches)[1..] {
                        batch.blend_mode = render_model::UI_BLEND_INVERT;
                    }
                }
                let Some(raster) = Raster::new(&input) else {
                    return;
                };
                let output = target(&raster.gpu, false);
                raster.draw(&input, &output, None);
                let pixels = raster.pixels(&output);
                for y in 0..10 {
                    for x in 0..17 {
                        let covered = (y < 9 && (x == 8 || both_edges && x == 0))
                            || ((dy..dy + 9).contains(&y) && (x == 16 || both_edges && x == 8));
                        for (channel, background) in [17., 99., 180.].into_iter().enumerate() {
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
                            let actual = f32::from(pixels[((y * SIDE + x) * 4) as usize + channel]);
                            assert!(
                                (actual - expected).abs() <= 1.5,
                                "({x}, {y}) inverse={inverse} both_edges={both_edges} dy={dy}: {actual} != {expected}"
                            );
                        }
                    }
                }
            }
        }
    }
}
