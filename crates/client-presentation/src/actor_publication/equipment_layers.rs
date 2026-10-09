//! Equipment publication keeps authored worn poses beneath the selected player body.

use super::*;
use crate::presentation::{actors::ActorPresentationBatch, equipment::ActorEquipmentInput};

/// Adds held and worn layers before cape artwork can replace a worn wing texture.
pub(super) fn attach(
    batch: &mut ActorPresentationBatch,
    equipment: &mut EquipmentRuntime,
    stream: &WorldStream,
    local_runtime_id: u64,
    input: &ActorEquipmentInput,
    java_posed: &[java::Posed],
    frame: (f32, f32, Option<&render::UiGlintSettings>),
) {
    let (partial_tick, delta_seconds, glint_settings) = frame;
    crate::presentation::actors::attach_layers(batch, equipment, |equipment, body| {
        let runtime_id = body.input.identity.runtime_id;
        let mut input = if runtime_id == local_runtime_id {
            local_equipment(stream, runtime_id, input)
        } else {
            remote_input(stream, runtime_id)
        };
        if java::posed(java_posed, runtime_id).is_some() {
            let using = stream
                .authority()
                .actor(runtime_id)
                .is_some_and(|actor| actor.is_using_item());
            input.java = Some(crate::presentation::equipment::JavaGrip { blocking: using });
        }
        let animation = stream
            .authority()
            .actor(runtime_id)
            .zip(stream.authority().actor_rig(runtime_id));
        let layers = equipment.layers_for(
            body,
            &input,
            animation.as_ref().map(|(owner, rig)| {
                crate::presentation::equipment::EquipmentAnimation {
                    owner,
                    rig,
                    frame_alpha: partial_tick,
                    delta_seconds,
                }
            }),
        );
        if let Some(settings) = glint_settings {
            for layer in layers.iter_mut() {
                layer.submission.material.glint.strength = settings.strength;
                layer.submission.material.glint.speed = settings.speed;
            }
        }
        layers
    });
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
