#[test]
fn additive_particles_preserve_destination_coverage() {
    let mut destination = [40, 60, 80, 255];
    super::blend_styled(
        &mut destination,
        [200, 100, 0, 128],
        [255; 4],
        ui::UiBlendMode::Alpha,
        render_model::UI_STYLE_ADDITIVE,
    );
    assert_eq!(destination, [140, 110, 80, 255]);
}

use super::*;
use ui::{UiNode, UiNodeId, UiPoint, UiRect, UiTree, UiVisual};

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> UiRect {
    UiRect::new(UiPoint::new(x0, y0).unwrap(), UiPoint::new(x1, y1).unwrap()).unwrap()
}

// A half-covered translucent solid blends once per pixel, including the diagonal.
#[test]
fn solids_fill_their_pixel_centres_exactly_once() {
    let node = UiNode::new(UiNodeId::new(1), None, rect(1.0, 1.0, 3.0, 3.0)).with_visual(
        UiVisual::Solid {
            texture_page: 0,
            color: [255, 0, 0, 128],
        },
    );
    let list = UiTree::new(vec![node]).unwrap().build_draw_list().unwrap();
    let white = Page::white();
    let mut canvas = Canvas::new(4, 4);
    canvas.draw(&list, |_| Some(&white));
    let rgba = canvas.into_rgba();
    let pixel = |x: usize, y: usize| &rgba[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
    assert_eq!(pixel(0, 0), [0, 0, 0, 0]);
    assert_eq!(pixel(1, 1), [255, 0, 0, 128]);
    assert_eq!(pixel(2, 2), [255, 0, 0, 128]);
    assert_eq!(pixel(3, 3), [0, 0, 0, 0]);
}

/// Rasterizes translucent triangles over a 4x4 canvas and returns each pixel's alpha.
fn translucent_alpha(points: &[[f32; 2]], triangles: &[[u32; 3]]) -> Vec<u8> {
    let mut list = quad();
    let template = UiVertex {
        color: [255, 0, 0, 128],
        ..list.vertices[0]
    };
    list.vertices = points
        .iter()
        .map(|&position| UiVertex {
            position,
            ..template
        })
        .collect();
    list.indices = triangles.concat();
    list.batches[0].index_range = 0..list.indices.len() as u32;
    let white = Page::white();
    let mut canvas = Canvas::new(4, 4);
    canvas.draw(&list, |_| Some(&white));
    canvas
        .into_rgba()
        .iter()
        .skip(3)
        .step_by(4)
        .copied()
        .collect()
}

// Separately drawn triangles sharing an edge through pixel centres blend it once.
#[test]
fn shared_triangle_edges_blend_once() {
    let square = [[0., 0.], [4., 0.], [4., 4.], [0., 4.]];
    assert_eq!(
        translucent_alpha(&square, &[[0, 1, 2], [2, 3, 0]]),
        [128; 16]
    );
    // A rounded-rectangle fan whose corner spokes fall between index groups.
    let fan = [
        [2., 2.],
        [0., 0.],
        [2., 0.],
        [4., 0.],
        [4., 2.],
        [4., 4.],
        [2., 4.],
        [0., 4.],
        [0., 2.],
    ];
    let triangles: Vec<[u32; 3]> = (1..=8).map(|i| [0, i, i % 8 + 1]).collect();
    assert_eq!(translucent_alpha(&fan, &triangles), [128; 16]);
}

/// Creates one production quad so tests can exercise vertex shading independently.
fn quad() -> UiDrawList {
    UiTree::new(vec![
        UiNode::new(UiNodeId::new(1), None, rect(0., 0., 4., 4.)).with_visual(UiVisual::Solid {
            texture_page: 0,
            color: [255; 4],
        }),
    ])
    .unwrap()
    .build_draw_list()
    .unwrap()
}

#[test]
fn particle_mesh_alpha_testing_thresholds_texture_before_node_opacity() {
    let mut list = quad();
    for vertex in &mut list.vertices {
        vertex.style_flags = render_model::UI_STYLE_PARTICLE;
        vertex.alpha_test = true;
        vertex.color[3] = 128;
    }
    for alpha in [127, 128, 192] {
        let texture = Page {
            repeat_content: None,
            width: 1,
            height: 1,
            rgba: [200, 80, 20, alpha].into(),
        };
        for draw in [&list, &triangle(list.clone())] {
            let expected = if alpha < 128 {
                [0; 4]
            } else {
                [199, 80, 20, 128]
            };
            assert_eq!(first_pixel(draw, &texture), expected);
        }
    }
    for vertex in &mut list.vertices {
        vertex.alpha_test = false;
    }
    let texture = Page {
        repeat_content: None,
        width: 1,
        height: 1,
        rgba: [200, 80, 20, 192].into(),
    };
    assert_eq!(first_pixel(&list, &texture)[3], 96);
}

#[test]
fn particle_sprite_columns_keep_equal_width_and_stay_inside_source_bounds() {
    let texture = Page {
        repeat_content: None,
        width: 18,
        height: 1,
        rgba: (0..18).flat_map(|i| [i * 10, 0, 0, 255]).collect(),
    };
    let mut list = quad();
    for vertex in &mut list.vertices {
        vertex.uv = [1.0 + vertex.position[0] * 4.0, 0.5];
        vertex.position[0] *= 8.0;
        vertex.overlay_color = [1.0, 0.0, 16.0, 1.0];
        vertex.style_flags = render_model::UI_STYLE_PARTICLE;
    }
    list.batches[0].clip = rect(0., 0., 32., 4.);
    for draw in [&list, &triangle(list.clone())] {
        let mut canvas = Canvas::new(32, 4);
        canvas.draw(draw, |_| Some(&texture));
        for (x, pixel) in canvas.into_rgba().as_chunks::<4>().0[..32]
            .iter()
            .enumerate()
        {
            if pixel[3] != 0 {
                assert_eq!(*pixel, [((x / 2 + 1) * 10) as u8, 0, 0, 255]);
            }
        }
    }
    for u in [-100.0, 100.0] {
        assert_eq!(
            render_model::ui_particle_uv([u, 0.5], [1., 0., 16., 1.])[0],
            if u < 0.0 { 1.5 } else { 16.5 }
        );
    }
}

#[test]
fn gradients_interpolate_colors_instead_of_taking_the_first_corner() {
    let mut list = quad();
    for vertex in &mut list.vertices {
        vertex.color = if vertex.position[1] == 0. {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
    }
    let white = Page::white();
    let mut canvas = Canvas::new(4, 4);
    canvas.draw(&list, |_| Some(&white));
    let rgba = canvas.into_rgba();
    assert_eq!(&rgba[..4], &[223, 0, 32, 255]);
    assert_eq!(&rgba[48..52], &[32, 0, 223, 255]);
}

#[test]
fn coverage_geometry_mixes_fill_and_border_without_sampling_a_texture() {
    let mut list = quad();
    let (color, overlay) = render_api::UiCoveragePaint {
        fill: [[255, 0, 0, 255]; 2],
        border: [0, 0, 255, 255],
    }
    .vertex_colors();
    for vertex in &mut list.vertices {
        vertex.style_flags = u32::from(render_api::UI_STYLE_COVERAGE_PAINT);
        vertex.color = color;
        vertex.overlay_color = overlay;
        vertex.uv = [0., 0.25];
        vertex.model_light = 0.75;
    }
    let white = Page::white();
    let mut canvas = Canvas::new(4, 4);
    canvas.draw(&list, |_| Some(&white));
    assert_eq!(&canvas.into_rgba()[..4], &[64, 0, 191, 255]);
}

#[test]
fn rounded_shadows_sample_the_analytic_shape_and_respect_the_clip() {
    for blur in [0.0, 0.75] {
        let mut list = quad();
        for vertex in &mut list.vertices {
            vertex.style_flags = u32::from(render_api::ui_shadow::UI_STYLE_SHADOW);
            vertex.uv = vertex.position.map(|axis| axis - 2.0);
            vertex.overlay_color = [1.0, 1.0, 0.5, blur];
            vertex.color = [200, 80, 20, 128];
        }
        list.batches[0].clip = rect(1.0, 0.0, 4.0, 4.0);
        for draw in [&list, &triangle(list.clone())] {
            let white = Page::white();
            let mut canvas = Canvas::new(4, 4);
            canvas.draw(draw, |_| Some(&white));
            let rgba = canvas.into_rgba();
            for y in 0..4 {
                for x in 0..4 {
                    let pixel = &rgba[(y * 4 + x) * 4..(y * 4 + x + 1) * 4];
                    let coverage = render_api::ui_shadow::coverage(
                        [x as f32 - 1.5, y as f32 - 1.5],
                        [1.0, 1.0, 0.5, blur],
                        1.0,
                    );
                    let alpha = if x == 0 || (draw.indices.len() == 3 && y > x) {
                        0
                    } else {
                        (coverage * 128.0).round() as u8
                    };
                    assert!(
                        (i16::from(pixel[3]) - i16::from(alpha)).abs() <= 1,
                        "{pixel:?} {alpha}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_single_triangle_is_not_dropped() {
    let mut list = quad();
    list.indices.truncate(3);
    list.batches[0].index_range = 0..3;
    let white = Page::white();
    let mut canvas = Canvas::new(4, 4);
    canvas.draw(&list, |_| Some(&white));
    assert!(
        canvas
            .into_rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != 0)
    );
}

/// Draws the first pixel of a production quad or its first triangle with the given texture.
fn first_pixel(list: &UiDrawList, texture: &Page) -> [u8; 4] {
    let mut canvas = Canvas::new(4, 4);
    canvas.draw(list, |_| Some(texture));
    canvas.into_rgba()[..4].try_into().unwrap()
}

/// Keeps only one triangle so the general path shades the same covered pixel as the quad.
fn triangle(mut list: UiDrawList) -> UiDrawList {
    list.indices.truncate(3);
    list.batches[0].index_range = 0..3;
    list
}

#[test]
fn bilinear_sampling_uses_texel_centres_and_clamps_page_edges() {
    let texture = Page {
        repeat_content: None,
        width: 2,
        height: 2,
        rgba: [[0, 0, 0, 255], [255; 4], [255; 4], [0, 0, 0, 255]]
            .into_iter()
            .flatten()
            .collect(),
    };
    let mut list = quad();
    for uv in [[0.5, 0.5], [1.0, 1.0], [-1.0, -1.0], [3.0, 3.0]] {
        for vertex in &mut list.vertices {
            vertex.uv = uv;
            vertex.style_flags = u32::from(ui::UI_STYLE_BILINEAR);
        }
        let expected = if uv == [1.0, 1.0] {
            [128, 128, 128, 255]
        } else {
            [0, 0, 0, 255]
        };
        assert_eq!(first_pixel(&list, &texture), expected);
        assert_eq!(first_pixel(&triangle(list.clone()), &texture), expected);
    }
}

#[test]
fn repeated_texture_filters_inside_its_atlas_region_on_both_raster_paths() {
    let mut pixels = [255, 0, 0, 255].repeat(16);
    for (at, value) in [
        (5, [0, 0, 0, 255]),
        (6, [255; 4]),
        (9, [255; 4]),
        (10, [0, 0, 0, 255]),
    ] {
        pixels[at * 4..at * 4 + 4].copy_from_slice(&value);
    }
    let texture = Page {
        repeat_content: None,
        width: 4,
        height: 4,
        rgba: pixels.into(),
    };
    for (uv, linear, outline) in [
        ([1.25, 1.25], false, 0),
        ([1.0, 1.0], true, 0),
        ([1.25, 1.25], false, assets::FONT_STYLE_OUTLINE_COVERAGE),
        ([1.0, 1.0], true, assets::FONT_STYLE_OUTLINE_COVERAGE),
    ] {
        let expected = [0, 0, 0, 255];
        let mut list = quad();
        for vertex in &mut list.vertices {
            vertex.uv = uv;
            vertex.overlay_color = [1.0, 1.0, 2.0, 2.0];
            vertex.style_flags = u32::from(render_api::ui_repeat::UI_STYLE_REPEAT)
                | u32::from(outline)
                | if linear {
                    u32::from(ui::UI_STYLE_BILINEAR)
                } else {
                    0
                };
        }
        assert_eq!(first_pixel(&list, &texture), expected);
        assert_eq!(first_pixel(&triangle(list), &texture), expected);
    }
}

#[test]
fn minified_outline_strokes_keep_their_area_across_fractional_phases() {
    let texture = Page {
        repeat_content: None,
        width: 32,
        height: 32,
        rgba: (0..1024)
            .flat_map(|index| [255, 255, 255, if index % 4 == 0 { 255 } else { 0 }])
            .collect(),
    };
    for phase in [0., 0.25, 0.5, 0.75] {
        let mut list = quad();
        for vertex in &mut list.vertices {
            vertex.uv = vertex.position.map(|position| position * 4. + 4. + phase);
            vertex.style_flags = u32::from(assets::FontRendering::OutlineCoverage.style_flags());
            vertex.style_flags |= u32::from(ui::UI_STYLE_BILINEAR);
        }
        for draw in [&list, &triangle(list.clone())] {
            let pixel = first_pixel(draw, &texture);
            assert!(
                pixel[3].abs_diff(64) <= 1,
                "one-texel strokes occupy one quarter of each pixel at phase {phase}: {pixel:?}"
            );
            assert_eq!(pixel[..3], [255; 3]);
        }
    }
}

#[test]
fn repeated_original_image_pages_preserve_translucent_coverage() {
    let first = Page {
        width: 2,
        height: 2,
        rgba: [80, 160, 240, 128].repeat(4).into(),
        repeat_content: Some([0, 0, 1, 2]),
    };
    let second = Page {
        repeat_content: Some([1, 0, 2, 2]),
        ..first.clone()
    };
    for outline in [0, assets::FONT_STYLE_OUTLINE_COVERAGE] {
        let mut list = quad();
        for vertex in &mut list.vertices {
            vertex.uv = vertex.uv.map(|v| v * 13.25);
            vertex.overlay_color = [0.0, 0.0, 2.0, 2.0];
            vertex.style_flags = u32::from(render_api::ui_repeat::UI_STYLE_REPEAT)
                | u32::from(render_api::ui_repeat::UI_STYLE_REPEAT_PAGE)
                | u32::from(ui::UI_STYLE_BILINEAR)
                | u32::from(outline);
        }
        let mut other = list.batches[0].clone();
        other.texture_page = 1;
        list.batches.push(other);
        let mut canvas = Canvas::new(4, 4);
        canvas.draw(&list, |page| Some(if page == 0 { &first } else { &second }));
        assert!(
            canvas
                .into_rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 128)
        );
    }
}

#[test]
fn native_coverage_keeps_nearest_sampling_and_uses_text_colour_gamma() {
    let texture = Page {
        repeat_content: None,
        width: 2,
        height: 1,
        rgba: [0, 0, 0, 128, 0, 0, 0, 0].into(),
    };
    let mut list = quad();
    for (color, alpha) in [([255; 4], 187u8), ([0, 0, 0, 255], 94)] {
        for vertex in &mut list.vertices {
            vertex.uv = [0.9, 0.5];
            vertex.color = color;
            vertex.style_flags =
                u32::from(assets::FONT_STYLE_COVERAGE_GAMMA | ui::UI_STYLE_BILINEAR);
        }
        for draw in [&list, &triangle(list.clone())] {
            let pixel = first_pixel(draw, &texture);
            assert!(pixel[3].abs_diff(alpha) <= 1, "{pixel:?}");
            assert_eq!(&pixel[..3], &color[..3]);
        }
    }
}

#[test]
fn sdf_samples_are_filtered_before_coverage_and_gamma_conversion() {
    let texture = Page {
        repeat_content: None,
        width: 2,
        height: 1,
        rgba: [0, 0, 0, 0, 0, 0, 0, 255].into(),
    };
    let mut list = quad();
    for vertex in &mut list.vertices {
        vertex.uv = [1.0, 0.5];
        // SDF selects linear sampling even without the sprite bilinear flag.
        vertex.style_flags = u32::from(assets::FONT_STYLE_SDF | assets::FONT_STYLE_COVERAGE_GAMMA);
    }
    for draw in [&list, &triangle(list.clone())] {
        let pixel = first_pixel(draw, &texture);
        assert_eq!(&pixel[..3], &[255; 3]);
        assert!(pixel[3].abs_diff(187) <= 1, "{pixel:?}");
    }
}

#[test]
fn sdf_smoothing_tracks_the_uv_derivative_area_on_both_paths() {
    let texture = Page {
        repeat_content: None,
        width: 1,
        height: 1,
        rgba: [0, 0, 0, 132].into(),
    };
    for scale in [0.5, 1.0, 2.0, 4.0] {
        let mut list = quad();
        for vertex in &mut list.vertices {
            vertex.uv = vertex.position.map(|coordinate| coordinate * scale);
            vertex.style_flags =
                u32::from(assets::FONT_STYLE_SDF | assets::FONT_STYLE_COVERAGE_GAMMA);
        }
        let coverage = render_model::font_shading::sdf_coverage(132.0 / 255.0, scale);
        let coverage = render_model::font_shading::coverage_gamma(coverage, [1.0; 3]);
        let expected = (coverage * 255.0).round() as u8;
        let rect_pixel = first_pixel(&list, &texture);
        assert_eq!(rect_pixel, [255, 255, 255, expected]);
        // Shearing the UVs preserves their determinant but rules out the rectangle path.
        for vertex in &mut list.vertices {
            vertex.uv[0] += vertex.uv[1];
        }
        assert_eq!(first_pixel(&list, &texture), rect_pixel);
        assert_eq!(first_pixel(&triangle(list), &texture), rect_pixel);
    }
}
