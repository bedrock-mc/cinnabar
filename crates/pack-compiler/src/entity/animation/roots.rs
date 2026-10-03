//! Activation roots a rig plays, distinct from the alias lookup dictionary.
use serde_json::Value;

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
                push(alias, None);
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
