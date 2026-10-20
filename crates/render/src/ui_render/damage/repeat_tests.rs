//! Pixel coverage and filter-tap comparisons through the production repeat shader.

use super::*;
use render_api::ui_repeat::{UI_STYLE_REPEAT, filtered_uv, source_uv};

const PAGE: [u8; 16] = [
    32, 96, 192, 255, 240, 32, 64, 255, 32, 192, 96, 255, 192, 64, 240, 255,
];

/// Samples the original fixture page at a texel center.
fn texel(uv: [f32; 2]) -> [f32; 4] {
    let [x, y] = uv.map(|v| v.floor().clamp(0.0, 1.0) as usize);
    std::array::from_fn(|channel| f32::from(PAGE[(y * 2 + x) * 4 + channel]))
}

/// Computes the repeat sampling contract independently of the GPU shader.
fn expected(coords: [f32; 2], bounds: [f32; 4], linear: bool) -> [u8; 4] {
    let uv = source_uv(coords, bounds, UI_STYLE_REPEAT);
    if !linear {
        return texel(uv).map(|v| v as u8);
    }
    let uv = filtered_uv(uv, bounds);
    let low = uv.map(|v| (v - 0.5).floor() + 0.5);
    let f = uv.map(|v| v - 0.5 - (v - 0.5).floor());
    let taps = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
        .map(|at| texel([low[0] + at[0], low[1] + at[1]]));
    std::array::from_fn(|channel| {
        let a = taps[0][channel] * (1.0 - f[0]) + taps[1][channel] * f[0];
        let b = taps[2][channel] * (1.0 - f[0]) + taps[3][channel] * f[0];
        (a * (1.0 - f[1]) + b * f[1]).round() as u8
    })
}

