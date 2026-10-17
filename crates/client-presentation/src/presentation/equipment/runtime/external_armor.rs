//! External armor models resolve through the startup catalog's owner-specific definitions.

use super::*;

/// Native external models and the startup items whose variants supply their geometry.
#[derive(Default)]
pub(super) struct ExternalArmor {
    items: BTreeMap<Box<str>, Arc<[Box<str>]>>,
}

impl ExternalArmor {
    /// Derives model families from the loaded catalog rather than guessing geometry aliases.
    pub(super) fn new(catalog: Option<&RuntimeEquipmentCatalog>) -> Self {
        let mut items = BTreeMap::<Box<str>, Vec<Box<str>>>::new();
        for binding in catalog.into_iter().flat_map(|catalog| catalog.bindings()) {
            if matches!(binding.category, EquipmentCategory::Armor { .. })
                && binding.geometry.resolution == EntityDependencyResolution::External
                && binding.attachable.is_none()
            {
                items
                    .entry(binding.geometry.identifier.clone())
                    .or_default()
                    .push(binding.identifier.clone());
            }
        }
        Self {
            items: items
                .into_iter()
                .map(|(model, mut items)| {
                    items.sort();
                    items.dedup();
                    (model, items.into())
                })
                .collect(),
        }
    }

    /// Whether startup armor definitions recognize this external model family.
    pub(super) fn contains(&self, identifier: &str) -> bool {
        self.items.contains_key(identifier)
    }
}

impl EquipmentRuntime {
    /// Selects the native family's drawable model using the actual wearer's owner context.
    pub(super) fn external_armor_geometry(
        &mut self,
        identifier: &str,
        equipment: &ActorEquipmentInput,
        animation: EquipmentAnimation<'_>,
        slot: ArmorSlot,
    ) -> Option<(Box<str>, Arc<ArmorGeometry>)> {
        let items = Arc::clone(self.external_armor.items.get(identifier)?);
        let catalog = Arc::clone(self.catalog.as_ref()?);
        let mut operations_left = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK;
        for item in items.iter() {
            let Some(index) = self.attachables.select_equipment_binding(
                &catalog,
                item,
                animation.owner,
                animation.rig,
                animation.worn_input(equipment, slot, &[]),
                &mut operations_left,
                |binding| {
                    !self
                        .assets
                        .geometry_candidates(&binding.geometry.identifier)
                        .is_empty()
                },
            ) else {
                continue;
            };
            let model = &catalog.bindings()[index].geometry.identifier;
            if let Some(geometry) = self.armor_geometry_for(model, false) {
                return Some((model.clone(), geometry));
            }
        }
        None
    }
}
