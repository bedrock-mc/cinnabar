//! Existing-cell stacking rules for local placement prediction.

use assets::facts::BlockProperty as P;
use assets::facts::{BlockFact, PlacementRule as R};
use serde_json::{Map, Value};

/// Adds one verified stack unit; snow requires a whole-cell actor check by the caller.
pub(crate) fn stacked_placement_state(
    held_fact: BlockFact,
    existing_fact: BlockFact,
    existing_states: &Map<String, Value>,
    face: u8,
) -> Option<Map<String, Value>> {
    if held_fact.placement == R::SeaPickle
        && held_fact.identity.is_some()
        && held_fact.identity == existing_fact.identity
        && (1..=5).contains(&face)
        && crate::placement_state::only_keys(
            existing_states,
            &[P::ClusterCount.name(), P::DeadBit.name()],
        )
    {
        let count =
            crate::placement_state::value(existing_states, P::ClusterCount.name())?.as_u64()?;
        if count >= crate::placement_state::maximum(existing_fact, P::ClusterCount)? {
            return None;
        }
        let mut states = existing_states.clone();
        crate::placement_state::set_value(
            &mut states,
            P::ClusterCount.name(),
            Value::from(count + 1),
        )?;
        crate::placement_state::set_bit(&mut states, P::DeadBit.name(), true)?;
        return Some(states);
    }
    if held_fact.identity.is_some()
        && held_fact.identity == existing_fact.identity
        && held_fact.placement == R::Candle
        && (1..=5).contains(&face)
        && crate::placement_state::only_keys(existing_states, &[P::Candles.name(), P::Lit.name()])
    {
        let count = crate::placement_state::value(existing_states, P::Candles.name())?.as_u64()?;
        if count >= crate::placement_state::maximum(existing_fact, P::Candles)? {
            return None;
        }
        let mut states = existing_states.clone();
        let lit = crate::placement_state::value(&states, P::Lit.name())?.clone();
        crate::placement_state::set_bit(
            &mut states,
            P::Lit.name(),
            lit.as_bool().or_else(|| lit.as_u64().map(|v| v == 1))?,
        )?;
        crate::placement_state::set_value(&mut states, P::Candles.name(), Value::from(count + 1))?;
        return Some(states);
    }
    if face > 5
        || held_fact.placement != R::Snow
        || held_fact.identity.is_none()
        || held_fact.identity != existing_fact.identity
    {
        return None;
    }
    if existing_states.len() != 2
        || existing_states
            .keys()
            .any(|key| ![P::Height.name(), P::CoveredBit.name()].contains(&key.as_str()))
    {
        return None;
    }
    let covered = existing_states.get(P::CoveredBit.name())?;
    if covered.get("type")?.as_str()? != "byte"
        || !matches!(covered.get("value")?.as_u64(), Some(0 | 1))
        || existing_states
            .get(P::Height.name())?
            .get("type")?
            .as_str()?
            != "int"
    {
        return None;
    }
    let height = existing_states
        .get(P::Height.name())?
        .get("value")?
        .as_u64()?;
    if height >= crate::placement_state::maximum(existing_fact, P::Height)? {
        return None;
    }
    let mut states = existing_states.clone();
    let state = states.get_mut(P::Height.name())?.as_object_mut()?;
    *state.get_mut("value")? = Value::from(height + 1);
    Some(states)
}

/// Parses the stored palette envelope before resolving an existing-cell stack.
pub(crate) fn stacked_placement_canonical(
    held_fact: BlockFact,
    existing_fact: BlockFact,
    existing_canonical: &str,
    face: u8,
) -> Option<Map<String, Value>> {
    let states = serde_json::from_str(existing_canonical).ok()?;
    stacked_placement_state(held_fact, existing_fact, &states, face)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::block_fact;

    /// Supplies the real held identity facts for stack behavior cases.
    fn stacked_placement_state(
        held: &str,
        existing: &str,
        states: &Map<String, Value>,
        face: u8,
    ) -> Option<Map<String, Value>> {
        super::stacked_placement_state(block_fact(held), block_fact(existing), states, face)
    }

    /// Parses the tested envelope using compiled held-block admission.
    fn stacked_placement_canonical(
        held: &str,
        existing: &str,
        state: &str,
        face: u8,
    ) -> Option<Map<String, Value>> {
        super::stacked_placement_canonical(block_fact(held), block_fact(existing), state, face)
    }
    use serde_json::json;

    #[test]
    fn candle_and_pickle_stacks_cover_every_count_face_and_flag() {
        for (identifier, count_key, bit_key) in [
            ("minecraft:candle", "candles", "lit"),
            ("minecraft:red_candle", "candles", "lit"),
            ("minecraft:sea_pickle", "cluster_count", "dead_bit"),
        ] {
            for count in 0..=3 {
                for flag in 0..=1 {
                    let original = serde_json::from_value(json!({
                        count_key: {"type":"int","value":count},
                        bit_key: {"type":"byte","value":flag}
                    }))
                    .unwrap();
                    for face in 0..=6 {
                        let result =
                            stacked_placement_state(identifier, identifier, &original, face);
                        if count == 3 || face == 0 || face == 6 {
                            assert!(result.is_none());
                        } else {
                            let result = result.unwrap();
                            assert_eq!(result[count_key]["value"], count + 1);
                            assert_eq!(
                                result[bit_key]["value"],
                                if bit_key == "dead_bit" { 1 } else { flag }
                            );
                        }
                    }
                }
            }
        }
    }

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
