use super::*;
use super::{coverage::Coverage, raster::Domain};
use serde_json::json;

/// Constructs an authored fixture without depending on any product's artwork.
fn key(paths: Value, stroke: Option<f32>, size: f32, dpi: f32) -> Key {
    let mut data: BTreeMap<String, Value> = serde_json::from_value(json!({
        "view_box": [0, 0, 16, 16], "paths": paths,
    }))
    .unwrap();
    if let Some(width) = stroke {
        data.insert("stroke_width".into(), json!(width));
    }
    Key {
        path: parse(&data).unwrap(),
        size: [size.to_bits(); 2],
        dpi: dpi.to_bits(),
        page: 0,
        color: [255, 255, 255, 128],
    }
}

/// Culling uses painted coverage rather than layout boxes, and does no raster allocation.
#[test]
fn clipped_paths_skip_work_without_losing_caps_outside_art_or_transformed_edges() {
    let path = key(json!([[[0, 0], [16, 0]]]), Some(2.), 16., 1.);
    let dest = [0., 0., 16., 16.];
    let (draws, allocations) =
        test_alloc::count(|| visible(&path, dest, [0., 32., 16., 48.], None));
    assert!(!draws);
    assert_eq!(allocations, 0);
    assert!(
        visible(&path, dest, [16.5, -1., 18., 1.], None),
        "round cap remains visible"
    );
    let moved = json_ui::transform::Affine {
        translation: [0., 40.],
        ..json_ui::transform::Affine::IDENTITY
    };
    assert!(visible(&path, dest, [0., 39., 16., 41.], Some(moved)));
    let outside = key(json!([[[0, 32], [16, 32]]]), Some(2.), 16., 1.);
    assert!(
        visible(&outside, dest, [0., 31., 16., 33.], None),
        "outside-view-box artwork remains visible"
    );
}

#[test]
fn diagonal_strokes_have_continuous_round_coverage_without_overdraw() {
    let key = key(
        json!([[[2, 2], [8, 8], [14, 2]], [[8, 8], [8, 14]]]),
        Some(2.0),
        32.0,
        1.0,
    );
    let coverage = Coverage::new(&key).unwrap();
    for step in 0..=96 {
        let coordinate = 4.0 + step as f32 / 96.0 * 12.0;
        assert_eq!(coverage.at([coordinate; 2]), 1.0);
    }
    assert_eq!(
        coverage.at([16.0; 2]),
        1.0,
        "rounded joined strokes are covered once"
    );
    let mesh = mesh(&key).unwrap();
    assert!(mesh.vertices().iter().all(|vertex| vertex.color[3] <= 128));
    assert!(
        mesh.vertices()
            .iter()
            .any(|vertex| vertex.color[3] > 0 && vertex.color[3] < 128)
    );
}

#[test]
fn filled_contours_leave_holes_transparent_and_center_each_edge_band() {
    let key = key(
        json!([
            [[0, 0], [16, 0], [16, 16], [0, 16]],
            [[4, 4], [12, 4], [12, 12], [4, 12]],
        ]),
        None,
        32.0,
        1.0,
    );
    let coverage = Coverage::new(&key).unwrap();
    assert_eq!(coverage.at([16.0; 2]), 0.0);
    assert_eq!(coverage.at([4.0; 2]), 1.0);
    for (x, expected) in [
        (-0.5, 0.0),
        (-0.25, 0.25),
        (0.0, 0.5),
        (0.25, 0.75),
        (0.5, 1.0),
    ] {
        assert!((coverage.at([x, 16.0]) - expected).abs() < 1e-6);
    }
    assert_eq!(
        coverage.at([8.0, 16.0]),
        0.5,
        "inner and outer silhouettes both soften"
    );
}

/// A shared boundary is interior even when one rectangle subdivides that edge.
#[test]
fn adjacent_filled_rectangles_have_no_shared_edge_seam() {
    for shared in [
        json!([[8, 16], [8, 0]]),
        json!([[8, 16], [8, 11], [8, 5], [8, 0]]),
    ] {
        let mut right = vec![json!([8, 0]), json!([16, 0]), json!([16, 16])];
        right.extend(shared.as_array().unwrap().iter().cloned());
        let path = key(
            json!([[[0, 0], [8, 0], [8, 16], [0, 16]], right]),
            None,
            16.0,
            1.0,
        );
        let coverage = Coverage::new(&path).unwrap();
        for y in [1.0, 5.0, 8.0, 11.0, 15.0] {
            for x in [7.75, 8.0, 8.25] {
                assert_eq!(coverage.at([x, y]), 1.0, "interior at ({x}, {y})");
            }
        }
        assert_eq!(coverage.at([8.0, 0.0]), 0.5);
    }
}

