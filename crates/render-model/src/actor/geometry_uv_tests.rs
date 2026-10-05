use super::*;
use assets::{EntityGeometryFaceUvs, EntityGeometryScalar};

fn scalar(value: f32) -> EntityGeometryScalar {
    EntityGeometryScalar::new(value).unwrap()
}

fn authored_face(uv_size: Option<[f32; 2]>) -> EntityGeometryFaceUv {
    EntityGeometryFaceUv {
        uv: [scalar(0.0); 2],
        uv_size: uv_size.map(|size| size.map(scalar)),
    }
}

fn empty_faces() -> EntityGeometryFaceUvs {
    EntityGeometryFaceUvs {
        north: None,
        south: None,
        east: None,
        west: None,
        up: None,
        down: None,
    }
}

#[test]
fn missing_per_face_uv_sizes_use_the_native_face_dimensions_without_rounding() {
    let face = Some(authored_face(None));
    let uv = EntityGeometryUv::Faces(EntityGeometryFaceUvs {
        north: face.clone(),
        south: face.clone(),
        east: face.clone(),
        west: face.clone(),
        up: face.clone(),
        down: face,
    });
    let cube = EntityGeometryCube {
        origin: [scalar(0.0); 3],
        size: [2.5, 3.75, 4.25].map(scalar),
        pivot: [scalar(0.0); 3],
        rotation: [scalar(0.0); 3],
        uv,
        inflate: scalar(0.0),
        mirror: false,
    };
    let quads = entity_face_uvs(&cube, (32, 64)).unwrap();
    for (quad, dimensions) in quads.into_iter().zip([
        [2.5, 3.75],
        [2.5, 3.75],
        [4.25, 3.75],
        [4.25, 3.75],
        [2.5, 4.25],
        [2.5, 4.25],
    ]) {
        assert_eq!(
            quad.unwrap()[2],
            [dimensions[0] / 32.0, dimensions[1] / 64.0]
        );
    }
}

#[test]
fn explicit_signed_uv_size_overrides_the_native_face_dimensions() {
    let quad = face_uv_quad(
        Some(&authored_face(Some([-6.0, 7.0]))),
        [2.0, 3.0],
        &|origin, dimensions| [origin, dimensions, origin, dimensions],
    )
    .unwrap();
    assert_eq!(quad[1], [-6.0, 7.0]);
}

#[test]
fn box_uv_planes_keep_matching_v_coordinates_on_their_opposing_faces() {
    for (cube_mirror, bone_mirror) in [(false, false), (true, false), (false, true)] {
        let cube = EntityGeometryCube {
            origin: [scalar(0.0); 3],
            size: [2.0, 0.0, 3.0].map(scalar),
            pivot: [scalar(0.0); 3],
            rotation: [scalar(0.0); 3],
            uv: EntityGeometryUv::Box([2.0, 7.0].map(scalar)),
            inflate: scalar(0.0),
            mirror: cube_mirror,
        };
        let mut vertices = Vec::new();
        append_entity_cube_vertices(&mut vertices, &cube, 0, (16, 16), bone_mirror, 0.0).unwrap();
        assert_eq!(vertices.len(), 12);
        for vertex in &vertices[..6] {
            let opposing = vertices[6..]
                .iter()
                .find(|opposing| opposing.position == vertex.position)
                .expect("both authored faces cover the same physical plane");
            assert_eq!(vertex.uv[1], opposing.uv[1]);
            assert_eq!(opposing.uv[0] - vertex.uv[0], cube.size[0].get() / 16.0);
            assert_eq!(opposing.normal, vertex.normal.map(|axis| -axis));
        }
    }
}

#[test]
fn vanilla_arrow_planes_sample_the_whole_shaft_and_end_cap() {
    // Pinned arrow.geo.json: two crossed 16×5 shaft quads and one 5×5 south cap.
    for (size, faces, maximum) in [
        (
            [0.0, 5.0, 16.0],
            EntityGeometryFaceUvs {
                east: Some(authored_face(None)),
                ..empty_faces()
            },
            [16.0, 5.0],
        ),
        (
            [5.0, 5.0, 0.0],
            EntityGeometryFaceUvs {
                south: Some(EntityGeometryFaceUv {
                    uv: [scalar(0.0), scalar(5.0)],
                    uv_size: None,
                }),
                ..empty_faces()
            },
            [5.0, 10.0],
        ),
    ] {
        let cube = EntityGeometryCube {
            origin: [scalar(0.0); 3],
            size: size.map(scalar),
            pivot: [scalar(0.0); 3],
            rotation: [scalar(0.0); 3],
            uv: EntityGeometryUv::Faces(faces),
            inflate: scalar(0.0),
            mirror: false,
        };
        let mut vertices = Vec::new();
        append_entity_cube_vertices(&mut vertices, &cube, 0, (32, 32), false, 0.0).unwrap();
        assert_eq!(vertices.len(), 6);
        assert!(vertices.iter().all(|vertex| vertex.back_uv == vertex.uv));
        let actual = vertices.iter().fold([0.0_f32; 2], |maximum, vertex| {
            [
                maximum[0].max(vertex.uv[0] * 32.0),
                maximum[1].max(vertex.uv[1] * 32.0),
            ]
        });
        assert_eq!(actual, maximum);
    }
}

#[test]
fn native_nocull_samples_the_authored_face_uv_from_both_sides() {
    let cube = EntityGeometryCube {
        origin: [scalar(0.0); 3],
        size: [0.0, 5.0, 16.0].map(scalar),
        pivot: [scalar(0.0); 3],
        rotation: [scalar(0.0); 3],
        uv: EntityGeometryUv::Faces(EntityGeometryFaceUvs {
            east: Some(authored_face(None)),
            ..empty_faces()
        }),
        inflate: scalar(0.0),
        mirror: false,
    };
    let mut vertices = Vec::new();
    append_entity_cube_vertices(&mut vertices, &cube, 0, (32, 32), false, 0.0).unwrap();
    assert_eq!(
        vertices.len(),
        6,
        "only the authored arrow shaft face exists"
    );
    assert!(vertices.iter().all(|vertex| vertex.back_uv == vertex.uv));
    assert_eq!(vertices[0].normal, [1.0, 0.0, 0.0]);
}
