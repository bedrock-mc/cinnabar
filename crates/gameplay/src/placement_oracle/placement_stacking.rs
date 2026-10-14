//! Retained oracle: Existing-cell stacking rules for local placement prediction.

use serde_json::{Map, Value};

/// Adds one verified stack unit; snow requires a whole-cell actor check by the caller.
pub(crate) fn stacked_placement_state(
    held_identifier: &str,
    existing_identifier: &str,
    existing_states: &Map<String, Value>,
    face: u8,
) -> Option<Map<String, Value>> {
    if held_identifier == "minecraft:sea_pickle"
        && held_identifier == existing_identifier
        && (1..=5).contains(&face)
        && crate::placement_oracle::placement_state::only_keys(
            existing_states,
            &["cluster_count", "dead_bit"],
        )
    {
        let count =
            crate::placement_oracle::placement_state::value(existing_states, "cluster_count")?
                .as_u64()?;
        if count >= 3 {
            return None;
        }
        let mut states = existing_states.clone();
        crate::placement_oracle::placement_state::set_value(
            &mut states,
            "cluster_count",
            Value::from(count + 1),
        )?;
        crate::placement_oracle::placement_state::set_bit(&mut states, "dead_bit", true)?;
        return Some(states);
    }
    if held_identifier == existing_identifier
        && crate::placement_oracle::placement_support::is_candle(held_identifier)
        && (1..=5).contains(&face)
        && crate::placement_oracle::placement_state::only_keys(existing_states, &["candles", "lit"])
    {
        let count = crate::placement_oracle::placement_state::value(existing_states, "candles")?
            .as_u64()?;
        if count >= 3 {
            return None;
        }
        let mut states = existing_states.clone();
        let lit = crate::placement_oracle::placement_state::value(&states, "lit")?.clone();
        crate::placement_oracle::placement_state::set_bit(
            &mut states,
            "lit",
            lit.as_bool().or_else(|| lit.as_u64().map(|v| v == 1))?,
        )?;
        crate::placement_oracle::placement_state::set_value(
            &mut states,
            "candles",
            Value::from(count + 1),
        )?;
        return Some(states);
    }
    if face > 5
        || held_identifier != "minecraft:snow_layer"
        || held_identifier != existing_identifier
    {
        return None;
    }
    if existing_states.len() != 2
        || existing_states
            .keys()
            .any(|key| !["height", "covered_bit"].contains(&key.as_str()))
    {
        return None;
    }
    let covered = existing_states.get("covered_bit")?;
    if covered.get("type")?.as_str()? != "byte"
        || !matches!(covered.get("value")?.as_u64(), Some(0 | 1))
        || existing_states.get("height")?.get("type")?.as_str()? != "int"
    {
        return None;
    }
    let height = existing_states.get("height")?.get("value")?.as_u64()?;
    if height >= 7 {
        return None;
    }
    let mut states = existing_states.clone();
    let state = states.get_mut("height")?.as_object_mut()?;
    *state.get_mut("value")? = Value::from(height + 1);
    Some(states)
}
