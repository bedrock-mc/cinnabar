//! Boxes of an entity geometry's bone tree in one frame, for block entities drawn from mob
//! geometry: dragon and piglin heads, copper golem statues, beds and spawner mobs.
//!
//! Skull scale and the dragon's jaw pose need native measurement.

use glam::{EulerRot, Mat4, Quat, Vec3};

use super::{
    BoxSpec, ModelFace, SKULL_HAT, SKULL_HAT_INFLATE, SKULL_HEAD, skull_has_hat, skull_texture,
};
use crate::{EntityGeometry, EntityGeometryBone, EntityGeometryCube, EntityGeometryUv};

/// Entity geometry identifiers for standing, sitting, star and running statue poses.
pub const STATUE_GEOMETRIES: [&str; 4] = [
    "geometry.copper_golem",
    "geometry.copper_golem.sitting",
    "geometry.copper_golem.star",
    "geometry.copper_golem.running",
];

/// The body and eye textures shared by placed and carried copper golem statues.
#[must_use]
pub fn statue_textures(age: crate::facts::CopperAge) -> [String; 2] {
    use crate::facts::CopperAge;
    let suffix = match age {
        CopperAge::Unaffected => "",
        CopperAge::Exposed => "_exposed",
        CopperAge::Weathered => "_weathered",
        CopperAge::Oxidized => "_oxidized",
    };
    ["", "_eyes"].map(|layer| format!("textures/entity/copper_golem/copper_golem{layer}{suffix}"))
}

/// One box with the transform that places it in the model frame.
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

/// Where a head drawn from mob geometry comes from: the `head` bone tree of `geometry`, with
/// `texture`, scaled by `scale` to its skull size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobHead {
    pub geometry: &'static str,
    pub texture: &'static str,
    pub scale: f32,
}

/// The mob head a legacy `SkullType` ordinal draws: dragon (5) and piglin (6) heads; the other
/// heads are box models (see [`super::skull_texture`]).
#[must_use]
pub const fn skull_mob_head(skull_type: u8) -> Option<MobHead> {
    Some(match skull_type {
        // The dragon geometry is authored for a full-size mob.
        5 => MobHead {
            geometry: "geometry.dragon",
            texture: "textures/entity/dragon/dragon",
            scale: 0.75,
        },
        6 => MobHead {
            geometry: "geometry.piglin",
            texture: "textures/entity/piglin/piglin",
            scale: 1.0,
        },
        _ => return None,
    })
}

impl MobHead {
    /// The head's boxes from its geometry in `geometries`, bottom on the floor; `None` without
    /// that geometry or a head bone.
    #[must_use]
    pub fn model(self, geometries: &[EntityGeometry]) -> Option<HeadModel> {
        geometries
            .iter()
            .find(|geometry| geometry.identifier.as_ref() == self.geometry)
            .and_then(|geometry| HeadModel::build(geometry, self.scale))
    }
}

/// The model of a head of legacy `SkullType` ordinal `skull_type` in the skull frame, bottom on
/// the floor and front toward -Z, with its pack texture: the head box (and hat) of a box-model
/// head, or the `head` bone tree of the dragon or piglin geometry in `geometries`.
#[must_use]
pub fn skull_model(
    skull_type: u8,
    geometries: &[EntityGeometry],
) -> Option<(HeadModel, &'static str)> {
    if let Some(source) = skull_mob_head(skull_type) {
        return Some((source.model(geometries)?, source.texture));
    }
    let (texture, logical) = skull_texture(skull_type)?;
    let mut boxes = vec![HeadBox {
        matrix: Mat4::IDENTITY,
        spec: SKULL_HEAD.into(),
    }];
    if skull_has_hat(skull_type) {
        boxes.push(HeadBox {
            matrix: Mat4::IDENTITY,
            spec: BoxSpec::from(SKULL_HAT).inflated(SKULL_HAT_INFLATE),
        });
    }
    Some((
        HeadModel {
            boxes,
            texture: logical,
        },
        texture,
    ))
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

fn values(scalars: [crate::EntityGeometryScalar; 3]) -> [f32; 3] {
    scalars.map(crate::EntityGeometryScalar::get)
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
            .find(|candidate| candidate.name.eq_ignore_ascii_case(name))?;
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
        if current.eq_ignore_ascii_case(root) {
            return true;
        }
        name = geometry
            .bones
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(current))
            .and_then(|candidate| candidate.parent.as_deref());
    }
    false
}

