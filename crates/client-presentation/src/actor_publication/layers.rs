//! Composes actor layers in draw order and applies final visibility and cosmetic routes.
use super::*;
use crate::presentation::equipment::remote_input;
use crate::presentation::{
    actors::ActorPresentationBatch, cape::CapeState, entity_layers::LayerPoseCache,
    equipment::ActorEquipmentInput, skin_layers::SkinLayerCache, skin_rig::SkinRigCache,
};

/// Sampled actor poses used by equipment, capes and skin layers.
pub(super) struct LayerPoses<'a> {
    pub local_runtime_id: u64,
    pub local_equipment: &'a ActorEquipmentInput,
    pub java_posed: &'a [java::Posed],
    pub local_emote_pose: Option<&'a actor_animation::CustomEmotePose>,
    pub native_body_sampled: bool,
    pub native_pose: &'a hand::NativePoseCache,
}

/// Retained layer geometry and pose conversions, borrowed only during layer assembly.
pub(super) struct LayerCaches<'a> {
    pub layer_poses: &'a mut LayerPoseCache,
    pub equipment: Option<&'a mut EquipmentRuntime>,
    pub cape_state: &'a mut CapeState,
    pub skin_layers: &'a mut SkinLayerCache,
    pub skin_rigs: &'a mut SkinRigCache,
}

/// Applies player controllers before equipment and capes, then entity controllers and skin layers.
#[allow(clippy::too_many_arguments)]
pub(super) fn attach(
    batch: &mut ActorPresentationBatch,
    stream: Option<&WorldStream>,
    mut render_frame: Option<&mut actor_animation::ActorRenderFrame<'_>>,
    poses: LayerPoses<'_>,
    caches: LayerCaches<'_>,
    artwork: &render_model::ActorArtworkPages,
    entity_assets: Option<&assets::RuntimeEntityAssets>,
    scene: &mut ActorRenderScene,
    frame: (f32, f32, Option<&render_scene::UiGlintSettings>),
    new_geometries: &mut Vec<render_model::ActorRigGeometry>,
) {
    let LayerPoses {
        local_runtime_id,
        local_equipment: local_equipment_input,
        java_posed,
        local_emote_pose,
        native_body_sampled,
        native_pose,
    } = poses;
    let LayerCaches {
        layer_poses,
        mut equipment,
        cape_state,
        skin_layers,
        skin_rigs,
    } = caches;
    let (partial_tick, delta_seconds, glint_settings) = frame;
    if let (Some(stream), Some(render_frame)) = (stream, render_frame.as_mut()) {
        crate::presentation::entity_layers::apply_render_layers_cached(
            batch,
            |runtime_id| {
                if !stream
                    .authority()
                    .actor(runtime_id)
                    .is_some_and(|actor| matches!(actor.kind, protocol::ActorKind::Player { .. }))
                {
                    return None;
                }
                if runtime_id == local_runtime_id
                    && let Some(pose) = local_emote_pose
                {
                    Some(std::borrow::Cow::Borrowed(pose.render.as_slice()))
                } else {
                    render_frame.layers(runtime_id)
                }
            },
            artwork,
            layer_poses,
        );
    }
    if let (Some(equipment), Some(stream)) = (equipment.as_deref_mut(), stream) {
        equipment_layers::attach(
            batch,
            equipment,
            stream,
            local_runtime_id,
            local_equipment_input,
            java_posed,
            (partial_tick, delta_seconds, glint_settings),
        );
    }
    if let (Some(stream), Some(cape)) = (stream, cape_state.rig(entity_assets)) {
        if !scene.contains_geometry(cape.id) {
            let _ = scene.insert_geometry(cape.geometry.clone());
        }
        crate::presentation::cape::apply_capes(
            batch,
            cape,
            |runtime_id| {
                stream.authority().actor_rig(runtime_id).map(|rig| {
                    if runtime_id == local_runtime_id
                        && let Some(pose) = local_emote_pose
                    {
                        pose.snapshot(rig)
                    } else {
                        rig
                    }
                })
            },
            |runtime_id| stream.authority().actor_player_profile(runtime_id),
            |runtime_id| java::posed(java_posed, runtime_id).map(|posed| posed.cape),
            |runtime_id| {
                let input = if runtime_id == local_runtime_id {
                    local_equipment(stream, runtime_id, local_equipment_input)
                } else {
                    remote_input(stream, runtime_id)
                };
                input.armor[1].as_ref().is_some_and(|item| {
                    equipment
                        .as_deref()
                        .is_some_and(|equipment| equipment.is_elytra(&item.identifier))
                })
            },
        );
    }

    // Entity equipment rides the original rig even when a controller draws another model.
    if let Some(render_frame) = render_frame.as_mut() {
        crate::presentation::entity_layers::apply_render_layers_cached(
            batch,
            |runtime_id| {
                if stream.is_some_and(|stream| {
                    stream.authority().actor(runtime_id).is_some_and(|actor| {
                        matches!(actor.kind, protocol::ActorKind::Player { .. })
                    })
                }) {
                    return None;
                }
                if runtime_id == local_runtime_id
                    && let Some(pose) = local_emote_pose
                {
                    Some(std::borrow::Cow::Borrowed(pose.render.as_slice()))
                } else {
                    render_frame.layers(runtime_id)
                }
            },
            artwork,
            layer_poses,
        );
    }
    if let Some(stream) = stream
        && let Some(pages) = skin_layers.apply(
            batch,
            artwork,
            |runtime_id| {
                let rig = stream.authority().actor_rig(runtime_id)?;
                let emote = (runtime_id == local_runtime_id)
                    .then_some(local_emote_pose)
                    .flatten();
                let java_layers =
                    java::posed(java_posed, runtime_id).map(|posed| posed.skin_layers.as_slice());
                let native_layers = (runtime_id == local_runtime_id && native_body_sampled)
                    .then(|| native_pose.skin_layers())
                    .flatten();
                Some(emote_geometry::skin_layer_snapshot(
                    rig,
                    emote,
                    java_layers,
                    native_layers,
                ))
            },
            skin_rigs,
            |geometry| new_geometries.push(geometry),
        )
    {
        scene.configure_artwork(pages);
    }
}

