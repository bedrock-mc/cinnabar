use super::*;
use mod_api::drawing::{api, shapes};

const COLOR: [f32; 4] = [1.; 4];

/// Uploads every SDK geometry into one empty owner using host geometry and aggregate validation.
fn admit_shape(shape: Vec<api::GeometrySpec>) {
    let mut store = Store::default();
    for geometry in shape {
        let api::GeometrySpec::Line(line) = geometry else {
            panic!("expected a polyline");
        };
        store
            .create_geometry(GeometrySpec::Polyline {
                vertices: line
                    .vertices
                    .into_iter()
                    .map(|vertex| Vertex {
                        position: vertex.position.into(),
                        color: vertex.color.into(),
                        uv: vertex.uv.into(),
                        data: vertex.data.into(),
                    })
                    .collect(),
                closed: line.closed,
            })
            .unwrap();
    }
    store.commit();
}

/// Finds the largest accepted subdivision, admits it on the host and checks the next SDK call.
fn subdivision_boundary(build: impl Fn(u32) -> Result<Vec<api::GeometrySpec>, api::DrawError>) {
    let mut accepted = 3;
    let mut rejected = u32::try_from(mod_api::MAX_DRAW_VERTICES).unwrap() + 1;
    assert!(build(accepted).is_ok());
    assert_eq!(
        build(rejected).unwrap_err().code,
        api::ErrorCode::BudgetExceeded
    );
    while rejected - accepted > 1 {
        let middle = accepted + (rejected - accepted) / 2;
        match build(middle) {
            Ok(_) => accepted = middle,
            Err(error) => {
                assert_eq!(error.code, api::ErrorCode::BudgetExceeded);
                rejected = middle;
            }
        }
    }
    admit_shape(build(accepted).unwrap());
    assert_eq!(
        build(accepted + 1).unwrap_err().code,
        api::ErrorCode::BudgetExceeded
    );
}

#[test]
fn sdk_shapes_at_their_subdivision_limits_fit_an_empty_host_owner() {
    subdivision_boundary(|segments| shapes::circle(1., segments, COLOR));
    subdivision_boundary(|segments| shapes::sphere_wireframe(1., segments, COLOR));
    for closed in [false, true] {
        subdivision_boundary(|count| {
            let points = vec![[0.; 3]; count as usize];
            shapes::polyline(&points, closed, COLOR)
        });
    }
    subdivision_boundary(|count| {
        let points = vec![[0.; 3]; count as usize];
        shapes::path_gradient(&points, COLOR, COLOR)
    });
    for axis in 0..2 {
        subdivision_boundary(|count| {
            let mut cells = [1; 2];
            cells[axis] = count;
            shapes::grid([1.; 2], cells, COLOR)
        });
    }
}

#[test]
fn sdk_shapes_without_subdivisions_fit_an_empty_host_owner() {
    let bounds = [[0.; 3], [1.; 3]];
    for shape in [
        shapes::box_wireframe(bounds, COLOR),
        shapes::entity_bounding_box(bounds, COLOR),
        shapes::chunk_border(bounds, COLOR),
        shapes::arrow([0.; 3], [1., 0., 0.], 0.25, 0.25, COLOR),
    ] {
        admit_shape(shape.unwrap());
    }
}
