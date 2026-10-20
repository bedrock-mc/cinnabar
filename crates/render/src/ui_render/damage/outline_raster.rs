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
                let expected = if center && !interior {
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
