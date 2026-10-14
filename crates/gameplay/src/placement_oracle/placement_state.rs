//! Retained oracle: Click-dependent block states, resolved without world or rendering dependencies.
pub(crate) const CONNECTION_STATES: &[(&str, u8)] = &[
    ("minecraft:connection_west", 4),
    ("minecraft:connection_east", 5),
    ("minecraft:connection_north", 2),
    ("minecraft:connection_south", 3),
];
use crate::placement_state::PlacementInput;
use serde_json::{Map, Value};

/// Resolve a registered held state; unsupported state families wait for the server.
pub fn resolve_placement_state(
    identifier: &str,
    canonical_state: &str,
    input: PlacementInput,
) -> Option<Map<String, Value>> {
    if input.face > 5 {
        return None;
    }
    let mut states: Map<String, Value> = serde_json::from_str(canonical_state).ok()?;
    if states.is_empty() {
        return Some(states);
    }
    if !identifier.starts_with("minecraft:") {
        return None;
    }
    if identifier.ends_with("_stairs")
        && only_keys(
            &states,
            &["weirdo_direction", "upside_down_bit", "minecraft:corner"],
        )
    {
        integer_in(&states, "weirdo_direction", &[0, 1, 2, 3])?;
        bit(&states, "upside_down_bit")?;
        string_in(
            &states,
            "minecraft:corner",
            &[
                "none",
                "inner_left",
                "inner_right",
                "outer_left",
                "outer_right",
            ],
        )?;
        set_value(
            &mut states,
            "weirdo_direction",
            Value::from([2, 1, 3, 0][yaw_quadrant(input.yaw)?]),
        )?;
        if upper_half(input)? {
            set_bit(&mut states, "upside_down_bit", true)?;
        }
        set_value(&mut states, "minecraft:corner", Value::from("none"))?;
    } else if identifier == "minecraft:loom" && only_keys(&states, &["direction"]) {
        integer_in(&states, "direction", &[0, 1, 2, 3])?;
        set_value(
            &mut states,
            "direction",
            Value::from([2, 3, 0, 1][yaw_quadrant(input.yaw)?]),
        )?;
    } else if identifier.ends_with("_glazed_terracotta")
        && only_keys(&states, &["facing_direction"])
    {
        integer_in(&states, "facing_direction", &[0, 1, 2, 3, 4, 5])?;
        set_value(
            &mut states,
            "facing_direction",
            Value::from([2, 5, 3, 4][yaw_quadrant(input.yaw)?]),
        )?;
    } else if identifier == "minecraft:snow_layer" && only_keys(&states, &["height", "covered_bit"])
    {
        integer_in(&states, "height", &[0, 1, 2, 3, 4, 5, 6, 7])?;
        set_value(&mut states, "height", Value::from(0))?;
        set_bit(&mut states, "covered_bit", false)?;
    } else if identifier == "minecraft:sea_pickle"
        && only_keys(&states, &["cluster_count", "dead_bit"])
    {
        integer_in(&states, "cluster_count", &[0, 1, 2, 3])?;
        set_value(&mut states, "cluster_count", Value::from(0))?;
        set_bit(&mut states, "dead_bit", true)?;
    } else if crate::placement_oracle::placement_support::is_candle(identifier)
        && only_keys(&states, &["candles", "lit"])
    {
        integer_in(&states, "candles", &[0, 1, 2, 3])?;
        set_value(&mut states, "candles", Value::from(0))?;
        set_bit(&mut states, "lit", false)?;
    } else if only_keys(&states, &["pillar_axis"]) {
        string_in(&states, "pillar_axis", &["x", "y", "z"])?;
        let axis = ["y", "y", "z", "z", "x", "x"][usize::from(input.face)];
        set_value(&mut states, "pillar_axis", Value::from(axis))?;
    } else if single_slab(identifier) && only_keys(&states, &["minecraft:vertical_half"]) {
        string_in(&states, "minecraft:vertical_half", &["bottom", "top"])?;
        set_value(
            &mut states,
            "minecraft:vertical_half",
            Value::from(if upper_half(input)? { "top" } else { "bottom" }),
        )?;
    } else if (identifier == "minecraft:trapdoor" || identifier.ends_with("_trapdoor"))
        && only_keys(&states, &["direction", "open_bit", "upside_down_bit"])
    {
        integer_in(&states, "direction", &[0, 1, 2, 3])?;
        bit(&states, "open_bit")?;
        bit(&states, "upside_down_bit")?;
        // Trapdoors use their own four-way encoding rather than cardinal direction.
        let direction = [3, 0, 2, 1][yaw_quadrant(input.yaw)?];
        set_value(&mut states, "direction", Value::from(direction))?;
        set_bit(&mut states, "upside_down_bit", upper_half(input)?)?;
    } else if identifier == "minecraft:hopper"
        && only_keys(&states, &["facing_direction", "toggle_bit"])
    {
        integer_in(&states, "facing_direction", &[0, 2, 3, 4, 5])?;
        bit(&states, "toggle_bit")?;
        let facing = if input.face < 2 { 0 } else { input.face ^ 1 };
        set_value(&mut states, "facing_direction", Value::from(facing))?;
        set_bit(&mut states, "toggle_bit", false)?;
    } else if states.len() == CONNECTION_STATES.len()
        && CONNECTION_STATES
            .iter()
            .all(|(key, _)| states.contains_key(*key))
    {
        // The world-aware resolver fills connection bits before checking collision.
        for &(key, _) in CONNECTION_STATES {
            bit(&states, key)?;
            set_bit(&mut states, key, false)?;
        }
    } else {
        return None;
    }
    Some(states)
}

