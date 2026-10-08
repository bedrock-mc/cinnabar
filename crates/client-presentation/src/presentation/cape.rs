//! Player cape publication from actor poses and profile skin rasters.
use std::sync::{Arc, Mutex};

use assets::RuntimeEntityAssets;
use client_world::{ActorRigSnapshot, PlayerProfile};
use protocol::{PlayerSkin, SkinRgba8};
use render::{ACTOR_LAYER_BODY, ActorRigRoute, ActorRigSubmission};
use render_model::{MAX_RENDERED_PLAYERS, RenderBoneTransform, java_animation::JavaCapeInput};
use view_presentation::cape::{ACTOR_LAYER_CAPE, CapeRig, cape_layer, cape_pose, java_cape_pose};

use super::actors::ActorPresentationBatch;

/// The cape rig, resolved on first use; an absent geometry leaves capes undrawn.
#[derive(Default)]
pub struct CapeState {
    resolved: bool,
    rig: Option<CapeRig>,
}

impl CapeState {
    pub fn rig(&mut self, assets: Option<&RuntimeEntityAssets>) -> Option<&CapeRig> {
        if !self.resolved
            && let Some(assets) = assets
        {
            self.resolved = true;
            self.rig = CapeRig::resolve(assets);
        }
        self.rig.as_ref()
    }
}

fn rest_pose(bones: &[client_world::BoneTransform], index: usize) -> Option<RenderBoneTransform> {
    let bone = bones.get(index)?;
    RenderBoneTransform::from_model_space_scaled(
        bone.rotation,
        bone.translation_scale,
        bone.axis_scale,
    )
}

/// The cape layer, resampled and hashed once per source raster; entries hold their source, so a
/// matched pointer is never a reused allocation.
fn cape_of(profile: &PlayerProfile) -> Option<SkinRgba8> {
    type Entry = (Arc<[u8]>, u32, u32, Option<SkinRgba8>);
    static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
    let PlayerSkin::Standard(skin) = &profile.skin else {
        return None;
    };
    let cape = skin.cape.as_ref()?;
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((.., layer)) = cache.iter().find(|(source, width, height, _)| {
        Arc::ptr_eq(source, &cape.rgba8) && *width == cape.width && *height == cape.height
    }) {
        return layer.clone();
    }
    let layer = cape_layer(cape.width, cape.height, &cape.rgba8).map(SkinRgba8::new);
    if cache.len() == MAX_RENDERED_PLAYERS {
        cache.remove(0);
    }
    cache.push((
        Arc::clone(&cape.rgba8),
        cape.width,
        cape.height,
        layer.clone(),
    ));
    layer
}

/// Uses cape art on worn wings, otherwise appends a cape with Java inputs when supplied.
/// Capes beyond the skin-layer budget are dropped without invalidating the frame.
pub fn apply_capes<'a>(
    batch: &mut ActorPresentationBatch,
    cape: &CapeRig,
    rig_of: impl Fn(u64) -> Option<ActorRigSnapshot<'a>>,
    profile_of: impl Fn(u64) -> Option<&'a PlayerProfile>,
    java_cape: impl Fn(u64) -> Option<JavaCapeInput>,
    elytra_of: impl Fn(u64) -> bool,
) {
    let mut capes: Vec<(SkinRgba8, usize)> = Vec::new();
    let mut extras = Vec::new();
    for index in 0..batch.submissions.len() {
        let body = &batch.submissions[index];
        let identity = body.input.identity;
        if identity.layer != ACTOR_LAYER_BODY
            || body.route == ActorRigRoute::NoDraw
            || batch.artwork.contains_key(&identity)
        {
            continue;
        }
        let Some(cape_pixels) = profile_of(identity.runtime_id).and_then(cape_of) else {
            continue;
        };
        let layer = match capes.iter().find(|(known, _)| *known == cape_pixels) {
            Some((_, layer)) => *layer,
            None if batch.skin_layers.len() < MAX_RENDERED_PLAYERS => {
                batch.skin_layers.push(cape_pixels.clone());
                capes.push((cape_pixels, batch.skin_layers.len() - 1));
                batch.skin_layers.len() - 1
            }
            None => continue,
        };
        if elytra_of(identity.runtime_id) {
            replace_elytra_texture(batch, identity.runtime_id, layer as u32);
            continue;
        }
        let Some(rig) = rig_of(identity.runtime_id) else {
            continue;
        };
        let body = &batch.submissions[index];
        let mut submission: ActorRigSubmission = body.clone();
        submission.input.identity.layer = ACTOR_LAYER_CAPE;
        submission.input.rig = cape.id;
        match java_cape(identity.runtime_id) {
            Some(input) => {
                let pose = java_cape_pose(
                    cape,
                    rig.bone_names,
                    |index| rest_pose(rig.rest, index),
                    &body.input.current_bones,
                    &input,
                );
                submission.input.previous_bones = Arc::clone(&pose);
                submission.input.current_bones = pose;
            }
            None => {
                submission.input.previous_bones =
                    cape_pose(cape, rig.bone_names, |index| rest_pose(rig.rest, index), &body.input.previous_bones);
                submission.input.current_bones =
                    cape_pose(cape, rig.bone_names, |index| rest_pose(rig.rest, index), &body.input.current_bones);
            }
        }
        submission.texture_layer = layer as u32;
        submission.tint = 0;
        submission.overlay_rgba8 = 0;
        extras.push(submission);
    }
    batch.submissions.extend(extras);
}

/// The cape replaces the worn wings' base image while retaining their geometry and glint.
fn replace_elytra_texture(batch: &mut ActorPresentationBatch, runtime_id: u64, layer: u32) {
    for submission in &mut batch.submissions {
        let identity = submission.input.identity;
        if identity.runtime_id == runtime_id && super::equipment::is_elytra_layer(identity.layer) {
            submission.texture_layer = layer;
            batch.artwork.remove(&identity);
        }
    }
}

#[cfg(test)]
#[path = "cape_elytra_tests.rs"]
mod elytra_tests;
