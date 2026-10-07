use assets::{EntityGeometryBone, EntityGeometryCube, EntityGeometryFaceUv, EntityGeometryUv};
use glam::Vec3;

use super::{ActorRigGeometryError, ActorRigSurface, ActorRigVertex};

#[cfg(test)]
#[path = "dragon_geometry_tests.rs"]
mod dragon_geometry_tests;

/// Face corners as `[top-left, top-right, bottom-right, bottom-left]` seen from outside, in
/// authored geometry space where the model faces -Z and its right side is -X.
const ENTITY_FACES: [[usize; 4]; 6] = [
    [3, 2, 1, 0], // north: front
    [6, 7, 4, 5], // south: back
    [7, 3, 0, 4], // -X: the model's right side, authored as `east`
    [2, 6, 5, 1], // +X: the model's left side, authored as `west`
    [7, 6, 2, 3], // up
    [0, 1, 5, 4], // down, with the texture's V reversed relative to up
];

/// Emits a cube in the rig frame: authored X is mirrored so the model's right side is +X,
/// matching the renderer's right-handed actor space.
pub fn append_entity_cube_vertices(
    vertices: &mut Vec<ActorRigVertex>,
    cube: &EntityGeometryCube,
    bone_index: u32,
    texture_size: (u16, u16),
    bone_mirror: bool,
    bone_inflate: f32,
) -> Result<(), ActorRigGeometryError> {
    append_cube_vertices(
        vertices,
        cube,
        bone_index,
        texture_size,
        bone_mirror,
        bone_inflate,
        None,
    )
}

/// Bind-pose rotation belongs to the part's cubes, not its children or animation channels.
pub(super) fn append_entity_bone_cube_vertices(
    vertices: &mut Vec<ActorRigVertex>,
    cube: &EntityGeometryCube,
    bone_index: u32,
    texture_size: (u16, u16),
    bone: &EntityGeometryBone,
) -> Result<(), ActorRigGeometryError> {
    let bind = bone.bind_pose_rotation.map(|rotation| {
        (
            bone.pivot
                .map_or([0.0; 3], |pivot| pivot.map(|value| value.get() / 16.0)),
            rotation.map(|value| value.get()),
        )
    });
    append_cube_vertices(
        vertices,
        cube,
        bone_index,
        texture_size,
        bone.mirror.unwrap_or(false),
        bone.inflate.map_or(0.0, |inflate| inflate.get()),
        bind,
    )
}

