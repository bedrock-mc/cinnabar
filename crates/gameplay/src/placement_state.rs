//! Click-dependent block states, resolved without world or rendering dependencies.
use serde_json::{Map, Value};

const CONNECTION_STATES: &[&str] = &[
    "minecraft:connection_north",
    "minecraft:connection_south",
    "minecraft:connection_east",
    "minecraft:connection_west",
];

/// The click inputs used by the supported placement rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementInput {
    pub face: u8,
    pub click_position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
}

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
    if only_keys(&states, &["pillar_axis"]) {
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
    } else if (identifier.ends_with("_fence")
        || identifier.ends_with("_pane")
        || identifier == "minecraft:iron_bars")
        && only_keys(&states, CONNECTION_STATES)
    {
        // The world-aware resolver fills connection bits before checking collision.
        for &key in CONNECTION_STATES {
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
fn single_slab(identifier: &str) -> bool {
    identifier.starts_with("minecraft:")
        && identifier.ends_with("_slab")
        && !identifier.contains("double_")
}

/// Map modern slab identifiers to their registered double counterparts.
fn double_slab_identifier(identifier: &str) -> Option<String> {
    if let Some(prefix) = identifier.strip_suffix("cut_copper_slab") {
        return Some(format!("{prefix}double_cut_copper_slab"));
    }
    Some(format!("{}_double_slab", identifier.strip_suffix("_slab")?))
}

/// Reject state keys whose placement behavior has not been modeled.
fn only_keys(states: &Map<String, Value>, allowed: &[&str]) -> bool {
    states.len() == allowed.len() && states.keys().all(|key| allowed.contains(&key.as_str()))
}

/// Read a state value in either canonical typed form or plain test form.
fn value<'a>(states: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
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
fn integer_in(states: &Map<String, Value>, key: &str, allowed: &[u64]) -> Option<()> {
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
fn set_value(states: &mut Map<String, Value>, key: &str, replacement: Value) -> Option<()> {
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
fn set_bit(states: &mut Map<String, Value>, key: &str, enabled: bool) -> Option<()> {
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
fn yaw_quadrant(yaw: f32) -> Option<usize> {
    yaw.is_finite()
        .then(|| (yaw / 90.0 + 0.5).floor().rem_euclid(4.0) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a click with the inputs changed by each table row.
    fn input(face: u8, height: f32, yaw: f32) -> PlacementInput {
        PlacementInput {
            face,
            click_position: [0.5, height, 0.5],
            yaw,
            pitch: 0.0,
        }
    }

    #[test]
    fn pillar_axis_follows_each_clicked_face() {
        for (face, axis) in ["y", "y", "z", "z", "x", "x"].into_iter().enumerate() {
            let states = resolve_placement_state(
                "minecraft:oak_log",
                r#"{"pillar_axis":{"type":"string","value":"y"}}"#,
                input(face as u8, 0.5, 0.0),
            )
            .unwrap();
            assert_eq!(states["pillar_axis"], json!({"type":"string","value":axis}));
        }
    }

    #[test]
    fn slabs_use_face_and_strict_hit_midpoint() {
        for (face, height, expected) in [
            (0, 0.1, "top"),
            (1, 0.9, "bottom"),
            (2, 0.5, "bottom"),
            (3, 0.5001, "top"),
            (4, 0.1, "bottom"),
            (5, 0.9, "top"),
        ] {
            let states = resolve_placement_state(
                "minecraft:oak_slab",
                r#"{"minecraft:vertical_half":{"type":"string","value":"bottom"}}"#,
                input(face, height, 0.0),
            )
            .unwrap();
            assert_eq!(
                value(&states, "minecraft:vertical_half"),
                Some(&json!(expected))
            );
        }
    }

    #[test]
    fn matching_slabs_merge_only_through_the_exposed_clicked_half() {
        for (existing_half, face) in [("bottom", 1), ("top", 0)] {
            let existing =
                json!({"minecraft:vertical_half":{"type":"string","value":existing_half}})
                    .to_string();
            let (identifier, states) = merge_slab_state(
                "minecraft:oak_slab",
                &existing,
                "minecraft:oak_slab",
                &existing,
                Some(face),
            )
            .unwrap();
            assert_eq!(identifier, "minecraft:oak_double_slab");
            assert_eq!(
                value(&states, "minecraft:vertical_half"),
                Some(&json!("bottom"))
            );
            assert!(
                merge_slab_state(
                    "minecraft:oak_slab",
                    &existing,
                    "minecraft:oak_slab",
                    &existing,
                    Some(face ^ 1)
                )
                .is_none()
            );
            assert!(
                merge_slab_state(
                    "minecraft:oak_slab",
                    &existing,
                    "minecraft:oak_slab",
                    &existing,
                    Some(2)
                )
                .is_none()
            );
            assert!(
                merge_slab_state(
                    "minecraft:oak_slab",
                    &existing,
                    "minecraft:oak_slab",
                    &existing,
                    None
                )
                .is_some()
            );
            assert!(
                merge_slab_state(
                    "minecraft:oak_slab",
                    &existing,
                    "minecraft:birch_slab",
                    &existing,
                    None
                )
                .is_none()
            );
        }
    }

    #[test]
    fn copper_slab_names_preserve_oxidation_and_wax() {
        for (single, double) in [
            (
                "minecraft:cut_copper_slab",
                "minecraft:double_cut_copper_slab",
            ),
            (
                "minecraft:waxed_exposed_cut_copper_slab",
                "minecraft:waxed_exposed_double_cut_copper_slab",
            ),
        ] {
            assert_eq!(double_slab_identifier(single).as_deref(), Some(double));
        }
    }

    #[test]
    fn connection_states_start_clear_for_neighbor_derived_geometry() {
        let canonical = json!({
            "minecraft:connection_north":{"type":"byte","value":1},
            "minecraft:connection_south":{"type":"byte","value":1},
            "minecraft:connection_east":{"type":"byte","value":1},
            "minecraft:connection_west":{"type":"byte","value":1},
        })
        .to_string();
        for identifier in [
            "minecraft:oak_fence",
            "minecraft:glass_pane",
            "minecraft:iron_bars",
        ] {
            let states =
                resolve_placement_state(identifier, &canonical, input(1, 0.5, 0.0)).unwrap();
            for &key in CONNECTION_STATES {
                assert_eq!(value(&states, key), Some(&json!(0)));
            }
        }
    }

    #[test]
    fn trapdoors_have_their_own_yaw_encoding_and_half() {
        let canonical = r#"{"direction":{"type":"int","value":0},"open_bit":{"type":"byte","value":0},"upside_down_bit":{"type":"byte","value":0}}"#;
        for (yaw, direction) in [(0.0, 3), (90.0, 0), (180.0, 2), (-90.0, 1), (360.0, 3)] {
            let states =
                resolve_placement_state("minecraft:oak_trapdoor", canonical, input(0, 0.1, yaw))
                    .unwrap();
            assert_eq!(value(&states, "direction"), Some(&json!(direction)));
            assert_eq!(value(&states, "upside_down_bit"), Some(&json!(1)));
            assert_eq!(value(&states, "open_bit"), Some(&json!(0)));
        }
    }

    #[test]
    fn hoppers_point_into_the_clicked_face_and_vertical_clicks_point_down() {
        let canonical = r#"{"facing_direction":{"type":"int","value":0},"toggle_bit":{"type":"byte","value":0}}"#;
        for (face, expected) in [0, 0, 3, 2, 5, 4].into_iter().enumerate() {
            let states =
                resolve_placement_state("minecraft:hopper", canonical, input(face as u8, 0.5, 0.0))
                    .unwrap();
            assert_eq!(value(&states, "facing_direction"), Some(&json!(expected)));
            assert_eq!(value(&states, "toggle_bit"), Some(&json!(0)));
        }
    }

    #[test]
    fn unknown_or_invalid_inputs_wait_for_authority() {
        for (identifier, canonical, click) in [
            (
                "custom:pillar",
                r#"{"pillar_axis":"y"}"#,
                input(1, 0.5, 0.0),
            ),
            (
                "minecraft:oak_log",
                r#"{"pillar_axis":"y","unknown":0}"#,
                input(1, 0.5, 0.0),
            ),
            (
                "minecraft:oak_slab",
                r#"{"minecraft:vertical_half":"bottom"}"#,
                input(2, f32::NAN, 0.0),
            ),
            ("minecraft:stone", "{}", input(6, 0.5, 0.0)),
            (
                "minecraft:observer",
                r#"{"minecraft:facing_direction":"north"}"#,
                input(1, 0.5, 0.0),
            ),
        ] {
            assert!(resolve_placement_state(identifier, canonical, click).is_none());
        }
    }

    #[test]
    fn malformed_supported_states_wait_for_authority() {
        for (identifier, canonical) in [
            ("minecraft:oak_log", json!({"pillar_axis":"q"})),
            (
                "minecraft:oak_log",
                json!({"pillar_axis":{"type":"byte","value":"y"}}),
            ),
            (
                "minecraft:oak_slab",
                json!({"minecraft:vertical_half":"middle"}),
            ),
            (
                "minecraft:oak_trapdoor",
                json!({"direction":4,"open_bit":0,"upside_down_bit":0}),
            ),
            (
                "minecraft:oak_trapdoor",
                json!({"direction":0,"open_bit":2,"upside_down_bit":0}),
            ),
            (
                "minecraft:oak_trapdoor",
                json!({"direction":0,"upside_down_bit":0}),
            ),
            (
                "minecraft:hopper",
                json!({"facing_direction":1,"toggle_bit":0}),
            ),
            (
                "minecraft:hopper",
                json!({"facing_direction":0,"toggle_bit":-1}),
            ),
            (
                "minecraft:glass_pane",
                json!({
                    "minecraft:connection_north":2,
                    "minecraft:connection_south":0,
                    "minecraft:connection_east":0,
                    "minecraft:connection_west":0,
                }),
            ),
        ] {
            assert!(
                resolve_placement_state(identifier, &canonical.to_string(), input(1, 0.5, 0.0))
                    .is_none(),
                "{identifier}: {canonical}"
            );
        }
        let valid = r#"{"minecraft:vertical_half":{"type":"string","value":"bottom"}}"#;
        let invalid = r#"{"minecraft:vertical_half":{"type":"int","value":"bottom"}}"#;
        for (held, existing) in [(invalid, valid), (valid, invalid)] {
            assert!(
                merge_slab_state(
                    "minecraft:oak_slab",
                    held,
                    "minecraft:oak_slab",
                    existing,
                    Some(1)
                )
                .is_none()
            );
        }
    }
}
