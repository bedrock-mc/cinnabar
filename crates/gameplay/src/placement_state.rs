//! Click-dependent block states, resolved without world or rendering dependencies.
use crate::placement_connections::CONNECTION_STATES;
use assets::facts::{BlockFact, BlockProperty as P, PlacementRule as R, PropertyDomain};
use serde_json::{Map, Value};

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
    fact: BlockFact,
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
    if fact.placement == R::Unresolved {
        return None;
    }
    if fact.placement == R::Stair
        && only_keys(
            &states,
            &[
                P::WeirdoDirection.name(),
                P::UpsideDownBit.name(),
                P::MinecraftCorner.name(),
            ],
        )
    {
        valid_property(fact, &states, P::WeirdoDirection)?;
        bit(&states, P::UpsideDownBit.name())?;
        valid_property(fact, &states, P::MinecraftCorner)?;
        set_value(
            &mut states,
            P::WeirdoDirection.name(),
            Value::from([2, 1, 3, 0][yaw_quadrant(input.yaw)?]),
        )?;
        if upper_half(input)? {
            set_bit(&mut states, P::UpsideDownBit.name(), true)?;
        }
        set_value(&mut states, P::MinecraftCorner.name(), Value::from("none"))?;
    } else if fact.placement == R::Loom && only_keys(&states, &[P::Direction.name()]) {
        valid_property(fact, &states, P::Direction)?;
        set_value(
            &mut states,
            P::Direction.name(),
            Value::from([2, 3, 0, 1][yaw_quadrant(input.yaw)?]),
        )?;
    } else if fact.placement == R::Glazed && only_keys(&states, &[P::FacingDirection.name()]) {
        valid_property(fact, &states, P::FacingDirection)?;
        set_value(
            &mut states,
            P::FacingDirection.name(),
            Value::from([2, 5, 3, 4][yaw_quadrant(input.yaw)?]),
        )?;
    } else if fact.placement == R::Snow
        && only_keys(&states, &[P::Height.name(), P::CoveredBit.name()])
    {
        valid_property(fact, &states, P::Height)?;
        set_value(&mut states, P::Height.name(), Value::from(0))?;
        set_bit(&mut states, P::CoveredBit.name(), false)?;
    } else if fact.placement == R::SeaPickle
        && only_keys(&states, &[P::ClusterCount.name(), P::DeadBit.name()])
    {
        valid_property(fact, &states, P::ClusterCount)?;
        set_value(&mut states, P::ClusterCount.name(), Value::from(0))?;
        set_bit(&mut states, P::DeadBit.name(), true)?;
    } else if fact.placement == R::Candle && only_keys(&states, &[P::Candles.name(), P::Lit.name()])
    {
        valid_property(fact, &states, P::Candles)?;
        set_value(&mut states, P::Candles.name(), Value::from(0))?;
        set_bit(&mut states, P::Lit.name(), false)?;
    } else if fact.placement == R::Axis && only_keys(&states, &[P::PillarAxis.name()]) {
        valid_property(fact, &states, P::PillarAxis)?;
        let axis = ["y", "y", "z", "z", "x", "x"][usize::from(input.face)];
        set_value(&mut states, P::PillarAxis.name(), Value::from(axis))?;
    } else if fact.placement == R::Slab && only_keys(&states, &[P::MinecraftVerticalHalf.name()]) {
        valid_property(fact, &states, P::MinecraftVerticalHalf)?;
        set_value(
            &mut states,
            P::MinecraftVerticalHalf.name(),
            Value::from(if upper_half(input)? { "top" } else { "bottom" }),
        )?;
    } else if fact.placement == R::Trapdoor
        && only_keys(
            &states,
            &[
                P::Direction.name(),
                P::OpenBit.name(),
                P::UpsideDownBit.name(),
            ],
        )
    {
        valid_property(fact, &states, P::Direction)?;
        bit(&states, P::OpenBit.name())?;
        bit(&states, P::UpsideDownBit.name())?;
        // Trapdoors use their own four-way encoding rather than cardinal direction.
        let direction = [3, 0, 2, 1][yaw_quadrant(input.yaw)?];
        set_value(&mut states, P::Direction.name(), Value::from(direction))?;
        set_bit(&mut states, P::UpsideDownBit.name(), upper_half(input)?)?;
    } else if fact.placement == R::Hopper
        && only_keys(&states, &[P::FacingDirection.name(), P::ToggleBit.name()])
    {
        integer_in(&states, P::FacingDirection.name(), &[0, 2, 3, 4, 5])?;
        bit(&states, P::ToggleBit.name())?;
        let facing = if input.face < 2 { 0 } else { input.face ^ 1 };
        set_value(&mut states, P::FacingDirection.name(), Value::from(facing))?;
        set_bit(&mut states, P::ToggleBit.name(), false)?;
    } else if fact.placement == R::Connections
        && states.len() == CONNECTION_STATES.len()
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
    held_fact: BlockFact,
    held_canonical_state: &str,
    existing_fact: BlockFact,
    existing_canonical_state: &str,
    clicked_face: Option<u8>,
) -> Option<(u32, Map<String, Value>)> {
    if held_fact.identity.is_none()
        || held_fact.identity != existing_fact.identity
        || held_fact.placement != R::Slab
    {
        return None;
    }
    let held: Map<String, Value> = serde_json::from_str(held_canonical_state).ok()?;
    let existing: Map<String, Value> = serde_json::from_str(existing_canonical_state).ok()?;
    if !only_keys(&held, &[P::MinecraftVerticalHalf.name()])
        || !only_keys(&existing, &[P::MinecraftVerticalHalf.name()])
    {
        return None;
    }
    valid_property(held_fact, &held, P::MinecraftVerticalHalf)?;
    let upper = match value(&existing, P::MinecraftVerticalHalf.name())?.as_str()? {
        "top" => true,
        "bottom" => false,
        _ => return None,
    };
    if let Some(face) = clicked_face
        && face != u8::from(!upper)
    {
        return None;
    }
    let identity = held_fact.alternate?;
    let mut states = held;
    // A double slab is the double block's default state, irrespective of the first half.
    set_value(
        &mut states,
        P::MinecraftVerticalHalf.name(),
        Value::from("bottom"),
    )?;
    Some((identity, states))
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

/// Checks a state against the values the block's own pinned states admit.
pub(crate) fn valid_property(fact: BlockFact, states: &Map<String, Value>, key: P) -> Option<()> {
    let value = value(states, key.name())?;
    let valid = match fact.domain(key) {
        PropertyDomain::Bool => value.is_boolean() || matches!(value.as_u64(), Some(0 | 1)),
        PropertyDomain::Int(domain) => value
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .is_some_and(|v| domain.contains(&v)),
        PropertyDomain::String(domain) => value.as_str().is_some_and(|v| domain.contains(&v)),
    };
    valid.then_some(())
}

/// Returns the largest integer the block's own pinned states admit for `key`.
pub(crate) fn maximum(fact: BlockFact, key: P) -> Option<u64> {
    let PropertyDomain::Int(domain) = fact.domain(key) else {
        return None;
    };
    u64::try_from(*domain.iter().max()?).ok()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::block_fact;

    /// Supplies numeric facts and the collected counterpart for each tested identity.
    fn resolve_placement_state(
        name: &str,
        state: &str,
        input: PlacementInput,
    ) -> Option<Map<String, Value>> {
        super::resolve_placement_state(block_fact(name), state, input)
    }

    /// Joins a real pinned slab counterpart before applying the merge boundary.
    fn merge_slab_state(
        held: &str,
        held_state: &str,
        existing: &str,
        existing_state: &str,
        face: Option<u8>,
    ) -> Option<(String, Map<String, Value>)> {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let fact = block_fact(held);
        let (identity, states) =
            super::merge_slab_state(fact, held_state, block_fact(existing), existing_state, face)?;
        Some((records.get(identity as usize)?.name.to_string(), states))
    }
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
    fn connection_states_start_clear_for_neighbor_derived_geometry() {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let mut names = std::collections::HashSet::new();
        let mut connected_held_state = false;
        for record in records.iter().filter(|record| {
            matches!(
                record.model_family,
                assets::ModelFamily::Fence | assets::ModelFamily::Pane
            )
        }) {
            names.insert(record.name.as_ref());
            let canonical: Map<String, Value> =
                serde_json::from_str(&record.canonical_state).unwrap();
            connected_held_state |= CONNECTION_STATES
                .iter()
                .any(|(key, _)| value(&canonical, key) == Some(&json!(1)));
            let states =
                resolve_placement_state(&record.name, &record.canonical_state, input(1, 0.5, 0.0))
                    .expect(&record.name);
            for &(key, _) in CONNECTION_STATES {
                assert_eq!(value(&states, key), Some(&json!(0)));
            }
        }
        assert!(connected_held_state);
        assert!(names.contains("minecraft:oak_fence"));
        assert!(names.contains("minecraft:glass_pane"));
        assert!(names.contains("minecraft:iron_bars"));
        assert!(names.contains("minecraft:copper_bars"));
        assert!(names.contains("minecraft:waxed_oxidized_copper_bars"));
    }

    #[test]
    fn trapdoors_have_their_own_yaw_encoding_and_half() {
        let canonical = r#"{"direction":{"type":"int","value":0},"open_bit":{"type":"byte","value":0},"upside_down_bit":{"type":"byte","value":0}}"#;
        for (yaw, direction) in [(0.0, 3), (90.0, 0), (180.0, 2), (-90.0, 1), (360.0, 3)] {
            let states =
                resolve_placement_state("minecraft:trapdoor", canonical, input(0, 0.1, yaw))
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
    fn shared_property_names_keep_each_blocks_own_domain() {
        let state = json!({"direction": 7});
        let chalkboard = block_fact("minecraft:chalkboard");
        let trapdoor = block_fact("minecraft:trapdoor");
        assert!(valid_property(chalkboard, state.as_object().unwrap(), P::Direction).is_some());
        assert!(valid_property(trapdoor, state.as_object().unwrap(), P::Direction).is_none());
        assert_eq!(maximum(chalkboard, P::Direction), Some(15));
        assert_eq!(maximum(trapdoor, P::Direction), Some(3));
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
                "minecraft:trapdoor",
                json!({"direction":4,"open_bit":0,"upside_down_bit":0}),
            ),
            (
                "minecraft:trapdoor",
                json!({"direction":0,"open_bit":2,"upside_down_bit":0}),
            ),
            (
                "minecraft:trapdoor",
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
    #[test]
    fn stair_yaw_half_and_held_half_follow_the_placement_table() {
        for (yaw, direction) in [(0.0, 2), (90.0, 1), (180.0, 3), (-90.0, 0)] {
            for (face, y, upper) in [
                (0, 0.0, 1),
                (1, 1.0, 0),
                (2, 0.5, 0),
                (2, 0.5001, 1),
                (3, 0.2, 0),
                (4, 0.8, 1),
                (5, 0.1, 0),
            ] {
                for held_upper in 0..=1 {
                    let original = serde_json::json!({"weirdo_direction":{"type":"int","value":0},"upside_down_bit":{"type":"byte","value":held_upper},"minecraft:corner":{"type":"string","value":"outer_right"}}).to_string();
                    let input = PlacementInput {
                        face,
                        click_position: [0.5, y, 0.5],
                        yaw,
                        pitch: 0.0,
                    };
                    let state =
                        resolve_placement_state("minecraft:oak_stairs", &original, input).unwrap();
                    assert_eq!(
                        value(&state, "weirdo_direction"),
                        Some(&Value::from(direction))
                    );
                    assert_eq!(
                        value(&state, "upside_down_bit"),
                        Some(&Value::from(upper | held_upper))
                    );
                    assert_eq!(
                        value(&state, "minecraft:corner"),
                        Some(&Value::from("none"))
                    );
                }
            }
        }
    }

    #[test]
    fn legacy_direction_blocks_use_their_registered_rotation_tables() {
        for (name, key, table) in [
            ("minecraft:loom", "direction", [2, 3, 0, 1]),
            (
                "minecraft:white_glazed_terracotta",
                "facing_direction",
                [2, 5, 3, 4],
            ),
        ] {
            for (index, yaw) in [0.0, 90.0, 180.0, -90.0].into_iter().enumerate() {
                let original = serde_json::json!({key:{"type":"int","value":0}}).to_string();
                let state = resolve_placement_state(
                    name,
                    &original,
                    PlacementInput {
                        face: 1,
                        click_position: [0.5; 3],
                        yaw,
                        pitch: 0.0,
                    },
                )
                .unwrap();
                assert_eq!(value(&state, key), Some(&Value::from(table[index])));
            }
        }
    }

    #[test]
    fn initial_stacks_reset_count_and_light_state() {
        for (name, count, flag, expected_flag) in [
            ("minecraft:snow_layer", "height", "covered_bit", 0),
            ("minecraft:candle", "candles", "lit", 0),
            ("minecraft:sea_pickle", "cluster_count", "dead_bit", 1),
        ] {
            let original =
                serde_json::json!({count:{"type":"int","value":3},flag:{"type":"byte","value":1}})
                    .to_string();
            let state = resolve_placement_state(
                name,
                &original,
                PlacementInput {
                    face: 1,
                    click_position: [0.5; 3],
                    yaw: 0.0,
                    pitch: 0.0,
                },
            )
            .unwrap();
            assert_eq!(value(&state, count), Some(&Value::from(0)));
            assert_eq!(value(&state, flag), Some(&Value::from(expected_flag)));
        }
    }
}
