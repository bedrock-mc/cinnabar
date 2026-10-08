//! Cape geometry, rest-pose retargeting and standard cape rasters.
use std::sync::Arc;

use assets::{CAPE_GEOMETRY_IDENTIFIER, RuntimeEntityAssets};
use bevy::math::{EulerRot, Quat, Vec3};
use render_model::{
    ActorRigGeometry, EntityRigId, RenderBoneTransform, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE,
    entity_geometry, equipment_rig_id, find_geometry_index,
    java_animation::{JavaCapeInput, java_cape_bone},
    resolve_geometry_bones,
};

/// Render layer of a player's cape, below the extra texture layers.
pub const ACTOR_LAYER_CAPE: u8 = 24;

/// The cape geometry and its bone order, resolved once from the entity catalog.
#[derive(Clone)]
pub struct CapeRig {
    pub id: EntityRigId,
    pub geometry: ActorRigGeometry,
    bone_names: Vec<Box<str>>,
    pose_frames: Vec<CapePoseFrame>,
}

#[derive(Clone)]
struct CapePoseFrame {
    parent: Option<Box<str>>,
    bind: Quat,
    pivot_offset: Vec3,
}

fn pose_frames(bones: &[assets::EntityGeometryBone], pivots: &[[f32; 3]]) -> Vec<CapePoseFrame> {
    bones
        .iter()
        .enumerate()
        .map(|(index, bone)| {
            let [x, y, z] = bone.bind_pose_rotation.map_or([0.0; 3], |angles| {
                angles.map(|angle| angle.get().to_radians())
            });
            CapePoseFrame {
                parent: bone.parent.clone(),
                bind: Quat::from_euler(EulerRot::XYZEx, -x, -y, z),
                pivot_offset: Vec3::from_array(pivots[index])
                    - bone
                        .parent
                        .as_ref()
                        .and_then(|parent| {
                            bones
                                .iter()
                                .position(|bone| bone.name.eq_ignore_ascii_case(parent))
                        })
                        .map_or(Vec3::ZERO, |parent| Vec3::from_array(pivots[parent])),
            }
        })
        .collect()
}

impl CapeRig {
    pub fn resolve(assets: &RuntimeEntityAssets) -> Option<Self> {
        let index = find_geometry_index(assets, CAPE_GEOMETRY_IDENTIFIER)?;
        let id = equipment_rig_id(index);
        let bones = resolve_geometry_bones(assets, index as usize).ok()?;
        let geometry = entity_geometry(assets, index as usize, id).ok()?;
        Self::from_geometry(geometry, &bones)
    }

    /// Resolves attachment frames for geometry whose pivots follow the supplied bone order.
    pub fn from_geometry(
        geometry: ActorRigGeometry,
        bones: &[assets::EntityGeometryBone],
    ) -> Option<Self> {
        if geometry.bone_pivots.len() != bones.len() {
            return None;
        }
        let pose_frames = pose_frames(bones, &geometry.bone_pivots);
        Some(Self {
            id: geometry.id,
            geometry,
            bone_names: bones.iter().map(|bone| bone.name.clone()).collect(),
            pose_frames,
        })
    }
}

/// Resamples a cape raster into one standard skin layer; the cape geometry's texture
/// coordinates are normalised, so any cape size maps onto the layer exactly.
pub fn cape_layer(width: u32, height: u32, rgba8: &[u8]) -> Option<Arc<[u8]>> {
    let (width, height) = (width as usize, height as usize);
    let expected = width.checked_mul(height)?.checked_mul(4)?;
    if width == 0 || height == 0 || rgba8.len() != expected {
        return None;
    }
    let side = STANDARD_SKIN_SIDE;
    let mut layer = Vec::with_capacity(STANDARD_SKIN_BYTES);
    for y in 0..side {
        let source_y = y * height / side;
        for x in 0..side {
            let source_x = x * width / side;
            let offset = (source_y * width + source_x) * 4;
            layer.extend_from_slice(&rgba8[offset..offset + 4]);
        }
    }
    Some(layer.into())
}

