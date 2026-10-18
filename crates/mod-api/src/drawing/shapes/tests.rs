use super::*;

const COLOR: [f32; 4] = [0.25, 0.5, 0.75, 1.];

/// Extracts independent line payloads while checking every neutral vertex attribute.
fn lines(shape: Vec<GeometrySpec>) -> Vec<(Vec<[f32; 3]>, bool)> {
    shape
        .into_iter()
        .map(|geometry| {
            let GeometrySpec::Line(line) = geometry else {
                panic!("expected a polyline")
            };
            let points = line
                .vertices
                .into_iter()
                .map(|vertex| {
                    let color: (f32, f32, f32, f32) = COLOR.into();
                    assert_eq!(vertex.color, color);
                    assert_eq!(vertex.uv, (0., 0.));
                    assert_eq!(vertex.data, (0., 0., 0., 0.));
                    vertex.position.into()
                })
                .collect();
            (points, line.closed)
        })
        .collect()
}

/// Lists the twelve expected edges independently of the builder's bit traversal.
fn box_edges() -> Vec<(Vec<[f32; 3]>, bool)> {
    [
        [[0., 0., 0.], [1., 0., 0.]],
        [[0., 0., 0.], [0., 2., 0.]],
        [[0., 0., 0.], [0., 0., 3.]],
        [[1., 0., 0.], [1., 2., 0.]],
        [[1., 0., 0.], [1., 0., 3.]],
        [[0., 2., 0.], [1., 2., 0.]],
        [[0., 2., 0.], [0., 2., 3.]],
        [[1., 2., 0.], [1., 2., 3.]],
        [[0., 0., 3.], [1., 0., 3.]],
        [[0., 0., 3.], [0., 2., 3.]],
        [[1., 0., 3.], [1., 2., 3.]],
        [[0., 2., 3.], [1., 2., 3.]],
    ]
    .into_iter()
    .map(|edge| (edge.to_vec(), false))
    .collect()
}

#[test]
fn box_entity_and_chunk_helpers_emit_exactly_twelve_independent_edges() {
    let bounds = [[0., 0., 0.], [1., 2., 3.]];
    for shape in [
        box_wireframe(bounds, COLOR),
        entity_bounding_box(bounds, COLOR),
        chunk_border(bounds, COLOR),
    ] {
        assert_eq!(lines(shape.unwrap()), box_edges());
    }
}

