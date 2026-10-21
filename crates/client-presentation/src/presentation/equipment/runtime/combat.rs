//! Worn combat profiles preserve held-item and authored overlay policies.
use super::*;
use render_api::CombatVisuals;

/// Applies worn overlay and glint rules without changing held draws or Java poses.
pub(super) fn apply(
    layers: &mut [EquipmentPresentation],
    body: &ActorRigSubmission,
    input: &ActorEquipmentInput,
    profile: CombatVisuals,
    animation: Option<EquipmentAnimation<'_>>,
) {
    for layer in layers {
        let layer_id = layer.submission.input.identity.layer;
        if layer_id < LAYER_HELMET {
            continue;
        }
        let submission = &mut layer.submission;
        submission.material.glint.profile = profile;
        if profile == CombatVisuals::Legacy112 {
            if let Some(slot) = layer_id.checked_sub(LAYER_HELMET)
                && let Some(Some(item)) = input.armor.get(usize::from(slot))
                && item.has_glint()
                && submission.material.state.is_none()
            {
                submission.material.kind = assets::EntityRenderMaterial::Glint;
            }
            if let Some(animation) = animation {
                submission.material.glint.time_seconds = (animation.rig.completed_tick as f32
                    + animation.frame_alpha)
                    * simulation_clock::TICK_DURATION.as_secs_f32();
            }
        }
        if !submission.material.allows_overlay() {
            submission.overlay_rgba8 = 0;
        } else if profile == CombatVisuals::Legacy112 {
            if submission.overlay_rgba8 == 0 {
                submission.overlay_rgba8 = body.overlay_rgba8;
            }
        } else if input.java.is_some() {
            // Modern retains Java's existing worn-overlay behavior.
            submission.overlay_rgba8 = body.overlay_rgba8;
        }
    }
}
