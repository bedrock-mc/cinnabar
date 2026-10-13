use super::*;

/// Makes a small rounded shape in logical pixels for coverage checks.
fn shape(width: f32, height: f32, radius: f32, border: f32, fringe: f32) -> Shape {
    Shape {
        width,
        height,
        radius,
        border,
        fringe,
    }
}

/// Uses physical-size-independent tint for a checked shape mesh.
fn key(width: f32, height: f32, radius: f32, border: f32, fringe: f32) -> Key {
    Key {
        width: width.to_bits(),
        height: height.to_bits(),
        radius: radius.to_bits(),
        fringe: fringe.to_bits(),
        border: border.to_bits(),
        page: 0,
        color: [0, 0, 255, 64],
        end_color: [0, 0, 255, 64],
        horizontal: false,
        border_color: [255; 4],
    }
}

/// Samples interpolated premultiplied tint using the mesh's color interpolation mode.
fn sample_color(mesh: &UiMesh, size: [f32; 2], point: [f32; 2]) -> [f32; 4] {
    let edge = |from: [f32; 2], to: [f32; 2], point: [f32; 2]| {
        (to[0] - from[0]) * (point[1] - from[1]) - (to[1] - from[1]) * (point[0] - from[0])
    };
    for triangle in mesh.indices().as_chunks::<3>().0 {
        let corners = triangle.map(|index| mesh.vertices()[index as usize]);
        let [a, b, c] =
            corners.map(|vertex| std::array::from_fn(|axis| vertex.position[axis] * size[axis]));
        let area = edge(a, b, c);
        if area.abs() < 0.000001 {
            continue;
        }
        let weights = [
            edge(b, c, point) / area,
            edge(c, a, point) / area,
            edge(a, b, point) / area,
        ];
        if weights.iter().all(|weight| *weight >= -0.00001) {
            let interpolate = |values: [f32; 3]| {
                values
                    .into_iter()
                    .zip(weights)
                    .map(|(value, weight)| value * weight)
                    .sum::<f32>()
            };
            if corners[0].style_flags & render_api::UI_STYLE_COVERAGE_PAINT != 0 {
                let paint = render_api::UiCoveragePaint::from_vertex_colors(
                    corners[0].color,
                    corners[0].overlay_color,
                );
                return paint.sample(
                    interpolate(corners.map(|vertex| vertex.uv[0])),
                    [
                        interpolate(corners.map(|vertex| vertex.uv[1])),
                        interpolate(corners.map(|vertex| vertex.model_light)),
                    ],
                );
            }
            let mut color: [f32; 4] = std::array::from_fn(|channel| {
                interpolate(corners.map(|vertex| f32::from(vertex.color[channel]) / 255.0))
            });
            let alpha = color[3];
            for value in &mut color[..3] {
                *value *= alpha;
            }
            return color;
        }
    }
    [0.0; 4]
}

/// Returns byte alpha from the same interpolated tint used by color checks.
fn sample_alpha(mesh: &UiMesh, size: [f32; 2], point: [f32; 2]) -> u8 {
    (sample_color(mesh, size, point)[3] * 255.0).round() as u8
}

/// Transparent blue contributes no RGB between the red inner border rings, including during fades.
#[test]
fn opaque_border_over_transparent_fill_never_interpolates_purple() {
    for opacity in [1.0, 0.5] {
        let mesh = mesh(Key {
            color: [0, 0, 255, 0],
            end_color: [0, 0, 255, 0],
            border_color: [255, 0, 0, 255],
            ..key(40.0, 20.0, 6.0, 2.125, 1.0)
        })
        .unwrap()
        .with_opacity(opacity);
        for y in [2.0, 2.25, 2.5, 2.6] {
            let color = sample_color(&mesh, [40.0, 20.0], [20.0, y]);
            assert!(color[3] > 0.0 && color[3] < opacity);
            let expected = (2.625 - y) * opacity;
            assert!((color[3] - expected).abs() <= 1.0 / 255.0, "{color:?}");
            assert!((color[0] - color[3]).abs() < 0.00001, "{color:?}");
            assert_eq!(color[1], 0.0);
            assert_eq!(color[2], 0.0, "invisible fill RGB must not tint the border");
        }
    }
}