#[test]
fn repeated_atlas_regions_keep_coverage_clipping_and_filtered_seams() {
    let mut input = scene();
    input.vertices = Arc::from([]);
    input.indices = Arc::from([]);
    input.batches = Arc::from([]);
    input.textures = Arc::new(
        UiTextureCatalog::new(
            vec![UiTexturePage::owned([2, 2], Arc::from(PAGE)).unwrap(); 2],
            2,
        )
        .unwrap(),
    );
    quad(
        &mut input,
        [0.0, 0.0, SIDE as f32, SIDE as f32],
        [255; 4],
        0,
        None,
    );
    let Some(raster) = Raster::new(&input) else {
        return;
    };
    for source in [
        [0.0, 0.0, 1.0, 2.0],
        [1.0, 0.0, 1.0, 2.0],
        [0.0, 0.0, 2.0, 2.0],
        [0.25, 0.5, 1.5, 1.0],
        [0.75, 0.75, 0.25, 0.25],
    ] {
        for linear in [false, true] {
            for (vertex, uv) in Arc::make_mut(&mut input.vertices).iter_mut().zip([
                [0.25, 0.375],
                [13.5, 0.375],
                [13.5, 7.875],
                [0.25, 7.875],
            ]) {
                vertex.uv = uv;
                vertex.overlay_color = source;
                vertex.style_flags = u32::from(UI_STYLE_REPEAT)
                    | if linear {
                        u32::from(render_api::UI_STYLE_BILINEAR)
                    } else {
                        0
                    };
            }
            Arc::make_mut(&mut input.batches)[0].scissor = UiScissor::new(3, 5, 56, 51);
            let output = target(&raster.gpu, false);
            raster.draw(&input, &output, None);
            for (at, actual) in raster.pixels(&output).as_chunks::<4>().0.iter().enumerate() {
                let x = at as u32 % SIDE;
                let y = at as u32 / SIDE;
                if !(3..59).contains(&x) || !(5..56).contains(&y) {
                    assert_eq!(actual, &[0; 4]);
                } else {
                    let coords = [
                        0.25 + (x as f32 + 0.5) * 13.25 / SIDE as f32,
                        0.375 + (y as f32 + 0.5) * 7.5 / SIDE as f32,
                    ];
                    let want = expected(coords, source, linear);
                    assert!(
                        actual
                            .iter()
                            .zip(want)
                            .all(|(a, b)| (i16::from(*a) - i16::from(b)).abs() <= 2),
                        "source={source:?} linear={linear} at {x},{y}: {actual:?} vs {want:?}"
                    );
                }
            }
        }
    }
    for (vertex, uv) in Arc::make_mut(&mut input.vertices).iter_mut().zip([
        [0.0, 0.0],
        [128_000.0, 0.0],
        [128_000.0, 64_000.0],
        [0.0, 64_000.0],
    ]) {
        vertex.uv = uv;
    }
    Arc::make_mut(&mut input.batches)[0].scissor = UiScissor::new(0, 0, SIDE, SIDE);
    let output = target(&raster.gpu, false);
    raster.draw(&input, &output, None);
    assert!(
        raster
            .pixels(&output)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    assert_eq!(input.vertices.len(), 4);
}

#[test]
fn repeated_original_image_pages_keep_filter_halos_without_overlapping_coverage() {
    let mut input = scene();
    input.vertices = Arc::from([]);
    input.indices = Arc::from([]);
    input.batches = Arc::from([]);
    let mut pixels = PAGE;
    for pixel in pixels.as_chunks_mut::<4>().0.iter_mut() {
        pixel[3] = 128;
    }
    input.textures = Arc::new(
        UiTextureCatalog::new(
            (0..2)
                .map(|column| {
                    UiTexturePage::owned([2, 2], Arc::from(pixels))
                        .unwrap()
                        .with_repeat_content([column, 0, column + 1, 2])
                        .unwrap()
                })
                .collect(),
            2,
        )
        .unwrap(),
    );
    for page in 0..2 {
        quad(
            &mut input,
            [0.0, 0.0, SIDE as f32, SIDE as f32],
            [255; 4],
            page,
            None,
        );
    }
    let Some(raster) = Raster::new(&input) else {
        return;
    };
    for bounds in [[0.0, 0.0, 2.0, 2.0], [-1.0, -1.0, 4.0, 4.0]] {
        for linear in [false, true] {
            for vertices in Arc::make_mut(&mut input.vertices)
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
            {
                for (vertex, uv) in vertices.iter_mut().zip([
                    [0.25, 0.375],
                    [13.5, 0.375],
                    [13.5, 7.875],
                    [0.25, 7.875],
                ]) {
                    vertex.uv = uv;
                    vertex.overlay_color = bounds;
                    vertex.style_flags =
                        u32::from(UI_STYLE_REPEAT | render_api::ui_repeat::UI_STYLE_REPEAT_PAGE)
                            | if linear {
                                u32::from(render_api::UI_STYLE_BILINEAR)
                            } else {
                                0
                            };
                }
            }
            let output = target(&raster.gpu, false);
            raster.draw(&input, &output, None);
            for (at, actual) in raster.pixels(&output).as_chunks::<4>().0.iter().enumerate() {
                let x = at as u32 % SIDE;
                let y = at as u32 / SIDE;
                let coords = [
                    0.25 + (x as f32 + 0.5) * 13.25 / SIDE as f32,
                    0.375 + (y as f32 + 0.5) * 7.5 / SIDE as f32,
                ];
                let want = expected(coords, bounds, linear);
                for channel in 0..3 {
                    let premultiplied = (u32::from(want[channel]) * 128 + 127) / 255;
                    assert!(
                        (i16::from(actual[channel]) - premultiplied as i16).abs() <= 2,
                        "linear={linear} bounds={bounds:?} at {x},{y}: {actual:?} vs {want:?}"
                    );
                }
                assert_eq!(
                    actual[3], 128,
                    "a page halo must not contribute duplicate alpha"
                );
            }
        }
    }
}
