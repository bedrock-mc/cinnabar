//! Measures borrowed artwork before either complete plan constructs its selected models.
use super::*;
pub(super) use render_model::dropped_item::ItemModelCost as ModelCost;

/// The exact upload and source capacity under which an artwork source was refused.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct RejectionBudget {
    upload: render_model::dropped_item::ItemModelBudget,
    layers: usize,
    pixels: usize,
}

impl ModelCache {
    /// Resolves source metadata once per artwork generation, without building a model.
    pub(super) fn measure_cost(
        &mut self,
        key: &ModelKey,
        measure: impl FnOnce() -> Option<ModelCost>,
    ) -> Option<ModelCost> {
        if self.invalid_meshes.contains(key)
            || self
                .rejected
                .get(key)
                .is_some_and(|entry| entry.unavailable)
        {
            return None;
        }
        if let Some(cost) = self.costs.get(key) {
            return Some(*cost);
        }
        #[cfg(test)]
        {
            *self.mesh_validations.entry(key.clone()).or_default() += 1;
        }
        let Some(cost) = measure() else {
            self.reject_unavailable(key);
            if !matches!(key, ModelKey::Block { .. } | ModelKey::PottedBlock { .. }) {
                self.invalid_meshes.insert(key.clone());
            }
            return None;
        };
        self.costs.insert(key.clone(), cost);
        Some(cost)
    }

    /// Charges cached costs atomically and remembers refusals until their capacity changes.
    pub(super) fn charge_cost(
        &mut self,
        key: &ModelKey,
        cost: ModelCost,
        layers: usize,
        pixels: usize,
    ) -> bool {
        let context = RejectionBudget {
            upload: self.budget,
            layers,
            pixels,
        };
        let rejection_key = (key.clone(), self.planning, self.sprite_only);
        if self.budget_rejections.get(&rejection_key) == Some(&context) {
            self.budget_refusals = self.budget_refusals.saturating_add(1);
            return false;
        }
        #[cfg(test)]
        {
            *self.admission_checks.entry(key.clone()).or_default() += 1;
        }
        let mut budget = self.budget;
        if cost.layers > layers
            || cost.pixels > pixels
            || budget
                .admit_dimensions(cost.side, cost.layers, cost.vertices)
                .is_none()
        {
            self.budget_refusals = self.budget_refusals.saturating_add(1);
            self.budget_rejections.insert(rejection_key, context);
            self.remember_rejection(key.clone(), false);
            return false;
        }
        self.budget = budget;
        self.budget_rejections.remove(&rejection_key);
        true
    }

    /// Reserves distinct sources once without copying pixels or evicting resident models.
    pub(super) fn plan_cost(&mut self, key: &ModelKey, cost: ModelCost) -> Option<u32> {
        if self.planned.contains(key) {
            return Some(u32::MAX);
        }
        if !self.charge_cost(
            key,
            cost,
            MAX_ITEM_LAYERS - 1,
            assets::texture::policy::MAX_SUBSCRIBER_BYTES.saturating_sub(self.planned_pixels),
        ) {
            return None;
        }
        self.planned_pixels += cost.pixels;
        self.planned.insert(key.clone());
        Some(u32::MAX)
    }

    /// Resolves and admits source costs while complete plans compare their item counts.
    pub(super) fn plan_model(
        &mut self,
        key: ModelKey,
        measure: impl FnOnce() -> Option<ModelCost>,
    ) -> Option<u32> {
        self.request(&key);
        let cost = self.measure_cost(&key, measure)?;
        self.plan_cost(&key, cost)
    }
}
