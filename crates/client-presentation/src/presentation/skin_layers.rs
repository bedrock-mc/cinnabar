//! Animated player skin textures on their own geometry and shared actor pose.

use std::sync::Arc;

use bevy::platform::collections::{HashMap, hash_map::Entry};

use actor_animation::ActorRigSnapshot;
use render_model::ActorRigGeometry;
use render_model::{
    ACTOR_LAYER_BODY, ACTOR_LAYER_SKIN_BASE, ActorArtworkLocation, ActorArtworkPages,
    ActorRigRoute, ActorRigSubmission, EquipmentRaster,
};

use super::{
    actors::ActorPresentationBatch, entity_layers::LayerPoseCache, skin_rig::SkinRigCache,
};

const _: () = assert!(ACTOR_LAYER_SKIN_BASE > super::cape::ACTOR_LAYER_CAPE);

/// Whether an instance belongs to a player's animated skin, rather than equipment.
pub fn is_skin_layer(layer: u8) -> bool {
    (ACTOR_LAYER_SKIN_BASE..ACTOR_LAYER_SKIN_BASE + render_api::MAX_SKIN_ANIMATION_LAYERS as u8)
        .contains(&layer)
}

/// One full rendered crowd's animation images may remain resident between views.
const MAX_RESIDENT_SKIN_IMAGES: usize =
    render_model::MAX_RENDERED_PLAYERS * render_api::MAX_SKIN_ANIMATION_LAYERS;

/// Texture pages and converted poses retained while the same artwork base remains selected.
#[derive(Default)]
pub struct SkinLayerCache {
    base: Option<ActorArtworkPages>,
    packed: bool,
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
fn image_raster(image: &render_api::SkinAnimation) -> Option<EquipmentRaster> {
    Some(EquipmentRaster {
        width: u16::try_from(image.width).ok()?,
        height: u16::try_from(image.height).ok()?,
        rgba8: Arc::clone(&image.rgba8),
    })
}

impl SkinLayerCache {
    /// Retains the first occurrence of each immutable source image in draw order.
    fn retain_image(&mut self, raster: EquipmentRaster) {
        if raster.width == 0
            || raster.height == 0
            || raster.rgba8.len() != usize::from(raster.width) * usize::from(raster.height) * 4
        {
            return;
        }
        #[cfg(test)]
        tests::record_probe();
        // The desired vector may be cleared by the next frame before its index is reused.
        if self.desired.is_empty() {
            self.desired_index.clear();
        }
        if let Entry::Vacant(entry) = self.desired_index.entry(ImageKey::new(&raster)) {
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

    /// Retains bounded immutable page slots across camera turns and empty views.
    /// Only new sources hash pixels; a full cache replaces slots that are no longer visible.
    fn update_pages(&mut self, base: &ActorArtworkPages) -> Option<ActorArtworkPages> {
        let base_changed = self
            .base
            .as_ref()
            .is_none_or(|old| !old.shares_storage_with(base));
        let had_pages = self.pages.is_some();
        if base_changed {
            self.clear_pages();
            self.base = Some(base.clone());
        }
        if self.desired.iter().all(|raster| {
            self.image_index
                .get(&ImageKey::new(raster))
                .is_some_and(|(retained, _)| same_image(retained, raster))
        }) {
            return (base_changed && had_pages).then(|| base.clone());
        }
        if self.packed {
            self.clear_pages();
        }
        let mut reusable: Vec<_> = self
            .rasters
            .iter()
            .enumerate()
            .filter_map(|(index, raster)| {
                (!self.desired_index.contains_key(&ImageKey::new(raster))).then_some(index)
            })
            .rev()
            .collect();
        let mut updates = Vec::new();
        for raster in &self.desired {
            if self.image_index.contains_key(&ImageKey::new(raster)) {
                continue;
            }
            let slot = if self.rasters.len() < MAX_RESIDENT_SKIN_IMAGES {
                self.rasters.push(raster.clone());
                self.locations.push(None);
                self.rasters.len() - 1
            } else if let Some(slot) = reusable.pop() {
                self.rasters[slot] = raster.clone();
                slot
            } else {
                return Some(self.pack_visible(base));
            };
            updates.push((base.pages().len() + slot, raster.clone()));
        }
        let bytes = base
            .pages()
            .iter()
            .map(|page| page.pixels().len())
            .sum::<usize>()
            + render_model::PLAYER_SKIN_BUDGET_BYTES
            + self
                .rasters
                .iter()
                .map(|raster| raster.rgba8.len())
                .sum::<usize>();
        // Preserve grouped admission and its downscaling when retained slots cannot fit.
        if bytes > render_model::MAX_ACTOR_GPU_PIXEL_BYTES
            || base.pages().len() + self.rasters.len() >= render_model::MAX_ACTOR_TEXTURE_PAGES
        {
            return Some(self.pack_visible(base));
        }
        let previous = self.pages.take().unwrap_or_else(|| base.clone());
        let (pages, locations) = previous.with_equipment_raster_page_updates(&updates);
        if locations.iter().any(Option::is_none) {
            return Some(self.pack_visible(base));
        }
        for ((page, _), location) in updates.into_iter().zip(locations) {
            self.locations[page - base.pages().len()] = location;
        }
        self.index_images();
        self.pages = Some(pages.clone());
        Some(pages)
    }

    /// Drops the old base's page slots without discarding this frame's selected images.
    fn clear_pages(&mut self) {
        self.rasters.clear();
        self.image_index.clear();
        self.locations.clear();
        self.pages = None;
        self.packed = false;
    }

    /// Rebuilds the pointer index after page slots have been appended or reused.
    fn index_images(&mut self) {
        self.image_index.clear();
        self.image_index.extend(
            self.rasters
                .iter()
                .enumerate()
                .map(|(index, image)| (ImageKey::new(image), (image.clone(), index))),
        );
    }

    /// Uses the existing grouped budget policy when exact single-layer residency cannot fit.
    fn pack_visible(&mut self, base: &ActorArtworkPages) -> ActorArtworkPages {
        self.rasters.clone_from(&self.desired);
        let (pages, locations) = base.clone().with_equipment_rasters(&self.rasters);
        self.locations = locations;
        self.index_images();
        self.pages = Some(pages.clone());
        self.packed = true;
        pages
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
                submission.input.identity.layer =
                    ACTOR_LAYER_SKIN_BASE + layer.image.kind.slot() as u8;
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
#[path = "skin_layers_tests.rs"]
mod tests;
