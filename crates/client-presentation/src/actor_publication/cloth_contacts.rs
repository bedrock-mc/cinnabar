//! Cloth contacts follow the geometry selected for this actor, including worn armor.
use crate::presentation::equipment::{EquipmentRuntime, is_armor_layer};
use render_model::{
    ActorRigGeometry, ActorRigRoute, ActorRigSubmission, EntityRigId,
    hats::cloth::{Collider, placement},
};
use render_scene::ActorRenderScene;

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
    names: &[Box<str>],
    submissions: &[ActorRigSubmission],
    scene: &ActorRenderScene,
    alpha: f32,
    (pending, equipment): (&[ActorRigGeometry], Option<&EquipmentRuntime>),
) -> Vec<Collider> {
    let body_geometry = geometry(body.input.rig, pending, scene);
    let mut contacts: Vec<_> = names
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
    let Some(equipment) = equipment else {
        return contacts;
    };
    for worn in submissions.iter().filter(|worn| {
        worn.input.identity.runtime_id == body.input.identity.runtime_id
            && worn.input.identity.spawn_revision == body.input.identity.spawn_revision
            && is_armor_layer(worn.input.identity.layer)
            && worn.route != ActorRigRoute::NoDraw
    }) {
        let Some(names) = equipment.armor_bone_names(worn.input.rig) else {
            continue;
        };
        let Some(geometry) = geometry(worn.input.rig, pending, scene) else {
            continue;
        };
        contacts.extend(
            names
                .iter()
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
