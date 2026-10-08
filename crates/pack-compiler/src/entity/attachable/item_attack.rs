//! Behavior item extraction for equipment timing facts.

use super::*;
use assets::{CompiledItemAttackCooldown, CompiledItemAttackTiming, CompiledKineticWeaponTiming};

/// Compiles authored attack facts, excluding use cooldowns and malformed optional fields.
pub fn compile_item_attack_timings(
    behavior_pack: &Path,
) -> Result<Vec<CompiledItemAttackTiming>, AssetError> {
    Ok(components(behavior_pack)?
        .into_iter()
        .filter_map(|(identifier, components)| {
            let swing_duration_seconds = components
                .get("minecraft:swing_duration")
                .and_then(|swing| swing.get("value"))
                .and_then(seconds);
            let attack_cooldown = components.get("minecraft:cooldown").and_then(|cooldown| {
                (cooldown.get("type")?.as_str()? == "attack").then_some(())?;
                Some(CompiledItemAttackCooldown {
                    category: bounded_identifier(cooldown.get("category")?.as_str()?).ok()?,
                    duration_seconds: seconds(cooldown.get("duration")?)?,
                })
            });
            let kinetic_weapon = components
                .get("minecraft:kinetic_weapon")
                .and_then(|weapon| {
                    let ticks =
                        |value: &Value| value.as_u64().and_then(|ticks| u32::try_from(ticks).ok());
                    let phase = |name| ticks(weapon.get(name)?.get("max_duration")?);
                    Some(CompiledKineticWeaponTiming {
                        delay_ticks: ticks(weapon.get("delay")?)?,
                        dismount_ticks: phase("dismount_conditions")?,
                        knockback_ticks: phase("knockback_conditions")?,
                        damage_ticks: phase("damage_conditions")?,
                    })
                });
            let piercing_weapon = components
                .get("minecraft:piercing_weapon")
                .is_some_and(Value::is_object);
            let is_spear = components
                .pointer("/minecraft:tags/tags")
                .and_then(Value::as_array)
                .is_some_and(|tags| {
                    tags.iter()
                        .any(|tag| tag.as_str() == Some("minecraft:is_spear"))
                });
            (swing_duration_seconds.is_some()
                || attack_cooldown.is_some()
                || is_spear
                || piercing_weapon
                || kinetic_weapon.is_some())
            .then_some(CompiledItemAttackTiming {
                identifier,
                swing_duration_seconds,
                attack_cooldown,
                is_spear,
                piercing_weapon,
                kinetic_weapon,
            })
        })
        .collect())
}

/// Reads bounded, valid item documents once; malformed items contribute no facts.
pub(super) fn components(behavior_pack: &Path) -> Result<BTreeMap<Box<str>, Value>, AssetError> {
    const MAX_ITEM_JSON_BYTES: u64 = 256 * 1024;
    let directory = behavior_pack.join("items");
    let entries = std::fs::read_dir(&directory).map_err(|source| AssetError::Io {
        path: directory.clone(),
        source,
    })?;
    let mut items = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let bounded = entry
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_ITEM_JSON_BYTES);
        if !bounded || path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Some(item) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| parse_unique_json(&path, &bytes).ok())
            .and_then(|document| document.get("minecraft:item").cloned())
        else {
            continue;
        };
        let (Some(identifier), Some(components)) = (
            item.pointer("/description/identifier")
                .and_then(Value::as_str),
            item.get("components"),
        ) else {
            continue;
        };
        let Ok(identifier) = bounded_identifier(identifier) else {
            continue;
        };
        if components.is_object() {
            items.insert(identifier, components.clone());
        }
    }
    Ok(items)
}

/// Retains finite positive seconds; the presentation boundary supplies the shared tick rate.
fn seconds(value: &Value) -> Option<ItemDisplayScalar> {
    let value = value.as_f64()?;
    (value > 0.0).then_some(())?;
    ItemDisplayScalar::new(value as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_spear_attack_facts_are_compiled_without_treating_throw_cooldowns_as_attacks() {
        let Some(root) = std::env::var_os("CINNABAR_VANILLA_BEHAVIOR_ROOT") else {
            eprintln!("missing fixture: CINNABAR_VANILLA_BEHAVIOR_ROOT pinned behavior pack");
            return;
        };
        let facts = compile_item_attack_timings(Path::new(&root)).unwrap();
        let iron = facts
            .iter()
            .find(|entry| entry.identifier.as_ref() == "minecraft:iron_spear")
            .unwrap();
        assert_eq!(iron.swing_duration_seconds.unwrap().get(), 0.95);
        let cooldown = iron.attack_cooldown.as_ref().unwrap();
        assert_eq!(cooldown.category.as_ref(), "spear");
        assert_eq!(cooldown.duration_seconds.get(), 0.95);
        assert!(iron.piercing_weapon);
        assert!(iron.is_spear);
        assert_eq!(
            iron.kinetic_weapon,
            Some(CompiledKineticWeaponTiming {
                delay_ticks: 12,
                dismount_ticks: 50,
                knockback_ticks: 135,
                damage_ticks: 225,
            })
        );
        assert!(
            !facts
                .iter()
                .any(|entry| entry.identifier.as_ref() == "minecraft:wind_charge")
        );
    }

    #[test]
    fn attack_facts_read_commented_components_and_keep_use_cooldowns_separate() {
        let root = tempfile::tempdir().unwrap();
        let items = root.path().join("items");
        std::fs::create_dir(&items).unwrap();
        std::fs::write(items.join("spear.json"), r#"{"minecraft:item":{
            "description":{"identifier":"fixture:spear"},"components":{
                "minecraft:swing_duration":{"value":0.75}, // Authored seconds.
                "minecraft:cooldown":{"category":"fixture:jab","duration":0.5,"type":"attack"},
                "minecraft:piercing_weapon":{},
                "minecraft:kinetic_weapon":{"delay":4,"dismount_conditions":{"max_duration":30},
                    "knockback_conditions":{"max_duration":60},"damage_conditions":{"max_duration":90}}
            }}}"#).unwrap();
        std::fs::write(
            items.join("use.json"),
            r#"{"minecraft:item":{
            "description":{"identifier":"fixture:throw"},"components":{
                "minecraft:cooldown":{"category":"fixture:throw","duration":0.5},
                "minecraft:swing_duration":{"value":-1},
                "minecraft:kinetic_weapon":{"delay":4},"minecraft:piercing_weapon":false
            }}}"#,
        )
        .unwrap();
        std::fs::write(items.join("broken.json"), "{").unwrap();
        let facts = compile_item_attack_timings(root.path()).unwrap();
        assert_eq!(facts.len(), 1);
        let fact = &facts[0];
        assert_eq!(fact.identifier.as_ref(), "fixture:spear");
        assert_eq!(fact.swing_duration_seconds.unwrap().get(), 0.75);
        let cooldown = fact.attack_cooldown.as_ref().unwrap();
        assert_eq!(cooldown.category.as_ref(), "fixture:jab");
        assert_eq!(cooldown.duration_seconds.get(), 0.5);
        assert!(fact.piercing_weapon);
        // Neither the item's suffix nor a kinetic component supplies the native spear tag.
        assert!(!fact.is_spear);
        assert_eq!(
            fact.kinetic_weapon,
            Some(CompiledKineticWeaponTiming {
                delay_ticks: 4,
                dismount_ticks: 30,
                knockback_ticks: 60,
                damage_ticks: 90,
            })
        );
    }
}
