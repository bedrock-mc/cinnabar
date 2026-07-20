use assets::{EntityGeometryCube, EntityGeometryFaceUv, EntityGeometryUv};
use bevy::math::Vec3;

use super::rig::{ActorRigGeometryError, ActorRigVertex};

pub(super) fn append_entity_cube_vertices(
    vertices: &mut Vec<ActorRigVertex>,
    cube: &EntityGeometryCube,
    bone_index: u32,
    texture_size: (u16, u16),
    bone_mirror: bool,
    bone_inflate: f32,
) -> Result<(), ActorRigGeometryError> {
    let origin = cube.origin.map(|value| value.get());
    let size = cube.size.map(|value| value.get());
    let inflate = cube.inflate.get() + bone_inflate;
    if origin
        .iter()
        .chain(size.iter())
        .chain([inflate].iter())
        .any(|value| !value.is_finite())
        || size.iter().any(|value| *value <= 0.0)
        || texture_size.0 == 0
        || texture_size.1 == 0
    {
        return Err(ActorRigGeometryError::InvalidAssetGeometry);
    }
    let min = std::array::from_fn(|axis| (origin[axis] - inflate) / 16.0);
    let max = std::array::from_fn(|axis| (origin[axis] + size[axis] + inflate) / 16.0);
    let mut corners = cuboid_corners(min, max);
    let pivot = cube.pivot.map(|value| value.get() / 16.0);
    let rotation = cube.rotation.map(|value| value.get());
    if rotation.iter().any(|value| *value != 0.0) {
        for corner in &mut corners {
            *corner = rotate_euler_around(*corner, pivot, rotation)
                .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?;
        }
    }
    let mirror = cube.mirror ^ bone_mirror;
    let face_uvs = entity_face_uvs(&cube.uv, size, texture_size, mirror)?;
    let faces = [
        [0, 2, 1, 0, 3, 2],
        [5, 6, 4, 4, 6, 7],
        [4, 3, 0, 4, 7, 3],
        [1, 2, 5, 5, 2, 6],
        [3, 7, 2, 2, 7, 6],
        [4, 0, 5, 5, 0, 1],
    ];
    for (face, uv) in faces.into_iter().zip(face_uvs) {
        let Some(uv) = uv else {
            continue;
        };
        let indices = if mirror {
            [face[0], face[2], face[1], face[3], face[5], face[4]]
        } else {
            face
        };
        let face_uv = [uv[0], uv[2], uv[1], uv[0], uv[3], uv[2]];
        let normal = triangle_normal(
            corners[indices[0]],
            corners[indices[1]],
            corners[indices[2]],
        );
        vertices.extend(
            indices
                .into_iter()
                .zip(face_uv)
                .map(|(corner, uv)| ActorRigVertex {
                    position: corners[corner],
                    normal,
                    uv,
                    bone_index,
                }),
        );
    }
    Ok(())
}

type FaceUvQuad = [[f32; 2]; 4];

fn entity_face_uvs(
    uv: &EntityGeometryUv,
    size: [f32; 3],
    texture_size: (u16, u16),
    mirror: bool,
) -> Result<[Option<FaceUvQuad>; 6], ActorRigGeometryError> {
    let (width, height) = (f32::from(texture_size.0), f32::from(texture_size.1));
    let quad = |origin: [f32; 2], dimensions: [f32; 2]| {
        let mut left = origin[0] / width;
        let mut right = (origin[0] + dimensions[0]) / width;
        if mirror {
            std::mem::swap(&mut left, &mut right);
        }
        let top = origin[1] / height;
        let bottom = (origin[1] + dimensions[1]) / height;
        [[left, top], [right, top], [right, bottom], [left, bottom]]
    };
    let result = match uv {
        EntityGeometryUv::Box(origin) => {
            let [u, v] = origin.map(|value| value.get());
            let [x, y, z] = size;
            [
                Some(quad([u + z, v + z], [x, y])),
                Some(quad([u + z + x + z, v + z], [x, y])),
                Some(quad([u + z + x, v + z], [z, y])),
                Some(quad([u, v + z], [z, y])),
                Some(quad([u + z, v], [x, z])),
                Some(quad([u + z + x, v], [x, z])),
            ]
        }
        EntityGeometryUv::Faces(faces) => [
            face_uv_quad(faces.north.as_ref(), &quad),
            face_uv_quad(faces.south.as_ref(), &quad),
            face_uv_quad(faces.west.as_ref(), &quad),
            face_uv_quad(faces.east.as_ref(), &quad),
            face_uv_quad(faces.up.as_ref(), &quad),
            face_uv_quad(faces.down.as_ref(), &quad),
        ],
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
    Ok(result)
}

fn face_uv_quad(
    face: Option<&EntityGeometryFaceUv>,
    quad: &impl Fn([f32; 2], [f32; 2]) -> FaceUvQuad,
) -> Option<FaceUvQuad> {
    face.map(|face| {
        quad(
            face.uv.map(|value| value.get()),
            face.uv_size
                .map_or([1.0, 1.0], |size| size.map(|value| value.get())),
        )
    })
}

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

pub(super) fn cuboid_vertices(
    min: [f32; 3],
    max: [f32; 3],
    bone_index: u32,
) -> Vec<ActorRigVertex> {
    let corners = cuboid_corners(min, max);
    let faces = [
        [0, 2, 1, 0, 3, 2],
        [5, 6, 4, 4, 6, 7],
        [4, 3, 0, 4, 7, 3],
        [1, 2, 5, 5, 2, 6],
        [3, 7, 2, 2, 7, 6],
        [4, 0, 5, 5, 0, 1],
    ];
    let uv = [
        [0.0, 0.0],
        [1.0, 1.0],
        [1.0, 0.0],
        [0.0, 0.0],
        [0.0, 1.0],
        [1.0, 1.0],
    ];
    faces
        .into_iter()
        .flat_map(|face| {
            let normal = triangle_normal(corners[face[0]], corners[face[1]], corners[face[2]]);
            face.into_iter()
                .zip(uv)
                .map(move |(corner, uv)| ActorRigVertex {
                    position: corners[corner],
                    normal,
                    uv,
                    bone_index,
                })
        })
        .collect()
}

pub(super) fn triangle_normal(first: [f32; 3], second: [f32; 3], third: [f32; 3]) -> [f32; 3] {
    let left = Vec3::from_array(second) - Vec3::from_array(first);
    let right = Vec3::from_array(third) - Vec3::from_array(first);
    left.cross(right).normalize_or_zero().to_array()
}
