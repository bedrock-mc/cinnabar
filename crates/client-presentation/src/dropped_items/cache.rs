//! Bounded CPU models and atlas slots for current item consumers.
use std::sync::Arc;

use bevy::platform::collections::{HashMap, HashSet};

use render_model::{
    DroppedItemModel, DroppedItemSpawnPose, MAX_ITEM_LAYERS, dropped_item::DroppedItemBounds,
};

mod planning;
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
    pub(super) budget: render_model::dropped_item::ItemModelBudget,
    session: u64,
    assets: Option<render_scene::ChunkTextureAssetIdentity>,
    icons: u64,
    pub(super) revision: u64,
    pub(super) models: Vec<DroppedItemModel>,
    layers: usize,
    pixel_bytes: usize,
    /// Admission attempts refused by layer or source-pixel capacity this frame.
    budget_refusals: u64,
    /// Successful artwork resolutions that recovered from a budget refusal this frame.
    pub(super) budget_fallbacks: u64,
    /// Whole preparations downgraded to sprites in this frame.
    pub(super) cube_plan_downgrades: u64,
    sprite_only: bool,
    planning: bool,
    costs: HashMap<ModelKey, planning::ModelCost>,
    planned: HashSet<ModelKey>,
    planned_pixels: usize,
    /// Invalid geometry stays rejected until the session or artwork generation changes.
    invalid_meshes: HashSet<ModelKey>,
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
    #[cfg(test)]
    pub(super) mesh_validations: HashMap<ModelKey, usize>,
    #[cfg(test)]
    pub(super) model_builds: usize,
}

struct Resident {
    key: ModelKey,
    /// Last frame with a current scene consumer.
    requested: u64,
    /// Last frame that wanted this model for speculative animation residency.
    warmed: u64,
    layers: usize,
    vertices: usize,
    admitted: u64,
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
    /// Plans without changing residency, then prepares the complete chosen artwork mode.
    /// The callback replaces its frame-local output and skips draws during cost planning.
    pub(super) fn prepare<T>(&mut self, mut prepare: impl FnMut(&mut Self) -> T) -> T {
        self.sprite_only = false;
        self.planning = true;
        self.begin_frame();
        prepare(self);
        let downgrade = self.budget_refusals > 0;
        self.planning = false;
        self.sprite_only = downgrade;
        self.begin_frame();
        self.collect_reclaimable();
        let plan = prepare(self);
        self.prune_rejections();
        self.cube_plan_downgrades = u64::from(downgrade);
        plan
    }

    /// Whether this pass only measures artwork costs, without preparing draws.
    pub(super) fn is_planning(&self) -> bool {
        self.planning
    }

    /// Reuses a successfully admitted model while preserving current-consumer protection.
    pub(super) fn resident(&mut self, key: &ModelKey) -> Option<u32> {
        let slot = *self.index.get(key)?;
        if self.planning {
            let cost = *self.costs.get(key).expect("resident has validated costs");
            return self.plan_cost(key, cost);
        }
        if !self.admit_resident(slot) {
            return None;
        }
        self.request(key);
        Some(slot as u32)
    }

