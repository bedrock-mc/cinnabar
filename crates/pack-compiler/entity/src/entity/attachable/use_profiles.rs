//! Authored use facts compiled from the independent behavior-item collection.

use super::*;
use assets::{
    equipment::CompiledItemUseOverlay,
    item_use::{ItemConsumption, ItemUseCategory},
};

/// Emits explicit optional facts for every collected item, including unresolved durations.
pub fn compile_item_use_profiles(
    behavior_pack: &Path,
) -> Result<Vec<CompiledItemUseOverlay>, AssetError> {
    let durations: BTreeMap<_, _> = super::compile_item_use(behavior_pack)?
        .into_iter()
        .map(|entry| (entry.identifier, entry.ticks))
        .collect();
    Ok(item_attack::components(behavior_pack)?
        .into_iter()
        .map(|(identifier, components)| {
            let modifiers = components
                .get("minecraft:use_modifiers")
                .filter(|modifiers| modifiers.is_object());
            let duration_ticks = durations.get(identifier.as_ref()).copied();
            let food_can_always_eat = components
                .get("minecraft:food")
                .filter(|food| food.is_object())
                .map(|food| {
                    food.get("can_always_eat")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                });
            let animation = components
                .get("minecraft:use_animation")
                .and_then(|animation| {
                    animation
                        .as_str()
                        .or_else(|| animation.get("value")?.as_str())
                });
            let consumption = animation
                .map(|animation| match animation {
                    "eat" => ItemConsumption::Eat,
                    "drink" => ItemConsumption::Drink,
                    _ => ItemConsumption::None,
                })
                .or_else(|| food_can_always_eat.map(|_| ItemConsumption::Eat));
            let movement_modifier_bits = modifiers
                .and_then(|modifiers| modifiers.get("movement_modifier"))
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(f64::to_bits);
            let cooldown = components.get("minecraft:cooldown").and_then(|cooldown| {
                if cooldown.get("type").and_then(Value::as_str) == Some("attack") {
                    return None;
                }
                let duration = cooldown.get("duration")?.as_f64()?;
                if !duration.is_finite() || duration < 0.0 {
                    return None;
                }
                Some(ItemUseCategory {
                    category: bounded_identifier(cooldown.get("category")?.as_str()?).ok()?,
                    ticks: simulation_clock::whole_ticks_from_seconds(duration as f32)?,
                })
            });
            CompiledItemUseOverlay {
                identifier,
                duration_ticks,
                food_can_always_eat,
                consumption,
                use_modifiers_present: modifiers.is_some(),
                movement_modifier_bits,
                cooldown,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_food_movement_and_cooldowns_do_not_come_from_item_names() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("items");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("fixture.json"),
            r#"{"minecraft:item":{
            "description":{"identifier":"fixture:arbitrary"},"components":{
                "minecraft:food":{"can_always_eat":true},"minecraft:use_animation":"drink",
                "minecraft:use_modifiers":{"use_duration":1.6,"movement_modifier":0.75},
                "minecraft:cooldown":{"category":"fixture:shared","duration":0.549}
            }}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.join("unknown.json"),
            r#"{"minecraft:item":{
            "description":{"identifier":"fixture:unadmitted"},"components":{}}}"#,
        )
        .unwrap();
        let profiles = compile_item_use_profiles(root.path()).unwrap();
        assert_eq!(profiles.len(), 2);
        let profile = &profiles[0];
        assert_eq!(profile.identifier.as_ref(), "fixture:arbitrary");
        assert_eq!(profile.duration_ticks, Some(32));
        assert_eq!(profile.food_can_always_eat, Some(true));
        assert_eq!(profile.consumption, Some(ItemConsumption::Drink));
        assert!(profile.use_modifiers_present);
        assert_eq!(profile.movement_modifier_bits, Some(0.75_f64.to_bits()));
        assert_eq!(
            profile.cooldown.as_ref().unwrap().category.as_ref(),
            "fixture:shared"
        );
        assert_eq!(profile.cooldown.as_ref().unwrap().ticks, 10);
        assert_eq!(profiles[1].duration_ticks, None);
        assert_eq!(profiles[1].consumption, None);
        assert!(!profiles[1].use_modifiers_present);
    }

    #[test]
    fn authored_use_modifiers_presence_preserves_missing_and_invalid_movement_values() {
        for (name, declaration, present, movement) in [
            ("absent", None, false, None),
            ("empty", Some(serde_json::json!({})), true, None),
            (
                "duration",
                Some(serde_json::json!({"use_duration": 1.6})),
                true,
                None,
            ),
            (
                "zero",
                Some(serde_json::json!({"movement_modifier": 0.0})),
                true,
                Some(0.0_f64),
            ),
            (
                "fraction",
                Some(serde_json::json!({"movement_modifier": 0.75})),
                true,
                Some(0.75_f64),
            ),
            ("scalar", Some(serde_json::json!(0.75)), false, None),
            (
                "array",
                Some(serde_json::json!([{"movement_modifier": 0.75}])),
                false,
                None,
            ),
            (
                "invalid_movement",
                Some(serde_json::json!({"movement_modifier": "0.75"})),
                true,
                None,
            ),
            (
                "negative",
                Some(serde_json::json!({"movement_modifier": -1.0})),
                true,
                None,
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let directory = root.path().join("items");
            std::fs::create_dir(&directory).unwrap();
            let mut components = serde_json::Map::new();
            if let Some(declaration) = declaration {
                components.insert("minecraft:use_modifiers".into(), declaration);
            }
            let item = serde_json::json!({"minecraft:item": {
                "description": {"identifier": format!("fixture:{name}")},
                "components": components,
            }});
            std::fs::write(
                directory.join("fixture.json"),
                serde_json::to_vec(&item).unwrap(),
            )
            .unwrap();
            let profiles = compile_item_use_profiles(root.path()).unwrap();
            assert_eq!(profiles.len(), 1, "{name}");
            assert_eq!(profiles[0].use_modifiers_present, present, "{name}");
            assert_eq!(
                profiles[0].movement_modifier_bits,
                movement.map(f64::to_bits),
                "{name}"
            );
        }
    }
}
