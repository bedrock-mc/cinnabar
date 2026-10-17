//! Bounded CPU models and atlas slots for current item consumers.
use std::sync::Arc;

use bevy::platform::collections::HashMap;

use render_model::{
    DroppedItemModel, DroppedItemSpawnPose, MAX_ITEM_LAYERS, dropped_item::DroppedItemBounds,
};

mod publication;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum ModelKey {
    PottedBlock {
        id: u32,
    },
    Block {
        hashed: bool,
        id: u32,
    },
    CarriedBlock {
        hashed: bool,
        id: u32,
    },
    SessionBlock {
        page: u16,
        uv: [u16; 4],
    },
    Icon {
        page: u16,
        uv: [u16; 4],
        native: bool,
    },
}

impl ModelKey {
    /// Shares a pack cube by its admitted six-face sheet rather than its thumbnail.
    pub(super) fn session_block(sheet: ui::IconRef) -> Self {
        Self::SessionBlock {
            page: sheet.page,
            uv: sheet.uv,
        }
    }

    /// Uses the resolved image, so metadata fallback and identifier aliases share a model.
    pub(super) fn icon(icon: ui::IconRef, native: bool) -> Self {
        Self::Icon {
            page: icon.page,
            uv: icon.uv,
            native,
        }
    }
}

/// Keeps geometry and pixels only for models occupying the bounded atlas.
#[derive(Default)]
pub(crate) struct ModelCache {
    session: u64,
    assets: Option<render_scene::ChunkTextureAssetIdentity>,
    icons: u64,
    pub(super) revision: u64,
    pub(super) models: Vec<DroppedItemModel>,
    layers: usize,
    pixel_bytes: usize,
    pub(super) shared: Arc<[DroppedItemModel]>,
    index: HashMap<ModelKey, usize>,
    /// Bounded layer estimates and permanent block failures still demanded.
    rejected: HashMap<ModelKey, RejectedModel>,
    residents: Vec<Option<Resident>>,
    free_slots: Vec<usize>,
    reclaimable: Vec<usize>,
    /// Free layers plus layers belonging to consumers absent from this frame.
    available: usize,
    /// Free source bytes plus bytes belonging to consumers absent from this frame.
    available_pixels: usize,
    demand_ready: bool,
    frame: u64,
    dirty: bool,
    pub(super) spawn_poses: HashMap<(u64, u64), DroppedItemSpawnPose>,
    #[cfg(test)]
    inspections: usize,
}

struct Resident {
    key: ModelKey,
    /// Last frame with a current scene consumer.
    requested: u64,
    /// Last frame that wanted this model for speculative animation residency.
    warmed: u64,
    layers: usize,
    /// Uninspected until a dropped consumer needs bounds; an inspected unknown stays unknown.
    bounds: Option<Option<DroppedItemBounds>>,
}

struct RejectedModel {
    /// None marks a block that cannot be built from the current assets.
    layers: Option<usize>,
    pixel_bytes: Option<usize>,
    requested: u64,
}

/// Counts retained source pixels independently of physical GPU layer expansion.
fn pixel_cost(model: &DroppedItemModel) -> usize {
    match model {
        DroppedItemModel::Vacant => 0,
        DroppedItemModel::Cube(cube) => cube.faces.iter().map(|face| face.len()).sum(),
        DroppedItemModel::Block(block) => block
            .materials
            .iter()
            .map(|(sprite, _)| sprite.rgba8.len())
            .sum(),
        DroppedItemModel::Sprite(sprite) | DroppedItemModel::NativeSprite(sprite) => {
            sprite.rgba8.len()
        }
    }
}

impl ModelCache {
    /// Reuses a successfully admitted model while preserving current-consumer protection.
    pub(super) fn resident(&mut self, key: &ModelKey) -> Option<u32> {
        self.request(key);
        self.index.get(key).map(|slot| *slot as u32)
    }

    /// Derives bounds once per slot occupant, without inspecting geometry for other consumers.
    pub(super) fn bounds(&mut self, model: u32) -> Option<DroppedItemBounds> {
        let entry = self.residents.get_mut(model as usize)?.as_mut()?;
        *entry
            .bounds
            .get_or_insert_with(|| DroppedItemBounds::from_model(&self.models[model as usize]))
    }