    /// Charges a cached model once per frame before returning it to an artwork chain.
    fn admit_resident(&mut self, slot: usize) -> bool {
        let entry = self.residents[slot].as_mut().expect("indexed resident");
        if entry.admitted == self.frame {
            return true;
        }
        if self
            .budget
            .admit(&self.models[slot], entry.vertices)
            .is_none()
        {
            self.budget_refusals = self.budget_refusals.saturating_add(1);
            return false;
        }
        entry.admitted = self.frame;
        true
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
        self.budget.reset();
        self.planned.clear();
        self.planned_pixels = 0;
        self.budget_refusals = 0;
        self.budget_fallbacks = 0;
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

    /// Drops stale estimates after this frame's attempted sources have been identified.
    fn prune_rejections(&mut self) {
        self.rejected.retain(|_, entry| {
            #[cfg(test)]
            {
                self.inspections += 1;
            }
            entry.requested == self.frame
        });
    }

    /// Counts reclaimable capacity after protecting explicitly collected test consumers.
    #[cfg(test)]
    pub(super) fn finish_demand(&mut self) {
        self.prune_rejections();
        self.collect_reclaimable();
    }

    /// Makes unselected residents reclaimable while a complete plan chooses its models.
    fn collect_reclaimable(&mut self) {
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

    /// Remembers unavailable block sources whose assets cannot become ready later this generation.
    fn reject_unavailable(&mut self, key: &ModelKey) {
        if matches!(key, ModelKey::Block { .. } | ModelKey::PottedBlock { .. }) {
            self.remember_rejection(key.clone(), None, None);
        }
    }

    /// Counts successful fallback chains that encountered at least one capacity refusal.
    pub(super) fn artwork<T>(
        &mut self,
        attempt: impl FnMut(&mut Self, crate::session_artwork::ItemArtwork) -> Option<T>,
    ) -> Option<T> {
        let refusals = self.budget_refusals;
        let mut attempt = attempt;
        let result = crate::session_artwork::item_artwork(self, |cache, source| {
            if cache.sprite_only
                && matches!(
                    source,
                    crate::session_artwork::ItemArtwork::SessionCube
                        | crate::session_artwork::ItemArtwork::StartupCube
                )
            {
                None
            } else {
                attempt(cache, source)
            }
        });
        if result.is_some() && self.budget_refusals != refusals {
            self.budget_fallbacks = self.budget_fallbacks.saturating_add(1);
        }
        result
    }

    /// Builds missing models that fit, skipping remembered permanent block failures.
    pub(super) fn model(
        &mut self,
        key: ModelKey,
        cost: impl FnOnce() -> Option<usize>,
        build: impl FnOnce() -> Option<DroppedItemModel>,
    ) -> Option<u32> {
        if self.planning {
            return self.plan_model(key, cost, build);
        }
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
        if self.invalid_meshes.contains(&key) {
            return None;
        }
        if let Some(&slot) = self.index.get(&key) {
            if reclaim {
                return self.resident(&key);
            }
            self.residents[slot]
                .as_mut()
                .expect("indexed resident")
                .warmed = self.frame;
            return Some(slot as u32);
        }
        if reclaim {
            self.request(&key);
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
            self.budget_refusals = self.budget_refusals.saturating_add(1);
            return None;
        }
        let estimate = match self.rejected.get(&key) {
            Some(entry) => {
                if entry
                    .pixel_bytes
                    .is_some_and(|bytes| bytes > available_pixels)
                {
                    self.budget_refusals = self.budget_refusals.saturating_add(1);
                    return None;
                }
                entry.layers
            }
            None => {
                let estimate = cost();
                if estimate.is_none() {
                    self.reject_unavailable(&key);
                }
                estimate
            }
        };
        let cost = estimate?;
        if cost == 0 {
            return None;
        }
        if cost > available {
            self.budget_refusals = self.budget_refusals.saturating_add(1);
            let pixel_bytes = self.rejected.get(&key).and_then(|entry| entry.pixel_bytes);
            self.remember_rejection(key, Some(cost), pixel_bytes);
            return None;
        }
        #[cfg(test)]
        {
            self.model_builds += 1;
        }
        let Some(model) = build() else {
            self.reject_unavailable(&key);
            return None;
        };
        assert_eq!(
            model.texture_layers(),
            cost,
            "model layer estimate must match its geometry"
        );
        let pixel_bytes = pixel_cost(&model);
        if pixel_bytes > available_pixels {
            self.budget_refusals = self.budget_refusals.saturating_add(1);
            self.remember_rejection(key, Some(cost), Some(pixel_bytes));
            return None;
        }
        let vertices = self.validate_model(&key, &model)?.vertices;
        if reclaim && self.budget.admit(&model, vertices).is_none() {
            self.budget_refusals = self.budget_refusals.saturating_add(1);
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
            vertices,
            admitted: if reclaim { self.frame } else { 0 },
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
    pub(super) fn finish_frame(&mut self) {
        if self.cube_plan_downgrades > 0 {
            tracing::debug!(
                cube_plan_downgrades = self.cube_plan_downgrades,
                "item cube plan exceeded capacity; preparing all items as sprites"
            );
        }
        if self.budget_fallbacks > 0 {
            tracing::debug!(
                budget_fallbacks = self.budget_fallbacks,
                "item artwork recovered from cache pressure"
            );
        }
        let published = self.publication_mask();
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
