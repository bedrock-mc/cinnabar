//! The wolf-only native tail query; authored clips retain ownership of the final pose.
use super::{AnimatedActor, actor_flag};
use assets::facts::entity::NativeEntityClass;
use protocol_ids::ActorFlag;

#[cfg(any(test, feature = "test-support"))]
pub(crate) const WOLF_IDENTIFIER: &str = "minecraft:wolf";

// The query admits Wolf (type 0x530e), then reads its tail angle.
// The result is radians, not a render-frame-interpolated angle.
const ANGRY_ANGLE: f32 = f32::from_bits(0x3fc5_0a6b);
const WILD_ANGLE: f32 = f32::from_bits(0x3f20_d97c);
const HEALTH_WEIGHT: f32 = 0.4;
const HEALTH_BASE: f32 = 0.15;

pub(super) fn tail_angle(actor: &dyn AnimatedActor) -> f32 {
    if !is_wolf(actor) {
        return 0.0;
    }
    if actor_flag(actor, ActorFlag::Angry.wire_value()) {
        return ANGRY_ANGLE;
    }
    if !actor_flag(actor, ActorFlag::Tamed.wire_value()) {
        return WILD_ANGLE;
    }
    // A native Wolf always owns HEALTH. Missing/non-finite server attributes cannot
    // reproduce that branch; retain the native wild angle until valid health arrives.
    let Some(health) = actor
        .health()
        .filter(|health| health.current.is_finite() && health.max.is_finite() && health.max > 0.0)
    else {
        return WILD_ANGLE;
    };
    let angle = (health.current / health.max * HEALTH_WEIGHT + HEALTH_BASE) * std::f32::consts::PI;
    if angle.is_finite() { angle } else { WILD_ANGLE }
}

fn is_wolf(actor: &dyn AnimatedActor) -> bool {
    actor.entity_fact().native_class == NativeEntityClass::Wolf
}

#[cfg(test)]
mod tests;
