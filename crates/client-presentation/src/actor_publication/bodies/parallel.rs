//! Samples ordinary remote bodies between ordered admission-budget barriers.
use super::*;

pub(super) struct RemoteBody<'a> {
    pub rig: actor_animation::ActorRigSnapshot<'a>,
    pub actor: &'a client_world::ActorSnapshot,
    pub presentation: ActorRigPresentation,
}

/// Finishes every queued body before the next admission step can spend the shared budget.
#[allow(clippy::too_many_arguments)]
pub(super) fn publish(
    pending: &mut Vec<RemoteBody<'_>>,
    frame: Option<&mut actor_animation::ActorRenderFrame<'_>>,
    cull_view: Option<ActorCullView>,
    combat_visuals: render_api::CombatVisuals,
    skin_rigs: &mut SkinRigCache,
    mut equipment: Option<&mut EquipmentRuntime>,
    new_geometries: &mut Vec<render_model::ActorRigGeometry>,
    remote_skin_layers: &mut RemoteSkinLayers,
    remotes: &mut Vec<ActorRigPresentation>,
) {
    if pending.is_empty() {
        return;
    }
    let Some(frame) = frame else {
        return;
    };
    let ids: Vec<_> = pending
        .iter()
        .map(|body| body.rig.actor.runtime_id)
        .collect();
    frame.sample_ordered(&ids);
    for RemoteBody {
        rig,
        actor,
        mut presentation,
    } in pending.drain(..)
    {
        let skin = hand::NativePoseCache::apply_frame_body(frame, &mut presentation);
        if let Some(skin) = &skin {
            remote_skin_layers.insert(rig.actor.runtime_id, Arc::clone(skin));
        }
        let admission_rig = emote_geometry::skin_layer_snapshot(rig, None, None, skin.as_deref());
        if !crate::presentation::actors::admit_frame_bounds(
            &mut presentation,
            &admission_rig,
            cull_view,
            Some(frame),
        ) {
            continue;
        }
        preparation::register_player_skin(
            &mut presentation,
            &rig,
            skin_rigs,
            equipment.as_deref_mut(),
            new_geometries,
        );
        presentation.submission.overlay_rgba8 =
            crate::presentation::combat::overlay(actor, combat_visuals);
        remotes.push(presentation);
    }
}
