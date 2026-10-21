//! Composes actor layers in draw order and applies final visibility and cosmetic routes.
use super::*;
use crate::presentation::{
    actors::ActorPresentationBatch, cape::CapeState, entity_layers::LayerPoseCache,
    equipment::ActorEquipmentInput, skin_layers::SkinLayerCache, skin_rig::SkinRigCache,
};

/// Sampled actor poses used by equipment, capes and skin layers.
pub(super) struct LayerPoses<'a> {
    pub local_runtime_id: u64,
    pub local_equipment: &'a ActorEquipmentInput,
    pub java_posed: &'a [java::Posed],
    pub combat_visuals: render_api::CombatVisuals,
    pub local_emote_pose: Option<&'a actor_animation::CustomEmotePose>,
    pub remote_emote_poses:
        &'a bevy::platform::collections::HashMap<u64, actor_animation::CustomEmotePose>,
    pub native_body_sampled: bool,
    pub local_coverage: bool,
    pub remote_skin_layers: &'a super::bodies::RemoteSkinLayers,
    pub native_pose: &'a hand::NativePoseCache,
}

impl<'a> LayerPoses<'a> {
    /// Selects one render-only overlay for body-dependent clothing, cape and equipment layers.
    pub(super) fn emote(&self, runtime_id: u64) -> Option<&'a actor_animation::CustomEmotePose> {
        if runtime_id == self.local_runtime_id {
            self.local_emote_pose
        } else {
            self.remote_emote_poses.get(&runtime_id)
        }
    }
}

/// Retained layer geometry and pose conversions, borrowed only during layer assembly.
pub(super) struct LayerCaches<'a> {
    pub layer_poses: &'a mut LayerPoseCache,
    pub equipment: Option<&'a mut EquipmentRuntime>,
    pub cape_state: &'a mut CapeState,
    pub skin_layers: &'a mut SkinLayerCache,
    pub skin_rigs: &'a mut SkinRigCache,
}

