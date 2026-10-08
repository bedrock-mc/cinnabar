//! Cube-local bind poses from vanilla model-part cube setup, not hierarchical bone rotations.
use super::*;
use assets::EntityGeometryScalar;

fn scalar(value: f32) -> EntityGeometryScalar {
    EntityGeometryScalar::new(value).unwrap()
}

fn bone() -> EntityGeometryBone {
    EntityGeometryBone {
        name: "body".into(),
        parent: None,
        pivot: Some([-2.0, 15.0, 12.0].map(scalar)),
        rotation: None,
        bind_pose_rotation: Some([90.0, 0.0, 0.0].map(scalar)),
        mirror: None,
        inflate: None,
        never_render: None,
        reset: None,
        binding: None,
        texture_meshes: Box::new([]),
        cubes: Box::new([]),
    }
}

#[test]
fn native_bind_pose_lays_polar_body_cubes_horizontally_without_moving_the_bone_pivot() {
    let body = bone();
    // First adult polar-body cube from the shipped native base geometry.
    let cube = EntityGeometryCube {
        origin: [-7.0, 14.0, 5.0].map(scalar),
        size: [14.0, 14.0, 11.0].map(scalar),
        pivot: [EntityGeometryScalar::ZERO; 3],
        rotation: [EntityGeometryScalar::ZERO; 3],
        uv: EntityGeometryUv::Box([EntityGeometryScalar::ZERO; 2]),
        inflate: EntityGeometryScalar::ZERO,
        mirror: false,
    };
    let mut vertices = Vec::new();
    append_entity_bone_cube_vertices(&mut vertices, &cube, 1, (128, 64), &body).unwrap();
    let min: [f32; 3] = std::array::from_fn(|axis| {
        vertices
            .iter()
            .map(|v| v.position[axis] * 16.0)
            .fold(f32::INFINITY, f32::min)
    });
    let max: [f32; 3] = std::array::from_fn(|axis| {
        vertices
            .iter()
            .map(|v| v.position[axis] * 16.0)
            .fold(f32::NEG_INFINITY, f32::max)
    });
    for (actual, expected) in min
        .into_iter()
        .zip([-7.0, 8.0, -1.0])
        .chain(max.into_iter().zip([7.0, 19.0, 13.0]))
    {
        assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
    }
    assert_eq!(body.pivot.unwrap().map(|v| v.get()), [-2.0, 15.0, 12.0]);
    assert!(body.rotation.is_none());
    assert!(vertices.iter().all(|v| v.bone_index == 1));
    assert!(
        vertices
            .iter()
            .all(|v| (Vec3::from_array(v.normal).length() - 1.0).abs() < 1e-5)
    );
}

#[test]
fn native_bind_pose_adds_euler_channels_and_rotates_an_explicit_cube_pivot_separately() {
    let body = bone();
    let cube = EntityGeometryCube {
        origin: [1.0, 3.0, 2.0].map(scalar),
        size: [2.0, 4.0, 3.0].map(scalar),
        pivot: [2.0, 5.0, 3.0].map(scalar),
        rotation: [10.0, 20.0, 30.0].map(scalar),
        uv: EntityGeometryUv::Box([EntityGeometryScalar::ZERO; 2]),
        inflate: EntityGeometryScalar::ZERO,
        mirror: false,
    };
    let mut vertices = Vec::new();
    append_entity_bone_cube_vertices(&mut vertices, &cube, 0, (128, 64), &body).unwrap();
    let pivot = cube.pivot.map(|v| v.get() / 16.0);
    let bone_pivot = body.pivot.unwrap().map(|v| v.get() / 16.0);
    let rotated_pivot = rotate_euler_around(pivot, bone_pivot, [-90.0, 0.0, 0.0]).unwrap();
    // Independent native contract: R(cubeEuler+bindEuler)*(v−cubePivot)+R(bind)*(cubePivot−bonePivot)+bonePivot.
    let corner = cube.origin.map(|v| v.get() / 16.0);
    let mut expected = rotate_euler_around(corner, pivot, [-100.0, 20.0, -30.0]).unwrap();
    expected = std::array::from_fn(|axis| expected[axis] + rotated_pivot[axis] - pivot[axis]);
    expected[0] = -expected[0];
    assert!(vertices.iter().any(|v| {
        v.position
            .into_iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 1e-5)
    }));
}

#[test]
fn inherited_llama_body_bind_keeps_its_torso_horizontal_and_child_frame_unchanged() {
    let mut body = bone();
    body.pivot = Some([0.0, 19.0, 2.0].map(scalar));
    let cube = EntityGeometryCube {
        origin: [-6.0, 11.0, -5.0].map(scalar),
        size: [12.0, 18.0, 10.0].map(scalar),
        pivot: [EntityGeometryScalar::ZERO; 3],
        rotation: [EntityGeometryScalar::ZERO; 3],
        uv: EntityGeometryUv::Box([29.0, 0.0].map(scalar)),
        inflate: EntityGeometryScalar::ZERO,
        mirror: false,
    };
    let mut vertices = Vec::new();
    append_entity_bone_cube_vertices(&mut vertices, &cube, 0, (128, 64), &body).unwrap();
    for axis in 0..3 {
        let min = vertices
            .iter()
            .map(|v| v.position[axis] * 16.0)
            .fold(f32::INFINITY, f32::min);
        let max = vertices
            .iter()
            .map(|v| v.position[axis] * 16.0)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((min - [-6.0, 12.0, -8.0][axis]).abs() < 1e-4);
        assert!((max - [6.0, 22.0, 10.0][axis]).abs() < 1e-4);
    }
    assert_eq!(body.pivot.unwrap().map(|v| v.get()), [0.0, 19.0, 2.0]);
    assert!(body.rotation.is_none());
}
