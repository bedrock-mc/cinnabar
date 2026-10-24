//! Native shulker presentation: the synced peek target, its per-tick approach and the
//! attach face the pack's `variable.shulker.*` scripts orient the model from.

use super::*;
use assets::facts::entity::NativeEntityClass;
use protocol_ids::ActorDataKey;

/// Lid-open fraction per unit of the synced peek data item.
const PEEK_UNIT: f32 = 0.01;
/// Largest change in the lid-open fraction per tick.
const PEEK_STEP: f32 = 0.05;

/// Whether `actor` is a shulker or a custom actor built on the native shulker class.
pub(super) fn is_shulker(actor: &dyn AnimatedActor) -> bool {
    actor.entity_fact().native_class == NativeEntityClass::Shulker
}

/// Moves a shulker's lid-open fraction one tick toward its synced peek target.
///
/// Only an Int peek data item counts; a missing or differently typed item closes the lid.
pub(super) fn advance_peek(peek: &mut f32, actor: &dyn AnimatedActor) {
    if !is_shulker(actor) {
        return;
    }
    let target = match actor.metadata().get(&ActorDataKey::PeekID.wire_value()) {
        Some(ActorMetadataValue::Int(value)) => *value as f32 * PEEK_UNIT,
        _ => 0.0,
    };
    *peek = approach(*peek, target);
}

/// One native peek step: opening is bounded by zero and the target, closing by the target and
/// one, and the upper bound wins when the step overshoots it.
fn approach(current: f32, target: f32) -> f32 {
    let (step, lower, upper) = if current > target {
        (-PEEK_STEP, target, 1.0)
    } else if current < target {
        (PEEK_STEP, 0.0, target)
    } else {
        return current;
    };
    let next = current + step;
    if upper < next { upper } else { next.max(lower) }
}

/// Publishes `variable.shulker.peekamount` and `variable.shulker.facingdirection` before
/// `pre_animation`; other actors keep any values their pack assigns.
///
/// The peek amount eases the lid-open fraction through the native sine table. The facing
/// direction is the signed byte attach face, which a riding shulker resets to down.
pub(super) fn publish(
    engine: &EngineSlots,
    variables: &mut MolangVariables,
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
    motion: &MotionState,
) {
    if !is_shulker(actor) {
        return;
    }
    let (riding, peek) = (context.is_riding, motion.shulker_peek);
    let eased = 0.5 - render_model::native_trig::sine((peek + 0.5) * std::f32::consts::PI) * 0.5;
    let facing = match actor.metadata().get(&ActorDataKey::AttachFace.wire_value()) {
        Some(ActorMetadataValue::Byte(face)) if !riding => f32::from(*face),
        _ => 0.0,
    };
    variables.set(engine.shulker_peek_amount, eased);
    variables.set(engine.shulker_facing_direction, facing);
}

#[cfg(test)]
#[path = "shulker_tests.rs"]
mod tests;
