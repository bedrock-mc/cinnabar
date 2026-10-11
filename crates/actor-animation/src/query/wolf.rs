//! The wolf-only native tail query and dye tint; authored clips retain ownership of the final pose.
use super::{ActorKind, ActorMetadataValue, AnimatedActor, FLAG_ANGRY, FLAG_TAMED, actor_flag};

pub(crate) const WOLF_IDENTIFIER: &str = "minecraft:wolf";
pub(crate) const COLOR_METADATA_KEY: u32 = 3;

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
    if actor_flag(actor, FLAG_ANGRY) {
        return ANGRY_ANGLE;
    }
    if !actor_flag(actor, FLAG_TAMED) {
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

/// Supplies the wolf's tamed dye tint when its controller inherits actor color.
/// Other actors and wild wolves retain the neutral multiplier.
pub(crate) fn default_render_color(actor: &dyn AnimatedActor) -> [f32; 4] {
    if !is_wolf(actor) || !actor_flag(actor, FLAG_TAMED) {
        return [1.0; 4];
    }
    let index = match actor.metadata().get(&COLOR_METADATA_KEY) {
        Some(ActorMetadataValue::Byte(value)) => *value as u8,
        _ => 0,
    };
    assets::dye::actor_palette_color(index)
}

fn is_wolf(actor: &dyn AnimatedActor) -> bool {
    matches!(actor.kind(), ActorKind::Entity { identifier } if identifier.as_ref() == WOLF_IDENTIFIER)
}

#[cfg(test)]
mod tests;