/// Different collinear sampling and winding cannot leave the cancelled silhouette visible.
#[test]
fn differently_subdivided_filled_contours_leave_no_ghost_outline() {
    let path = key(
        json!([
            [[0, 0], [16, 0], [16, 16], [0, 16]],
            [
                [0, 0],
                [0, 8],
                [0, 16],
                [8, 16],
                [16, 16],
                [16, 8],
                [16, 0],
                [8, 0]
            ],
        ]),
        None,
        16.0,
        1.0,
    );
    let coverage = Coverage::new(&path).unwrap();
    for point in [[0.0, 8.0], [8.0, 0.0], [16.0, 8.0], [8.0, 16.0], [8.0; 2]] {
        assert_eq!(coverage.at(point), 0.0);
    }
    assert!(mesh(&path).is_none());
}

/// Partially shared edges cancel only along their overlap; the remaining outline still softens.
#[test]
fn partially_shared_filled_edges_keep_the_exposed_boundary() {
    let path = key(
        json!([
            [[0, 0], [8, 0], [8, 16], [0, 16]],
            [[8, 4], [16, 4], [16, 12], [8, 12]],
        ]),
        None,
        16.0,
        1.0,
    );
    let coverage = Coverage::new(&path).unwrap();
    assert_eq!(coverage.at([8.0, 8.0]), 1.0);
    assert_eq!(coverage.at([8.0, 2.0]), 0.5);
    assert_eq!(coverage.at([8.25, 2.0]), 0.25);
    assert_eq!(coverage.at([8.0, 14.0]), 0.5);
}

#[test]
fn coverage_width_stays_one_physical_pixel_at_fractional_dpi() {
    for dpi in [1.0, 1.25, 1.5, 2.0] {
        let key = key(
            json!([[[0, 0], [16, 0], [16, 16], [0, 16]]]),
            None,
            32.0,
            dpi,
        );
        let coverage = Coverage::new(&key).unwrap();
        assert!((coverage.at([-0.25 / dpi, 16.0]) - 0.25).abs() < 1e-6);
        assert!((coverage.at([0.25 / dpi, 16.0]) - 0.75).abs() < 1e-6);
    }
}

/// Both boundaries limit the peak alpha, including horizontal details and corners at fractional DPI.
#[test]
fn thin_filled_contours_have_width_limited_pixel_coverage() {
    for dpi in [1.0, 1.5, 2.0] {
        for width in [0.5, 0.25, 0.0625, 0.001] {
            let logical_width = width / dpi;
            for horizontal in [false, true] {
                let points = [
                    [8.0, 0.0],
                    [8.0 + logical_width, 0.0],
                    [8.0 + logical_width, 16.0],
                    [8.0, 16.0],
                ];
                let points = points.map(|point| {
                    if horizontal {
                        [point[1], point[0]]
                    } else {
                        point
                    }
                });
                let mut path = key(json!([points]), None, 16.0, dpi);
                path.color = [255; 4];
                let coverage = Coverage::new(&path).unwrap();
                let point = |along: f32| {
                    let narrow = 8.0 + logical_width * 0.5;
                    if horizontal {
                        [along, narrow]
                    } else {
                        [narrow, along]
                    }
                };
                assert!((coverage.at(point(8.0)) - width).abs() < 0.00001);
                assert!((coverage.at(point(0.0)) - width * 0.5).abs() < 0.00001);
                let mesh = mesh(&path);
                if width * 255.0 < 0.5 {
                    assert!(
                        mesh.is_none(),
                        "sub-byte coverage disappears after alpha quantization"
                    );
                    continue;
                }
                let mesh = mesh.unwrap();
                let peak = mesh
                    .vertices()
                    .iter()
                    .map(|vertex| vertex.color[3])
                    .max()
                    .unwrap();
                assert!(peak > 0);
                assert!(f32::from(peak) <= (width * 255.0).round() + 1.0);
            }
        }
    }
}

/// Pixel coverage combines both sides of a narrow hole without filling its geometric interior.
#[test]
fn thin_holes_preserve_the_two_visible_pixel_portions() {
    let path = key(
        json!([
            [[0, 0], [16, 0], [16, 16], [0, 16]],
            [[8, 2], [8.25, 2], [8.25, 14], [8, 14]],
        ]),
        None,
        16.0,
        1.0,
    );
    let coverage = Coverage::new(&path).unwrap();
    assert_eq!(coverage.at([8.125, 8.0]), 0.75);
    assert_eq!(coverage.at([7.5, 8.0]), 1.0);
}

/// Crossing contours keep even-odd area rather than depending on boundary ordering within a pixel.
#[test]
fn self_intersections_split_pixel_coverage_into_disjoint_areas() {
    let path = key(
        json!([[[0, 0], [16, 16], [0, 16], [16, 0]]]),
        None,
        16.0,
        1.0,
    );
    let coverage = Coverage::new(&path).unwrap();
    assert_eq!(coverage.at([8.0; 2]), 0.5);
    assert_eq!(coverage.at([8.0, 4.0]), 1.0);
    assert_eq!(coverage.at([4.0, 8.0]), 0.0);
}