fn append_cube_vertices(
    vertices: &mut Vec<ActorRigVertex>,
    cube: &EntityGeometryCube,
    bone_index: u32,
    texture_size: (u16, u16),
    bone_mirror: bool,
    bone_inflate: f32,
    bind: Option<([f32; 3], [f32; 3])>,
) -> Result<(), ActorRigGeometryError> {
    let origin = cube.origin.map(|value| value.get());
    let size = cube.size.map(|value| value.get());
    let inflate = cube.inflate.get() + bone_inflate;
    if origin
        .iter()
        .chain(size.iter())
        .chain([inflate].iter())
        .any(|value| !value.is_finite())
        || size.iter().any(|value| *value < 0.0)
        || texture_size.0 == 0
        || texture_size.1 == 0
    {
        return Err(ActorRigGeometryError::InvalidAssetGeometry);
    }
    let zero_axes: Vec<_> = size
        .iter()
        .enumerate()
        .filter(|(_, value)| **value + 2.0 * inflate == 0.0)
        .map(|(axis, _)| axis)
        .collect();
    if zero_axes.len() > 1 {
        return Err(ActorRigGeometryError::InvalidAssetGeometry);
    }
    let min = std::array::from_fn(|axis| (origin[axis] - inflate) / 16.0);
    let max = std::array::from_fn(|axis| (origin[axis] + size[axis] + inflate) / 16.0);
    if (0..3).any(|axis| min[axis] > max[axis]) {
        return Err(ActorRigGeometryError::InvalidAssetGeometry);
    }
    let mut corners = cuboid_corners(min, max);
    let pivot = cube.pivot.map(|value| value.get() / 16.0);
    let cube_rotation = cube.rotation.map(|value| value.get());
    let bind_rotation = bind.map_or([0.0; 3], |(_, rotation)| rotation);
    let rotation: [f32; 3] = std::array::from_fn(|axis| cube_rotation[axis] + bind_rotation[axis]);
    // Vanilla model-part cube setup adds the bind Euler angles
    // to the cube's angles and rotates its pivot about the part's pivot separately.
    let bind_offset = match bind {
        None => [0.0; 3],
        Some((bone_pivot, _)) => {
            let rotated = rotate_euler_around(
                pivot,
                bone_pivot,
                [-bind_rotation[0], bind_rotation[1], -bind_rotation[2]],
            )
            .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?;
            std::array::from_fn(|axis| rotated[axis] - pivot[axis])
        }
    };
    if rotation.iter().any(|value| *value != 0.0) || bind.is_some() {
        // Authored rotations turn X and Z the opposite way to a right-handed rotation.
        let authored = [-rotation[0], rotation[1], -rotation[2]];
        for corner in &mut corners {
            *corner = rotate_euler_around(*corner, pivot, authored)
                .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?;
            *corner = std::array::from_fn(|axis| corner[axis] + bind_offset[axis]);
        }
    }
    let corners = corners.map(mirror_x);
    let mirror = cube.mirror ^ bone_mirror;
    let face_uvs = entity_face_uvs(cube, texture_size)?;
    // A mirrored cube reflects each face across the cube's X midplane, carrying its UVs.
    let face_corners = ENTITY_FACES.map(|face| {
        if mirror {
            face.map(|corner| corner ^ REFLECT_X_BIT)
        } else {
            face
        }
    });
    // Mirroring authored X flips handedness, so the unreflected winding already faces out.
    let order = if mirror {
        [0, 2, 3, 0, 1, 2]
    } else {
        [0, 3, 2, 0, 2, 1]
    };
    let faces: &[usize] = match zero_axes.first() {
        Some(0) => &[3, 2],
        Some(1) => &[4, 5],
        Some(2) => &[0, 1],
        _ => &[0, 1, 2, 3, 4, 5],
    };
    // Coincident faces remain independent: a nocull quad can show through a transparent
    // opposite quad. Choosing its UV by view direction would implicitly cull that face.
    for &face in faces {
        let Some(uv) = face_uvs[face] else {
            continue;
        };
        let quad = face_corners[face];
        let normal = triangle_normal(
            corners[quad[order[0]]],
            corners[quad[order[1]]],
            corners[quad[order[2]]],
        );
        vertices.extend(order.into_iter().map(|index| ActorRigVertex {
            position: corners[quad[index]],
            normal,
            uv: uv[index],
            back_uv: uv[index],
            bone_index,
            surface: ActorRigSurface::SINGLE_FACE,
        }));
    }
    Ok(())
}

/// Back UV of a plane with one textured face; the shaders discard its back side.
pub const ONE_SIDED_BACK_UV: [f32; 2] = [-1.0e9, -1.0e9];

/// Corner-index bit that selects the max-X corner of a cuboid.
const REFLECT_X_BIT: usize = 1;

fn mirror_x(point: [f32; 3]) -> [f32; 3] {
    [-point[0], point[1], point[2]]
}

