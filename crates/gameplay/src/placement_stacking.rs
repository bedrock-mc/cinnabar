//! Existing-cell stacking rules for local placement prediction.

use serde_json::{Map, Value};

/// Adds a snow layer in the clicked cell; the caller checks actors against the whole cell.
pub(crate) fn stacked_placement_state(
    held_identifier: &str,
    existing_identifier: &str,
    existing_states: &Map<String, Value>,
    face: u8,
) -> Option<Map<String, Value>> {
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

/// Parses the stored palette envelope before resolving an existing-cell stack.
pub(crate) fn stacked_placement_canonical(
    held_identifier: &str,
    existing_identifier: &str,
    existing_canonical: &str,
    face: u8,
) -> Option<Map<String, Value>> {
    let states = serde_json::from_str(existing_canonical).ok()?;
    stacked_placement_state(held_identifier, existing_identifier, &states, face)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Keeps the unrelated cover flag so a stack cannot silently replace its block state.
    fn states(height: u64) -> Map<String, Value> {
        serde_json::from_value(json!({
            "height": {"type":"int","value":height},
            "covered_bit": {"type":"byte","value":1}
        }))
        .unwrap()
    }

    #[test]
    fn snow_stacks_on_every_face_without_changing_other_states() {
        for face in 0..=5 {
            for height in 0..=6 {
                let original = states(height);
                let placed = stacked_placement_state(
                    "minecraft:snow_layer",
                    "minecraft:snow_layer",
                    &original,
                    face,
                )
                .unwrap();
                assert_eq!(placed["height"]["value"], Value::from(height + 1));
                assert_eq!(placed["height"]["type"], original["height"]["type"]);
                assert_eq!(placed["covered_bit"], original["covered_bit"]);
            }
        }
    }

    #[test]
    fn full_snow_and_mismatched_items_do_not_merge_in_place() {
        for face in 0..=5 {
            assert!(
                stacked_placement_state(
                    "minecraft:snow_layer",
                    "minecraft:snow_layer",
                    &states(7),
                    face,
                )
                .is_none()
            );
        }
        for (held, existing) in [
            ("minecraft:stone", "minecraft:snow_layer"),
            ("minecraft:snow_layer", "minecraft:snow"),
            ("minecraft:candle", "minecraft:candle"),
            ("minecraft:sea_pickle", "minecraft:sea_pickle"),
        ] {
            assert!(stacked_placement_state(held, existing, &states(0), 1).is_none());
        }
    }

    #[test]
    fn malformed_snow_states_and_invalid_faces_do_not_merge() {
        for canonical in [
            "invalid",
            "{}",
            r#"{"height":{"value":-1}}"#,
            r#"{"height":{"value":8}}"#,
        ] {
            assert!(
                stacked_placement_canonical(
                    "minecraft:snow_layer",
                    "minecraft:snow_layer",
                    canonical,
                    1,
                )
                .is_none()
            );
        }
        assert!(
            stacked_placement_state(
                "minecraft:snow_layer",
                "minecraft:snow_layer",
                &states(0),
                6,
            )
            .is_none()
        );
    }
}
