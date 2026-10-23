//! Retains necessary component conditions without inventing group activation or callback state.

use super::*;
use assets::entity::offered_use::{
    ComponentUse, EntityUseAdmission, UseCondition as C, UseSelector,
};

/// Joins behavior definitions to the entity inventory; missing definitions remain unresolved.
pub(super) fn compile(
    symbols: &[EntityAssetSymbol],
    sources: &[EntityAssetSource],
    payloads: &SourcePayloads,
) -> Result<BTreeMap<Box<str>, EntityUseAdmission>, AssetError> {
    let mut definitions = BTreeMap::new();
    for (index, source) in sources.iter().enumerate() {
        if !source.path.starts_with("behavior/entities/") {
            continue;
        }
        let Some(bytes) = payloads.get(&source.path) else {
            continue;
        };
        let document = parse_unique_json(Path::new(source.path.as_ref()), bytes)?;
        let Some(entity) = document.get("minecraft:entity") else {
            continue;
        };
        let Some(identifier) = entity
            .pointer("/description/identifier")
            .and_then(Value::as_str)
        else {
            continue;
        };
        if definitions
            .insert(identifier.to_owned(), (index as u32, entity.clone()))
            .is_some()
        {
            return Err(invalid("duplicate behavior interaction definition"));
        }
    }
    let player = assets::facts::entity::EntityFacts::pinned()
        .player()
        .native_class
        .identifier();
    let families = player
        .and_then(|player| definitions.get(player))
        .and_then(|(_, entity)| player_families(entity));
    Ok(symbols
        .iter()
        .filter(|symbol| symbol.kind == EntityAssetKind::Entity)
        .map(|symbol| {
            let admission = definitions.get(symbol.identifier.as_ref()).map_or(
                EntityUseAdmission::Unresolved,
                |(source, entity)| EntityUseAdmission::Components {
                    source: *source,
                    offerings: offerings(entity, families.as_deref()).into(),
                },
            );
            (symbol.identifier.clone(), admission)
        })
        .collect::<BTreeMap<_, _>>())
}

/// A player's conditional family changes cannot establish a fixed rider or filter identity.
fn player_families(entity: &Value) -> Option<Vec<&str>> {
    if let Some(groups) = entity.get("component_groups")
        && groups
            .as_object()?
            .values()
            .any(|group| group.get("minecraft:type_family").is_some())
    {
        return None;
    }
    entity
        .pointer("/components/minecraft:type_family/family")?
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect()
}

/// Collects every possibly active component; selectors never imply that a group is active.
fn offerings(entity: &Value, player_families: Option<&[&str]>) -> Vec<ComponentUse> {
    let mut output = Vec::new();
    if entity
        .get("component_groups")
        .is_some_and(|groups| !groups.is_object())
    {
        output.push(ComponentUse {
            selector: UseSelector::Base,
            component: "minecraft:unresolved".into(),
            condition: C::Unresolved,
        });
    }
    components(
        &entity["components"],
        UseSelector::Base,
        player_families,
        &mut output,
    );
    if let Some(groups) = entity.get("component_groups").and_then(Value::as_object) {
        for (index, group) in groups.values().enumerate() {
            components(
                group,
                UseSelector::UnresolvedComponentGroup {
                    group: index as u32,
                },
                player_families,
                &mut output,
            );
        }
    }
    output
}

/// Compiles only necessary conditions for instant throwable use; other item uses stay unresolved.
fn components(
    value: &Value,
    selector: UseSelector,
    player_families: Option<&[&str]>,
    output: &mut Vec<ComponentUse>,
) {
    let Some(components) = value.as_object() else {
        output.push(ComponentUse {
            selector,
            component: "minecraft:unresolved".into(),
            condition: C::Unresolved,
        });
        return;
    };
    for (name, component) in components {
        let condition = match name.as_str() {
            "minecraft:interact" => component
                .get("interactions")
                .and_then(Value::as_array)
                .map_or(C::Unresolved, |interactions| {
                    C::Any(
                        interactions
                            .iter()
                            .map(|interaction| {
                                filter(interaction.pointer("/on_interact/filters"), player_families)
                            })
                            .collect(),
                    )
                }),
            "minecraft:rideable" => rideable(component, player_families),
            "minecraft:breedable" => items(component.get("breed_items")),
            "minecraft:ageable" => C::Any(
                ["feed_items", "pause_growth_items", "reset_growth_items"]
                    .into_iter()
                    .filter_map(|field| component.get(field))
                    .map(|value| items(Some(value)))
                    .collect(),
            ),
            "minecraft:healable" => items(component.get("items")),
            "minecraft:tameable" => items(component.get("tame_items")),
            "minecraft:tamemount" => C::Any(
                ["feed_items", "auto_reject_items"]
                    .into_iter()
                    .map(|field| items(component.get(field)))
                    .collect(),
            ),
            "minecraft:bribeable" => items(component.get("bribe_items")),
            "minecraft:giveable" => component.get("triggers").and_then(Value::as_array).map_or(
                C::Unresolved,
                |triggers| {
                    C::Any(
                        triggers
                            .iter()
                            // Event filters run after the offering has been selected.
                            .map(|trigger| items(trigger.get("items")))
                            .collect(),
                    )
                },
            ),
            "minecraft:equippable" => {
                component
                    .get("slots")
                    .and_then(Value::as_array)
                    .map_or(C::Unresolved, |slots| {
                        C::Any(
                            slots
                                .iter()
                                .map(|slot| items(slot.get("accepted_items")))
                                .collect(),
                        )
                    })
            }
            "minecraft:inventory"
            | "minecraft:economy_trade_table"
            | "minecraft:trade_table"
            | "minecraft:trusting"
            | "minecraft:leashable"
            | "minecraft:leashable_to"
            | "minecraft:balloonable"
            | assets::facts::entity::NPC_COMPONENT_IDENTIFIER
            | "minecraft:sittable"
            | "minecraft:interact_with" => C::Unresolved,
            // These callbacks require a non-throwable item role. The runtime admits this proof
            // only for a known retail throwable without a server component override.
            "minecraft:nameable"
            | "minecraft:dyeable"
            | "minecraft:spawn_egg_interaction"
            | "minecraft:boostable" => C::Never,
            // A namespace alone cannot rule out an interaction from an unclassified component.
            _ => C::Unresolved,
        };
        output.push(ComponentUse {
            selector,
            component: name.clone().into(),
            condition,
        });
    }
}

