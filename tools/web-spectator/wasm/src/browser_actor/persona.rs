//! Bounded adapters for canonical actor-animation persona layer snapshots.
use client_world::SkinRenderLayer;
use render_model::{EntityRigId, RenderBoneTransform, equipment::EquipmentRaster};
use render::{
    ActorArtworkLocation, ActorArtworkPages, ActorRenderIdentity, ActorRenderScene,
    ActorRigSubmission,
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

#[derive(Default)]
pub(super) struct PersonaLayers {
    rigs: BTreeMap<[u8; 32], EntityRigId>,
    rasters: Vec<EquipmentRaster>,
    locations: Vec<Option<ActorArtworkLocation>>,
    base: [u8; 32],
}
impl PersonaLayers {
    pub(super) fn apply(
        &mut self,
        scene: &mut ActorRenderScene,
        base: &ActorArtworkPages,
        selections: &[(ActorRigSubmission, Vec<SkinRenderLayer>)],
        submissions: &mut Vec<ActorRigSubmission>,
        assignments: &mut HashMap<ActorRenderIdentity, ActorArtworkLocation>,
    ) {
        let mut desired = Vec::<EquipmentRaster>::new();
        for (_, layers) in selections {
            for layer in layers {
                if !desired
                    .iter()
                    .any(|r| Arc::ptr_eq(&r.rgba8, &layer.image.rgba8))
                {
                    desired.push(EquipmentRaster {
                        width: layer.image.width as u16,
                        height: layer.image.height as u16,
                        rgba8: Arc::clone(&layer.image.rgba8),
                    });
                }
            }
        }
        let changed = self.base != base.identity()
            || desired.len() != self.rasters.len()
            || desired.iter().zip(&self.rasters).any(|(a, b)| {
                a.width != b.width || a.height != b.height || !Arc::ptr_eq(&a.rgba8, &b.rgba8)
            });
        if changed {
            self.base = base.identity();
            self.rasters = desired;
            let (pages, locations) = base.clone().with_equipment_rasters(&self.rasters);
            self.locations = locations;
            scene.configure_artwork(pages);
        }
        for (body, layers) in selections {
            for layer in layers {
                let Some(location) = self
                    .rasters
                    .iter()
                    .position(|r| Arc::ptr_eq(&r.rgba8, &layer.image.rgba8))
                    .and_then(|i| self.locations[i])
                else {
                    continue;
                };
                let rig = if let Some(rig) = self.rigs.get(&layer.geometry.digest) {
                    *rig
                } else {
                    if self.rigs.len() >= 192 {
                        continue;
                    }
                    let rig = render_model::skin_rig_id(1024 + self.rigs.len() as u32);
                    let Some(mut geometry) = layer.mesh.clone() else { continue; };
                    geometry.id = rig;
                    if scene.insert_geometry(geometry).is_err() {
                        continue;
                    }
                    self.rigs.insert(layer.geometry.digest, rig);
                    rig
                };
                let convert=|bones:&[client_world::BoneTransform]|->Option<Arc<[RenderBoneTransform]>> {
    let mut result=bones.iter().map(|b|RenderBoneTransform::from_model_space_scaled(b.rotation,b.translation_scale,b.axis_scale)).collect::<Option<Vec<_>>>()?;
    for &i in layer.hidden_bones.iter() {if let Some(b)=result.get_mut(i as usize){b.translation_scale=[0.0;4];}}
    Some(result.into())
   };
                let (Some(previous), Some(current)) =
                    (convert(&layer.previous), convert(&layer.current))
                else {
                    continue;
                };
                let mut submission = body.clone();
                submission.input.identity.layer =
                    view_presentation::cape::ACTOR_LAYER_CAPE + 1 + layer.image.kind.slot() as u8;
                submission.input.rig = rig;
                submission.input.previous_bones = previous;
                submission.input.current_bones = current;
                submission.texture_layer = location.layer();
                submission.uv_anim = layer.uv_anim;
                assignments.insert(submission.input.identity, location);
                submissions.push(submission);
            }
        }
    }
}