/// A square inner corner cannot spread its two-axis coverage across a curved ring's straight edge.
#[test]
fn straight_border_coverage_survives_a_square_inner_outline() {
    for dpi in [1.0, 1.5, 2.0, 3.0] {
        let size = [40.0 / dpi, 20.0 / dpi];
        for border in [1.75, 1.875] {
            let mesh = mesh(Key {
                color: [0; 4],
                end_color: [0; 4],
                border_color: [255, 0, 0, 255],
                ..key(
                    size[0],
                    size[1],
                    1.75 / dpi,
                    border / dpi,
                    coverage_width(dpi),
                )
            })
            .unwrap();
            for distance in [1.5, 1.75, 2.0] {
                let expected = (border - distance + 0.5_f32).clamp(0.0, 1.0);
                for point in [
                    [4.5, distance],
                    [20.5, distance],
                    [35.5, distance],
                    [4.5, 20.0 - distance],
                    [20.5, 20.0 - distance],
                    [35.5, 20.0 - distance],
                    [distance, 4.5],
                    [distance, 10.5],
                    [distance, 15.5],
                    [40.0 - distance, 4.5],
                    [40.0 - distance, 10.5],
                    [40.0 - distance, 15.5],
                ] {
                    let color = sample_color(&mesh, size, point.map(|value| value / dpi));
                    for (actual, expected) in color.into_iter().zip([expected, 0.0, 0.0, expected])
                    {
                        assert!(
                            (actual - expected).abs() < 0.00001,
                            "at {point:?}, DPI {dpi}, border {border}: {color:?}"
                        );
                    }
                }
            }
        }
    }
}

/// Raster samples at fractional outlines retain partial coverage on squares, curves and hairlines.
#[test]
fn raster_samples_have_coverage_on_both_sides_of_authored_edges() {
    let white = Key {
        color: [255; 4],
        end_color: [255; 4],
        ..key(10.0, 6.0, 0.0, 0.0, 1.0)
    };
    let square = mesh(white).unwrap();
    assert!((142..=144).contains(&sample_alpha(&square, [10.0, 6.0], [0.25, 0.25])));
    assert_eq!(sample_alpha(&square, [10.0, 6.0], [0.5, 0.5]), 255);
    let curve = mesh(Key {
        width: 20_f32.to_bits(),
        height: 20_f32.to_bits(),
        radius: 10_f32.to_bits(),
        ..white
    })
    .unwrap();
    let alpha = sample_alpha(&curve, [20.0; 2], [2.8, 2.8]);
    assert!(
        alpha > 0 && alpha < 160,
        "outside curved sample should have partial coverage: {alpha}"
    );
    let line = mesh(Key {
        color: [0; 4],
        end_color: [0; 4],
        ..key(20.0, 12.0, 4.0, 0.25, 1.0)
    })
    .unwrap();
    for y in [0.0, 0.5] {
        assert!((63..=65).contains(&sample_alpha(&line, [20.0, 12.0], [10.0, y])));
    }
    assert_eq!(sample_alpha(&line, [20.0, 12.0], [10.0, 1.0]), 0);
}

/// Device density changes the logical band, preserving one physical pixel of edge coverage.
#[test]
fn coverage_band_is_one_physical_pixel_at_each_dpi() {
    for dpi in [1.0, 1.5, 2.0, 3.0] {
        assert!((coverage_width(dpi) * dpi - 1.0).abs() < 0.00001);
    }
}