/// Attaches equipment to unmasked poses, then masks the body before composing capes.
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
        local_equipment: _,
        java_posed,
        combat_visuals: _,
        local_emote_pose: _,
        remote_emote_poses: _,
        native_body_sampled,
        local_coverage,
        remote_skin_layers,
        native_pose,
    } = poses;
    let LayerCaches {
        layer_poses,
        equipment,
        cape_state,
        skin_layers,
        skin_rigs,
    } = caches;
    let (partial_tick, delta_seconds, glint_settings) = frame;
    let wearing_elytra = if let (Some(equipment), Some(stream)) = (equipment, stream) {
        equipment_layers::attach(
            batch,
            equipment,
            stream,
            render_frame.as_deref_mut(),
            &poses,
            (partial_tick, delta_seconds, glint_settings),
            |body, runtime_skin, render_frame| {
                let runtime_id = body.input.identity.runtime_id;
                let render_frame = render_frame?;
                let layers = if let Some(pose) = poses.emote(runtime_id) {
                    std::borrow::Cow::Borrowed(pose.render.as_slice())
                } else if local_coverage && runtime_id == local_runtime_id {
                    stream.authority().actor_ui_layers(runtime_id)?.render
                } else {
                    render_frame.layers_with_skin(runtime_id)?.render
                };
                Some(crate::presentation::entity_layers::controller_style(
                    body,
                    &layers,
                    artwork,
                    layer_poses,
                    runtime_skin,
                ))
            },
        )
    } else {
        Default::default()
    };
    let mut layers_of = |runtime_id| {
        let render_frame = render_frame.as_mut()?;
        if let Some(pose) = poses.emote(runtime_id) {
            Some(std::borrow::Cow::Borrowed(pose.render.as_slice()))
        } else if local_coverage && runtime_id == local_runtime_id {
            stream?
                .authority()
                .actor_ui_layers(runtime_id)
                .map(|layers| layers.render)
        } else {
            render_frame
                .layers_with_skin(runtime_id)
                .map(|layers| layers.render)
        }
    };
    // Equipment inherits animation poses before body-only visibility masks are applied.
    crate::presentation::entity_layers::apply_render_layers_cached(
        batch,
        &mut layers_of,
        artwork,
        layer_poses,
    );
    if let (Some(stream), Some((cape, preparation))) = (stream, cape_state.rig(entity_assets)) {
        if !scene.contains_geometry(cape.geometry.id) {
            let _ = scene.insert_geometry(cape.geometry.clone());
        }
        crate::presentation::cape::apply_capes(
            batch,
            cape,
            preparation,
            |runtime_id| {
                stream.authority().actor_rig(runtime_id).map(|mut rig| {
                    if local_coverage
                        && runtime_id == local_runtime_id
                        && let Some(pose) = stream.authority().actor_ui_pose(runtime_id)
                    {
                        rig.previous = pose;
                        rig.current = pose;
                    }
                    if let Some(pose) = poses.emote(runtime_id) {
                        pose.snapshot(rig)
                    } else {
                        rig
                    }
                })
            },
            |runtime_id| stream.authority().actor_player_profile(runtime_id),
            |runtime_id| java::posed(java_posed, runtime_id).map(|posed| posed.cape),
            |runtime_id| wearing_elytra.contains(&runtime_id),
        );
    }

    if let Some(stream) = stream
        && let Some(pages) = skin_layers.apply(
            batch,
            artwork,
            |runtime_id| {
                let mut rig = stream.authority().actor_rig(runtime_id)?;
                if local_coverage && runtime_id == local_runtime_id {
                    let pose = stream.authority().actor_ui_pose(runtime_id)?;
                    rig.previous = pose;
                    rig.current = pose;
                    let layers = stream.authority().actor_ui_layers(runtime_id)?;
                    // UI layers are retained by the animation owner; no frame-owned copy.
                    if let std::borrow::Cow::Borrowed(skin) = layers.skin {
                        rig.skin_layers = skin;
                    }
                }
                let emote = poses.emote(runtime_id);
                let java_layers =
                    java::posed(java_posed, runtime_id).map(|posed| posed.skin_layers.as_slice());
                let native_layers = if local_coverage && runtime_id == local_runtime_id {
                    None
                } else if runtime_id == local_runtime_id && native_body_sampled {
                    native_pose.skin_layers()
                } else {
                    remote_skin_layers.get(&runtime_id).map(AsRef::as_ref)
                };
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
    if style.is_none() && stream.is_none() {
        return;
    }
    // Each submission's overlay and route depend only on itself, so one pass with one lookup
    // per run of an actor's adjacent layers matches separate style and visibility passes.
    let mut looked_up: Option<(u64, Option<&client_world::ActorSnapshot>)> = None;
    for submission in &mut batch.submissions {
        let identity = submission.input.identity;
        let actor = match looked_up {
            Some((runtime_id, actor)) if runtime_id == identity.runtime_id => actor,
            _ => {
                let actor = stream.and_then(|stream| stream.authority().actor(identity.runtime_id));
                looked_up = Some((identity.runtime_id, actor));
                actor
            }
        };
        if !submission.material.allows_overlay() {
            submission.overlay_rgba8 = 0;
        } else if let Some(style) = style {
            let armor = (3..=6).contains(&identity.layer);
            let hurt = actor.is_some_and(|actor| actor.hurt_overlay_active());
            let attackable = style.attackable_actor == Some(identity.runtime_id)
                && actor
                    .is_some_and(|actor| matches!(actor.kind, protocol::ActorKind::Player { .. }));
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
        // Hiding skin layers keeps armor and held items visible.
        if (identity.layer == render_model::ACTOR_LAYER_BODY
            || identity.layer == crate::presentation::cape::ACTOR_LAYER_CAPE
            || render_model::hats::is_cosmetic_layer(identity.layer)
            || crate::presentation::skin_layers::is_skin_layer(identity.layer)
            || identity.layer >= crate::presentation::entity_layers::ACTOR_LAYER_TEXTURE_BASE)
            && actor.is_some_and(|actor| actor.is_invisible())
        {
            submission.route = render_model::ActorRigRoute::NoDraw;
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