    /// Drops models from another session, block texture set or icon generation.
    pub(super) fn sync(
        &mut self,
        session: u64,
        assets: Option<render_scene::ChunkTextureAssetIdentity>,
        icons: u64,
    ) {
        if self.session != session || self.assets != assets || self.icons != icons {
            *self = Self {
                session,
                assets,
                icons,
                revision: self.revision.wrapping_add(1),
                ..Self::default()
            };
        }
    }

    /// Starts demand collection before any residency decisions are made.
    pub(super) fn begin_frame(&mut self) {
        self.frame = self
            .frame
            .checked_add(1)
            .expect("model demand frame overflow");
        self.reclaimable.clear();
        self.available = MAX_ITEM_LAYERS - 1 - self.layers;
        self.available_pixels = assets::texture::policy::MAX_SUBSCRIBER_BYTES - self.pixel_bytes;
        self.demand_ready = false;
    }

    /// Protects a current consumer, including a late request after demand collection.
    pub(super) fn request(&mut self, key: &ModelKey) {
        if let Some(&slot) = self.index.get(key) {
            let entry = self.residents[slot].as_mut().expect("indexed resident");
            if entry.requested != self.frame {
                if self.demand_ready {
                    self.available -= entry.layers;
                    self.available_pixels -= pixel_cost(&self.models[slot]);
                }
                entry.requested = self.frame;
            }
        } else if let Some(entry) = self.rejected.get_mut(key) {
            entry.requested = self.frame;
        }
    }

    /// Counts reclaimable capacity once, scanning only the bounded resident table.
    pub(super) fn finish_demand(&mut self) {
        self.rejected.retain(|_, entry| {
            #[cfg(test)]
            {
                self.inspections += 1;
            }
            entry.requested == self.frame
        });
        self.reclaimable.clear();
        self.available = MAX_ITEM_LAYERS - 1 - self.layers;
        self.available_pixels = assets::texture::policy::MAX_SUBSCRIBER_BYTES - self.pixel_bytes;
        for (slot, entry) in self.residents.iter().enumerate().rev() {
            #[cfg(test)]
            {
                self.inspections += 1;
            }
            if let Some(entry) = entry.as_ref().filter(|entry| entry.requested != self.frame) {
                self.reclaimable.push(slot);
                self.available += entry.layers;
                self.available_pixels += pixel_cost(&self.models[slot]);
            }
        }
        self.demand_ready = true;
    }

    /// Remembers admission costs; absent layers mean a permanent block failure.
    fn remember_rejection(
        &mut self,
        key: ModelKey,
        layers: Option<usize>,
        pixel_bytes: Option<usize>,
    ) {
        if self.rejected.len() < MAX_ITEM_LAYERS - 1 || self.rejected.contains_key(&key) {
            self.rejected.insert(
                key,
                RejectedModel {
                    layers,
                    pixel_bytes,
                    requested: self.frame,
                },
            );
        }
    }

    /// Builds missing models that fit, skipping remembered permanent block failures.
    pub(super) fn model(
        &mut self,
        key: ModelKey,
        cost: impl FnOnce() -> Option<usize>,
        build: impl FnOnce() -> Option<DroppedItemModel>,
    ) -> Option<u32> {
        self.admit(key, cost, build, true)
    }

    /// Caches a speculative icon in unused source capacity; publication applies GPU limits.
    pub(super) fn warm_icon(
        &mut self,
        key: ModelKey,
        build: impl FnOnce() -> Option<DroppedItemModel>,
    ) -> Option<u32> {
        self.admit(key, || Some(1), build, false)
    }

