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
            let modifiers = components.get("minecraft:use_modifiers");
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
        assert_eq!(profile.movement_modifier_bits, Some(0.75_f64.to_bits()));
        assert_eq!(
            profile.cooldown.as_ref().unwrap().category.as_ref(),
            "fixture:shared"
        );
        assert_eq!(profile.cooldown.as_ref().unwrap().ticks, 10);
        assert_eq!(profiles[1].duration_ticks, None);
        assert_eq!(profiles[1].consumption, None);
    }
}