fn entity_face_uvs(
    cube: &EntityGeometryCube,
    texture_size: (u16, u16),
) -> Result<[Option<FaceUvQuad>; 6], ActorRigGeometryError> {
    let size = cube.size.map(|value| value.get());
    let (width, height) = (f32::from(texture_size.0), f32::from(texture_size.1));
    let quad = |origin: [f32; 2], dimensions: [f32; 2]| {
        let left = origin[0] / width;
        let right = (origin[0] + dimensions[0]) / width;
        let top = origin[1] / height;
        let bottom = (origin[1] + dimensions[1]) / height;
        [[left, top], [right, top], [right, bottom], [left, bottom]]
    };
    let result = match &cube.uv {
        EntityGeometryUv::Box(origin) => {
            let [u, v] = origin.map(|value| value.get());
            // Box layout spans whole texels of the authored size.
            let [x, y, z] = size.map(f32::trunc);
            [
                Some(quad([u + z, v + z], [x, y])),
                Some(quad([u + z + x + z, v + z], [x, y])),
                Some(quad([u, v + z], [z, y])),
                Some(quad([u + z + x, v + z], [z, y])),
                Some(quad([u + z, v], [x, z])),
                Some(quad([u + z + x, v + z], [x, -z])),
            ]
        }
        EntityGeometryUv::Faces(faces) => {
            let faces = [
                &faces.north,
                &faces.south,
                &faces.east,
                &faces.west,
                &faces.up,
                &faces.down,
            ];
            let dimensions = cube.face_uv_dimensions();
            std::array::from_fn(|face| face_uv_quad(faces[face].as_ref(), dimensions[face], &quad))
        }
    };
    if result
        .iter()
        .flatten()
        .flatten()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(ActorRigGeometryError::InvalidAssetGeometry);
    }
    // Vanilla clips cube UV corners before interpolation; sampler clamping would
    // stretch a transparent edge texel across the overflowing part of the face.
    Ok(result
        .map(|face| face.map(|corners| corners.map(|uv| uv.map(|value| value.clamp(0.0, 1.0))))))
}

type FaceUvQuad = [[f32; 2]; 4];

fn face_uv_quad(
    face: Option<&EntityGeometryFaceUv>,
    dimensions: [f32; 2],
    quad: &impl Fn([f32; 2], [f32; 2]) -> FaceUvQuad,
) -> Option<FaceUvQuad> {
    face.map(|face| {
        quad(
            face.uv.map(|value| value.get()),
            // Vanilla box face UVs first copy the cube's
            // face dimensions, then optionally replaces them with authored uv_size.
            face.uv_size
                .map_or(dimensions, |size| size.map(|value| value.get())),
        )
    })
}

#[cfg(test)]
#[path = "geometry_uv_tests.rs"]
mod uv_tests;

fn cuboid_corners(min: [f32; 3], max: [f32; 3]) -> [[f32; 3]; 8] {
    [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ]
}

fn rotate_euler_around(point: [f32; 3], pivot: [f32; 3], degrees: [f32; 3]) -> Option<[f32; 3]> {
    if point
        .iter()
        .chain(pivot.iter())
        .chain(degrees.iter())
        .any(|value| !value.is_finite())
    {
        return None;
    }
    let [x, y, z] = degrees.map(|value| value.to_radians());
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let mut value = std::array::from_fn(|axis| point[axis] - pivot[axis]);
    value = [
        value[0],
        value[1] * cx - value[2] * sx,
        value[1] * sx + value[2] * cx,
    ];
    value = [
        value[0] * cy + value[2] * sy,
        value[1],
        -value[0] * sy + value[2] * cy,
    ];
    value = [
        value[0] * cz - value[1] * sz,
        value[0] * sz + value[1] * cz,
        value[2],
    ];
    Some(std::array::from_fn(|axis| value[axis] + pivot[axis]))
}

pub(crate) fn cuboid_vertices(
    min: [f32; 3],
    max: [f32; 3],
    bone_index: u32,
) -> Vec<ActorRigVertex> {
    let corners = cuboid_corners(min, max);
    let faces = [
        [0, 1, 2, 3],
        [5, 4, 7, 6],
        [4, 0, 3, 7],
        [1, 5, 6, 2],
        [3, 2, 6, 7],
        [4, 5, 1, 0],
    ];
    let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    faces
        .into_iter()
        .flat_map(|face| {
            let normal = triangle_normal(corners[face[0]], corners[face[2]], corners[face[1]]);
            [0, 2, 1, 0, 3, 2].into_iter().map(move |index| {
                let uv = uv[index];
                ActorRigVertex {
                    position: corners[face[index]],
                    normal,
                    uv,
                    back_uv: uv,
                    bone_index,
                    surface: ActorRigSurface::SINGLE_FACE,
                }
            })
        })
        .collect()
}

