//! Render-controller layers of entity bodies: the first replaces the body's default texture
//! (and model, when its controller picks another), later layers draw after it.
use std::{borrow::Cow, collections::HashMap, sync::Arc};

#[cfg(any(test, feature = "test-support"))]
use client_world::ActorRigSnapshot;
use client_world::{BoneTransform, RenderTextureLayer};
use render::{
    ACTOR_LAYER_BODY, ActorArtworkLocation, ActorArtworkPages, ActorRigSubmission,
    pack_overlay_rgba8,
};
use render_model::{EntityRigId, RenderBoneTransform, layer_geometry_rig_id};

use super::actors::ActorPresentationBatch;

/// First render layer id of extra texture layers; equipment layers use the ids below.
pub const ACTOR_LAYER_TEXTURE_BASE: u8 = 32;
/// Every layer id above the base, so no authored controller is dropped.
const MAX_TEXTURE_LAYERS: usize = (u8::MAX - ACTOR_LAYER_TEXTURE_BASE) as usize + 2;

/// A controller's own model: its rig id and poses.
#[derive(Debug)]
struct LayerModel {
    rig: EntityRigId,
    previous: Arc<[RenderBoneTransform]>,
    current: Arc<[RenderBoneTransform]>,
}

#[derive(Debug)]
struct ResolvedLayer {
    material: render::ActorMaterial,
    location: ActorArtworkLocation,
    tint: u32,
    overlay: Option<u32>,
    hidden_bones: Arc<[u32]>,
    uv_anim: [f32; 4],
    model: Option<LayerModel>,
    ignore_lighting: bool,
}

/// Render-space layer poses and their hidden-bone variants, kept while their source poses are
/// drawn so every frame of a tick reuses one conversion.
#[derive(Debug, Default)]
pub struct LayerPoseCache {
    /// Source pose, its conversion and the frame it was last drawn, by source allocation.
    converted: HashMap<usize, ConvertedPose>,
    /// Pose, hidden bones, the pose with them hidden and the frame it was last drawn.
    hidden: HashMap<(usize, usize), HiddenPose>,
    frame: u64,
    /// One body's resolved layers, refilled per body.
    resolved: Vec<ResolvedLayer>,
    extras: Vec<ActorRigSubmission>,
}

type RenderPose = Arc<[RenderBoneTransform]>;
type ConvertedPose = (Arc<[BoneTransform]>, Option<RenderPose>, u64);
type HiddenPose = (RenderPose, Arc<[u32]>, RenderPose, u64);

/// Frames a pose may go undrawn before its conversion is released.
const LAYER_POSE_RETENTION_FRAMES: u64 = 4;

impl LayerPoseCache {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        let oldest = self.frame.saturating_sub(LAYER_POSE_RETENTION_FRAMES);
        self.converted.retain(|_, entry| entry.2 >= oldest);
        self.hidden.retain(|_, entry| entry.3 >= oldest);
    }

    /// Converts a shared source pose once while its allocation remains in use.
    pub(super) fn convert(
        &mut self,
        pose: &Arc<[BoneTransform]>,
    ) -> Option<Arc<[RenderBoneTransform]>> {
        let frame = self.frame;
        let key = Arc::as_ptr(pose).cast::<u8>() as usize;
        match self.converted.get_mut(&key) {
            Some(entry) if Arc::ptr_eq(&entry.0, pose) => {
                entry.2 = frame;
                entry.1.clone()
            }
            _ => {
                let converted = convert(pose);
                self.converted
                    .insert(key, (Arc::clone(pose), converted.clone(), frame));
                converted
            }
        }
    }

    /// Reuses the zero-scale variant of a pose for an unchanged hidden-bone list.
    pub(super) fn hide(
        &mut self,
        poses: &Arc<[RenderBoneTransform]>,
        hidden: &Arc<[u32]>,
    ) -> Arc<[RenderBoneTransform]> {
        let frame = self.frame;
        let key = (
            Arc::as_ptr(poses).cast::<u8>() as usize,
            Arc::as_ptr(hidden).cast::<u8>() as usize,
        );
        match self.hidden.get_mut(&key) {
            Some(entry) if Arc::ptr_eq(&entry.0, poses) && Arc::ptr_eq(&entry.1, hidden) => {
                entry.3 = frame;
                Arc::clone(&entry.2)
            }
            _ => {
                let result = hide_bones(poses, hidden);
                self.hidden.insert(
                    key,
                    (
                        Arc::clone(poses),
                        Arc::clone(hidden),
                        Arc::clone(&result),
                        frame,
                    ),
                );
                result
            }
        }
    }
}

