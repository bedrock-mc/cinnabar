//! Ordinary player attacks, composed in model space so children share their parent's grip.
use super::*;
use assets::facts::VanillaItem;
use protocol_ids::ActorFlag;
use render_api::{BEDROCK_112, CombatVisuals};
use render_model::legacy_combat::legacy_swing_pose;

const BOW: &str = VanillaItem::Bow.identifier();
const CROSSBOW: &str = VanillaItem::Crossbow.identifier();
const EMPTY_MAP: &str = VanillaItem::EmptyMap.identifier();
const FILLED_MAP: &str = assets::facts::FILLED_MAP_IDENTIFIER;

#[cfg(test)]
mod pack_tests;
#[cfg(test)]
mod tests;

/// Leaves item-use and special movement postures on their authored animation path.
pub(super) fn ordinary(actor: &dyn AnimatedActor, context: &ActorTickContext, swim: f32) -> bool {
    context.combat_visuals == CombatVisuals::Legacy112
        && matches!(actor.kind(), ActorKind::Player { .. })
        && context.attachable.is_none()
        && !actor.is_using_item()
        && !actor.is_sleeping()
        && swim <= 0.0
        && !context.main_hand_is_spear
        && ![
            ActorFlag::Swimming.wire_value(),
            ActorFlag::Blocking.wire_value(),
            ActorFlag::Gliding.wire_value(),
            ActorFlag::Crawling.wire_value(),
            ActorFlag::Emoting.wire_value(),
        ]
        .into_iter()
        .any(|flag| query::actor_flag(actor, flag))
        && !context
            .main_hand
            .as_deref()
            .is_some_and(|item| matches!(item, BOW | CROSSBOW | FILLED_MAP | EMPTY_MAP))
        && !context
            .off_hand
            .as_deref()
            .is_some_and(|item| matches!(item, FILLED_MAP | EMPTY_MAP))
}

/// Attack changes retained by named drivers until their destination skeleton is composed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Delta {
    rotation: [f32; 3],
    translation: [f32; 3],
}

impl Delta {
    /// Retains exact channel identity, including signed zero, when composing a cached pose.
    pub(super) fn same_bits(self, other: Self) -> bool {
        let Self {
            rotation,
            translation,
        } = self;
        rotation.map(f32::to_bits) == other.rotation.map(f32::to_bits)
            && translation.map(f32::to_bits) == other.translation.map(f32::to_bits)
    }
}

/// Records relative attack changes, preserving skin-specific pivots and authored rotations.
pub(super) fn apply(
    names: &[Box<str>],
    local: &mut [pose::LocalDelta],
    progress: f32,
    head_pitch: f32,
    weight: f32,
) -> Result<(), EvalError> {
    let swing = legacy_swing_pose(progress, head_pitch);
    for (index, name) in names.iter().enumerate() {
        let (rotation, offset) = match name.as_ref() {
            "body" => ([0.0, swing.body_yaw, 0.0], [0.0; 3]),
            "rightarm" => (
                swing.right_rotation.to_array(),
                [
                    swing.right_shoulder.x - BEDROCK_112.swing_shoulder_pixels,
                    0.0,
                    swing.right_shoulder.z,
                ],
            ),
            "leftarm" => (
                swing.left_rotation.to_array(),
                [
                    swing.left_shoulder.x + BEDROCK_112.swing_shoulder_pixels,
                    0.0,
                    swing.left_shoulder.z,
                ],
            ),
            // Keep a body-parented head on its original look rather than inheriting the twist.
            "head" => ([0.0; 3], [0.0; 3]),
            _ => continue,
        };
        local.get_mut(index).ok_or(EvalError::Invalid)?.legacy_swing = Some(Delta {
            rotation: rotation.map(|angle| angle * weight),
            translation: offset.map(|offset| offset * weight),
        });
    }
    Ok(())
}

/// Computes model-space targets from the destination's baseline after named driver mapping.
pub(super) fn targets(
    base: &[BoneTransform],
    local: &[pose::LocalDelta],
) -> Vec<Option<BoneTransform>> {
    base.iter()
        .enumerate()
        .map(|(index, base)| {
            let delta = local.get(index)?.legacy_swing?;
            let mut target = *base;
            if delta.rotation != [0.0; 3] {
                let [x, y, z, w] = target.rotation;
                let angles = [
                    (2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y)),
                    (2.0 * (w * y - z * x)).clamp(-1.0, 1.0).asin(),
                    (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z)),
                ];
                target.rotation = pose::quat_from_euler(std::array::from_fn(|axis| {
                    (angles[axis] + delta.rotation[axis] * if axis == 2 { 1.0 } else { -1.0 })
                        .to_degrees()
                }));
            }
            for (translation, offset) in target.translation_scale[..3]
                .iter_mut()
                .zip(delta.translation)
            {
                *translation += offset;
            }
            Some(target)
        })
        .collect()
}
