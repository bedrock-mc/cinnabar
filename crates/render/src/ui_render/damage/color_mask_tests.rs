//! Per-stack sprite masks through the production fragment shader.

use super::*;

#[test]
fn sprite_color_mask_preserves_trim_cutout_and_independent_node_opacity() {
    let mut input = scene();
    input.vertices = Arc::from([]);
    input.indices = Arc::from([]);
    input.batches = Arc::from([]);
    input.textures = Arc::new(
        UiTextureCatalog::new(
            vec![
                UiTexturePage::owned(
                    [2, 2],
                    Arc::from([
                        200, 100, 50, 4, 255, 255, 255, 255, 20, 40, 80, 0, 255, 255, 255, 128,
                    ]),
                )
                .unwrap(),
            ],
            1,
        )
        .unwrap(),
    );
    quad(
        &mut input,
        [0., 0., SIDE as f32, SIDE as f32],
        [128, 255, 255, 128],
        0,
        None,
    );
    let tint = [51, 153, 204];
    for vertex in Arc::make_mut(&mut input.vertices) {
        vertex.style_flags = render_api::UI_STYLE_SPRITE_COLOR_MASK;
        vertex.alpha_cutoff = 1.0 / 255.0;
        vertex.overlay_color = [
            tint[0] as f32 / 255.,
            tint[1] as f32 / 255.,
            tint[2] as f32 / 255.,
            0.,
        ];
    }
    let Some(raster) = Raster::new(&input) else {
        return;
    };
    let output = target(&raster.gpu, false);
    raster.draw(&input, &output, None);
    let pixels = raster.pixels(&output);
    for (x, y, source) in [
        (8, 8, [200, 100, 50, 4]),
        (48, 8, [255, 255, 255, 255]),
        (8, 48, [20, 40, 80, 0]),
        (48, 48, [255, 255, 255, 128]),
    ] {
        let colored = assets::color_mask_texel(source, tint);
        let opacity = f32::from(colored[3]) / 255. * 128. / 255.;
        let expected = [
            colored[0] as f32 * 128. / 255. * opacity,
            colored[1] as f32 * opacity,
            colored[2] as f32 * opacity,
            255. * opacity,
        ];
        let at = (y * SIDE as usize + x) * 4;
        for c in 0..4 {
            assert!(
                (pixels[at + c] as f32 - expected[c]).abs() <= 2.,
                "{x},{y} channel{c}: {} vs {}",
                pixels[at + c],
                expected[c]
            );
        }
    }
}