/// Retargets animation deltas onto the cape's attachment, independent of the skin's rest markers.
/// The rest accessor supplies transforms in the same rendered units as `body`.
pub fn cape_pose(
    cape: &CapeRig,
    body_names: &[Box<str>],
    body_rest: impl Fn(usize) -> Option<RenderBoneTransform>,
    body: &[RenderBoneTransform],
) -> Arc<[RenderBoneTransform]> {
    cape.bone_names
        .iter()
        .zip(&cape.pose_frames)
        .map(|(name, frame)| {
            let index = body_names
                .iter()
                .position(|candidate| candidate.eq_ignore_ascii_case(name));
            let pose = index.and_then(|index| body.get(index).copied());
            match pose {
                Some(mut pose) => {
                    let parent_index = if let Some(parent) = &frame.parent {
                        let Some(index) = body_names
                            .iter()
                            .position(|name| name.eq_ignore_ascii_case(parent))
                        else {
                            return pose;
                        };
                        Some(index)
                    } else {
                        None
                    };
                    let identity = RenderBoneTransform {
                        rotation: Quat::IDENTITY.to_array(),
                        translation_scale: [0.0, 0.0, 0.0, 1.0],
                        axis_scale: render_model::UNIT_AXIS_SCALE,
                    };
                    let Some(parent_pose) =
                        parent_index.map_or(Some(&identity), |index| body.get(index))
                    else {
                        return pose;
                    };
                    let parent = Quat::from_array(parent_pose.rotation);
                    let mut local = parent.inverse() * Quat::from_array(pose.rotation);
                    let rest = index.and_then(&body_rest);
                    let parent_rest = parent_index.map_or(Some(identity), &body_rest);
                    if let (Some(rest), Some(parent_rest)) = (rest, parent_rest) {
                        let scale = total_scale(parent_pose);
                        let rest_scale = total_scale(&parent_rest);
                        if scale.abs().min_element() > f32::EPSILON
                            && rest_scale.abs().min_element() > f32::EPSILON
                        {
                            let animated = parent.inverse()
                                * (position(&pose) - position(parent_pose))
                                / scale;
                            let rest_parent = Quat::from_array(parent_rest.rotation);
                            let origin = rest_parent.inverse()
                                * (position(&rest) - position(&parent_rest))
                                / rest_scale;
                            let attached = position(parent_pose)
                                + parent * ((frame.pivot_offset + animated - origin) * scale);
                            pose.translation_scale[..3].copy_from_slice(&attached.to_array());
                            let rest_local =
                                rest_parent.inverse() * Quat::from_array(rest.rotation);
                            local = rest_local.inverse() * local;
                        }
                    }
                    pose.rotation = (parent * frame.bind * local * frame.bind.inverse()).to_array();
                    pose
                }
                None => RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0; 4],
                    axis_scale: render_model::UNIT_AXIS_SCALE,
                },
            }
        })
        .collect()
}

fn position(pose: &RenderBoneTransform) -> Vec3 {
    Vec3::new(
        pose.translation_scale[0],
        pose.translation_scale[1],
        pose.translation_scale[2],
    )
}

fn total_scale(pose: &RenderBoneTransform) -> Vec3 {
    Vec3::new(pose.axis_scale[0], pose.axis_scale[1], pose.axis_scale[2])
        * pose.translation_scale[3]
}

/// The cape pose with its cape bone placed by Java's cape stack instead of the body's pose.
pub fn java_cape_pose(
    cape: &CapeRig,
    body_names: &[Box<str>],
    body_rest: impl Fn(usize) -> Option<RenderBoneTransform>,
    body: &[RenderBoneTransform],
    input: &JavaCapeInput,
) -> Arc<[RenderBoneTransform]> {
    let mut pose = cape_pose(cape, body_names, body_rest, body).to_vec();
    for ((name, bone), pivot) in cape
        .bone_names
        .iter()
        .zip(&mut pose)
        .zip(cape.geometry.bone_pivots.iter())
    {
        if name.eq_ignore_ascii_case("cape") {
            let (rotation, translation) = java_cape_bone(input, Vec3::from_array(*pivot));
            *bone = RenderBoneTransform {
                rotation: rotation.to_array(),
                translation_scale: [translation.x, translation.y, translation.z, 1.0],
                axis_scale: render_model::UNIT_AXIS_SCALE,
            };
        }
    }
    pose.into()
}

