use crate::presentation::{equipment::EquipmentRuntime, skin_rig::SkinRigCache};

/// Skin layers read the same render-only emote snapshot as the body and cape.
pub(super) fn skin_layer_snapshot<'a>(
    rig: client_world::ActorRigSnapshot<'a>,
    emote: Option<&'a client_world::CustomEmotePose>,
    java_layers: Option<&'a [client_world::SkinRenderLayer]>,
    native_layers: Option<&'a [client_world::SkinRenderLayer]>,
) -> client_world::ActorRigSnapshot<'a> {
    let rig = emote.map_or(rig, |pose| pose.snapshot(rig));
    match java_layers.or(native_layers) {
        Some(skin_layers) => client_world::ActorRigSnapshot { skin_layers, ..rig },
        None => rig,
    }
}

pub(super) fn apply(
    rig: &client_world::ActorRigSnapshot<'_>,
    cache: &mut SkinRigCache,
    mut equipment: Option<&mut EquipmentRuntime>,
    pending: &mut Vec<render_model::ActorRigGeometry>,
    local: &mut render::ActorRigSubmission,
    animated: render::ActorRigSubmission,
) {
    if let Some(geometry) = rig.skin_geometry {
        let Some(id) = cache.rig(geometry, |built| {
            if let Some(equipment) = equipment.as_deref_mut() {
                equipment.register_skin_rig(built.id, rig.bone_names.to_vec());
            }
            pending.push(built);
        }) else {
            return;
        };
        local.input.rig = id;
    }
    // Keep placement/materials and the canonical first-person hand unchanged.
    local.input.previous_bones = animated.input.previous_bones;
    local.input.current_bones = animated.input.current_bones;
}
