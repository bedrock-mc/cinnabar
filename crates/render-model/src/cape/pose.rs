//! Retargets cape attachments without changing the owning body pose.
use super::*;

/// Retargets animation deltas onto the cape's attachment, independent of the skin's rest markers.
pub(super) fn cape_pose(
    cape: &CapeRig,
    bindings: &[crate::CapeBoneBinding],
    body_rest: &impl Fn(usize) -> Option<RenderBoneTransform>,
    body: &[RenderBoneTransform],
) -> Arc<[RenderBoneTransform]> {
    cape.pose_frames
        .iter()
        .zip(bindings)
        .map(|(frame, binding)| {
            let index = binding.bone;
            let pose = index.and_then(|index| body.get(index).copied());
            match pose {
                Some(mut pose) => {
                    if binding.needs_parent && binding.parent.is_none() {
                        return pose;
                    }
                    let parent_index = binding.parent;
                    let identity = RenderBoneTransform {
                        rotation: Quat::IDENTITY.to_array(),
                        translation_scale: [0.0, 0.0, 0.0, 1.0],
                        axis_scale: crate::UNIT_AXIS_SCALE,
                    };
                    let Some(parent_pose) =
                        parent_index.map_or(Some(&identity), |index| body.get(index))
                    else {
                        return pose;
                    };
                    let parent = Quat::from_array(parent_pose.rotation);
                    let mut local = parent.inverse() * Quat::from_array(pose.rotation);
                    let rest = index.and_then(body_rest);
                    let parent_rest = parent_index.map_or(Some(identity), body_rest);
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
                    let mut attached = frame.bind * local * frame.bind.inverse();
                    if frame.is_cape {
                        attached = bound_cape_rotation(attached);
                    }
                    pose.rotation = (parent * attached).to_array();
                    pose
                }
                None => RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0; 4],
                    axis_scale: crate::UNIT_AXIS_SCALE,
                },
            }
        })
        .collect()
}

/// Limits cape tilt in the parent's frame without changing an already hanging rotation.
fn bound_cape_rotation(rotation: Quat) -> Quat {
    let down = rotation * Vec3::NEG_Y;
    let (sine, cosine) = CAPE_MAX_SWING_DEGREES.to_radians().sin_cos();
    if down.dot(Vec3::NEG_Y) >= cosine {
        return rotation;
    }
    let horizontal = Vec3::new(down.x, 0.0, down.z)
        .try_normalize()
        .unwrap_or(Vec3::Z);
    let bounded = horizontal * sine + Vec3::NEG_Y * cosine;
    // This arc is at most a quarter turn and must retain even tiny corrections near horizontal.
    let cross = down.cross(bounded);
    let correction =
        Quat::from_xyzw(cross.x, cross.y, cross.z, 1.0 + down.dot(bounded)).normalize();
    correction * rotation
}

/// Reads a bone origin in block coordinates.
pub(super) fn position(pose: &RenderBoneTransform) -> Vec3 {
    Vec3::new(
        pose.translation_scale[0],
        pose.translation_scale[1],
        pose.translation_scale[2],
    )
}

/// Combines a bone's uniform and per-axis scale.
fn total_scale(pose: &RenderBoneTransform) -> Vec3 {
    Vec3::new(pose.axis_scale[0], pose.axis_scale[1], pose.axis_scale[2])
        * pose.translation_scale[3]
}

/// The cape pose with its cape bone placed by Java's cape stack instead of the body's pose.
pub(super) fn java_cape_pose(
    cape: &CapeRig,
    bindings: &[crate::CapeBoneBinding],
    body_rest: &impl Fn(usize) -> Option<RenderBoneTransform>,
    body: &[RenderBoneTransform],
    input: &JavaCapeInput,
) -> Arc<[RenderBoneTransform]> {
    let mut pose = cape_pose(cape, bindings, body_rest, body).to_vec();
    for ((frame, bone), pivot) in cape
        .pose_frames
        .iter()
        .zip(&mut pose)
        .zip(cape.geometry.bone_pivots.iter())
    {
        if frame.is_cape {
            let (rotation, translation) = java_cape_bone(input, Vec3::from_array(*pivot));
            *bone = RenderBoneTransform {
                rotation: rotation.to_array(),
                translation_scale: [translation.x, translation.y, translation.z, 1.0],
                axis_scale: crate::UNIT_AXIS_SCALE,
            };
        }
    }
    pose.into()
}
