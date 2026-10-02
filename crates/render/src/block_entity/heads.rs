//! Dragon and piglin heads, built from the pack's entity geometry rather than authored here.
//!
//! The head bone and its descendants become boxes in the skull frame, bottom on the block
//! floor. Skull scale and the dragon's jaw pose need native measurement.

use assets::{
    EntityGeometry, EntityGeometryBone, EntityGeometryCube, EntityGeometryUv, RuntimeEntityAssets,
};
use bevy::math::{EulerRot, Mat4, Quat, Vec3};

use super::mesh::BoxSpec;

/// Skull scale for the dragon head; the geometry is authored for a full-size mob.
const DRAGON_SKULL_SCALE: f32 = 0.75;

/// One head box with the transform that places it in the skull frame.
#[derive(Clone, Debug, PartialEq)]
pub struct HeadBox {
    pub matrix: Mat4,
    pub spec: BoxSpec,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeadModel {
    pub boxes: Vec<HeadBox>,
    /// Texture size the geometry's UVs were authored against.
    pub texture: [f32; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadModels {
    pub piglin: Option<HeadModel>,
    pub dragon: Option<HeadModel>,
    /// Copper golem statue poses: standing, sitting, star, running.
    pub statues: [Option<HeadModel>; 4],
}

/// Entity geometry identifiers for the statue poses, in [`HeadModels::statues`] order.
const STATUE_GEOMETRIES: [&str; 4] = [
    "geometry.copper_golem",
    "geometry.copper_golem.sitting",
    "geometry.copper_golem.star",
    "geometry.copper_golem.running",
];

impl HeadModels {
    #[must_use]
    pub fn from_assets(assets: &RuntimeEntityAssets) -> Self {
        let find = |identifier: &str| {
            assets
                .geometries()
                .iter()
                .find(|geometry| geometry.identifier.as_ref() == identifier)
        };
        Self {
            piglin: find("geometry.piglin").and_then(|geometry| HeadModel::build(geometry, 1.0)),
            dragon: find("geometry.dragon")
                .and_then(|geometry| HeadModel::build(geometry, DRAGON_SKULL_SCALE)),
            statues: STATUE_GEOMETRIES.map(|identifier| {
                find(identifier)
                    .and_then(|geometry| HeadModel::build_tree(geometry, None, 1.0, false))
            }),
        }
    }
}

/// Rotation about a pivot in the rig frame, where authored X is mirrored: rig Euler angles
/// are `(-x, -y, z)` of the authored ones.
fn pivoted(pivot: [f32; 3], rotation: [f32; 3]) -> Mat4 {
    let pivot = Vec3::new(-pivot[0], pivot[1], pivot[2]);
    let turn = Quat::from_euler(
        EulerRot::ZYX,
        rotation[2].to_radians(),
        (-rotation[1]).to_radians(),
        (-rotation[0]).to_radians(),
    );
    Mat4::from_translation(pivot) * Mat4::from_quat(turn) * Mat4::from_translation(-pivot)
}

fn values(scalars: [assets::EntityGeometryScalar; 3]) -> [f32; 3] {
    scalars.map(assets::EntityGeometryScalar::get)
}

fn bone_local(bone: &EntityGeometryBone) -> Mat4 {
    match (bone.rotation, bone.pivot) {
        (Some(rotation), Some(pivot)) => pivoted(values(pivot), values(rotation)),
        _ => Mat4::IDENTITY,
    }
}

/// Transform from a bone's frame to the geometry root, or `None` on a parent cycle.
fn chain(geometry: &EntityGeometry, bone: &EntityGeometryBone) -> Option<Mat4> {
    let mut matrix = bone_local(bone);
    let mut parent = bone.parent.as_deref();
    for _ in 0..geometry.bones.len() {
        let Some(name) = parent else {
            return Some(matrix);
        };
        let ancestor = geometry
            .bones
            .iter()
            .find(|candidate| candidate.name.as_ref() == name)?;
        matrix = bone_local(ancestor) * matrix;
        parent = ancestor.parent.as_deref();
    }
    None
}

fn descends_from(geometry: &EntityGeometry, bone: &EntityGeometryBone, root: &str) -> bool {
    let mut name = Some(bone.name.as_ref());
    for _ in 0..=geometry.bones.len() {
        let Some(current) = name else {
            return false;
        };
        if current == root {
            return true;
        }
        name = geometry
            .bones
            .iter()
            .find(|candidate| candidate.name.as_ref() == current)
            .and_then(|candidate| candidate.parent.as_deref());
    }
    false
}

fn cube_box(cube: &EntityGeometryCube, bone_inflate: f32) -> Option<(BoxSpec, Mat4)> {
    let EntityGeometryUv::Box(uv) = &cube.uv else {
        return None;
    };
    let [ox, oy, oz] = values(cube.origin);
    let [sx, sy, sz] = values(cube.size);
    let spec = BoxSpec::new(
        [-(ox + sx), oy, oz],
        [sx, sy, sz],
        uv.map(|value| value.get()),
    )
    .inflated(cube.inflate.get() + bone_inflate);
    let rotation = values(cube.rotation);
    let local = if rotation == [0.0; 3] {
        Mat4::IDENTITY
    } else {
        pivoted(values(cube.pivot), rotation)
    };
    Some((spec, local))
}

impl HeadModel {
    /// Boxes of the `head` bone tree, bottom on the floor; `None` when there are none.
    #[must_use]
    pub fn build(geometry: &EntityGeometry, scale: f32) -> Option<Self> {
        Self::build_tree(geometry, Some("head"), scale, true)
    }

    /// Boxes of `root` and its descendants (every drawn bone when `root` is `None`), scaled about
    /// the origin, optionally sitting on the floor; `None` when there are no boxes.
    #[must_use]
    pub fn build_tree(
        geometry: &EntityGeometry,
        root: Option<&str>,
        scale: f32,
        align_floor: bool,
    ) -> Option<Self> {
        let mut boxes = Vec::new();
        for bone in geometry.bones.iter().filter(|bone| {
            bone.never_render != Some(true)
                && root.is_none_or(|root| descends_from(geometry, bone, root))
        }) {
            let Some(bone_matrix) = chain(geometry, bone) else {
                continue;
            };
            for cube in &bone.cubes {
                // Per-face UV cubes are not expressible as box UVs and are skipped.
                let Some((spec, local)) =
                    cube_box(cube, bone.inflate.map_or(0.0, |value| value.get()))
                else {
                    continue;
                };
                boxes.push(HeadBox {
                    matrix: bone_matrix * local,
                    spec,
                });
            }
        }
        let floor = if align_floor {
            boxes
                .iter()
                .flat_map(|head_box| {
                    let spec = head_box.spec;
                    (0..8).map(move |corner| {
                        let point = Vec3::from_array(std::array::from_fn(|axis| {
                            spec.origin[axis]
                                + if corner & (1 << axis) == 0 {
                                    -spec.inflate
                                } else {
                                    spec.size[axis] + spec.inflate
                                }
                        }));
                        head_box.matrix.transform_point3(point).y
                    })
                })
                .fold(f32::MAX, f32::min)
        } else {
            0.0
        };
        if boxes.is_empty() || !floor.is_finite() {
            return None;
        }
        let align = Mat4::from_scale(Vec3::splat(scale))
            * Mat4::from_translation(Vec3::new(0.0, -floor, 0.0));
        for head_box in &mut boxes {
            head_box.matrix = align * head_box.matrix;
        }
        Some(Self {
            boxes,
            texture: [
                f32::from(geometry.texture_width),
                f32::from(geometry.texture_height),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use assets::EntityGeometryScalar;

    use super::*;

    fn scalar(value: f32) -> EntityGeometryScalar {
        EntityGeometryScalar::new(value).unwrap()
    }

    fn cube(origin: [f32; 3], size: [f32; 3]) -> EntityGeometryCube {
        EntityGeometryCube {
            origin: origin.map(scalar),
            size: size.map(scalar),
            pivot: [scalar(0.0); 3],
            rotation: [scalar(0.0); 3],
            uv: EntityGeometryUv::Box([scalar(0.0); 2]),
            inflate: scalar(0.0),
            mirror: false,
        }
    }

    fn bone(
        name: &str,
        parent: Option<&str>,
        cubes: Vec<EntityGeometryCube>,
    ) -> EntityGeometryBone {
        EntityGeometryBone {
            name: name.into(),
            binding: None,
            texture_meshes: Box::new([]),
            parent: parent.map(Into::into),
            pivot: None,
            rotation: None,
            mirror: None,
            inflate: None,
            never_render: None,
            reset: None,
            cubes: cubes.into(),
        }
    }

    fn geometry(bones: Vec<EntityGeometryBone>) -> EntityGeometry {
        EntityGeometry {
            identifier: "geometry.test".into(),
            inherits: None,
            source_index: 0,
            texture_width: 64,
            texture_height: 64,
            bones: bones.into(),
        }
    }

    #[test]
    fn head_boxes_sit_on_the_floor_and_mirror_authored_x() {
        let model = HeadModel::build(
            &geometry(vec![
                bone(
                    "body",
                    None,
                    vec![cube([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0])],
                ),
                bone(
                    "head",
                    Some("body"),
                    vec![cube([-5.0, 24.0, -4.0], [10.0, 8.0, 8.0])],
                ),
                bone(
                    "ear",
                    Some("head"),
                    vec![cube([5.0, 28.0, 0.0], [2.0, 4.0, 1.0])],
                ),
            ]),
            1.0,
        )
        .unwrap();
        // The body is excluded; head and ear remain, with the lowest box on y = 0.
        assert_eq!(model.boxes.len(), 2);
        let head = &model.boxes[0];
        assert_eq!(head.spec.origin, [-5.0, 24.0, -4.0]);
        let bottom = head.matrix.transform_point3(Vec3::new(0.0, 24.0, 0.0));
        assert!(bottom.y.abs() < 1.0e-5);
        // Authored +X (5..7) lands on rig -X (-7..-5).
        assert_eq!(model.boxes[1].spec.origin[0], -7.0);
        assert_eq!(model.texture, [64.0, 64.0]);
    }

    #[test]
    fn geometry_without_a_head_bone_has_no_model() {
        assert!(
            HeadModel::build(
                &geometry(vec![bone("body", None, vec![cube([0.0; 3], [1.0; 3])])]),
                1.0
            )
            .is_none()
        );
    }
    #[test]
    fn review_render_rotated_inflated_head_sits_on_floor() {
        let mut cube = cube([0.0, 1.0, 0.0], [1.0; 3]);
        cube.rotation = [scalar(180.0), scalar(0.0), scalar(0.0)];
        cube.inflate = scalar(0.25);
        let model = HeadModel::build(&geometry(vec![bone("head", None, vec![cube])]), 1.0).unwrap();
        let head = &model.boxes[0];
        let mut bottom = f32::MAX;
        for x in [
            head.spec.origin[0] - head.spec.inflate,
            head.spec.origin[0] + head.spec.size[0] + head.spec.inflate,
        ] {
            for y in [
                head.spec.origin[1] - head.spec.inflate,
                head.spec.origin[1] + head.spec.size[1] + head.spec.inflate,
            ] {
                for z in [
                    head.spec.origin[2] - head.spec.inflate,
                    head.spec.origin[2] + head.spec.size[2] + head.spec.inflate,
                ] {
                    bottom = bottom.min(head.matrix.transform_point3(Vec3::new(x, y, z)).y);
                }
            }
        }
        assert!(bottom.abs() < 1.0e-5, "bottom {bottom}");
    }
}
