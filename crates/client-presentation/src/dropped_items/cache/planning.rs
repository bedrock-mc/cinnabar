//! Measures artwork without changing the resident models used by the final preparation.
use super::*;

/// Validated source and upload demand, reusable until the artwork generation changes.
#[derive(Clone, Copy)]
pub(super) struct ModelCost {
    side: u32,
    layers: usize,
    pixels: usize,
    pub(super) vertices: usize,
}

impl ModelCache {
    /// Validates geometry once and retains only its cost when it is not resident.
    pub(super) fn validate_model(
        &mut self,
        key: &ModelKey,
        model: &DroppedItemModel,
    ) -> Option<ModelCost> {
        if let Some(cost) = self.costs.get(key) {
            return Some(*cost);
        }
        #[cfg(test)]
        {
            *self.mesh_validations.entry(key.clone()).or_default() += 1;
        }
        let cost = model.atlas_side(self.budget.policy()).and_then(|side| {
            let vertices = model.mesh(side, 1)?.len();
            (vertices > 0).then(|| ModelCost {
                side,
                layers: model.texture_layers(),
                pixels: pixel_cost(model),
                vertices,
            })
        });
        if let Some(cost) = cost {
            self.costs.insert(key.clone(), cost);
        } else {
            self.invalid_meshes.insert(key.clone());
        }
        cost
    }

    /// Charges distinct sources atomically, reserving both upload and source-pixel capacity.
    pub(super) fn plan_cost(&mut self, key: &ModelKey, cost: ModelCost) -> Option<u32> {
        if self.planned.contains(key) {
            return Some(u32::MAX);
        }
        let mut budget = self.budget;
        if cost.pixels
            > assets::texture::policy::MAX_SUBSCRIBER_BYTES.saturating_sub(self.planned_pixels)
            || budget
                .admit_dimensions(cost.side, cost.layers, cost.vertices)
                .is_none()
        {
            self.budget_refusals = self.budget_refusals.saturating_add(1);
            return None;
        }
        self.budget = budget;
        self.planned_pixels += cost.pixels;
        self.planned.insert(key.clone());
        // Planning handles never reach publication or bounds inspection.
        Some(u32::MAX)
    }

    /// Resolves missing cost metadata without admitting models or evicting resident artwork.
    pub(super) fn plan_model(
        &mut self,
        key: ModelKey,
        estimate: impl FnOnce() -> Option<usize>,
        build: impl FnOnce() -> Option<DroppedItemModel>,
    ) -> Option<u32> {
        if self.invalid_meshes.contains(&key)
            || self
                .rejected
                .get(&key)
                .is_some_and(|entry| entry.layers.is_none())
        {
            return None;
        }
        let cost = if let Some(cost) = self.costs.get(&key) {
            *cost
        } else {
            let Some(layers) = estimate() else {
                self.reject_unavailable(&key);
                return None;
            };
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
                layers,
                "model layer estimate must match its geometry"
            );
            self.validate_model(&key, &model)?
        };
        self.plan_cost(&key, cost)
    }
}
