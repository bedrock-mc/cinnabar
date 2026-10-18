//! Equipment publication keeps authored worn poses beneath the selected player body.

use super::*;
use crate::presentation::actors::ActorPresentationBatch;
use crate::presentation::equipment::remote_input;

/// Adds held and worn layers using the local actor identity and inventory facts.
/// Cape artwork is applied afterward and may replace a worn wing texture.
pub(super) fn attach(
    batch: &mut ActorPresentationBatch,
    equipment: &mut EquipmentRuntime,
    stream: &WorldStream,
    mut render_frame: Option<&mut actor_animation::ActorRenderFrame<'_>>,
    poses: &super::layers::LayerPoses<'_>,
    frame: (f32, f32, Option<&render_scene::UiGlintSettings>),
    mut style_of: impl FnMut(
        &render_model::ActorRigSubmission,
        bool,
        Option<&mut actor_animation::ActorRenderFrame<'_>>,
    ) -> Option<crate::presentation::entity_layers::ControllerStyle>,
) -> std::collections::BTreeSet<u64> {
    let (partial_tick, delta_seconds, glint_settings) = frame;
    let mut wearing_elytra = std::collections::BTreeSet::new();
    crate::presentation::actors::attach_layers(
        batch,
        equipment,
        |equipment, body, runtime_skin| {
            let runtime_id = body.input.identity.runtime_id;
            let mut input = if runtime_id == poses.local_runtime_id {
                local_equipment(stream, runtime_id, poses.local_equipment)
            } else {
                remote_input(stream, runtime_id)
            };
            if java::posed(poses.java_posed, runtime_id).is_some() {
                let using = stream
                    .authority()
                    .actor(runtime_id)
                    .is_some_and(|actor| actor.is_using_item());
                input.java = Some(crate::presentation::equipment::JavaGrip { blocking: using });
            }
            let animation = stream.authority().actor(runtime_id).zip(
                stream.authority().actor_rig(runtime_id).map(|rig| {
                    let rig = render_frame
                        .as_mut()
                        .map_or(rig, |frame| frame.sample_held_item_scale(rig));
                    poses
                        .emote(runtime_id)
                        .map_or(rig, |pose| pose.snapshot(rig))
                }),
            );
            let mut styled_body = body.clone();
            if let Some(style) = runtime_skin
                .and_then(|runtime_skin| style_of(body, runtime_skin, render_frame.as_deref_mut()))
            {
                styled_body.overlay_rgba8 = style.overlay_rgba8;
                styled_body.light = style.light;
            }
            let selected = equipment.layers_for_profile(
                &styled_body,
                &input,
                poses.combat_visuals,
                animation.as_ref().map(|(owner, rig)| {
                    crate::presentation::equipment::EquipmentAnimation {
                        owner,
                        rig,
                        frame_alpha: partial_tick,
                        delta_seconds,
                    }
                }),
            );
            if selected.wearing_elytra {
                wearing_elytra.insert(runtime_id);
            }
            for layer in selected.layers.iter_mut() {
                animate_glint(&mut layer.submission, partial_tick, glint_settings);
            }
            selected.layers
        },
    );
    wearing_elytra
}

/// Reuses inventory facts while reading pose flags after this frame's local pose synchronization.
pub(super) fn local_equipment(
    stream: &WorldStream,
    runtime_id: u64,
    inventory: &crate::presentation::equipment::ActorEquipmentInput,
) -> crate::presentation::equipment::ActorEquipmentInput {
    let actor = stream.authority().actor(runtime_id);
    crate::presentation::equipment::ActorEquipmentInput {
        sneaking: actor.is_some_and(|actor| actor.is_sneaking()),
        sleeping: actor.is_some_and(|actor| actor.is_sleeping()),
        ..inventory.clone()
    }
}

/// Applies the glint accessibility settings and starts held-item glint scrolling at the
/// draw's interpolated tick; worn equipment keeps the clock its own layer set.
pub(super) fn animate_glint(
    submission: &mut render_model::ActorRigSubmission,
    partial_tick: f32,
    settings: Option<&render_scene::UiGlintSettings>,
) {
    let glint = &mut submission.material.glint;
    if let Some(settings) = settings {
        glint.strength = settings.strength;
        glint.speed = settings.speed;
    }
    if glint.texture == render_model::GlintTexture::Item {
        glint.time_seconds = (submission.input.completed_tick as f32 + partial_tick)
            * simulation_clock::TICK_DURATION.as_secs_f32();
    }
}
