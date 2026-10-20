//! Owner-scoped extension globals over the native HUD's immutable retained bindings.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{Catalog, DataSource, Scalar};
use mod_ui::PanelUiOwner;
use ui::{mod_hud::HudBindings, mod_panel::SurfaceValue};

use super::UiPresentationRuntime;

#[derive(Default)]
pub(super) struct Bindings {
    owners: BTreeMap<PanelUiOwner, HudBindings>,
    /// HUD and crosshair have independent base data, so both retain their merged copy.
    merged: [Option<Merged>; 2],
    catalog: Option<(Arc<Catalog>, Arc<Catalog>)>,
}

struct Merged {
    base: Arc<DataSource>,
    data: Arc<DataSource>,
}

impl UiPresentationRuntime {
    /// Replaces one extension's numeric HUD globals. `None` removes only this owner.
    /// The native HUD controller's data and every other extension remain intact.
    pub fn set_mod_hud_bindings(
        &mut self,
        owner: PanelUiOwner,
        bindings: Option<HudBindings>,
    ) -> Result<(), String> {
        self.mod_hud_bindings.set(owner, bindings)
    }
}

impl Bindings {
    /// Validates before replacing an owner, and drops merged copies only on a value change.
    fn set(&mut self, owner: PanelUiOwner, bindings: Option<HudBindings>) -> Result<(), String> {
        if let Some(bindings) = &bindings {
            bindings.validate()?;
        }
        let bindings = bindings.filter(|bindings| !bindings.0.is_empty());
        if self.owners.get(&owner) == bindings.as_ref() {
            return Ok(());
        }
        if let Some(bindings) = bindings {
            self.owners.insert(owner, bindings);
        } else {
            self.owners.remove(&owner);
        }
        self.merged = Default::default();
        if self.owners.is_empty() {
            self.catalog = None;
        }
        Ok(())
    }

    /// Opts only the trusted extension HUD into paint transforms, retaining its prepared clone.
    pub(super) fn catalog(&mut self, base: &Arc<Catalog>) -> Arc<Catalog> {
        if self.owners.is_empty() || base.paint_transforms() {
            return Arc::clone(base);
        }
        if let Some((previous, catalog)) = &self.catalog
            && Arc::ptr_eq(previous, base)
        {
            return Arc::clone(catalog);
        }
        let catalog = Arc::new((**base).clone().with_paint_transforms());
        super::forms::hud::prepare(&catalog);
        self.catalog = Some((Arc::clone(base), Arc::clone(&catalog)));
        catalog
    }

    /// Reuses a merged snapshot until either its native base or an owner's values change.
    pub(super) fn merge(&mut self, base: Arc<DataSource>, crosshair: bool) -> Arc<DataSource> {
        if self.owners.is_empty() {
            return base;
        }
        let cached = &mut self.merged[usize::from(crosshair)];
        if let Some(cached) = cached
            && Arc::ptr_eq(&cached.base, &base)
        {
            return Arc::clone(&cached.data);
        }
        let mut data = (*base).clone();
        // Ordered owners make a collision deterministic; removing one exposes the prior owner.
        for bindings in self.owners.values() {
            for (name, value) in &bindings.0 {
                let scalar = match value {
                    SurfaceValue::Bool(value) => Scalar::Bool(*value),
                    SurfaceValue::Number(value) => Scalar::Num(*value),
                    SurfaceValue::Text(value) => Scalar::Text(value.clone()),
                    SurfaceValue::Vector(value) => Scalar::Json(serde_json::json!(value)),
                };
                data.set_global(name, scalar);
            }
        }
        let data = Arc::new(data);
        *cached = Some(Merged {
            base,
            data: Arc::clone(&data),
        });
        data
    }
}

#[cfg(test)]
mod tests;