fn convert(bones: &[BoneTransform]) -> Option<Arc<[RenderBoneTransform]>> {
    bones
        .iter()
        .map(|bone| {
            RenderBoneTransform::from_model_space_scaled(
                bone.rotation,
                bone.translation_scale,
                bone.axis_scale,
            )
        })
        .collect::<Option<Vec<_>>>()
        .filter(|bones| !bones.is_empty())
        .map(Arc::from)
}

/// Packs a colour multiplier as the instance tint word; white leaves the texture untouched.
fn pack_layer_tint(color: [f32; 4]) -> u32 {
    let byte = |value: f32| {
        if value.is_finite() {
            (value.clamp(0.0, 1.0) * 255.0).round() as u32
        } else {
            255
        }
    };
    let [red, green, blue, alpha] = color.map(byte);
    if (red, green, blue, alpha) == (255, 255, 255, 255) {
        0
    } else {
        (alpha << 24) | (blue << 16) | (green << 8) | red
    }
}

fn resolve(
    submission: &ActorRigSubmission,
    layers: &[RenderTextureLayer],
    artwork: &ActorArtworkPages,
    cache: &mut LayerPoseCache,
    resolved: &mut Vec<ResolvedLayer>,
) {
    resolved.clear();
    resolved.extend(
        layers
            .iter()
            .filter_map(|layer| {
                let model = match layer.geometry {
                    None if layer.pose.is_empty() => None,
                    None => Some(LayerModel {
                        rig: submission.input.rig,
                        previous: cache.convert(&layer.previous_pose)?,
                        current: cache.convert(&layer.pose)?,
                    }),
                    Some(geometry) => Some(LayerModel {
                        rig: layer_geometry_rig_id(submission.input.rig, geometry),
                        previous: cache.convert(&layer.previous_pose)?,
                        current: cache.convert(&layer.pose)?,
                    }),
                };
                Some(ResolvedLayer {
                    material: render::ActorMaterial {
                        glint: Default::default(),
                        kind: layer.material,
                        state: layer.material_state,
                        dissolve_multiplier: layer.overlay[3],
                        light_color_multiplier: layer.light_color_multiplier,
                    },
                    model,
                    ignore_lighting: layer.ignore_lighting,
                    location: match layer.multitexture {
                        Some([second, third]) => artwork.multitexture_location(
                            submission.input.rig,
                            [layer.source, second, third],
                        )?,
                        None => artwork.variant_location(submission.input.rig, layer.source)?,
                    },
                    tint: pack_layer_tint(layer.color),
                    overlay: (layer.overlay[3] > 0.0).then(|| pack_overlay_rgba8(layer.overlay)),
                    hidden_bones: Arc::clone(&layer.hidden_bones),
                    uv_anim: layer.uv_anim,
                })
            })
            .take(MAX_TEXTURE_LAYERS),
    );
}

/// The zero-scale pose vanilla uses to hide a bone.
fn hide_bones(poses: &Arc<[RenderBoneTransform]>, hidden: &[u32]) -> Arc<[RenderBoneTransform]> {
    let mut poses = poses.to_vec();
    for &index in hidden {
        if let Some(bone) = poses.get_mut(index as usize) {
            bone.translation_scale = [0.0; 4];
        }
    }
    poses.into()
}

