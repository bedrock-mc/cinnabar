//! Bounded CPU models and atlas slots for current item consumers.
use std::{collections::HashMap, sync::Arc};

use render::{DroppedItemModel, DroppedItemSpawnPose, MAX_ITEM_LAYERS};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum ModelKey {
    Block {
        hashed: bool,
        id: u32,
    },
    CarriedBlock {
        hashed: bool,
        id: u32,
    },
    Icon {
        page: u16,
        uv: [u16; 4],
        native: bool,
    },
}

impl ModelKey {
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
    assets: Option<render::ChunkTextureAssetIdentity>,
    icons: u64,
    pub(super) revision: u64,
    pub(super) models: Vec<DroppedItemModel>,
    layers: usize,
    pub(super) shared: Arc<[DroppedItemModel]>,
    index: HashMap<ModelKey, usize>,
    /// Bounded layer estimates and permanent block failures still demanded.
    rejected: HashMap<ModelKey, RejectedModel>,
    residents: Vec<Option<Resident>>,
    free_slots: Vec<usize>,
    reclaimable: Vec<usize>,
    /// Free layers plus layers belonging to consumers absent from this frame.
    available: usize,
    demand_ready: bool,
    frame: u64,
    dirty: bool,
    pub(super) spawn_poses: HashMap<(u64, u64), DroppedItemSpawnPose>,
    #[cfg(test)]
    inspections: usize,
}

struct Resident {
    key: ModelKey,
    requested: u64,
    layers: usize,
}

struct RejectedModel {
    /// None marks a block that cannot be built from the current assets.
    layers: Option<usize>,
    requested: u64,
}

/// Returns the texture layers required by a model, excluding the shared white layer.
fn layer_cost(model: &DroppedItemModel) -> usize {
    match model {
        DroppedItemModel::Vacant => 0,
        DroppedItemModel::Cube(cube) => cube.faces.len(),
        DroppedItemModel::Block(block) => block.materials.len(),
        DroppedItemModel::Sprite(_) | DroppedItemModel::NativeSprite(_) => 1,
    }
}

impl ModelCache {
    /// Drops models from another session, block texture set or icon generation.
    pub(super) fn sync(
        &mut self,
        session: u64,
        assets: Option<render::ChunkTextureAssetIdentity>,
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
        self.demand_ready = false;
    }

    /// Protects a current consumer, including a late request after demand collection.
    pub(super) fn request(&mut self, key: &ModelKey) {
        if let Some(&slot) = self.index.get(key) {
            let entry = self.residents[slot].as_mut().expect("indexed resident");
            if entry.requested != self.frame {
                if self.demand_ready {
                    self.available -= entry.layers;
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
        for (slot, entry) in self.residents.iter().enumerate().rev() {
            #[cfg(test)]
            {
                self.inspections += 1;
            }
            if let Some(entry) = entry.as_ref().filter(|entry| entry.requested != self.frame) {
                self.reclaimable.push(slot);
                self.available += entry.layers;
            }
        }
        self.demand_ready = true;
    }

    /// Remembers a demanded rejection; absent layers mean a permanent block failure.
    fn remember_rejection(&mut self, key: ModelKey, layers: Option<usize>) {
        if self.rejected.len() < MAX_ITEM_LAYERS - 1 || self.rejected.contains_key(&key) {
            self.rejected.insert(
                key,
                RejectedModel {
                    layers,
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
        self.request(&key);
        if let Some(&slot) = self.index.get(&key) {
            return Some(slot as u32);
        }
        // Repeated rejections reuse the capacity summary without scans or model construction.
        if self.available == 0 {
            return None;
        }
        let estimate = match self.rejected.get(&key) {
            Some(entry) => entry.layers,
            None => {
                let estimate = cost();
                if estimate.is_none() && matches!(key, ModelKey::Block { .. }) {
                    self.remember_rejection(key.clone(), None);
                }
                estimate
            }
        };
        let cost = estimate?;
        if cost == 0 {
            return None;
        }
        if cost > self.available {
            self.remember_rejection(key, Some(cost));
            return None;
        }
        let Some(model) = build() else {
            if matches!(key, ModelKey::Block { .. }) {
                self.remember_rejection(key, None);
            }
            return None;
        };
        assert_eq!(
            layer_cost(&model),
            cost,
            "model layer estimate must match its geometry"
        );
        self.rejected.remove(&key);
        while self.layers + cost >= MAX_ITEM_LAYERS {
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
            requested: self.frame,
            layers: cost,
        });
        self.layers += cost;
        self.available -= cost;
        self.dirty = true;
        Some(slot as u32)
    }

    /// Shares the resident table once after all admissions.
    pub(super) fn finish_frame(&mut self) {
        if self.dirty {
            self.shared = Arc::from(self.models.as_slice());
            self.revision = self.revision.wrapping_add(1);
            self.dirty = false;
        }
    }
}

#[cfg(test)]
mod tests;