pub(crate) fn triangle_normal(first: [f32; 3], second: [f32; 3], third: [f32; 3]) -> [f32; 3] {
    let left = Vec3::from_array(second) - Vec3::from_array(first);
    let right = Vec3::from_array(third) - Vec3::from_array(first);
    left.cross(right).normalize_or_zero().to_array()
}

#[cfg(test)]
#[path = "bind_pose_tests.rs"]
mod bind_pose_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use assets::EntityGeometryScalar;
    fn scalar(value: f32) -> EntityGeometryScalar {
        EntityGeometryScalar::new(value).unwrap()
    }
    fn cube(origin: [f32; 3], size: [f32; 3], mirror: bool) -> EntityGeometryCube {
        EntityGeometryCube {
            origin: origin.map(scalar),
            size: size.map(scalar),
            pivot: [scalar(0.0); 3],
            rotation: [scalar(0.0); 3],
            uv: EntityGeometryUv::Box([scalar(0.0); 2]),
            inflate: scalar(0.0),
            mirror,
        }
    }
    fn plane(axis: usize, mirror: bool) -> EntityGeometryCube {
        let mut size = [2.0, 3.0, 4.0];
        size[axis] = 0.0;
        cube([0.0; 3], size, mirror)
    }
    fn build(cube: &EntityGeometryCube) -> Vec<ActorRigVertex> {
        let mut vertices = Vec::new();
        append_entity_cube_vertices(&mut vertices, cube, 0, (64, 64), false, 0.0).unwrap();
        vertices
    }
    fn face(vertices: &[ActorRigVertex], normal: [f32; 3]) -> Vec<ActorRigVertex> {
        vertices
            .iter()
            .filter(|vertex| {
                (Vec3::from_array(vertex.normal) - Vec3::from_array(normal)).length() < 1.0e-4
            })
            .copied()
            .collect()
    }
    /// The texel range a face samples and the rig-frame corner holding its top-left texel.
    fn region(face: &[ActorRigVertex]) -> ([f32; 4], [f32; 3]) {
        let texel = |value: f32| (value * 64.0).round();
        let [mut u0, mut v0, mut u1, mut v1] = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for vertex in face {
            u0 = u0.min(texel(vertex.uv[0]));
            v0 = v0.min(texel(vertex.uv[1]));
            u1 = u1.max(texel(vertex.uv[0]));
            v1 = v1.max(texel(vertex.uv[1]));
        }
        let top_left = face
            .iter()
            .find(|vertex| texel(vertex.uv[0]) == u0 && texel(vertex.uv[1]) == v0)
            .unwrap()
            .position
            .map(|value| value * 16.0);
        ([u0, v0, u1, v1], top_left)
    }

    #[test]
    fn arrow_face_without_uv_size_samples_the_whole_shaft() {
        let mut shaft = cube([0.0, -2.5, -3.0], [0.0, 5.0, 16.0], false);
        shaft.uv = EntityGeometryUv::Faces(assets::EntityGeometryFaceUvs {
            east: Some(EntityGeometryFaceUv {
                uv: [scalar(0.0); 2],
                uv_size: None,
            }),
            north: None,
            south: None,
            west: None,
            up: None,
            down: None,
        });
        let vertices = build(&shaft);
        assert_eq!(vertices.len(), 6);
        assert_eq!(region(&vertices).0, [0.0, 0.0, 16.0, 5.0]);
    }

    #[test]
    fn box_uv_faces_follow_the_skin_layout_in_the_mirrored_rig_frame() {
        let vertices = build(&cube([-4.0, 24.0, -4.0], [8.0; 3], false));
        assert_eq!(vertices.len(), 36);
        // Front faces -Z; the model's right side is +X once authored X is mirrored.
        let cases = [
            ([0.0, 0.0, -1.0], [8.0, 8.0, 16.0, 16.0], [4.0, 32.0, -4.0]),
            ([1.0, 0.0, 0.0], [0.0, 8.0, 8.0, 16.0], [4.0, 32.0, 4.0]),
            (
                [-1.0, 0.0, 0.0],
                [16.0, 8.0, 24.0, 16.0],
                [-4.0, 32.0, -4.0],
            ),
            ([0.0, 0.0, 1.0], [24.0, 8.0, 32.0, 16.0], [-4.0, 32.0, 4.0]),
            ([0.0, 1.0, 0.0], [8.0, 0.0, 16.0, 8.0], [4.0, 32.0, 4.0]),
            ([0.0, -1.0, 0.0], [16.0, 0.0, 24.0, 8.0], [4.0, 24.0, 4.0]),
        ];
        for (normal, texels, top_left) in cases {
            let face = face(&vertices, normal);
            assert_eq!(face.len(), 6, "face {normal:?}");
            assert_eq!(region(&face), (texels, top_left), "face {normal:?}");
        }
    }

    #[test]
    fn mirrored_cube_swaps_the_side_regions_and_flips_each_face_horizontally() {
        let vertices = build(&cube([-4.0, 24.0, -4.0], [8.0; 3], true));
        let (right, _) = region(&face(&vertices, [-1.0, 0.0, 0.0]));
        assert_eq!(right, [0.0, 8.0, 8.0, 16.0]);
        let (front, top_left) = region(&face(&vertices, [0.0, 0.0, -1.0]));
        assert_eq!(front, [8.0, 8.0, 16.0, 16.0]);
        assert_eq!(top_left, [-4.0, 32.0, -4.0]);
    }

    #[test]
    fn every_planar_axis_retains_both_authored_quads_with_finite_uvs_and_normals() {
        for axis in 0..3 {
            for mirror in [false, true] {
                let vertices = build(&plane(axis, mirror));
                assert_eq!(vertices.len(), 12);
                assert!((vertices[0].normal[axis].abs() - 1.0).abs() < 1.0e-6);
                assert!(vertices.iter().all(|vertex| {
                    vertex.position[axis] == 0.0
                        && vertex
                            .uv
                            .iter()
                            .chain(vertex.back_uv.iter())
                            .all(|value| value.is_finite())
                }));
                assert!(vertices.iter().all(|vertex| vertex.uv == vertex.back_uv));
                assert_eq!(vertices[6].normal, vertices[0].normal.map(|value| -value));
                let mut equivalent = Vec::new();
                append_entity_cube_vertices(
                    &mut equivalent,
                    &plane(axis, !mirror),
                    0,
                    (64, 64),
                    true,
                    0.0,
                )
                .unwrap();
                assert_eq!(equivalent, vertices);
                let mut rotated_cube = plane(axis, mirror);
                rotated_cube.rotation = [scalar(30.0), scalar(45.0), scalar(60.0)];
                for (before, after) in vertices.iter().zip(build(&rotated_cube)) {
                    assert_eq!(before.uv, after.uv);
                    assert_eq!(before.back_uv, after.back_uv);
                    assert!((Vec3::from_array(after.normal).length() - 1.0).abs() < 0.0001);
                }
            }
        }
    }

    #[test]
    fn authored_rotation_turns_x_and_z_against_the_right_hand_rule() {
        let mut arm = cube([0.0, 0.0, 0.0], [0.0, 4.0, 1.0], false);
        arm.rotation = [scalar(0.0), scalar(0.0), scalar(90.0)];
        // Authored +Z rotation swings +Y toward authored +X, which the rig frame mirrors to -X.
        let top = build(&arm).into_iter().map(|vertex| vertex.position).fold(
            [0.0_f32; 3],
            |best, position| {
                if position[0].abs() > best[0].abs() {
                    position
                } else {
                    best
                }
            },
        );
        assert!(top[0] < -0.2, "{top:?}");
    }

    #[test]
    fn captured_skin_inflated_plane_keeps_its_authored_face() {
        let geometry = assets::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.test"}}"#,
            r#"{"format_version":"1.14.0","minecraft:geometry":[{
                "description":{"identifier":"geometry.test","texture_width":256,"texture_height":256},
                "bones":[{"name":"helmet","cubes":[{
                    "origin":[-6,29.60000038146973,-6],"size":[12,0,12],
                    "pivot":[0,29.60000038146973,0],"inflate":0.6800000071525574,
                    "uv":{"up":{"uv":[13,13],"uv_size":[-12,-12]}}
                }]}]}]}"#,
        )
        .unwrap()
        .unwrap();
        let built = crate::actor::skin_geometry(&geometry, crate::actor::EntityRigId(1))
            .expect("inflation gives the zero-height cube a drawable top face");
        assert_eq!(built.vertices.len(), 6);
        let expected_y = (29.6_f32 + 0.68) / 16.0;
        assert!(built.vertices.iter().all(|vertex| {
            (vertex.position[1] - expected_y).abs() < 1.0e-6 && vertex.normal == [0.0, 1.0, 0.0]
        }));
        assert!(
            built
                .vertices
                .iter()
                .any(|vertex| vertex.uv == [13.0 / 256.0; 2])
        );
        assert!(
            built
                .vertices
                .iter()
                .any(|vertex| vertex.uv == [1.0 / 256.0; 2])
        );
    }

    #[test]
    fn planar_geometry_rejects_lines_points_and_negative_size() {
        let mut cube = plane(0, false);
        for size in [[0.0, 0.0, 3.0], [0.0; 3], [-1.0, 2.0, 3.0]] {
            cube.size = size.map(scalar);
            assert!(
                append_entity_cube_vertices(&mut Vec::new(), &cube, 0, (16, 16), false, 0.0)
                    .is_err()
            );
        }
    }

    // A missing UV omits that authored face; material culling controls the surviving quad.
    #[test]
    fn a_plane_with_one_textured_face_keeps_only_that_authored_quad() {
        let mut cube = plane(0, false);
        let face = || {
            Some(EntityGeometryFaceUv {
                uv: [scalar(0.0); 2],
                uv_size: Some([scalar(4.0), scalar(3.0)]),
            })
        };
        let faces = |east, west| {
            EntityGeometryUv::Faces(assets::EntityGeometryFaceUvs {
                north: None,
                south: None,
                east,
                west,
                up: None,
                down: None,
            })
        };
        for (east, west) in [(face(), None), (None, face())] {
            cube.uv = faces(east, west);
            let mut vertices = Vec::new();
            append_entity_cube_vertices(&mut vertices, &cube, 0, (16, 16), false, 0.0).unwrap();
            assert_eq!(vertices.len(), 6);
            assert!(vertices.iter().all(|vertex| vertex.back_uv == vertex.uv));
        }
        let front = {
            cube.uv = faces(face(), None);
            build(&cube)[0].normal
        };
        cube.uv = faces(None, face());
        assert_eq!(build(&cube)[0].normal, front.map(|value| -value));
        cube.uv = faces(None, None);
        assert!(build(&cube).is_empty());
    }
    #[test]
    fn review_render_cuboid_uvs_agree_at_shared_triangle_vertices() {
        for face in cuboid_vertices([0.0; 3], [1.0; 3], 0).chunks_exact(6) {
            for a in face {
                for b in face {
                    if a.position == b.position {
                        assert_eq!(a.uv, b.uv);
                    }
                }
            }
        }
    }
}
