//! Elytra wings posed from the attachable's literal animations, riding the body bone.

use assets::{AttachablePose, EquipmentBinding};
use bevy::math::{Quat, Vec3};
use render::{RenderBoneTransform, equipment::authored_rotation as rotation};

use super::armor::hidden_bone;

/// The literal pose an elytra takes for the wearer's stance. Gliding and swimming poses are
/// Molang-driven, so they fall back to the default pose until measured.
pub(super) fn stance_pose(
    binding: &EquipmentBinding,
    sneaking: bool,
    sleeping: bool,
) -> Option<&AttachablePose> {
    let key = if sleeping {
        "sleeping"
    } else if sneaking {
        "sneaking"
    } else {
        "default"
    };
    binding.pose(key).or_else(|| binding.pose("default"))
}

/// Poses the elytra bones (`names` in geometry order) from the body bone's pose: `body` follows
/// it (with its own scale), each other bone hangs off `body` at its literal offset.
pub(super) fn pose(
    names: &[Box<str>],
    pose: &AttachablePose,
    body: RenderBoneTransform,
) -> Vec<RenderBoneTransform> {
    let channel = |name: &str| {
        pose.bones
            .iter()
            .find(|bone| bone.bone.eq_ignore_ascii_case(name))
    };
    let scale_of = |bone: Option<&assets::AttachablePoseBone>| {
        bone.and_then(|bone| bone.scale)
            .map_or([1.0; 3], |scale| scale.map(|value| value.get()))
    };
    let [qx, qy, qz, qw] = body.rotation;
    let body_rotation = Quat::from_xyzw(qx, qy, qz, qw).normalize();
    let body_scale = body.translation_scale[3] * body.axis_scale[0];
    let body_origin = Vec3::new(
        body.translation_scale[0],
        body.translation_scale[1],
        body.translation_scale[2],
    );
    let own_body_scale = scale_of(channel("body"));
    names
        .iter()
        .map(|name| {
            if name.eq_ignore_ascii_case("body") {
                let mut posed = body;
                posed.translation_scale[3] = body_scale * own_body_scale[0];
                posed.axis_scale = render::UNIT_AXIS_SCALE;
                return posed;
            }
            let Some(bone) = channel(name) else {
                return hidden_bone();
            };
            // Offsets are pixels with authored X mirrored; the parent pivot equals the child's.
            let offset = bone.translation.map_or(Vec3::ZERO, |value| {
                let [x, y, z] = value.map(|value| value.get());
                Vec3::new(-x, y, z) / 16.0
            });
            let parent_scale = body_scale * own_body_scale[0];
            let origin = body_origin + body_rotation * (offset * parent_scale);
            let turn = bone.rotation.map_or(Quat::IDENTITY, |value| {
                rotation(value.map(|value| value.get()))
            });
            let axis = scale_of(Some(bone));
            RenderBoneTransform {
                rotation: (body_rotation * turn).normalize().to_array(),
                translation_scale: [origin.x, origin.y, origin.z, parent_scale],
                axis_scale: [axis[0], axis[1], axis[2], 1.0],
            }
        })
        .collect()
}
