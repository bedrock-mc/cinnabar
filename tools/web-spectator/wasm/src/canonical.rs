use std::collections::BTreeMap;

use assets::RegistryRecord;
use serde_json::{Map, Value};

use crate::model::PaletteEntry;

pub(super) type CanonicalIndex = BTreeMap<String, u32>;

pub(super) fn registry_index(records: &[RegistryRecord]) -> Result<CanonicalIndex, String> {
    let mut index = BTreeMap::new();
    for record in records {
        let typed: Map<String, Value> = serde_json::from_str(&record.canonical_state)
            .map_err(|error| format!("invalid canonical registry state: {error}"))?;
        let states = typed
            .iter()
            .map(|(name, field)| {
                let value = field
                    .get("value")
                    .ok_or("canonical registry state has no scalar value")?;
                Ok((name.clone(), scalar(value)?))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let key = state_key(&record.name, &states)?;
        if index.insert(key, record.sequential_id).is_some() {
            return Err("canonical registry contains an ambiguous untyped state".into());
        }
    }
    Ok(index)
}

pub(super) fn palette_ids(
    index: &CanonicalIndex,
    palette: &[PaletteEntry],
) -> Result<Vec<u32>, String> {
    palette
        .iter()
        .map(|entry| {
            let states = entry
                .states
                .iter()
                .map(|(name, value)| Ok((name.clone(), scalar(value)?)))
                .collect::<Result<BTreeMap<_, _>, String>>()?;
            let key = state_key(&entry.name, &states)?;
            index.get(&key).copied().ok_or_else(|| {
                format!(
                    "arena block {} with states {states:?} is absent from the terrain registry",
                    entry.name
                )
            })
        })
        .collect()
}

fn state_key(name: &str, states: &BTreeMap<String, Value>) -> Result<String, String> {
    let canonical_name = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    serde_json::to_string(&(canonical_name, states)).map_err(|error| error.to_string())
}

fn scalar(value: &Value) -> Result<Value, String> {
    match value {
        // Boolean bit states resolve to the registry's byte-valued form.
        Value::Bool(value) => Ok(Value::from(i64::from(*value))),
        Value::String(_) => Ok(value.clone()),
        Value::Number(number) if number.as_i64().is_some() => Ok(value.clone()),
        _ => Err("arena state must be a string, integer or boolean bit".into()),
    }
}
