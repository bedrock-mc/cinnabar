//! Cloth contacts follow the geometry selected for this actor, including worn armor.
use crate::presentation::{
    equipment::{EquipmentRuntime, is_armor_layer},
    skin_layers::is_skin_layer,
};
use render_model::{
    ActorRigGeometry, ActorRigRoute, ActorRigSubmission, EntityRigId,
    hats::cloth::{Collider, placement},
};
use render_scene::ActorRenderScene;

/// Evaluates hidden local contacts from the unmasked body, before body-only draw visibility.
pub(super) fn hidden(
    body: &ActorRigSubmission,
    rig: &actor_animation::ActorRigSnapshot<'_>,
    (stream, input, combat): (
        &chunk_pipeline::WorldStream,
        &crate::presentation::equipment::ActorEquipmentInput,
        render_api::CombatVisuals,
    ),
    (scene, pending, equipment): (
        &ActorRenderScene,
        &mut Vec<ActorRigGeometry>,
        Option<&mut EquipmentRuntime>,
    ),
    (alpha, delta): (f32, f32),
) -> Vec<Collider> {
    let mut equipment = equipment;
    let input =
        super::equipment_layers::local_equipment(stream, body.input.identity.runtime_id, input);
    let animation = stream
        .authority()
        .actor(body.input.identity.runtime_id)
        .map(|owner| crate::presentation::equipment::EquipmentAnimation {
            owner,
            rig,
            frame_alpha: alpha,
            delta_seconds: delta,
        });
    let worn: Vec<_> = if let Some(equipment) = equipment.as_deref_mut() {
        let worn = equipment
            .layers_for_profile(body, &input, combat, animation)
            .layers
            .iter()
            .map(|layer| layer.submission.clone())
            .collect();
        pending.extend(equipment.take_pending_geometries());
        worn
    } else {
        Vec::new()
    };
    let mut contacts = collect(
        body,
        rig,
        &worn,
        scene,
        alpha,
        (pending, equipment.as_deref()),
    );
    contacts.extend(hidden_skin(body, rig, alpha));
    contacts
}

/// Samples hidden persona geometry without publishing texture pages or visible submissions.
fn hidden_skin(
    body: &ActorRigSubmission,
    rig: &actor_animation::ActorRigSnapshot<'_>,
    alpha: f32,
) -> Vec<Collider> {
    let mut contacts = Vec::new();
    for layer in rig.skin_layers {
        let Some(mesh) = layer.mesh.as_ref() else {
            continue;
        };
        let (Some(previous), Some(current)) = (
            crate::presentation::actors::convert_bones(&layer.previous),
            crate::presentation::actors::convert_bones(&layer.current),
        ) else {
            continue;
        };
        contacts.extend(
            layer
                .geometry
                .bones
                .iter()
                .zip(current.iter())
                .enumerate()
                .filter_map(|(index, (bone, current))| {
                    if layer.hidden_bones.contains(&(index as u32)) {
                        return None;
                    }
                    Collider::sampled_geometry(
                        &bone.name,
                        mesh,
                        index,
                        [*previous.get(index)?, *current],
                        body.world_from_actor,
                        alpha,
                    )
                }),
        );
    }
    contacts
}

/// Resolves newly prepared geometry before it enters the scene's persistent registry.
fn geometry<'a>(
    id: EntityRigId,
    pending: &'a [ActorRigGeometry],
    scene: &'a ActorRenderScene,
) -> Option<&'a ActorRigGeometry> {
    pending
        .iter()
        .rev()
        .find(|geometry| geometry.id == id)
        .or_else(|| scene.geometry(id))
}

/// Builds body and worn contacts from the same bone transforms used by the current draw.
pub(super) fn collect(
    body: &ActorRigSubmission,
    rig: &actor_animation::ActorRigSnapshot<'_>,
    submissions: &[ActorRigSubmission],
    scene: &ActorRenderScene,
    alpha: f32,
    (pending, equipment): (&[ActorRigGeometry], Option<&EquipmentRuntime>),
) -> Vec<Collider> {
    let body_geometry = geometry(body.input.rig, pending, scene);
    let mut contacts: Vec<_> = rig
        .bone_names
        .iter()
        .zip(body.input.current_bones.iter())
        .enumerate()
        .filter_map(|(index, (name, bone))| {
            let actor = placement(body.world_from_actor);
            body_geometry
                .and_then(|geometry| {
                    Collider::sampled_geometry(
                        name,
                        geometry,
                        index,
                        [body.input.previous_bones[index], *bone],
                        body.world_from_actor,
                        alpha,
                    )
                })
                .or_else(|| {
                    body_geometry
                        .is_none()
                        .then(|| Collider::biped(name, *bone, actor))
                        .flatten()
                })
        })
        .collect();
    for worn in submissions.iter().filter(|worn| {
        worn.input.identity.runtime_id == body.input.identity.runtime_id
            && worn.input.identity.spawn_revision == body.input.identity.spawn_revision
            && (is_armor_layer(worn.input.identity.layer)
                || is_skin_layer(worn.input.identity.layer))
            && worn.route != ActorRigRoute::NoDraw
    }) {
        let armor_names = is_armor_layer(worn.input.identity.layer)
            .then(|| equipment.and_then(|equipment| equipment.armor_bone_names(worn.input.rig)))
            .flatten();
        let skin_names = rig
            .skin_layers
            .iter()
            .filter(|layer| {
                is_skin_layer(worn.input.identity.layer)
                    && worn.input.identity.layer
                        == render_model::ACTOR_LAYER_SKIN_BASE + layer.image.kind.slot() as u8
            })
            .flat_map(|layer| layer.geometry.bones.iter().map(|bone| bone.name.as_ref()));
        let Some(geometry) = geometry(worn.input.rig, pending, scene) else {
            continue;
        };
        contacts.extend(
            armor_names
                .into_iter()
                .flatten()
                .map(AsRef::as_ref)
                .chain(skin_names)
                .zip(worn.input.current_bones.iter())
                .enumerate()
                .filter_map(|(index, (name, bone))| {
                    Collider::sampled_geometry(
                        name,
                        geometry,
                        index,
                        [worn.input.previous_bones[index], *bone],
                        worn.world_from_actor,
                        alpha,
                    )
                }),
        );
    }
    contacts
}
