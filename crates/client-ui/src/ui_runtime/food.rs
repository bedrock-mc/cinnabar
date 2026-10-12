//! Food facts prefer admitted server components over version-pinned local definitions.
use super::UiRuntime;
use std::sync::Arc;
impl UiRuntime {
    /// Retains installation-owned food definitions across sessions; None removes the fallback.
    pub fn set_food_catalog(&mut self, catalog: Option<Arc<assets::RuntimeEquipmentCatalog>>) {
        self.food_catalog = catalog;
    }
    /// Prefers explicit server food values, then exact version-pinned definitions or legacy aliases.
    /// Unknown identifiers remain unavailable rather than receiving estimated nutrition.
    pub fn food_properties(&self, identifier: &str) -> Option<protocol::FoodProperties> {
        if let Some(food) = self
            .item_components(identifier)
            .and_then(|component| component.food_properties.clone())
        {
            return Some(food);
        }
        let catalog = self.food_catalog.as_ref()?;
        let legacy = match identifier {
            "minecraft:enchanted_golden_apple" => "minecraft:appleEnchanted",
            "minecraft:cooked_mutton" => "minecraft:muttonCooked",
            "minecraft:mutton" => "minecraft:muttonRaw",
            "minecraft:tropical_fish" => "minecraft:clownfish",
            "minecraft:cod" => "minecraft:fish",
            "minecraft:cooked_cod" => "minecraft:cooked_fish",
            _ => identifier,
        };
        let food = catalog
            .item_food(identifier)
            .or_else(|| catalog.item_food(legacy))?;
        protocol::FoodProperties::new(food.nutrition, food.saturation_modifier.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_food_survives_sessions_and_explicit_server_food_wins() {
        let food = assets::CompiledItemFood {
            identifier: "minecraft:apple".into(),
            nutrition: 4,
            saturation_modifier: assets::ItemDisplayScalar::new(0.3).unwrap(),
        };
        let bytes = assets::encode_equipment_catalog_with_use_profiles(
            [1; 32],
            [2; 32],
            &[],
            &[],
            assets::EquipmentItemFacts {
                item_food: &[food],
                ..Default::default()
            },
        )
        .unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime.set_food_catalog(Some(Arc::new(
            assets::RuntimeEquipmentCatalog::decode(&bytes).unwrap(),
        )));
        assert_eq!(
            runtime
                .food_properties("minecraft:apple")
                .unwrap()
                .nutrition(),
            4
        );
        assert!(runtime.food_properties("fixture:unknown").is_none());
        let components = protocol::ItemComponents {
            food_properties: protocol::FoodProperties::new(9, 0.8),
            ..Default::default()
        };
        runtime.set_session_items(Some(Arc::new(
            super::super::item_facts::SessionItemComponents::from_iter([(
                "minecraft:apple".into(),
                components,
            )]),
        )));
        assert_eq!(
            runtime
                .food_properties("minecraft:apple")
                .unwrap()
                .nutrition(),
            9
        );
        runtime.begin_session(2);
        assert_eq!(
            runtime
                .food_properties("minecraft:apple")
                .unwrap()
                .nutrition(),
            4
        );
    }
}