#[test]
fn translated_bounds_preserve_authorized_min_and_max() {
    let bounds = [[-2., -1., 4.], [-1., 1., 7.]];
    let expected = box_edges()
        .into_iter()
        .map(|(points, closed)| {
            (
                points
                    .into_iter()
                    .map(|p| [p[0] - 2., p[1] - 1., p[2] + 4.])
                    .collect(),
                closed,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(lines(entity_bounding_box(bounds, COLOR).unwrap()), expected);
}

#[test]
fn circle_and_sphere_have_exact_cardinal_vertices_without_duplicate_seams() {
    let xz = vec![[2., 0., 0.], [0., 0., 2.], [-2., 0., 0.], [0., 0., -2.]];
    assert_eq!(
        lines(circle(2., 4, COLOR).unwrap()),
        vec![(xz.clone(), true)]
    );
    assert_eq!(
        lines(sphere_wireframe(2., 4, COLOR).unwrap()),
        vec![
            (xz, true),
            (
                vec![[2., 0., 0.], [0., 2., 0.], [-2., 0., 0.], [0., -2., 0.]],
                true
            ),
            (
                vec![[0., 2., 0.], [0., 0., 2.], [0., -2., 0.], [0., 0., -2.]],
                true
            ),
        ]
    );
}

#[test]
fn paths_preserve_order_repetition_and_closed_flag() {
    let points = [[1., 2., 3.], [1., 2., 3.], [2., 4., 6.]];
    for closed in [false, true] {
        assert_eq!(
            lines(polyline(&points, closed, COLOR).unwrap()),
            vec![(points.to_vec(), closed)]
        );
    }
}

#[test]
fn gradient_uses_distance_and_repeated_points_do_not_advance_it() {
    let points = [[0.; 3], [1., 0., 0.], [1., 0., 0.], [1., 3., 0.]];
    let start = [1., 0., 0., 1.];
    let end = [0., 0., 1., 0.];
    let shape = path_gradient(&points, start, end).unwrap();
    let GeometrySpec::Line(line) = &shape[0] else {
        panic!("expected a line")
    };
    assert!(!line.closed);
    let expected_colors = [start, [0.75, 0., 0.25, 0.75], [0.75, 0., 0.25, 0.75], end];
    for ((vertex, position), color) in line.vertices.iter().zip(points).zip(expected_colors) {
        let position: (f32, f32, f32) = position.into();
        let color: (f32, f32, f32, f32) = color.into();
        assert_eq!(vertex.position, position);
        assert_eq!(vertex.color, color);
        assert_eq!(vertex.uv, (0., 0.));
        assert_eq!(vertex.data, (0., 0., 0., 0.));
    }
    let shape = path_gradient(&[[1.; 3]; 2], start, end).unwrap();
    let GeometrySpec::Line(line) = &shape[0] else {
        panic!("expected a line")
    };
    let color: (f32, f32, f32, f32) = start.into();
    assert!(line.vertices.iter().all(|v| v.color == color));
}

#[test]
fn gradient_preserves_exact_endpoint_colors() {
    let start = [0.1, 0.8, 0.2, 0.9];
    let end = [0.7, 0.1, 0.6, 0.3];
    let shape = path_gradient(&[[0.; 3], [1.; 3]], start, end).unwrap();
    let GeometrySpec::Line(line) = &shape[0] else {
        panic!("expected a line")
    };
    let expected: [(f32, f32, f32, f32); 2] = [start.into(), end.into()];
    assert_eq!(
        line.vertices.iter().map(|v| v.color).collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn arrow_has_exact_shaft_and_two_wings_without_crossing_the_head() {
    assert_eq!(
        lines(arrow([0.; 3], [4., 0., 0.], 1., 2., COLOR).unwrap()),
        vec![
            (vec![[0.; 3], [4., 0., 0.]], false),
            (vec![[4., 0., 0.], [3., 0., -1.]], false),
            (vec![[4., 0., 0.], [3., 0., 1.]], false),
        ]
    );
    assert_eq!(
        lines(arrow([0.; 3], [0., -4., 0.], 1., 2., COLOR).unwrap()),
        vec![
            (vec![[0.; 3], [0., -4., 0.]], false),
            (vec![[0., -4., 0.], [0., -3., -1.]], false),
            (vec![[0., -4., 0.], [0., -3., 1.]], false),
        ]
    );
}

#[test]
fn grid_has_exact_boundary_and_interior_lines() {
    assert_eq!(
        lines(grid([2., 4.], [2, 1], COLOR).unwrap()),
        vec![
            (vec![[0., 0., 0.], [0., 0., 4.]], false),
            (vec![[1., 0., 0.], [1., 0., 4.]], false),
            (vec![[2., 0., 0.], [2., 0., 4.]], false),
            (vec![[0., 0., 0.], [2., 0., 0.]], false),
            (vec![[0., 0., 4.], [2., 0., 4.]], false),
        ]
    );
}

#[test]
fn malformed_shapes_return_typed_errors() {
    for result in [
        box_wireframe([[0.; 3]; 2], COLOR),
        entity_bounding_box([[1.; 3], [0.; 3]], COLOR),
        chunk_border([[0.; 3], [f32::NAN; 3]], COLOR),
        circle(0., 4, COLOR),
        circle(f32::INFINITY, 4, COLOR),
        circle(1., 2, COLOR),
        sphere_wireframe(-1., 4, COLOR),
        polyline(&[], false, COLOR),
        polyline(&[[0.; 3]; 2], true, COLOR),
        polyline(&[[0.; 3], [f32::NAN; 3]], false, COLOR),
        polyline(&[[0.; 3]; 2], false, [f32::NAN; 4]),
        path_gradient(&[[0.; 3]; 2], COLOR, [2.; 4]),
        arrow([0.; 3], [0.; 3], 1., 1., COLOR),
        arrow([0.; 3], [1., 0., 0.], 2., 1., COLOR),
        arrow([0.; 3], [1., 0., 0.], 1., f32::NAN, COLOR),
        grid([1., 1.], [0, 1], COLOR),
        grid([-1., 1.], [1, 1], COLOR),
    ] {
        assert_eq!(result.unwrap_err().code, ErrorCode::Invalid);
    }
}

#[test]
fn generated_arrow_wings_and_local_points_respect_contract_extent() {
    let limit = MAX_PRIMITIVE_EXTENT_BLOCKS;
    assert_eq!(
        arrow([0., 0., limit], [1., 0., limit], 1., 2., COLOR)
            .unwrap_err()
            .code,
        ErrorCode::Invalid
    );
    assert!(polyline(&[[0.; 3], [limit, 0., 0.]], false, COLOR).is_ok());
    assert_eq!(
        polyline(&[[0.; 3], [limit * 2., 0., 0.]], false, COLOR)
            .unwrap_err()
            .code,
        ErrorCode::Invalid
    );
}

#[test]
fn sdk_shapes_reject_excessive_expansion() {
    for result in [
        circle(1., 10_000, COLOR),
        sphere_wireframe(1., 3_000, COLOR),
    ] {
        let Err(error) = result else {
            panic!("SDK accepted geometry beyond the expansion limit");
        };
        assert_eq!(error.code, ErrorCode::BudgetExceeded);
    }
}

#[test]
fn excessive_subdivisions_fail_before_allocating_and_use_owner_limits() {
    for result in [
        circle(1., u32::MAX, COLOR),
        sphere_wireframe(1., u32::MAX, COLOR),
        grid([1.; 2], [u32::MAX; 2], COLOR),
    ] {
        assert_eq!(result.unwrap_err().code, ErrorCode::BudgetExceeded);
    }
    let max_vertices = MAX_DRAW_VERTICES.min(
        MAX_DRAW_MUTATION_BYTES.min(MAX_DRAW_GEOMETRY_BYTES)
            / (std::mem::size_of::<Vertex>() + std::mem::size_of::<f32>()),
    );
    let max_vertices = (3..=max_vertices)
        .take_while(|&vertices| {
            expanded_vertices(vertices, 0, true, true) <= MAX_DRAW_EXPANDED_VERTICES
        })
        .last()
        .unwrap();
    let max_vertices = u32::try_from(max_vertices).unwrap();
    assert!(circle(1., max_vertices, COLOR).is_ok());
    assert_eq!(
        circle(1., max_vertices + 1, COLOR).unwrap_err().code,
        ErrorCode::BudgetExceeded
    );
    assert_eq!(
        sphere_wireframe(1., max_vertices / 3 + 1, COLOR)
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
}

#[test]
fn grid_admission_respects_the_callback_import_budget() {
    let max_lines = crate::MAX_RENDER_CALLS.min(MAX_DRAW_GEOMETRIES);
    let max_lines = u32::try_from(max_lines).unwrap();
    for cells in [[max_lines - 3, 1], [1, max_lines - 3]] {
        assert_eq!(
            grid([1.; 2], cells, COLOR).unwrap().len(),
            max_lines as usize
        );
    }
    for cells in [[max_lines - 2, 1], [1, max_lines - 2]] {
        assert_eq!(
            grid([1.; 2], cells, COLOR).unwrap_err().code,
            ErrorCode::BudgetExceeded
        );
    }
}