fn cube_box(cube: &EntityGeometryCube) -> Option<(BoxSpec, Mat4)> {
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
    .inflated(cube.inflate.get())
    .mirrored(cube.mirror);
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
                let Some((spec, local)) = cube_box(cube) else {
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
                .flat_map(|head_box| head_box.corners().map(|corner| corner.y))
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

impl HeadBox {
    /// The inflated box's faces in the model frame, in [`super::box_faces`] order.
    #[must_use]
    pub fn faces(&self) -> [ModelFace; 6] {
        self.spec.faces().map(|(corners, texels)| {
            (
                corners.map(|corner| {
                    self.matrix
                        .transform_point3(Vec3::from_array(corner))
                        .to_array()
                }),
                texels,
            )
        })
    }

    /// The inflated box's eight corners in the model frame.
    #[must_use]
    pub fn corners(&self) -> [Vec3; 8] {
        let spec = self.spec;
        std::array::from_fn(|corner| {
            let point = Vec3::from_array(std::array::from_fn(|axis| {
                spec.origin[axis]
                    + if corner & (1 << axis) == 0 {
                        -spec.inflate
                    } else {
                        spec.size[axis] + spec.inflate
                    }
            }));
            self.matrix.transform_point3(point)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::EntityGeometryScalar;

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
            locators: Box::default(),
            name: name.into(),
            binding: None,
            texture_meshes: Box::new([]),
            parent: parent.map(Into::into),
            pivot: None,
            rotation: None,
            bind_pose_rotation: None,
            mirror: None,
            inflate: None,
            never_render: None,
            reset: None,
            cubes: cubes.into(),
        }
    }

    fn geometry(bones: Vec<EntityGeometryBone>) -> EntityGeometry {
        EntityGeometry {
            animation_no_head_bob: false,
            visible_bounds: None,
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
    fn mirrored_head_boxes_keep_authored_texels_and_outward_winding() {
        let mut cube = cube([2.0, 7.0, 1.0], [3.0, 5.0, 2.0]);
        cube.uv = EntityGeometryUv::Box([scalar(1.0), scalar(2.0)]);
        cube.mirror = true;
        let model = HeadModel::build_tree(
            &geometry(vec![bone("head", None, vec![cube])]),
            Some("head"),
            1.0,
            false,
        )
        .unwrap();
        let faces = model.boxes[0].faces();
        let (corners, [u, v, width, height]) = faces[0];
        let uv = [
            [u, v],
            [u + width, v],
            [u + width, v + height],
            [u, v + height],
        ];
        let index = corners
            .iter()
            .position(|point| *point == [-5.0, 12.0, 1.0])
            .unwrap();
        assert_eq!(uv[index], [3.0, 4.0]);
        for (corners, _) in faces {
            let [a, b, c, _] = corners.map(Vec3::from_array);
            let normal = (c - a).cross(b - a).normalize();
            let center = Vec3::new(-3.5, 9.5, 2.0);
            assert!(normal.dot(a - center) > 0.0);
        }
    }

    #[test]
    fn review_render_rotated_inflated_head_sits_on_floor() {
        let mut cube = cube([0.0, 1.0, 0.0], [1.0; 3]);
        cube.rotation = [scalar(180.0), scalar(0.0), scalar(0.0)];
        cube.inflate = scalar(0.25);
        let model = HeadModel::build(&geometry(vec![bone("head", None, vec![cube])]), 1.0).unwrap();
        let bottom = model.boxes[0]
            .corners()
            .iter()
            .map(|corner| corner.y)
            .fold(f32::MAX, f32::min);
        assert!(bottom.abs() < 1.0e-5, "bottom {bottom}");
    }
}