/// A square has distinct real inset/outset contours instead of a collapsed fringe at each corner.
#[test]
fn square_edges_and_tiny_rectangles_have_visible_coverage_rings() {
    let square = shape(10.0, 6.0, 0.0, 0.0, 1.0);
    assert_eq!(square.point(0.5, 0, [-1.0, 0.0], 0.5), [0.5, 0.5]);
    assert_eq!(square.point(0.5, 2, [1.0, 0.0], 0.5), [9.5, 5.5]);
    assert_eq!(square.point(-0.5, 0, [-1.0, 0.0], 0.5), [-0.5, -0.5]);
    assert_eq!(square.point(0.0, 0, [-1.0, 0.0], 0.0), [0.0, 1.0]);
    assert_eq!(square.point(0.0, 0, [0.0, -1.0], 1.0), [1.0, 0.0]);
    assert_eq!(square.coverage(0.0), [0.5, 0.0]);
    assert_eq!(square.coverage_at([0.0, 0.0], 0.0), [0.25, 0.0]);
    assert_eq!(square.coverage_at([0.0, 1.0], 0.0), [0.5, 0.0]);
    assert_eq!(square.coverage_at([0.25, 0.25], 0.25), [0.5625, 0.0]);
    let tiny = shape(10.0, 0.25, 0.0, 0.0, 1.0);
    assert_eq!(tiny.coverage(0.125), [0.25, 0.0]);
    let mesh = mesh(Key {
        color: [255; 4],
        end_color: [255; 4],
        ..key(10.0, 0.25, 0.0, 0.0, 1.0)
    })
    .unwrap();
    assert_eq!(sample_alpha(&mesh, [10.0, 0.25], [5.0, 0.125]), 64);
    assert!(
        mesh.vertices()
            .iter()
            .any(|vertex| vertex.position[1] < 0.0)
    );
    assert!(mesh.vertices().iter().any(|vertex| {
        render_api::UiCoveragePaint::from_vertex_colors(vertex.color, vertex.overlay_color)
            .sample(vertex.uv[0], [vertex.uv[1], vertex.model_light])[3]
            == 0.0
    }));
}