#[test]
fn malformed_or_unbounded_authored_paths_are_rejected() {
    let base = json!({"view_box": [0, 0, 16, 16], "paths": [[[0, 0], [16, 0], [16, 16]]]});
    for (field, value) in [
        ("view_box", json!([0, 0, 0, 16])),
        ("paths", json!([])),
        ("paths", json!([[[0, 0], [1, 1]]])),
        ("paths", json!([[[0, 0], [1, 1], [1e100, 0]]])),
        ("paths", json!([vec![[0, 0]; MAX_POINTS + 1]])),
        ("paths", json!(vec![vec![[0, 0]; 3]; MAX_CONTOURS + 1])),
        ("stroke_width", json!(0)),
        ("closed", json!("false")),
        ("closed", json!(false)),
    ] {
        let mut value_base = base.clone();
        value_base[field] = value;
        let data = serde_json::from_value(value_base).unwrap();
        assert!(parse(&data).is_none(), "rejected {field}");
    }
}

#[test]
fn hot_path_meshes_remain_retained_when_cold_entries_expire() {
    let mut cache = Cache::default();
    let path = json!([[[2, 2], [14, 14]]]);
    let first = cache
        .get_or_create(key(path.clone(), Some(2.0), 16.0, 1.0))
        .unwrap();
    for revision in 0..MAX_CACHED_PATHS {
        let mut cold = key(path.clone(), Some(2.0), 16.0, 1.0);
        cold.color[0] = revision as u8;
        cache.get_or_create(cold).unwrap();
        assert!(Arc::ptr_eq(
            &cache
                .get_or_create(key(path.clone(), Some(2.0), 16.0, 1.0))
                .unwrap(),
            &first
        ));
    }
    assert!(cache.bytes <= MAX_CACHED_BYTES);
    assert!(cache.entries.len() <= MAX_CACHED_PATHS);
}

/// Slanted stripes vary coverage across each row and exceed the index budget after packing.
fn striped_key() -> Key {
    let stripes: Vec<_> = (0..16)
        .map(|index| {
            let y = index as f32;
            [[0.0, y], [16.0, y + 0.25], [16.0, y + 0.75], [0.0, y + 0.5]]
        })
        .collect();
    key(json!(stripes), None, 80.0, 1.0)
}

/// The next paint reuses an expanded-index rejection without allocating another raster grid.
#[test]
fn rejected_path_is_not_rasterized_on_the_next_paint() {
    let mut cache = Cache::default();
    let first = striped_key();
    assert!(Domain::new(&Coverage::new(&first).unwrap()).is_some());
    let next = striped_key();
    let (rejected, cold_allocations) = test_alloc::count(|| cache.get_or_create(first));
    assert!(rejected.is_none());
    assert!(cold_allocations > 0);
    let (rejected, hot_allocations) = test_alloc::count(|| cache.get_or_create(next));
    assert!(rejected.is_none());
    assert_eq!(hot_allocations, 0, "repainting must reuse the rejection");
}

/// Negative cache entries share bounded eviction and recently painted rejections remain hot.
#[test]
fn rejected_paths_share_the_bounded_cache_policy() {
    let mut cache = Cache::default();
    let path = json!([[[8, 0], [8, 16]]]);
    assert!(
        cache
            .get_or_create(key(path.clone(), Some(0.125), 128.0, 1.0))
            .is_none()
    );
    for revision in 0..=MAX_CACHED_PATHS {
        let mut cold = key(path.clone(), Some(0.125), 128.0, 1.0);
        cold.size[0] = (129.0 + revision as f32).to_bits();
        assert!(cache.get_or_create(cold).is_none());
        let hot = key(path.clone(), Some(0.125), 128.0, 1.0);
        let (rejected, allocations) = test_alloc::count(|| cache.get_or_create(hot));
        assert!(rejected.is_none());
        assert_eq!(allocations, 0);
    }
    assert_eq!(cache.entries.len(), MAX_CACHED_PATHS);
    assert!(cache.bytes > 0 && cache.bytes <= MAX_CACHED_BYTES);
}

/// Paths too large for half-pixel samples are refused rather than silently losing thin features.
#[test]
fn oversized_thin_strokes_are_rejected_without_coarsening() {
    let paths = json!([[[8, 0], [8, 16]]]);
    let oversized = key(paths.clone(), Some(0.125), 128.0, 1.0);
    assert!(Domain::new(&Coverage::new(&oversized).unwrap()).is_none());
    assert!(mesh(&oversized).is_none());
    let admitted = key(paths, Some(0.125), 96.0, 1.0);
    assert!(Domain::new(&Coverage::new(&admitted).unwrap()).is_some());
    assert!(mesh(&admitted).is_some());
}

