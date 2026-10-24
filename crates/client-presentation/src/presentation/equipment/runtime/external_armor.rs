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
            if matches!(
                binding.category,
                EquipmentCategory::Armor { .. } | EquipmentCategory::ArmorAndHeld { .. }
            ) && binding.geometry.resolution == EntityDependencyResolution::External
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

/// A resolved native model, retaining controller poses for bones absent from the wearer.
pub(super) struct ExternalArmorModel {
    pub identifier: Box<str>,
    pub geometry: Arc<ArmorGeometry>,
    pub render: Option<types::WornRender>,
}

impl EquipmentRuntime {
    /// Selects a drawable native variant using the caller's complete inputs and operation budget.
    pub(super) fn external_armor_binding(
        &mut self,
        identifier: &str,
        animation: EquipmentAnimation<'_>,
        input: actor_animation::AttachableAnimationInput<'_>,
        operations_left: &mut usize,
    ) -> Option<usize> {
        let items = Arc::clone(self.external_armor.items.get(identifier)?);
        let catalog = self.catalog.as_ref()?;
        items.iter().find_map(|item| {
            self.attachables.select_equipment_binding(
                catalog,
                item,
                animation.owner,
                animation.rig,
                input,
                operations_left,
                |binding| {
                    !self
                        .assets
                        .geometry_candidates(&binding.geometry.identifier)
                        .is_empty()
                },
            )
        })
    }

    /// Resolves the variant's evaluated model and pose; an authored empty draw remains hidden.
    pub(super) fn external_armor_model(
        &mut self,
        identifier: &str,
        (item, equipment, animation, slot): (
            &WornItem,
            &ActorEquipmentInput,
            EquipmentAnimation<'_>,
            ArmorSlot,
        ),
        body: &ActorRigSubmission,
        bones: &BodyBones,
    ) -> Result<Option<ExternalArmorModel>, ()> {
        let variables = [(
            assets::entity::EngineVariable::IsEnchanted.name(),
            f32::from(item.enchanted),
        )];
        let mut operations_left = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK;
        let index = self
            .external_armor_binding(
                identifier,
                animation,
                animation.worn_input(equipment, slot, &variables),
                &mut operations_left,
            )
            .ok_or(())?;
        let catalog = Arc::clone(self.catalog.as_ref().ok_or(())?);
        let binding = &catalog.bindings()[index];
        let render = match self.worn_render(
            item,
            slot,
            equipment,
            animation,
            false,
            binding.attachable_identifier(),
            body,
            bones,
        ) {
            Ok(None) => return Ok(None),
            Ok(render) => render,
            Err(()) => None,
        };
        let identifier = render
            .as_ref()
            .map_or(&binding.geometry.identifier, |render| &render.geometry);
        let geometry = self.armor_geometry_for(identifier, false).ok_or(())?;
        if identifier != &binding.geometry.identifier
            && self.selected_geometries.insert(geometry.rig)
            && let Some(index) = find_geometry_index(&self.assets, identifier)
            && let Some(mesh) =
                render_model::equipment_geometry(&self.assets, index as usize, geometry.rig)
        {
            self.pending.push(mesh);
        }
        Ok(Some(ExternalArmorModel {
            identifier: identifier.clone(),
            geometry,
            render,
        }))
    }
}
