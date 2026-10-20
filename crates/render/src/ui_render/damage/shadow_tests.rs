//! Analytic shadow pixels through the production UI shader.

use super::*;

#[test]
fn rounded_shadows_gpu_matches_the_cpu_contract_with_ancestor_clipping() {
    for blur in [0.0, 0.01, 0.1, 4.0] {
        let mut input = scene();
        input.vertices = Arc::from([]);
        input.indices = Arc::from([]);
        input.batches = Arc::from([]);
        quad(
            &mut input,
            [0.0, 0.0, SIDE as f32, SIDE as f32],
            [90, 150, 210, 192],
            0,
            None,
        );
        for vertex in Arc::make_mut(&mut input.vertices) {
            vertex.style_flags = u32::from(render_api::ui_shadow::UI_STYLE_SHADOW);
            vertex.uv = vertex.position.map(|axis| axis - SIDE as f32 * 0.5);
            vertex.overlay_color = [20.25, 14.25, 8.0, blur];
        }
        Arc::make_mut(&mut input.batches)[0].scissor = UiScissor::new(8, 0, SIDE - 8, SIDE);
        let Some(raster) = Raster::new(&input) else {
            return;
        };
        let output = target(&raster.gpu, false);
        raster.draw(&input, &output, None);
        let pixels = raster.pixels(&output);
        for y in [1, 12, 18, 32, 46, 50, 62] {
            for x in [2, 8, 14, 32, 54, 62] {
                let point = [
                    x as f32 + 0.5 - SIDE as f32 * 0.5,
                    y as f32 + 0.5 - SIDE as f32 * 0.5,
                ];
                let coverage = if x < 8 {
                    0.0
                } else {
                    render_api::ui_shadow::coverage(point, [20.25, 14.25, 8.0, blur], 1.0)
                };
                let alpha = 192.0 / 255.0 * coverage;
                let expected = [90.0 * alpha, 150.0 * alpha, 210.0 * alpha, 255.0 * alpha];
                let offset = ((y * SIDE + x) * 4) as usize;
                for (actual, expected) in pixels[offset..offset + 4].iter().zip(expected) {
                    assert!(
                        (f32::from(*actual) - expected).abs() <= 1.1,
                        "{x},{y}: {actual} != {expected}"
                    );
                }
            }
        }
    }
}
