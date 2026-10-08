use super::*;
use assets::{EntityGeometry, EntityGeometryBone, EntityGeometryUv};

fn vector(values: [assets::EntityGeometryScalar; 3]) -> [f32; 3] {
    values.map(|v| v.get())
}

pub(super) fn validated_geometry(
    geometry: &EntityGeometry,
    identity: [u8; 32],
) -> Option<ViewmodelGeometry> {
    if identity == [0; 32]
        || geometry.inherits.is_some()
        || u32::from(geometry.texture_width) != VIEWMODEL_TEXTURE_SIDE
        || u32::from(geometry.texture_height) != VIEWMODEL_TEXTURE_SIDE
    {
        return None;
    }
    let bone = |name: &str| {
        geometry
            .bones
            .iter()
            .find(|bone| bone.name.as_ref() == name)
    };
    for (name, parent, pivot) in [
        ("root", None, [0., 0., 0.]),
        ("waist", Some("root"), [0., 12., 0.]),
        ("body", Some("waist"), [0., 24., 0.]),
        ("rightArm", Some("body"), [-5., 22., 0.]),
        ("rightSleeve", Some("rightArm"), [-5., 22., 0.]),
    ] {
        let b = bone(name)?;
        if b.parent.as_deref() != parent
            || b.pivot.map(vector) != Some(pivot)
            || b.rotation.is_some_and(|r| vector(r) != [0.; 3])
            || b.mirror == Some(true)
            || b.inflate.is_some_and(|i| i.get() != 0.)
            || b.never_render == Some(true)
            || b.reset == Some(true)
            || b.binding.is_some()
            || !b.texture_meshes.is_empty()
        {
            return None;
        }
    }
    let mut vertices = Vec::with_capacity(72);
    append_arm(&mut vertices, bone("rightArm")?, [40., 16.], 0.)?;
    append_arm(&mut vertices, bone("rightSleeve")?, [40., 32.], 0.25)?;
    Some(ViewmodelGeometry {
        vertices: vertices.into(),
        identity,
        allowed_rigs: Arc::from([]),
        cube_origin: false,
    })
}

pub(super) fn neutral_arm_transform() -> Mat4 {
    // Column vectors: camera anchor, facing conversion, authored actor scale,
    // model origin, model units, then inherited arm-local pose. Apply each once.
    let u = 1.0 / 16.0;
    let [x, y, z] = [95.0_f32, -45.0, 115.0].map(f32::to_radians);
    Mat4::from_translation(Vec3::new(0., -f32::from_bits(0x3fcf5c7d), 0.))
        * Mat4::from_rotation_y(-180.0_f32.to_radians())
        * Mat4::from_scale(Vec3::new(-1., -1., 1.))
        * Mat4::from_scale(Vec3::splat(0.9375))
        * Mat4::from_translation(Vec3::new(0., -24. * u - 1. / 128., 0.))
        * Mat4::from_scale(Vec3::splat(u))
        * Mat4::from_translation(Vec3::new(8.5, 12., 12.))
        * Mat4::from_rotation_z(z)
        * Mat4::from_rotation_y(y)
        * Mat4::from_rotation_x(x)
}

fn append_arm(
    vertices: &mut Vec<HandVertex>,
    bone: &EntityGeometryBone,
    uv: [f32; 2],
    inflate: f32,
) -> Option<()> {
    let [cube] = bone.cubes.as_ref() else {
        return None;
    };
    if vector(cube.origin) != [-8., 12., -2.]
        || vector(cube.size) != [4., 12., 4.]
        || vector(cube.rotation) != [0.; 3]
        || cube.inflate.get() != inflate
        || cube.mirror
        || !matches!(&cube.uv, EntityGeometryUv::Box(value) if value.map(|v| v.get()) == uv)
    {
        return None;
    }
    let transform = neutral_arm_transform();
    let min = [-3. - inflate, -2. - inflate, -2. - inflate];
    let max = [1. + inflate, 10. + inflate, 2. + inflate];
    let corners = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    // UV top follows reflected authored Y, independently of outward winding.
    let faces = [
        [3, 2, 1, 0],
        [6, 7, 4, 5],
        [7, 3, 0, 4],
        [2, 6, 5, 1],
        [7, 6, 2, 3],
        [0, 1, 5, 4],
    ];
    let [u, v] = uv;
    let rects = [
        [u + 4., v + 4., 4., 12.],
        [u + 12., v + 4., 4., 12.],
        [u + 8., v + 4., 4., 12.],
        [u, v + 4., 4., 12.],
        [u + 4., v, 4., 4.],
        [u + 8., v, 4., 4.],
    ];
    for (face, [left, top, width, height]) in faces.into_iter().zip(rects) {
        let uv = [
            [left, top],
            [left + width, top],
            [left + width, top + height],
            [left, top + height],
        ];
        for i in [0, 1, 2, 0, 2, 3] {
            vertices.push(HandVertex {
                position: transform
                    .transform_point3(Vec3::from(corners[face[i]]))
                    .to_array(),
                uv: uv[i].map(|value| value / VIEWMODEL_TEXTURE_SIDE as f32),
            });
        }
    }
    Some(())
}