/// Merge a matching slab; a clicked slab exposes only its empty vertical half.
/// `clicked_face` is absent when the existing slab is in the neighboring destination.
pub fn merge_slab_state(
    held_identifier: &str,
    held_canonical_state: &str,
    existing_identifier: &str,
    existing_canonical_state: &str,
    clicked_face: Option<u8>,
) -> Option<(String, Map<String, Value>)> {
    if held_identifier != existing_identifier || !single_slab(held_identifier) {
        return None;
    }
    let held: Map<String, Value> = serde_json::from_str(held_canonical_state).ok()?;
    let existing: Map<String, Value> = serde_json::from_str(existing_canonical_state).ok()?;
    if !only_keys(&held, &["minecraft:vertical_half"])
        || !only_keys(&existing, &["minecraft:vertical_half"])
    {
        return None;
    }
    string_in(&held, "minecraft:vertical_half", &["bottom", "top"])?;
    let upper = match value(&existing, "minecraft:vertical_half")?.as_str()? {
        "top" => true,
        "bottom" => false,
        _ => return None,
    };
    if let Some(face) = clicked_face
        && face != u8::from(!upper)
    {
        return None;
    }
    let identifier = double_slab_identifier(held_identifier)?;
    let mut states = held;
    // A double slab is the double block's default state, irrespective of the first half.
    set_value(
        &mut states,
        "minecraft:vertical_half",
        Value::from("bottom"),
    )?;
    Some((identifier, states))
}

/// Whether the identifier names a single vanilla slab.
/// Recognizes the retained single-slab naming contract.
fn single_slab(identifier: &str) -> bool {
    identifier.starts_with("minecraft:")
        && identifier.ends_with("_slab")
        && !identifier.contains("double_")
}

/// Map modern slab identifiers to their registered double counterparts.
/// Resolves the retained name of the matching double slab.
fn double_slab_identifier(identifier: &str) -> Option<String> {
    if let Some(prefix) = identifier.strip_suffix("cut_copper_slab") {
        return Some(format!("{prefix}double_cut_copper_slab"));
    }
    Some(format!("{}_double_slab", identifier.strip_suffix("_slab")?))
}

/// Reject state keys whose placement behavior has not been modeled.
pub(crate) fn only_keys(states: &Map<String, Value>, allowed: &[&str]) -> bool {
    states.len() == allowed.len() && states.keys().all(|key| allowed.contains(&key.as_str()))
}

/// Read a state value in either canonical typed form or plain test form.
pub(crate) fn value<'a>(states: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    let entry = states.get(key)?;
    match entry {
        Value::Object(typed) => {
            if typed.len() != 2 {
                return None;
            }
            let value = typed.get("value")?;
            match typed.get("type")?.as_str()? {
                "string" if value.is_string() => Some(value),
                "byte" if value.as_i64().is_some_and(|n| i8::try_from(n).is_ok()) => Some(value),
                "int" if value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()) => Some(value),
                _ => None,
            }
        }
        plain => Some(plain),
    }
}

/// Require a string state to belong to its modeled domain.
fn string_in(states: &Map<String, Value>, key: &str, allowed: &[&str]) -> Option<()> {
    allowed
        .contains(&value(states, key)?.as_str()?)
        .then_some(())
}

/// Require an integer state to belong to its modeled domain.
pub(crate) fn integer_in(states: &Map<String, Value>, key: &str, allowed: &[u64]) -> Option<()> {
    allowed
        .contains(&value(states, key)?.as_u64()?)
        .then_some(())
}

/// Validate both accepted bit representations before changing a state.
fn bit(states: &Map<String, Value>, key: &str) -> Option<()> {
    match value(states, key)? {
        Value::Bool(_) => Some(()),
        Value::Number(number) if number.as_u64().is_some_and(|n| n <= 1) => Some(()),
        _ => None,
    }
}

/// Replace a state value while preserving the registry's type wrapper.
pub(crate) fn set_value(
    states: &mut Map<String, Value>,
    key: &str,
    replacement: Value,
) -> Option<()> {
    let old = value(states, key)?;
    if !(old.is_string() && replacement.is_string()
        || old.is_boolean() && replacement.is_boolean()
        || old.is_number() && replacement.is_number())
    {
        return None;
    }
    let entry = states.get_mut(key)?;
    let value = match entry {
        Value::Object(typed) => typed.get_mut("value")?,
        plain => plain,
    };
    *value = replacement;
    Some(())
}

/// Preserve boolean and byte representations when setting a state bit.
pub(crate) fn set_bit(states: &mut Map<String, Value>, key: &str, enabled: bool) -> Option<()> {
    bit(states, key)?;
    let replacement = match value(states, key)? {
        Value::Bool(_) => Value::from(enabled),
        Value::Number(number) if number.as_u64().is_some() => Value::from(u8::from(enabled)),
        _ => return None,
    };
    set_value(states, key, replacement)
}

/// Top and bottom faces decide the half; side clicks use a strict midpoint test.
fn upper_half(input: PlacementInput) -> Option<bool> {
    match input.face {
        0 => Some(true),
        1 => Some(false),
        2..=5 if input.click_position[1].is_finite() => Some(input.click_position[1] > 0.5),
        _ => None,
    }
}

/// Quantize yaw to the nearest quarter turn, wrapping negative and repeated turns.
pub(crate) fn yaw_quadrant(yaw: f32) -> Option<usize> {
    yaw.is_finite()
        .then(|| (yaw / 90.0 + 0.5).floor().rem_euclid(4.0) as usize)
}