fn layered(
    body: &ActorRigSubmission,
    layer: &ResolvedLayer,
    index: usize,
    cache: &mut LayerPoseCache,
) -> ActorRigSubmission {
    let mut submission = body.clone();
    if index > 0 {
        submission.input.identity.layer = ACTOR_LAYER_TEXTURE_BASE + (index - 1) as u8;
    }
    if let Some(model) = &layer.model {
        submission.input.rig = model.rig;
        submission.input.previous_bones = Arc::clone(&model.previous);
        submission.input.current_bones = Arc::clone(&model.current);
    }
    submission.texture_layer = layer.location.layer();
    submission.material = layer.material;
    if matches!(
        layer.material.kind,
        assets::EntityRenderMaterial::DissolveDepth | assets::EntityRenderMaterial::DissolveColor
    ) {
        submission.overlay_rgba8 = 0;
    }
    submission.tint = layer.tint;
    submission.uv_anim = layer.uv_anim;
    if layer.ignore_lighting {
        submission.light = 0;
    }
    if let Some(overlay) = layer.overlay
        && !matches!(
            layer.material.kind,
            assets::EntityRenderMaterial::DissolveDepth
                | assets::EntityRenderMaterial::DissolveColor
        )
    {
        submission.overlay_rgba8 = overlay;
    }
    if !layer.hidden_bones.is_empty() {
        submission.input.previous_bones =
            cache.hide(&submission.input.previous_bones, &layer.hidden_bones);
        submission.input.current_bones =
            cache.hide(&submission.input.current_bones, &layer.hidden_bones);
    }
    submission
}

/// Applies each entity body's selected texture layers to the batch: the first replaces the
/// body's texture, the rest are appended as extra layers of the same actor.
#[cfg(any(test, feature = "test-support"))]
pub fn apply_render_layers<'a>(
    batch: &mut ActorPresentationBatch,
    rig_of: impl Fn(u64) -> Option<ActorRigSnapshot<'a>>,
    artwork: &ActorArtworkPages,
) {
    apply_render_layers_cached(
        batch,
        |id| rig_of(id).map(|rig| Cow::Borrowed(rig.render)),
        artwork,
        &mut LayerPoseCache::default(),
    );
}

/// [`apply_render_layers`] reusing `cache`'s conversions across frames.
pub fn apply_render_layers_cached<'a>(
    batch: &mut ActorPresentationBatch,
    mut layers_of: impl FnMut(u64) -> Option<Cow<'a, [RenderTextureLayer]>>,
    artwork: &ActorArtworkPages,
    cache: &mut LayerPoseCache,
) {
    let mut resolved = std::mem::take(&mut cache.resolved);
    let mut extras = std::mem::take(&mut cache.extras);
    for index in 0..batch.submissions.len() {
        let body = &batch.submissions[index];
        let identity = body.input.identity;
        if identity.layer != ACTOR_LAYER_BODY || !batch.artwork.contains_key(&identity) {
            continue;
        }
        let Some(layers) = layers_of(identity.runtime_id) else {
            continue;
        };
        resolve(body, &layers, artwork, cache, &mut resolved);
        if resolved.is_empty() {
            continue;
        }
        let pristine = body.clone();
        for (layer_index, layer) in resolved.iter().enumerate() {
            let submission = layered(&pristine, layer, layer_index, cache);
            batch
                .artwork
                .insert(submission.input.identity, layer.location);
            if layer_index == 0 {
                batch.submissions[index] = submission;
            } else {
                extras.push(submission);
            }
        }
    }
    batch.submissions.append(&mut extras);
    resolved.clear();
    cache.resolved = resolved;
    cache.extras = extras;
}

#[cfg(test)]
mod tests {
    use super::pack_layer_tint;

    #[test]
    fn white_is_untinted_and_other_colours_enable_the_tint_word() {
        assert_eq!(pack_layer_tint([1.0, 1.0, 1.0, 1.0]), 0);
        assert_eq!(pack_layer_tint([1.0, 0.0, 0.0, 1.0]), 0xff00_00ff);
        assert_eq!(pack_layer_tint([0.0, 0.0, 1.0, 1.0]), 0xffff_0000);
        assert_eq!(pack_layer_tint([1.0, 1.0, 1.0, 0.5]), 0x80ff_ffff);
        assert_eq!(pack_layer_tint([1.0, 0.0, 0.0, 0.5]), 0x8000_00ff);
    }
}