#[cfg(test)]
mod tests {
    use super::cape_layer;

    #[test]
    fn installed_cape_hangs_behind_the_body_with_its_front_raster_outward() {
        use bevy::math::{Quat, Vec3};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/assets/compiled/vanilla-v1.mcbeent");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "skipping installed cape fixture: missing {}; make assets",
                    path.display()
                );
                return;
            }
            Err(error) => panic!("read installed cape fixture {}: {error}", path.display()),
        };
        let assets = assets::RuntimeEntityAssets::decode(&bytes).unwrap();
        let cape = super::CapeRig::resolve(&assets).expect("installed cape geometry");
        let names: Vec<Box<str>> = ["root", "body", "waist", "cape"].map(Into::into).into();
        for tilt in [0.0, 0.45] {
            for flap in [6.0_f32.to_radians(), 0.7] {
                let [body, cape_pose] = fixture_poses(tilt, flap);
                let root = render_model::RenderBoneTransform::from_model_space(
                    [0.0, 0.0, 0.0, 1.0],
                    [0.0, 0.0, 0.0, 1.0],
                )
                .unwrap();
                let rest = [
                    root,
                    fixture_poses(0.0, 0.0)[0],
                    root,
                    fixture_poses(0.0, 0.0)[1],
                ];
                let poses = super::cape_pose(
                    &cape,
                    &names,
                    rest_accessor(&rest),
                    &[root, body, root, cape_pose],
                );
                let index = cape
                    .bone_names
                    .iter()
                    .position(|name| &**name == "cape")
                    .unwrap();
                let rotation = Quat::from_array(poses[index].rotation);
                let parent = Quat::from_rotation_x(tilt);
                let pivot = Vec3::from_array(cape.geometry.bone_pivots[index]);
                let direction = parent * Quat::from_rotation_x(-flap) * Vec3::Z;
                let outward = cape
                    .geometry
                    .vertices
                    .iter()
                    .filter(|vertex| {
                        (rotation * Vec3::from_array(vertex.normal)).dot(direction) > 0.99
                    })
                    .collect::<Vec<_>>();
                assert_eq!(outward.len(), 6);
                assert!(
                    outward.iter().all(|vertex| vertex.uv[0] <= 11.0 / 64.0),
                    "cape exterior must display the front artwork"
                );
                for vertex in cape
                    .geometry
                    .vertices
                    .iter()
                    .filter(|vertex| vertex.position[1] < 0.51)
                {
                    let edge = rotation * (Vec3::from_array(vertex.position) - pivot);
                    assert!(
                        (parent.inverse() * edge).z > 0.0,
                        "cape hangs behind the shoulder"
                    );
                }
            }
        }
    }

    #[test]
    fn cape_rasters_resample_onto_one_skin_layer() {
        let mut cape = vec![0u8; 64 * 32 * 4];
        cape[..4].copy_from_slice(&[9, 8, 7, 255]);
        let layer = cape_layer(64, 32, &cape).unwrap();
        assert_eq!(layer.len(), render_model::STANDARD_SKIN_BYTES);
        assert_eq!(&layer[..4], &[9, 8, 7, 255]);
        let row = render_model::STANDARD_SKIN_SIDE * 4;
        let rows_per_source = render_model::STANDARD_SKIN_SIDE / 32;
        assert_eq!(&layer[row..row + 4], &[9, 8, 7, 255]);
        let next = rows_per_source * row;
        assert_eq!(&layer[next..next + 4], &[0, 0, 0, 0]);
        assert!(cape_layer(64, 32, &cape[1..]).is_none());
    }
    #[test]
    fn review_render_cape_rejects_overflowing_dimensions() {
        assert!(cape_layer(1 << 31, 1 << 31, &[]).is_none());
        assert!(cape_layer(u32::MAX, u32::MAX, &[]).is_none());
    }

    fn fixture_cape() -> super::CapeRig {
        let source = serde_json::json!({
            "format_version":"1.12.0",
            "minecraft:geometry":[{
                "description":{"identifier":"geometry.fixture","texture_width":64,"texture_height":32},
                "bones":[{"name":"body","pivot":[0,24,0]},
                    {"name":"cape","parent":"body","pivot":[0,24,3],"bind_pose_rotation":[0,180,0],
                    "cubes":[{"origin":[-5,8,3],"size":[10,16,1],"uv":[0,0]}]}]
            }]
        });
        let model = assets::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.fixture"}}"#,
            &source.to_string(),
        )
        .unwrap()
        .unwrap();
        let geometry = render_model::skin_geometry(&model, render_model::EntityRigId(1)).unwrap();
        super::CapeRig::from_geometry(geometry, &model.bones).unwrap()
    }

    fn rest_accessor(
        poses: &[render_model::RenderBoneTransform],
    ) -> impl Fn(usize) -> Option<render_model::RenderBoneTransform> + '_ {
        move |index| poses.get(index).copied()
    }

    fn fixture_poses(tilt: f32, flap: f32) -> [render_model::RenderBoneTransform; 2] {
        use bevy::math::Quat;
        let parent = Quat::from_rotation_x(tilt);
        let pose = |rotation: Quat, z: f32| render_model::RenderBoneTransform {
            rotation: rotation.to_array(),
            translation_scale: [0.0, 1.5, z, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        };
        [
            pose(parent, 0.0),
            pose(parent * Quat::from_rotation_x(flap), 3.0 / 16.0),
        ]
    }

    #[test]
    fn slim_avatar_cape_uses_the_cape_models_shoulder_attachment() {
        let cape = fixture_cape();
        let mut body = fixture_poses(0.0, 0.1);
        body[1].translation_scale[2] = -3.0 / 16.0;
        let mut rest = fixture_poses(0.0, 0.0);
        rest[1].translation_scale[2] = -3.0 / 16.0;
        let poses = super::cape_pose(
            &cape,
            &["body".into(), "cape".into()],
            rest_accessor(&rest),
            &body,
        );
        assert!(
            poses[1].translation_scale[2] > body[0].translation_scale[2],
            "a slim avatar's cape must attach behind the torso, not across its chest"
        );
    }

    #[test]
    fn cape_outer_face_keeps_the_front_raster_under_parent_tilt() {
        use bevy::math::{Quat, Vec3};
        let cape = fixture_cape();
        for tilt in [0.0, 0.45] {
            let parent = Quat::from_rotation_x(tilt);
            let poses = super::cape_pose(
                &cape,
                &["body".into(), "cape".into()],
                rest_accessor(&fixture_poses(0.0, 0.0)),
                &fixture_poses(tilt, 0.0),
            );
            let rotation = Quat::from_array(poses[1].rotation);
            let outward = cape
                .geometry
                .vertices
                .iter()
                .filter(|vertex| {
                    (rotation * Vec3::from_array(vertex.normal)).dot(parent * Vec3::Z) > 0.99
                })
                .collect::<Vec<_>>();
            assert_eq!(outward.len(), 6);
            assert!(
                outward.iter().all(|vertex| vertex.uv[0] <= 11.0 / 64.0),
                "outward face must sample the front strip, not the inside strip"
            );
        }
    }

    #[test]
    fn unresolved_parent_keeps_the_completed_pose() {
        let cape = fixture_cape();
        let body = fixture_poses(0.45, 0.7);
        let poses = super::cape_pose(&cape, &["cape".into()], |_| None, &body[1..]);
        assert_eq!(poses[1].rotation, body[1].rotation);
        assert_eq!(poses[1].translation_scale, body[1].translation_scale);
    }

    #[test]
    fn mixed_axis_bind_uses_mesh_rotation_order() {
        use bevy::math::{Quat, Vec3};
        let mut bone: assets::EntityGeometryBone = serde_json::from_value(serde_json::json!({
            "name":"cape", "cubes":[]
        }))
        .unwrap();
        bone.bind_pose_rotation =
            Some([25.0, 50.0, 15.0].map(|angle| assets::EntityGeometryScalar::new(angle).unwrap()));
        let frame = super::pose_frames(&[bone], &[[0.0; 3]]);
        let expected = Quat::from_rotation_z(15.0_f32.to_radians())
            * Quat::from_rotation_y(-50.0_f32.to_radians())
            * Quat::from_rotation_x(-25.0_f32.to_radians());
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            assert!((frame[0].bind * axis - expected * axis).length() < 1e-5);
        }
    }

    #[test]
    fn moving_cape_trails_behind_the_body_without_reversing_parent_tilt() {
        use bevy::math::{Quat, Vec3};
        let cape = fixture_cape();
        for tilt in [0.0, 0.45] {
            for flap in [0.2, 0.7, 1.4, 2.3] {
                let body = fixture_poses(tilt, flap);
                let poses = super::cape_pose(
                    &cape,
                    &["body".into(), "cape".into()],
                    rest_accessor(&fixture_poses(0.0, 0.0)),
                    &body,
                );
                for (actual, expected) in poses[1]
                    .translation_scale
                    .iter()
                    .zip(body[1].translation_scale)
                {
                    assert!(
                        (actual - expected).abs() < 1e-6,
                        "shoulder hinge stays attached"
                    );
                }
                let rotation = Quat::from_array(poses[1].rotation);
                let parent = Quat::from_rotation_x(tilt);
                let pivot = Vec3::from_array(cape.geometry.bone_pivots[1]);
                for vertex in cape
                    .geometry
                    .vertices
                    .iter()
                    .filter(|vertex| vertex.position[1] < 0.51)
                {
                    let from_hinge = rotation * (Vec3::from_array(vertex.position) - pivot);
                    assert!(
                        (parent.inverse() * from_hinge).z > 0.0,
                        "cape lower edge must swing behind the shoulder, not over the chest"
                    );
                }
            }
        }
    }

    #[test]
    fn cape_retargeting_preserves_neck_motion_under_parent_rotation_and_scale() {
        use bevy::math::{Quat, Vec3};
        let cape = fixture_cape();
        let parent = Quat::from_rotation_x(0.45);
        let scale = Vec3::new(1.1, 0.8, 1.3);
        let origin = Vec3::new(0.2, 1.4, -0.1);
        let motion = Vec3::new(0.0, 0.04, 0.0);
        let expected = origin + parent * ((Vec3::new(0.0, 0.0, 3.0 / 16.0) + motion) * scale);
        for rest_z in [-3.0 / 16.0, 3.0 / 16.0] {
            let mut rest = fixture_poses(0.0, 0.0);
            rest[1].translation_scale[2] = rest_z;
            let mut body = fixture_poses(0.45, 0.7);
            body[0].translation_scale[..3].copy_from_slice(&origin.to_array());
            body[0].axis_scale[..3].copy_from_slice(&scale.to_array());
            body[1].axis_scale[..3].copy_from_slice(&scale.to_array());
            let source = origin + parent * ((Vec3::new(0.0, 0.0, rest_z) + motion) * scale);
            body[1].translation_scale[..3].copy_from_slice(&source.to_array());
            let poses = super::cape_pose(
                &cape,
                &["body".into(), "cape".into()],
                rest_accessor(&rest),
                &body,
            );
            assert!((super::position(&poses[1]) - expected).length() < 1e-6);
            assert_eq!(poses[1].axis_scale, body[1].axis_scale);
        }
    }
}