/// Explicit rider-family exclusion is independent from seat occupancy or group activation.
fn rideable(component: &Value, player_families: Option<&[&str]>) -> C {
    match (
        component.get("family_types").and_then(Value::as_array),
        player_families,
    ) {
        (Some(families), Some(player))
            if !families.is_empty()
                && families.iter().all(|family| {
                    family
                        .as_str()
                        .is_some_and(|family| !player.contains(&family))
                }) =>
        {
            C::Never
        }
        _ => C::Unresolved,
    }
}

/// Normalizes item names and byte auxiliary values; unsupported descriptor syntax stays unknown.
fn item(value: &Value) -> C {
    let Some(value) = value
        .as_str()
        .or_else(|| value.get("item").and_then(Value::as_str))
    else {
        return C::Unresolved;
    };
    if !value.is_ascii() {
        return C::Unresolved;
    }
    let value = value.to_ascii_lowercase();
    let mut parts: Vec<_> = value.split(':').collect();
    let metadata = if parts.len() > 1
        && parts
            .last()
            .is_some_and(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        let Some(aux) = parts.pop().and_then(|part| part.parse::<u8>().ok()) else {
            // Invalid suffixes retain a caller-specific default, which is not modeled here.
            return C::Unresolved;
        };
        Some(u32::from(aux))
    } else {
        None
    };
    let identifier = match parts.as_slice() {
        [name] if !name.is_empty() => format!("minecraft:{name}"),
        [namespace, name] if !namespace.is_empty() && !name.is_empty() => {
            format!("{namespace}:{name}")
        }
        _ => return C::Unresolved,
    };
    if parts.last().is_some_and(|name| name.starts_with("tile.")) {
        return C::Unresolved;
    }
    C::HeldItem {
        identifier: identifier.into(),
        metadata,
    }
}

/// An omitted accepted-items list may allow any item and never proves absence.
fn items(value: Option<&Value>) -> C {
    match value {
        Some(Value::Array(values)) => C::Any(values.iter().map(item).collect()),
        Some(value) => item(value),
        None => C::Unresolved,
    }
}

/// Lowers supported JSON filter structure to necessary conditions without evaluating text expressions.
fn filter(value: Option<&Value>, player_families: Option<&[&str]>) -> C {
    let Some(value) = value else {
        return C::Always;
    };
    let Some(object) = value.as_object() else {
        return C::Unresolved;
    };
    if object.is_empty() {
        return C::Always;
    }
    for (key, any, negate) in [
        ("all_of", false, false),
        ("any_of", true, false),
        ("none_of", true, true),
    ] {
        if let Some(values) = object.get(key).and_then(Value::as_array) {
            if object.len() != 1 {
                return C::Unresolved;
            }
            let conditions = values
                .iter()
                .map(|value| filter(Some(value), player_families))
                .collect();
            let condition = if any {
                C::Any(conditions)
            } else {
                C::All(conditions)
            };
            return if negate {
                C::Not(Box::new(condition))
            } else {
                condition
            };
        }
    }
    let supported_fields = ["test", "subject", "domain", "operator", "value"];
    if object
        .keys()
        .any(|key| !supported_fields.contains(&key.as_str()))
    {
        return C::Unresolved;
    }
    let condition = match (
        value["test"].as_str(),
        value["subject"].as_str(),
        value["domain"].as_str(),
    ) {
        (Some("has_equipment"), Some("other"), Some("main_hand")) => item(&value["value"]),
        (Some("is_family"), Some("other"), None) => {
            match (value["value"].as_str(), player_families) {
                (Some(family), Some(player)) => {
                    if player.contains(&family) {
                        C::Always
                    } else {
                        C::Never
                    }
                }
                _ => C::Unresolved,
            }
        }
        _ => C::Unresolved,
    };
    match value.get("operator") {
        None => condition,
        Some(Value::String(operator)) => match operator.as_str() {
            "equals" | "==" => condition,
            "not" | "!=" | "not_equals" => C::Not(Box::new(condition)),
            _ => C::Unresolved,
        },
        _ => C::Unresolved,
    }
}

#[cfg(test)]
mod tests;
