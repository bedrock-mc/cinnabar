//! Animated player skin textures on their own geometry and shared actor pose.

use std::{collections::HashMap, sync::Arc};

use client_world::ActorRigSnapshot;
use render::{
    ACTOR_LAYER_BODY, ActorArtworkLocation, ActorArtworkPages, ActorRigRoute, ActorRigSubmission,
    EquipmentRaster,
};
use render_model::ActorRigGeometry;

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
    desired_index: HashMap<ImageKey, (EquipmentRaster, usize)>,
    image_index: HashMap<ImageKey, (EquipmentRaster, usize)>,
    locations: Vec<Option<ActorArtworkLocation>>,
    pages: Option<ActorArtworkPages>,
    poses: LayerPoseCache,
    extras: Vec<ActorRigSubmission>,
}

/// Keys are valid while their corresponding retained raster owns the immutable allocation.
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct ImageKey {
    address: usize,
    width: u16,
    height: u16,
}

impl ImageKey {
    /// Preserves the cache's existing pointer-and-dimensions equality exactly.
    fn new(raster: &EquipmentRaster) -> Self {
        Self {
            address: raster.rgba8.as_ptr() as usize,
            width: raster.width,
            height: raster.height,
        }
    }
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
    /// Retains the first occurrence of each immutable source image in draw order.
    fn retain_image(&mut self, raster: EquipmentRaster) {
        #[cfg(test)]
        tests::record_probe();
        // The desired vector may be cleared by the next frame before its index is reused.
        if self.desired.is_empty() {
            self.desired_index.clear();
        }
        if let std::collections::hash_map::Entry::Vacant(entry) =
            self.desired_index.entry(ImageKey::new(&raster))
        {
            entry.insert((raster.clone(), self.desired.len()));
            self.desired.push(raster);
        }
    }

    /// Resolves one retained source to its published artwork location.
    fn image_location(&self, raster: &EquipmentRaster) -> Option<ActorArtworkLocation> {
        #[cfg(test)]
        tests::record_probe();
        self.image_index
            .get(&ImageKey::new(raster))
            .and_then(|(_, index)| self.locations[*index])
    }

    /// Rebuilds rectangular image pages only when the base artwork changes, an image without a
    /// published cell becomes visible, or none remains visible. Images that leave view, as when
    /// turning or on a death, keep their cells, so the pages are not repacked and rehashed then.
    fn update_pages(&mut self, base: &ActorArtworkPages) -> Option<ActorArtworkPages> {
        let published = self.base == base.identity()
            && if self.desired.is_empty() {
                self.rasters.is_empty()
            } else {
                self.desired.iter().all(|raster| {
                    self.image_index
                        .get(&ImageKey::new(raster))
                        .is_some_and(|(retained, _)| same_image(retained, raster))
                })
            };
        if published {
            return None;
        }
        self.base = base.identity();
        std::mem::swap(&mut self.rasters, &mut self.desired);
        self.image_index.clear();
        self.image_index.extend(
            self.rasters
                .iter()
                .enumerate()
                .map(|(index, image)| (ImageKey::new(image), (image.clone(), index))),
        );
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
                self.retain_image(raster);
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
                let Some(location) = self.image_location(&raster) else {
                    continue;
                };
                let Some(geometry) = rigs.rig(&layer.geometry, layer.mesh.as_ref(), &mut register)
                else {
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
    /// Animated skins leaving view keep their published cells; only a newly visible image
    /// repacks, so a shrinking visible set never rehashes every visible skin image.
    #[test]
    fn images_leaving_view_keep_their_pages_until_a_new_image_appears() {
        let base = ActorArtworkPages::default();
        let (first, second, third) = (raster(), raster(), raster());
        let mut cache = SkinLayerCache::default();
        let publish = |cache: &mut SkinLayerCache, visible: &[&EquipmentRaster]| {
            cache.desired.clear();
            for image in visible {
                cache.retain_image((*image).clone());
            }
            cache.update_pages(&base)
        };
        let both = publish(&mut cache, &[&first, &second]).expect("first sight packs");
        assert!(publish(&mut cache, &[&first]).is_none(), "leaving view");
        assert!(cache.image_location(&first).is_some());
        assert!(publish(&mut cache, &[&second, &first]).is_none(), "returning");
        assert_eq!(cache.pages.as_ref().unwrap().identity(), both.identity());
        publish(&mut cache, &[&first, &third]).expect("a new image repacks");
        assert!(cache.image_location(&third).is_some());
        assert!(cache.image_location(&second).is_none(), "the repack drops what left");
    }

    thread_local! {
        static LOOKUP_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Counts explicit source-image candidates on this test thread.
    pub(super) fn record_probe() {
        LOOKUP_PROBES.with(|count| count.set(count.get() + 1));
    }

    #[test]
    fn bounded_lookup_work_for_distinct_skin_animation_images() {
        let images: Vec<_> = (0..render_model::MAX_RENDERED_PLAYERS)
            .map(|_| raster())
            .collect();
        let mut cache = SkinLayerCache::default();
        let base = ActorArtworkPages::default();
        LOOKUP_PROBES.with(|count| count.set(0));
        for image in &images {
            cache.retain_image(image.clone());
        }
        cache.update_pages(&base).unwrap();
        assert_eq!(
            cache.rasters.len(),
            images.len(),
            "equal pixels in separate allocations remain separate sources"
        );
        for image in &images {
            assert!(cache.image_location(image).is_some());
        }
        let work = LOOKUP_PROBES.with(std::cell::Cell::get);
        assert!(
            work <= images.len() * 4,
            "{work} image candidates for {} layers",
            images.len()
        );
        cache.desired.clear();
        for image in &images {
            cache.retain_image(image.clone());
        }
        assert!(cache.update_pages(&base).is_none());
        cache.desired.clear();
        cache.update_pages(&base).unwrap();
        for image in &images {
            assert!(cache.image_location(image).is_none());
        }
    }
}