/// A subpixel strip retains its exact pixel area throughout the interior of either long axis.
#[test]
fn thin_square_strips_keep_full_pixel_area_along_each_axis() {
    for dpi in [1.0, 1.5, 2.0, 3.0] {
        for width in [0.0625, 0.125, 0.25, 0.5] {
            for horizontal in [false, true] {
                let physical_size = if horizontal {
                    [10.0, width]
                } else {
                    [width, 10.0]
                };
                let size = physical_size.map(|extent| extent / dpi);
                let mesh = mesh(Key {
                    color: [255; 4],
                    end_color: [255; 4],
                    ..key(size[0], size[1], 0.0, 0.0, coverage_width(dpi))
                })
                .unwrap();
                for along in [1.5, 3.0, 5.0, 7.0, 8.5] {
                    for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                        let point = if horizontal {
                            [along, width * fraction]
                        } else {
                            [width * fraction, along]
                        };
                        let color = sample_color(&mesh, size, point.map(|value| value / dpi));
                        for actual in color {
                            assert!(
                                (actual - width).abs() < 0.00001,
                                "at {point:?}, DPI {dpi}, horizontal {horizontal}, width {width}: {color:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Subpixel borders distribute their authored width across the full coverage band.
#[test]
fn a_hairline_has_independent_outer_and_inner_antialiasing() {
    let shape = shape(20.0, 12.0, 4.0, 0.25, 1.0);
    assert_eq!(shape.coverage(-0.25), [0.0, 0.25]);
    assert_eq!(shape.coverage(0.0), [0.25, 0.25]);
    assert_eq!(shape.coverage(0.25), [0.5, 0.25]);
    assert_eq!(shape.coverage(0.5), [0.75, 0.25]);
    assert_eq!(shape.coverage(0.75), [1.0, 0.0]);
    for sample in -4..=8 {
        assert!(shape.coverage(sample as f32 * 0.125)[1] <= 0.25);
    }
    let rings = shape.rings();
    assert_eq!(*rings.first().unwrap(), -0.5);
    assert_eq!(*rings.last().unwrap(), 0.75);
}

/// One combined seam uses area-weighted alpha rather than painting a border over a fill.
#[test]
fn translucent_border_and_fill_do_not_double_blend_at_the_seam() {
    let paint = render_api::UiCoveragePaint {
        fill: [[0, 0, 255, 64]; 2],
        border: [255; 4],
    };
    assert_eq!(
        paint.sample(0.0, [0.25, 0.25]),
        [0.25, 0.25, 0.25 + 16.0 / 255.0, 0.25 + 16.0 / 255.0]
    );
    let key = key(40.0, 20.0, 6.0, 2.0, 1.0);
    let shape = shape(40.0, 20.0, 6.0, 2.0, 1.0);
    assert_eq!(shape.coverage(2.0), [0.5, 0.5]);
    let mesh = mesh(key).unwrap();
    assert_eq!(sample_alpha(&mesh, [40.0, 20.0], [20.0, 2.0]), 160);
    assert_eq!(sample_alpha(&mesh, [40.0, 20.0], [20.0, 10.0]), 64);
}

/// Larger display radii receive more arc samples while work remains bounded.
#[test]
fn corner_tessellation_tracks_physical_radius() {
    let small = shape(40.0, 20.0, 6.0, 0.0, 1.0).corner_steps();
    let large = shape(400.0, 200.0, 60.0, 0.0, 1.0).corner_steps();
    let dense = shape(400.0, 200.0, 60.0, 0.0, 0.5).corner_steps();
    assert!(large > small && dense > large);
    for radius in [0.0, 1.0, 6.0, 16.0, 60.0, 120.0] {
        let shape = shape(400.0, 300.0, radius, 0.0, 1.0);
        let angle = std::f32::consts::FRAC_PI_2 / shape.corner_steps() as f32;
        let error = (radius + 0.5) * (1.0 - (angle * 0.5).cos());
        assert!(error <= CHORD_ERROR_PIXELS + 0.00001);
    }
    assert_eq!(
        shape(1_000_000.0, 1_000_000.0, 500_000.0, 0.0, 1.0).corner_steps(),
        MAX_CORNER_STEPS
    );
}

/// Borders that consume the shape never create a phantom fill at a collapsed inner contour.
#[test]
fn full_width_border_and_gradients_preserve_authored_colors() {
    let shape = shape(10.0, 6.0, 3.0, 3.0, 1.0);
    assert_eq!(shape.coverage(3.0), [0.0, 1.0]);
    let border = mesh(key(10.0, 6.0, 3.0, 3.0, 1.0)).unwrap();
    assert_eq!(sample_color(&border, [10.0, 6.0], [5.0, 3.0]), [1.0; 4]);
    let mut key = key(40.0, 20.0, 6.0, 0.0, 1.0);
    key.color = [10, 20, 30, 64];
    key.end_color = [110, 120, 130, 128];
    let mesh = mesh(key).unwrap();
    let center = sample_color(&mesh, [40.0, 20.0], [20.0, 10.0]);
    for (actual, expected) in center.into_iter().zip([
        60.0 / 255.0 * 96.0 / 255.0,
        70.0 / 255.0 * 96.0 / 255.0,
        80.0 / 255.0 * 96.0 / 255.0,
        96.0 / 255.0,
    ]) {
        assert!((actual - expected).abs() < 0.00001);
    }
}

/// Fill RGBA depends only on the authored gradient axis, including when the alpha varies.
#[test]
fn fill_gradient_is_independent_of_triangle_topology() {
    for border in [0.0, 2.125] {
        for dpi in [1.0, 1.5, 2.0] {
            for horizontal in [false, true] {
                let key = Key {
                    color: [255, 0, 0, 255],
                    end_color: [0, 0, 255, 0],
                    horizontal,
                    ..key(40.0, 20.0, 6.0, border, coverage_width(dpi))
                };
                let mesh = mesh(key).unwrap();
                let points = if horizontal {
                    [[30.0, 5.0], [30.0, 10.0], [30.0, 15.0]]
                } else {
                    [[5.0, 15.0], [20.0, 15.0], [35.0, 15.0]]
                };
                for point in points {
                    let color = sample_color(&mesh, [40.0, 20.0], point);
                    let expected = [0.0625, 0.0, 0.1875, 0.25];
                    for (actual, expected) in color.into_iter().zip(expected) {
                        assert!(
                            (actual - expected).abs() < 0.00001,
                            "at {point:?}: {color:?}"
                        );
                    }
                }
            }
        }
    }
}
