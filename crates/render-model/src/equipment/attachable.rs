//! Single-bone attachable geometry (trident, shield) placed at the hand item bone.
//!
//! The attachable's model origin is the hand item bone's origin and its own axes follow that
//! bone; each bone turns about its authored pivot by its literal offset, rotation, and scale.

use crate::RenderBoneTransform;
use glam::{Quat, Vec3};

/// A bone's literal channels: offset in pixels, rotation in degrees, per-axis scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoneChannels {
    pub translation: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for BoneChannels {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

/// The bone's pose in the hand item bone's frame: `pivot` is the authored bind pivot (rig frame,
/// blocks), and the offset mirrors authored X. Only expression bindings use the humanoid origin.
pub fn attach(
    hand: RenderBoneTransform,
    pivot: [f32; 3],
    channels: BoneChannels,
    has_binding_expression: bool,
) -> Option<RenderBoneTransform> {
    let [rx, ry, rz, rw] = hand.rotation;
    let hand_rotation = Quat::from_vec4(glam::Vec4::new(rx, ry, rz, rw).try_normalize()?);
    let hand_scale = hand.translation_scale[3] * hand.axis_scale[0];
    let [x, y, z] = channels.translation;
    let offset = Vec3::new(-x, y, z) / 16.0;
    let mut local_pivot = Vec3::from_array(pivot);
    if has_binding_expression {
        local_pivot.y -= assets::gui_item::SHIELD_MODEL_PART_HEIGHT / 16.0;
    }
    let origin = Vec3::new(
        hand.translation_scale[0],
        hand.translation_scale[1],
        hand.translation_scale[2],
    ) + hand_rotation * ((local_pivot + offset) * hand_scale);
    let turned = (hand_rotation * authored_rotation(channels.rotation)).normalize();
    let [sx, sy, sz] = channels.scale;
    let bone = RenderBoneTransform {
        rotation: turned.to_array(),
        translation_scale: [origin.x, origin.y, origin.z, hand_scale],
        axis_scale: [sx, sy, sz, 1.0],
    };
    bone.is_finite().then_some(bone)
}

/// Zyx composition of authored degrees in the X-mirrored rig frame (X and Y turn against the
/// right-hand rule), matching the actor pose evaluator.
pub fn authored_rotation(degrees: [f32; 3]) -> Quat {
    let [x, y, z] = [-degrees[0], -degrees[1], degrees[2]].map(|angle| angle.to_radians() * 0.5);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    Quat::from_xyzw(
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    )
}
