//! Combat overlays sampled before authored material and controller overrides.

use client_world::ActorSnapshot;
use render_api::{BEDROCK_112, CombatVisuals};
use render_model::pack_overlay_rgba8;

/// Returns a flat damage overlay while the actor's accepted damage or death gate is active.
pub(crate) fn overlay(actor: &ActorSnapshot, profile: CombatVisuals) -> u32 {
    if !actor.hurt_overlay_active() {
        return 0;
    }
    let color = match profile {
        CombatVisuals::Legacy112 => BEDROCK_112.hurt_overlay,
        CombatVisuals::Modern => [1.0, 0.0, 0.0, client_world::HURT_OVERLAY_ALPHA],
    };
    pack_overlay_rgba8(color)
}

#[cfg(test)]
pub(crate) mod tests;
