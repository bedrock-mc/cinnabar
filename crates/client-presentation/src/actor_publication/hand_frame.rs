//! Selects the canonical first-person source and its world lighting.
use super::hand::hand_motion_matrix;
use super::*;
use crate::presentation::actors::ActorRigPresentation;
use crate::{camera::FirstPersonHandMotion, local_player::FrozenLocalAvatarVisibility};
use bevy::math::{Mat4, Vec3};
use render_model::HandRigLight;

/// Retained readiness and pose state reused by final first-person preparation.
pub(super) struct HandCaches<'a> {
    pub source: &'a mut Option<HandSource>,
    pub key: &'a Option<hand::SourceKey>,
    pub java_hand: &'a mut java::HandCache,
    pub equipment: Option<&'a mut EquipmentRuntime>,
}

/// Reuses the ready hand when its source key still matches, before local body placement.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    stream: Option<&WorldStream>,
    local_runtime_id: u64,
    canonical_local: &Option<ActorRigPresentation>,
    input: &ActorFrameInput,
    view: bodies::PoseView,
    hand_use: (
        Option<u32>,
        Option<actor_animation::AttachableAnimationInput<'static>>,
    ),
    artwork: &render_model::ActorArtworkPages,
    hand_motion: Option<&FirstPersonHandMotion>,
    caches: HandCaches<'_>,
) -> Option<HandSource> {
    let bodies::PoseView {
        partial_tick,
        first_person,
        java_mode,
        sampling_camera,
    } = view;
    let HandCaches {
        source: ready_hand_source,
        key: ready_hand_key,
        java_hand,
        mut equipment,
    } = caches;
    // First person draws the player's own rig near the camera: the visible arms with every other
    // bone hidden, and a drawable held item in its own first-person frame. Anything not covered
    // (an undrawable item) leaves the CPU viewmodel in charge.
    if java_mode
        && let Some(rig) = stream.and_then(|stream| stream.authority().actor_rig(local_runtime_id))
    {
        java_hand.remember(&rig, input.local_equipment.main.as_ref());
    }
    let mut reused_hand = false;
    let hand_source: Option<HandSource> = if first_person && input.renders_game {
        canonical_local.clone().and_then(|presentation| {
            let stream = stream?;
            let equipment = equipment.as_deref_mut()?;
            let equipment_input = local_equipment(stream, local_runtime_id, &input.local_equipment);
            let (consume_ticks, item_animation) = hand_use;
            let motion = hand_motion.map_or(Mat4::IDENTITY, hand_motion_matrix);
            let key = hand::source_key(stream, consume_ticks, item_animation, partial_tick);
            if key == *ready_hand_key {
                reused_hand = true;
                return ready_hand_source.take();
            }
            hand::source(
                hand::HandInputs {
                    stream,
                    presentation,
                    equipment_input: &equipment_input,
                    owner_equipment: &equipment_input,
                    consume_ticks,
                    item_animation,
                    alpha: partial_tick,
                    artwork,
                    motion,
                    sampling_camera,
                },
                java_mode,
                equipment,
                java_hand,
            )
        })
    } else {
        None
    };
    if let Some(equipment) = equipment {
        equipment.finish_hand_readiness(reused_hand);
    }
    hand_source
}

/// Lights the hand at visible feet, predicted feet or the authoritative eye, in that order.
#[allow(clippy::too_many_arguments)]
pub(super) fn lighting(
    stream: Option<&WorldStream>,
    local_runtime_id: u64,
    visibility_snapshot: Option<FrozenLocalAvatarVisibility>,
    predicted_feet: Option<[f32; 3]>,
    authoritative_subject_eye: Option<Vec3>,
    java_mode: bool,
    hand_motion: Option<&FirstPersonHandMotion>,
    rotation: bevy::math::Quat,
) -> HandRigLight {
    let hand_light = stream.map_or(
        HandRigLight {
            block_level: 0,
            sky_level: 0,
            daylight: 1.0,
            pad: 0,
            ..Default::default()
        },
        |stream| {
            let (block, sky) = stream
                .authority()
                .actor(local_runtime_id)
                .and_then(|actor| {
                    let feet = visibility_snapshot
                        .filter(|visibility| visibility.runtime_id() == local_runtime_id)
                        .map(|visibility| visibility.feet().to_array())
                        .or(predicted_feet)
                        .unwrap_or(actor.position);
                    stream.actor_light_at(actor, feet)
                })
                .or_else(|| {
                    authoritative_subject_eye.map(|eye| stream.light_level_at(eye.to_array()))
                })
                .unwrap_or((0, 0));
            HandRigLight {
                block_level: u32::from(block),
                sky_level: u32::from(sky),
                // Reserved legacy field; the hand samples the shared world lightmap.
                daylight: 1.0,
                pad: 0,
                ..Default::default()
            }
        },
    );
    if java_mode {
        hand_light.with_java_lighting(hand::java_light_matrix(hand_motion, rotation))
    } else {
        hand_light
    }
}