/// Applies cosmetic overlays before hiding body layers, leaving armor and held items visible.
pub(super) fn apply_style_and_visibility(
    batch: &mut ActorPresentationBatch,
    stream: Option<&WorldStream>,
    style: Option<&ActorPresentationStyle>,
) {
    if let Some(style) = style {
        for submission in &mut batch.submissions {
            let armor = (3..=6).contains(&submission.input.identity.layer);
            let hurt = stream.is_some_and(|stream| {
                stream
                    .authority()
                    .actor(submission.input.identity.runtime_id)
                    .is_some_and(|actor| actor.hurt_overlay_active())
            });
            let attackable = style.attackable_actor == Some(submission.input.identity.runtime_id)
                && stream.is_some_and(|stream| {
                    stream
                        .authority()
                        .actor(submission.input.identity.runtime_id)
                        .is_some_and(|actor| {
                            matches!(actor.kind, protocol::ActorKind::Player { .. })
                        })
                });
            if let Some(color) = cosmetic_overlay(
                style,
                hurt,
                armor,
                submission.overlay_rgba8 != 0,
                attackable,
            ) {
                submission.overlay_rgba8 = render_model::pack_overlay_rgba8(color);
            }
        }
    }
    // Hiding skin layers keeps armor and held items visible.
    if let Some(stream) = stream {
        for submission in &mut batch.submissions {
            let identity = submission.input.identity;
            if (identity.layer == render_model::ACTOR_LAYER_BODY
                || identity.layer == crate::presentation::cape::ACTOR_LAYER_CAPE
                || crate::presentation::skin_layers::is_skin_layer(identity.layer)
                || identity.layer >= crate::presentation::entity_layers::ACTOR_LAYER_TEXTURE_BASE)
                && stream
                    .authority()
                    .actor(identity.runtime_id)
                    .is_some_and(|actor| actor.is_invisible())
            {
                submission.route = render_model::ActorRigRoute::NoDraw;
            }
        }
    }
}

/// Selects cosmetic color without treating a reach overlay as evidence of damage.
pub(super) fn cosmetic_overlay(
    style: &ActorPresentationStyle,
    hurt: bool,
    armor: bool,
    native_overlay: bool,
    attackable: bool,
) -> Option<[f32; 4]> {
    if hurt {
        if armor {
            style.hurt_armor_color
        } else if native_overlay {
            style.hurt_color
        } else {
            None
        }
    } else if attackable {
        style.attackable_color
    } else {
        None
    }
}