/// Source bounds and stroke support determine geometry, so an outside endpoint keeps its round cap.
#[test]
fn outside_view_box_endpoints_retain_round_cap_coverage() {
    let key = key(json!([[[-2, 8], [0, 8]]]), Some(2.0), 32.0, 1.0);
    let coverage = Coverage::new(&key).unwrap();
    let (min, max) = coverage.raster_bounds().unwrap();
    assert_eq!(min, [-6.5, 13.5]);
    assert_eq!(max, [2.5, 18.5]);
    assert_eq!(coverage.at([2.25, 16.0]), 0.25);
    let mesh = mesh(&key).unwrap();
    assert!(
        mesh.vertices()
            .iter()
            .any(|vertex| vertex.position[0] < -6.0 / 32.0)
    );
    assert!(
        mesh.vertices()
            .iter()
            .any(|vertex| vertex.position[0] > 2.0 / 32.0)
    );
}

/// Rotation, reversed winding and a repeated closing point preserve even-odd cancellation.
#[test]
fn duplicate_filled_contours_leave_no_ghost_outline() {
    let square = json!([[0, 0], [16, 0], [16, 16], [0, 16]]);
    let reversed = json!([[16, 16], [16, 0], [0, 0], [0, 16], [16, 16]]);
    let cancelled = key(json!([square, reversed]), None, 32.0, 1.0);
    assert!(cancelled.path.contours.is_empty());
    let coverage = Coverage::new(&cancelled).unwrap();
    assert_eq!(coverage.at([0.0, 16.0]), 0.0);
    assert_eq!(coverage.at([16.0; 2]), 0.0);
    assert!(mesh(&cancelled).is_none());
    let odd = key(json!([square, reversed, square]), None, 32.0, 1.0);
    assert_eq!(odd.path.contours.len(), 1);
    assert_eq!(Coverage::new(&odd).unwrap().at([16.0; 2]), 1.0);
    assert!(mesh(&odd).is_some());
    let line = key(json!([[[0, 0], [8, 8], [16, 16]]]), None, 32.0, 1.0);
    assert!(mesh(&line).is_none(), "zero-area fill is not a stroke");
}

/// Subpixel stroke width controls peak coverage as the exact one-dimensional stripe overlap.
#[test]
fn subpixel_strokes_preserve_their_physical_width() {
    let key = key(json!([[[2, 8], [14, 8]]]), Some(0.25), 16.0, 1.0);
    let coverage = Coverage::new(&key).unwrap();
    for distance in [0.0, 0.1, 0.25] {
        assert_eq!(coverage.at([8.0, 8.0 + distance]), 0.25);
    }
    assert_eq!(coverage.at([8.0, 8.5]), 0.125);
    assert_eq!(coverage.at([8.0, 8.625]), 0.0);
    assert!(
        mesh(&key)
            .unwrap()
            .vertices()
            .iter()
            .all(|vertex| vertex.color[3] <= 32)
    );
}

/// Work and expanded-index admission fail before publishing oversized meshes.
#[test]
fn raster_work_and_expanded_geometry_have_independent_limits() {
    let points: Vec<_> = (0..MAX_POINTS)
        .map(|index| [if index % 2 == 0 { 0 } else { 16 }; 2])
        .collect();
    let expensive = key(json!([points]), Some(1.0), 32.0, 1.0);
    let coverage = Coverage::new(&expensive).unwrap();
    assert_eq!(coverage.sample_work(), MAX_POINTS - 1);
    assert!(Domain::new(&coverage).is_none());
    assert!(mesh(&expensive).is_none());
    let square = json!([[[0, 0], [16, 0], [16, 16], [0, 16]]]);
    let dense = key(square.clone(), None, 80.0, 1.0);
    assert!(Domain::new(&Coverage::new(&dense).unwrap()).is_some());
    assert!(mesh(&dense).unwrap().indices().len() <= MAX_MESH_INDICES);
    let detailed = striped_key();
    assert!(Domain::new(&Coverage::new(&detailed).unwrap()).is_some());
    assert!(
        mesh(&detailed).is_none(),
        "varying edge cells still respect the expanded-index budget"
    );
    let admitted = key(square, None, 30.0, 1.0);
    assert!(mesh(&admitted).unwrap().indices().len() <= MAX_MESH_INDICES);
}

/// Sparse strokes retain only grid vertices referenced by visible cells.
#[test]
fn sparse_mesh_packing_avoids_retaining_the_full_grid() {
    let key = key(json!([[[2, 2], [14, 14]]]), Some(2.0), 32.0, 1.0);
    let mesh = mesh(&key).unwrap();
    assert!(mesh.vertices().len() < 2_000);
    assert!(mesh.indices().len() <= MAX_MESH_INDICES);
}
