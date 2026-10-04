//! Animated player skin textures on their own geometry and shared actor pose.

use std::sync::Arc;

use client_world::ActorRigSnapshot;
use render::{
    ACTOR_LAYER_BODY, ActorArtworkLocation, ActorArtworkPages, ActorRigGeometry, ActorRigRoute,
    ActorRigSubmission, EquipmentRaster,
};

use super::{
    actors::ActorPresentationBatch, entity_layers::LayerPoseCache, skin_rig::SkinRigCache,
};

/// Animated skin layers follow the cape and precede generic render-controller layers.
pub const SKIN_LAYER_BASE: u8 = super::cape::ACTOR_LAYER_CAPE + 1;

/// Whether an instance belongs to a player's animated skin, rather than equipment.
pub fn is_skin_layer(layer: u8) -> bool {
    (SKIN_LAYER_BASE..SKIN_LAYER_BASE + protocol::MAX_SKIN_ANIMATION_LAYERS as u8).contains(&layer)
}

/// Texture pages and converted poses retained while the same skin images remain selected.
#[derive(Default)]
pub struct SkinLayerCache {
    base: [u8; 32],
    rasters: Vec<EquipmentRaster>,
    desired: Vec<EquipmentRaster>,
    locations: Vec<Option<ActorArtworkLocation>>,
    pages: Option<ActorArtworkPages>,
    poses: LayerPoseCache,
    extras: Vec<ActorRigSubmission>,
}

/// Two raster entries point at the same retained image and image dimensions.
fn same_image(left: &EquipmentRaster, right: &EquipmentRaster) -> bool {
    left.width == right.width
        && left.height == right.height
        && Arc::ptr_eq(&left.rgba8, &right.rgba8)
}

/// Keeps source dimensions exact when an animation image fits the artwork page format.
fn image_raster(image: &protocol::SkinAnimation) -> Option<EquipmentRaster> {
    Some(EquipmentRaster {
        width: u16::try_from(image.width).ok()?,
        height: u16::try_from(image.height).ok()?,
        rgba8: Arc::clone(&image.rgba8),
    })
}

impl SkinLayerCache {
    /// Rebuilds rectangular image pages only when the base artwork or selected image set changes.
    fn update_pages(&mut self, base: &ActorArtworkPages) -> Option<ActorArtworkPages> {
        let changed = self.base != base.identity()
            || self.desired.len() != self.rasters.len()
            || !self
                .desired
                .iter()
                .zip(&self.rasters)
                .all(|(a, b)| same_image(a, b));
        if !changed {
            return None;
        }
        self.base = base.identity();
        std::mem::swap(&mut self.rasters, &mut self.desired);
        if self.rasters.is_empty() {
            self.locations.clear();
            return self.pages.take().map(|_| base.clone());
        }
        let (pages, locations) = base.clone().with_equipment_rasters(&self.rasters);
        self.locations = locations;
        self.pages = Some(pages.clone());
        Some(pages)
    }

    /// Appends skin layers using their own mesh/poses and the body's placement and lighting.
    pub fn apply<'a>(
        &mut self,
        batch: &mut ActorPresentationBatch,
        base: &ActorArtworkPages,
        rig_of: impl Fn(u64) -> Option<ActorRigSnapshot<'a>>,
        rigs: &mut SkinRigCache,
        mut register: impl FnMut(ActorRigGeometry),
    ) -> Option<ActorArtworkPages> {
        self.desired.clear();
        self.poses.begin_frame();
        for body in &batch.submissions {
            if body.input.identity.layer != ACTOR_LAYER_BODY || body.route == ActorRigRoute::NoDraw
            {
                continue;
            }
            let Some(rig) = rig_of(body.input.identity.runtime_id) else {
                continue;
            };
            for layer in rig.skin_layers {
                let Some(raster) = image_raster(&layer.image) else {
                    continue;
                };
                if !self.desired.iter().any(|known| same_image(known, &raster)) {
                    self.desired.push(raster);
                }
            }
        }
        let changed = self.update_pages(base);
        for body in &batch.submissions {
            if body.input.identity.layer != ACTOR_LAYER_BODY || body.route == ActorRigRoute::NoDraw
            {
                continue;
            }
            let Some(rig) = rig_of(body.input.identity.runtime_id) else {
                continue;
            };
            for layer in rig.skin_layers {
                let Some(raster) = image_raster(&layer.image) else {
                    continue;
                };
                let Some(location) = self
                    .rasters
                    .iter()
                    .position(|known| same_image(known, &raster))
                    .and_then(|index| self.locations[index])
                else {
                    continue;
                };
                let Some(geometry) = rigs.rig(&layer.geometry, &mut register) else {
                    continue;
                };
                let (Some(previous), Some(current)) = (
                    self.poses.convert(&layer.previous),
                    self.poses.convert(&layer.current),
                ) else {
                    continue;
                };
                let mut submission = body.clone();
                submission.input.identity.layer = SKIN_LAYER_BASE + layer.image.kind.slot() as u8;
                submission.input.rig = geometry;
                submission.input.previous_bones = if layer.hidden_bones.is_empty() {
                    previous
                } else {
                    self.poses.hide(&previous, &layer.hidden_bones)
                };
                submission.input.current_bones = if layer.hidden_bones.is_empty() {
                    current
                } else {
                    self.poses.hide(&current, &layer.hidden_bones)
                };
                submission.route = ActorRigRoute::Compiled;
                submission.texture_layer = location.layer();
                submission.uv_anim = layer.uv_anim;
                batch.artwork.insert(submission.input.identity, location);
                self.extras.push(submission);
            }
        }
        batch.submissions.append(&mut self.extras);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One retained rectangular sprite sheet for the texture-page cache.
    fn raster() -> EquipmentRaster {
        EquipmentRaster {
            width: 24,
            height: 512,
            rgba8: vec![255; 24 * 512 * 4].into(),
        }
    }

    #[test]
    fn unchanged_animation_images_reuse_pages_and_removed_images_restore_base() {
        let base = ActorArtworkPages::default();
        let image = raster();
        let mut cache = SkinLayerCache::default();
        cache.desired.push(image.clone());
        let first = cache.update_pages(&base).unwrap();
        assert_eq!(first.pages()[0].dimensions(), (image.width, image.height));
        cache.desired.clear();
        cache.desired.push(image);
        assert!(cache.update_pages(&base).is_none());
        assert_eq!(cache.pages.as_ref().unwrap().identity(), first.identity());
        cache.desired.clear();
        assert_eq!(
            cache.update_pages(&base).unwrap().identity(),
            base.identity()
        );
    }
}
