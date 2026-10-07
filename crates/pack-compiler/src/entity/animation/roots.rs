//! Activation roots a rig plays, distinct from the alias lookup dictionary.
use std::collections::BTreeMap;

use assets::AssetError;
use serde_json::Value;

#[cfg(test)]
#[path = "roots_tests.rs"]
mod tests;

// Vanilla's 1.8 to 1.10 entity definition upgrade moves legacy controllers into a distinct animation alias before appending activation roots.
// In particular, a legacy controller named `move` must not activate an ordinary `move` clip.
fn legacy_controller_alias(alias: &str) -> Box<str> {
    format!("controller__{alias}").into_boxed_str()
}

pub(super) fn legacy_controller_aliases(
    value: Option<&Value>,
) -> Result<BTreeMap<Box<str>, Box<str>>, AssetError> {
    Ok(super::environment::parse_aliases(value)?
        .into_iter()
        .map(|(alias, target)| (legacy_controller_alias(&alias), target))
        .collect())
}

pub(super) fn animation_aliases(
    description: &serde_json::Map<String, Value>,
) -> Result<BTreeMap<Box<str>, Box<str>>, AssetError> {
    let mut aliases = super::environment::parse_aliases(description.get("animations"))?;
    // The native conversion assigns the generated controller target into this dictionary,
    // replacing an existing generated-name alias rather than scheduling both targets.
    aliases.extend(legacy_controller_aliases(
        description.get("animation_controllers"),
    )?);
    Ok(aliases)
}

pub(crate) fn description(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value
        .get("minecraft:client_entity")
        .or_else(|| value.get("minecraft:attachable"))?
        .get("description")?
        .as_object()
}

/// One `scripts.animate` entry: an alias, optionally gated by a blend-weight expression.
pub(crate) struct ActivationRoot {
    pub alias: String,
    pub condition: Option<String>,
}

/// Returns the aliases a rig plays each tick, or `None` for an unrecognized schema.
pub(crate) fn activation_roots(value: &Value) -> Option<Vec<ActivationRoot>> {
    let description = description(value)?;
    let version = value.get("format_version")?.as_str()?;
    let mut parts = version.split('.').map(str::parse::<u32>);
    let (major, minor) = (parts.next()?.ok()?, parts.next()?.ok()?);
    let mut roots = Vec::<ActivationRoot>::new();
    let mut push = |alias: &str, condition: Option<&str>| {
        if !roots.iter().any(|root| root.alias == alias) {
            roots.push(ActivationRoot {
                alias: alias.to_owned(),
                condition: condition.map(str::to_owned),
            });
        }
    };
    if let Some(entries) = description.get("animation_controllers") {
        for entry in entries.as_array()? {
            for alias in entry.as_object()?.keys() {
                push(&legacy_controller_alias(alias), None);
            }
        }
    }
    if (major, minor) >= (1, 10)
        && let Some(entries) = description.get("scripts").and_then(|v| v.get("animate"))
    {
        for entry in entries.as_array()? {
            match entry {
                Value::String(alias) => push(alias, None),
                Value::Object(weighted) if weighted.len() == 1 => {
                    let (alias, condition) = weighted.iter().next()?;
                    push(alias, Some(condition.as_str()?));
                }
                _ => return None,
            }
        }
    } else if (major, minor) < (1, 8) {
        return None;
    }
    Some(roots)
}