    /// Admits a model within both budgets, optionally reclaiming absent consumers.
    fn admit(
        &mut self,
        key: ModelKey,
        cost: impl FnOnce() -> Option<usize>,
        build: impl FnOnce() -> Option<DroppedItemModel>,
        reclaim: bool,
    ) -> Option<u32> {
        if reclaim {
            self.request(&key);
        }
        if let Some(&slot) = self.index.get(&key) {
            if !reclaim {
                self.residents[slot]
                    .as_mut()
                    .expect("indexed resident")
                    .warmed = self.frame;
            }
            return Some(slot as u32);
        }
        let available = if reclaim {
            self.available
        } else {
            MAX_ITEM_LAYERS - 1 - self.layers
        };
        let available_pixels = if reclaim {
            self.available_pixels
        } else {
            assets::texture::policy::MAX_SUBSCRIBER_BYTES - self.pixel_bytes
        };
        // Repeated rejections reuse the capacity summary without scans or model construction.
        if available == 0 {
            return None;
        }
        let estimate = match self.rejected.get(&key) {
            Some(entry) => {
                if entry
                    .pixel_bytes
                    .is_some_and(|bytes| bytes > available_pixels)
                {
                    return None;
                }
                entry.layers
            }
            None => {
                let estimate = cost();
                if estimate.is_none()
                    && matches!(key, ModelKey::Block { .. } | ModelKey::PottedBlock { .. })
                {
                    self.remember_rejection(key.clone(), None, None);
                }
                estimate
            }
        };
        let cost = estimate?;
        if cost == 0 {
            return None;
        }
        if cost > available {
            let pixel_bytes = self.rejected.get(&key).and_then(|entry| entry.pixel_bytes);
            self.remember_rejection(key, Some(cost), pixel_bytes);
            return None;
        }
        let Some(model) = build() else {
            if matches!(key, ModelKey::Block { .. } | ModelKey::PottedBlock { .. }) {
                self.remember_rejection(key, None, None);
            }
            return None;
        };
        assert_eq!(
            model.texture_layers(),
            cost,
            "model layer estimate must match its geometry"
        );
        let pixel_bytes = pixel_cost(&model);
        if pixel_bytes > available_pixels {
            self.remember_rejection(key, Some(cost), Some(pixel_bytes));
            return None;
        }
        self.rejected.remove(&key);
        while self.layers + cost >= MAX_ITEM_LAYERS
            || pixel_bytes
                > assets::texture::policy::MAX_SUBSCRIBER_BYTES.saturating_sub(self.pixel_bytes)
        {
            let slot = self
                .reclaimable
                .pop()
                .expect("counted reclaimable capacity");
            #[cfg(test)]
            {
                self.inspections += 1;
            }
            if self.residents[slot]
                .as_ref()
                .is_some_and(|entry| entry.requested == self.frame)
            {
                continue;
            }
            let entry = self.residents[slot].take().expect("reclaimable resident");
            self.layers -= entry.layers;
            self.pixel_bytes -= pixel_cost(&self.models[slot]);
            self.index.remove(&entry.key);
            self.models[slot] = DroppedItemModel::Vacant;
            self.free_slots.push(slot);
        }
        let slot = self.free_slots.pop().unwrap_or_else(|| {
            self.models.push(DroppedItemModel::Vacant);
            self.residents.push(None);
            self.models.len() - 1
        });
        self.models[slot] = model;
        self.index.insert(key.clone(), slot);
        self.residents[slot] = Some(Resident {
            key,
            requested: if reclaim { self.frame } else { 0 },
            warmed: if reclaim { 0 } else { self.frame },
            layers: cost,
            bounds: None,
        });
        self.layers += cost;
        self.pixel_bytes += pixel_bytes;
        if !reclaim && self.demand_ready {
            self.reclaimable.push(slot);
        } else {
            self.available -= cost;
            self.available_pixels -= pixel_bytes;
        }
        self.dirty = true;
        Some(slot as u32)
    }

    /// Whether this slot has a consumer in the current frame, including hidden candidates.
    fn demanded(&self, slot: usize) -> bool {
        self.residents[slot]
            .as_ref()
            .is_some_and(|entry| entry.requested == self.frame)
    }

    /// Publishes current demand and fitting speculative frames at stable slots.
    pub(super) fn finish_frame(
        &mut self,
        policy: assets::texture::policy::TexturePolicy,
        max_array_layers: usize,
    ) {
        let published = self.publication_mask(policy, max_array_layers);
        let demand_changed = self.models.iter().enumerate().any(|(slot, _)| {
            published[slot]
                != self
                    .shared
                    .get(slot)
                    .is_some_and(|model| !matches!(model, DroppedItemModel::Vacant))
        });
        if self.dirty || demand_changed {
            self.shared = self
                .models
                .iter()
                .enumerate()
                .map(|(slot, model)| {
                    if published[slot] {
                        model.clone()
                    } else {
                        DroppedItemModel::Vacant
                    }
                })
                .collect();
            self.revision = self.revision.wrapping_add(1);
            self.dirty = false;
        }
    }
}

#[cfg(test)]
mod tests;
