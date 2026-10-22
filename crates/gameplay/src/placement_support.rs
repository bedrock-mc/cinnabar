//! Attachment state and support requirements for local block placement.

use crate::placement_state::PlacementInput;
use assets::{
    facts::{BlockFact, BlockProperty as P, PlacementRule as R, SupportKind},
    registry::{BlockTraits as T, ModelFamily as F, SurvivalRule as S},
};
use serde_json::{Map, Value};

/// A support face is the offset from the destination toward its supporting block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupportRequirement {
    Independent,
    Face(u8),
    Unsupported,
}

/// Chooses attachment states only when every support consulted by vanilla is known.
pub(crate) fn attachment_states(
    fact: BlockFact,
    canonical: &Map<String, Value>,
    input: PlacementInput,
    supports: [Option<bool>; 6],
) -> Option<Map<String, Value>> {
    if input.face > 5 || fact.placement == R::Unresolved {
        return None;
    }
    let mut states = canonical.clone();
    if fact.placement == R::Vine {
        return crate::placement_vines::attachment_states(canonical, input.face, supports);
    }
    if fact.traits.contains(T::MULTIFACE) {
        return crate::placement_multiface::attachment_states(canonical, input.face, supports);
    }
    if fact.traits.contains(T::TORCH) {
        if !known_keys(canonical, &[P::TorchFacingDirection.name()]) {
            return None;
        }
        crate::placement_state::valid_property(fact, canonical, P::TorchFacingDirection)?;
        let clicked_support = input.face ^ 1;
        let support = if input.face != 0 && supports[usize::from(clicked_support)]? {
            clicked_support
        } else {
            let mut selected = None;
            for face in [3, 4, 2, 5, 0] {
                if supports[face]? {
                    selected = Some(face as u8);
                    break;
                }
            }
            selected?
        };
        let name = match support {
            0 => "top",
            2 => "north",
            3 => "south",
            4 => "west",
            5 => "east",
            _ => return None,
        };
        set_state(
            &mut states,
            P::TorchFacingDirection.name(),
            Value::from(name),
        )?;
    } else if fact.family == F::Lever {
        if !known_keys(canonical, &[P::LeverDirection.name(), P::OpenBit.name()])
            || state_bit(canonical, P::OpenBit.name()).is_none()
            || crate::placement_state::valid_property(fact, canonical, P::LeverDirection).is_none()
            || !supports[usize::from(input.face ^ 1)]?
        {
            return None;
        }
        let direction = match input.face {
            0 => {
                if crate::placement_state::yaw_quadrant(input.yaw)? & 1 == 0 {
                    "down_north_south"
                } else {
                    "down_east_west"
                }
            }
            1 => {
                if crate::placement_state::yaw_quadrant(input.yaw)? & 1 == 0 {
                    "up_north_south"
                } else {
                    "up_east_west"
                }
            }
            2 => "north",
            3 => "south",
            4 => "west",
            5 => "east",
            _ => return None,
        };
        crate::placement_state::set_value(
            &mut states,
            P::LeverDirection.name(),
            Value::from(direction),
        )?;
    } else if fact.family == F::Button {
        if !known_keys(
            canonical,
            &[P::FacingDirection.name(), P::ButtonPressedBit.name()],
        ) || state_value(canonical, P::FacingDirection.name())?.as_u64()? > 5
            || state_bit(canonical, P::ButtonPressedBit.name()).is_none()
        {
            return None;
        }
        if !supports[usize::from(input.face ^ 1)]? {
            return None;
        }
        set_state(
            &mut states,
            P::FacingDirection.name(),
            Value::from(input.face),
        )?;
        set_state(&mut states, P::ButtonPressedBit.name(), Value::from(0))?;
    } else if known_keys(canonical, &[P::Hanging.name()]) {
        state_bit(canonical, P::Hanging.name())?;
        let below = supports[0]?;
        let hanging = !below || (input.face == 0 && supports[1]?);
        if hanging && !supports[1]? {
            return None;
        }
        set_state(
            &mut states,
            P::Hanging.name(),
            Value::from(u8::from(hanging)),
        )?;
    } else {
        return None;
    }
    Some(states)
}

/// Refuses additional state keys whose placement behavior has not been established.
fn known_keys(states: &Map<String, Value>, allowed: &[&str]) -> bool {
    states.len() == allowed.len() && states.keys().all(|key| allowed.contains(&key.as_str()))
}

/// Reads compiled survival admission and resolves state-dependent support faces.
pub(crate) fn support_requirement(
    fact: BlockFact,
    states: &Map<String, Value>,
) -> SupportRequirement {
    let face = match fact.placement {
        R::Vine => crate::placement_vines::support_face(states),
        R::Multiface => crate::placement_multiface::support_face(states),
        R::StandingSign | R::WallSign => crate::placement_signs::support_face(fact, states),
        R::Torch => {
            match state_value(states, P::TorchFacingDirection.name()).and_then(Value::as_str) {
                Some("top") => Some(0),
                Some("north") => Some(2),
                Some("south") => Some(3),
                Some("west") => Some(4),
                Some("east") => Some(5),
                _ => None,
            }
        }
        R::Button => state_value(states, P::FacingDirection.name())
            .and_then(Value::as_u64)
            .filter(|face| *face <= 5)
            .map(|face| face as u8 ^ 1),
        R::Lever => match state_value(states, P::LeverDirection.name()).and_then(Value::as_str) {
            Some("down_east_west" | "down_north_south") => Some(1),
            Some("up_east_west" | "up_north_south") => Some(0),
            Some("north") => Some(3),
            Some("south") => Some(2),
            Some("west") => Some(5),
            Some("east") => Some(4),
            _ => None,
        },
        R::Lantern => state_bit(states, P::Hanging.name()).map(u8::from),
        _ => {
            return match fact.survival {
                S::Carpet | S::Door | S::Candle | S::SeaPickle | S::SnowLayer => {
                    SupportRequirement::Face(0)
                }
                S::None => SupportRequirement::Independent,
                _ => SupportRequirement::Unsupported,
            };
        }
    };
    face.map_or(SupportRequirement::Unsupported, SupportRequirement::Face)
}

/// Accepts full attachment support and carpet's non-air support using compiled facts.
pub(crate) fn accepts_support(fact: BlockFact, support: BlockFact, full_cube: bool) -> bool {
    if !support.readable {
        return false;
    }
    if fact.survival == S::Carpet {
        return support.support != SupportKind::Air;
    }
    full_cube
        && !matches!(
            support.support,
            SupportKind::Unresolved | SupportKind::Leaves | SupportKind::PowderSnow
        )
}

/// Preserves unresolved partial-shape support while removing identifier membership tests.
pub(crate) fn support_acceptance(
    fact: BlockFact,
    support: BlockFact,
    full_cube: bool,
) -> Option<bool> {
    if !support.readable {
        return None;
    }
    if fact.placement == R::Vine {
        return match support.support {
            SupportKind::Stone => Some(true),
            SupportKind::Air => Some(false),
            _ => None,
        };
    }
    if fact.placement == R::Snow {
        return if support.support == SupportKind::Stone
            || support.support == SupportKind::Leaves
            || (support.support == SupportKind::SnowLayer && full_cube)
        {
            Some(true)
        } else if matches!(
            support.support,
            SupportKind::Air | SupportKind::Water | SupportKind::Lava
        ) {
            Some(false)
        } else {
            None
        };
    }
    if accepts_support(fact, support, full_cube) {
        return Some(true);
    }
    if fact.survival == S::Carpet
        || matches!(
            support.support,
            SupportKind::Leaves
                | SupportKind::PowderSnow
                | SupportKind::Air
                | SupportKind::Water
                | SupportKind::Lava
                | SupportKind::Fire
        )
    {
        Some(false)
    } else {
        None
    }
}

/// Changes the value while retaining the palette's serialized state type.
fn set_state(states: &mut Map<String, Value>, key: &str, value: Value) -> Option<()> {
    let entry = states.get_mut(key)?.as_object_mut()?;
    *entry.get_mut("value")? = value;
    Some(())
}

/// Reads the typed palette entry without accepting a malformed state envelope.
fn state_value<'a>(states: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    states.get(key)?.as_object()?.get("value")
}

/// Accepts boolean state entries and byte-encoded booleans from the vanilla palette.
fn state_bit(states: &Map<String, Value>, key: &str) -> Option<bool> {
    let value = state_value(states, key)?;
    value.as_bool().or_else(|| match value.as_u64()? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::block_fact;

    /// Supplies compiled facts to the existing attachment behavior cases.
    fn attachment_states(
        name: &str,
        states: &Map<String, Value>,
        input: PlacementInput,
        support: [Option<bool>; 6],
    ) -> Option<Map<String, Value>> {
        super::attachment_states(block_fact(name), states, input, support)
    }

    /// Reads survival facts for the pinned identity tested by each case.
    fn support_requirement(name: &str, states: &Map<String, Value>) -> SupportRequirement {
        super::support_requirement(block_fact(name), states)
    }

    /// Resolves both ends of a support test from the pinned palette.
    fn support_acceptance(name: &str, support: &str, full: bool) -> Option<bool> {
        super::support_acceptance(block_fact(name), block_fact(support), full)
    }

    /// Uses numeric support classifications for the acceptance cases.
    fn accepts_support(name: &str, support: &str, full: bool) -> bool {
        super::accepts_support(block_fact(name), block_fact(support), full)
    }
    use serde_json::json;
    use std::{collections::HashSet, sync::OnceLock};

    /// Loads the committed palette once, including its real typed state envelopes.
    fn registry() -> &'static [assets::RegistryRecord] {
        static REGISTRY: OnceLock<Box<[assets::RegistryRecord]>> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            assets::read_registry_for_protocol(
                assets::pinned_block_registry_bytes(),
                assets::active_content_registry_protocol(),
            )
            .unwrap()
        })
    }

    /// Decodes a state from the pinned palette without replacing its keys or values.
    fn registered_states(record: &assets::RegistryRecord) -> Map<String, Value> {
        serde_json::from_str(&record.canonical_state).unwrap()
    }

    /// Provides one placement input with only the tested face differing.
    fn input(face: u8) -> PlacementInput {
        PlacementInput {
            face,
            click_position: [0.5; 3],
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    /// Supplies an actual held torch state from the pinned palette.
    fn torch_states() -> Map<String, Value> {
        registered_states(
            registry()
                .iter()
                .find(|record| record.name.as_ref() == "minecraft:torch")
                .unwrap(),
        )
    }

    #[test]
    fn lever_faces_and_yaw_parity_preserve_the_open_bit() {
        let states = serde_json::from_value(serde_json::json!({
            "lever_direction":{"type":"string","value":"east"},
            "open_bit":{"type":"byte","value":1}
        }))
        .unwrap();
        for (yaw, expected) in [
            (
                0.0,
                [
                    "down_north_south",
                    "up_north_south",
                    "north",
                    "south",
                    "west",
                    "east",
                ],
            ),
            (
                90.0,
                [
                    "down_east_west",
                    "up_east_west",
                    "north",
                    "south",
                    "west",
                    "east",
                ],
            ),
            (
                180.0,
                [
                    "down_north_south",
                    "up_north_south",
                    "north",
                    "south",
                    "west",
                    "east",
                ],
            ),
            (
                -90.0,
                [
                    "down_east_west",
                    "up_east_west",
                    "north",
                    "south",
                    "west",
                    "east",
                ],
            ),
        ] {
            for (face, direction) in expected.into_iter().enumerate() {
                let mut click = input(face as u8);
                click.yaw = yaw;
                let placed =
                    attachment_states("minecraft:lever", &states, click, [Some(true); 6]).unwrap();
                assert_eq!(
                    state_value(&placed, "lever_direction"),
                    Some(&Value::from(direction))
                );
                assert_eq!(state_bit(&placed, "open_bit"), Some(true));
                assert_eq!(
                    support_requirement("minecraft:lever", &placed),
                    SupportRequirement::Face(face as u8 ^ 1)
                );
                let mut unsupported = [Some(true); 6];
                unsupported[face ^ 1] = Some(false);
                assert!(
                    attachment_states("minecraft:lever", &states, click, unsupported).is_none()
                );
            }
        }
    }

    #[test]
    fn torch_fallback_uses_first_supported_wall_before_floor() {
        let supports = [
            Some(true),
            Some(false),
            Some(true),
            Some(true),
            Some(true),
            Some(true),
        ];
        let states =
            attachment_states("minecraft:torch", &torch_states(), input(0), supports).unwrap();
        assert_eq!(
            state_value(&states, "torch_facing_direction"),
            Some(&Value::from("south"))
        );
        let unknown = [
            Some(true),
            Some(false),
            Some(true),
            None,
            Some(true),
            Some(true),
        ];
        assert!(attachment_states("minecraft:torch", &torch_states(), input(0), unknown).is_none());
    }

    #[test]
    fn invalid_torch_support_and_invalid_faces_do_not_predict() {
        for face in 0..=6 {
            assert!(
                attachment_states(
                    "minecraft:torch",
                    &torch_states(),
                    input(face),
                    [Some(false); 6]
                )
                .is_none()
            );
        }
    }

    #[test]
    fn registered_torch_faces_point_toward_support() {
        let mut names = HashSet::new();
        for record in registry() {
            let canonical = registered_states(record);
            if !canonical.contains_key("torch_facing_direction") {
                continue;
            }
            names.insert(record.name.as_ref());
            for (face, expected, support) in [
                (1, "top", 0),
                (2, "south", 3),
                (3, "north", 2),
                (4, "east", 5),
                (5, "west", 4),
            ] {
                let mut supports = [Some(false); 6];
                supports[support] = Some(true);
                let placed = attachment_states(&record.name, &canonical, input(face), supports)
                    .unwrap_or_else(|| panic!("{} {}", record.name, record.canonical_state));
                assert_eq!(
                    state_value(&placed, "torch_facing_direction"),
                    Some(&json!(expected))
                );
                assert_eq!(
                    support_requirement(&record.name, &placed),
                    SupportRequirement::Face(support as u8)
                );
            }
            assert!(
                attachment_states(&record.name, &canonical, input(1), [Some(false); 6]).is_none()
            );
        }
        assert!(names.contains("minecraft:copper_torch"));
    }

    #[test]
    fn buttons_require_the_opposite_support_face() {
        let states = serde_json::from_value(json!({"facing_direction":{"type":"int","value":0},"button_pressed_bit":{"type":"byte","value":1}})).unwrap();
        for face in 0..=5 {
            let mut supports = [Some(false); 6];
            supports[usize::from(face ^ 1)] = Some(true);
            let placed =
                attachment_states("minecraft:stone_button", &states, input(face), supports)
                    .unwrap();
            assert_eq!(
                state_value(&placed, "facing_direction"),
                Some(&Value::from(face))
            );
            assert_eq!(
                state_value(&placed, "button_pressed_bit"),
                Some(&Value::from(0))
            );
            assert_eq!(
                support_requirement("minecraft:stone_button", &placed),
                SupportRequirement::Face(face ^ 1)
            );
        }
    }

    #[test]
    fn lantern_prefers_floor_except_when_clicking_a_supported_ceiling() {
        let mut names = HashSet::new();
        for record in registry() {
            let states = registered_states(record);
            if states.len() != 1 || !states.contains_key("hanging") {
                continue;
            }
            names.insert(record.name.as_ref());
            for (below, above, face, hanging) in [
                (true, true, 1, false),
                (true, true, 0, true),
                (true, false, 0, false),
                (false, true, 1, true),
            ] {
                let mut supports = [Some(false); 6];
                supports[0] = Some(below);
                supports[1] = Some(above);
                let placed = attachment_states(&record.name, &states, input(face), supports)
                    .expect(&record.name);
                assert_eq!(state_bit(&placed, "hanging"), Some(hanging));
                assert_eq!(
                    support_requirement(&record.name, &placed),
                    SupportRequirement::Face(u8::from(hanging))
                );
            }
            assert!(attachment_states(&record.name, &states, input(1), [Some(false); 6]).is_none());
        }
        assert!(names.contains("minecraft:lantern"));
        assert!(names.contains("minecraft:soul_lantern"));
        assert!(names.contains("minecraft:copper_lantern"));
        assert!(!names.contains("minecraft:sea_lantern"));
    }

    #[test]
    fn registered_multiface_blocks_resolve_each_attachment_and_require_support() {
        let mut names = HashSet::new();
        for record in registry().iter().filter(|record| {
            matches!(
                record.model_family,
                assets::ModelFamily::GlowLichen
                    | assets::ModelFamily::SculkVein
                    | assets::ModelFamily::ResinClump
            )
        }) {
            let canonical = registered_states(record);
            names.insert(record.name.as_ref());
            for (face, expected) in [2, 1, 4, 16, 32, 8].into_iter().enumerate() {
                let placed =
                    attachment_states(&record.name, &canonical, input(face as u8), [Some(true); 6])
                        .expect(&record.name);
                assert_eq!(
                    state_value(&placed, "multi_face_direction_bits"),
                    Some(&json!(expected))
                );
                assert_eq!(
                    support_requirement(&record.name, &placed),
                    SupportRequirement::Face(face as u8 ^ 1)
                );
            }
            assert!(
                attachment_states(&record.name, &canonical, input(1), [Some(false); 6]).is_none()
            );
        }
        assert!(names.contains("minecraft:resin_clump"));
    }

    #[test]
    fn current_heads_chains_and_golden_dandelion_wait_for_survival_rules() {
        for name in [
            "minecraft:skeleton_skull",
            "minecraft:wither_skeleton_skull",
            "minecraft:zombie_head",
            "minecraft:player_head",
            "minecraft:creeper_head",
            "minecraft:dragon_head",
            "minecraft:piglin_head",
            "minecraft:iron_chain",
            "minecraft:copper_chain",
            "minecraft:golden_dandelion",
        ] {
            let records: Vec<_> = registry()
                .iter()
                .filter(|record| record.name.as_ref() == name)
                .collect();
            assert!(!records.is_empty(), "missing registered block: {name}");
            for record in records {
                assert_eq!(
                    support_requirement(name, &registered_states(record)),
                    SupportRequirement::Unsupported,
                    "{name} {}",
                    record.canonical_state
                );
            }
        }
    }

    #[test]
    fn carpet_support_is_non_air_and_leaves_never_support_attachments() {
        assert!(accepts_support(
            "minecraft:white_carpet",
            "minecraft:water",
            false
        ));
        assert!(!accepts_support(
            "minecraft:white_carpet",
            "minecraft:air",
            false
        ));
        assert!(!accepts_support(
            "minecraft:torch",
            "minecraft:oak_leaves",
            true
        ));
        assert_eq!(
            support_acceptance("minecraft:torch", "minecraft:oak_stairs", false),
            None
        );
        assert_eq!(
            support_acceptance("minecraft:torch", "minecraft:air", false),
            Some(false)
        );
        assert_eq!(
            support_acceptance("minecraft:torch", "minecraft:stone", true),
            Some(true)
        );
    }

    #[test]
    fn carpet_support_preserves_every_admitted_non_air_state() {
        let facts = assets::facts::BlockFacts::new(registry()).unwrap();
        let carpet = block_fact("minecraft:white_carpet");
        for record in registry()
            .iter()
            .filter(|r| r.name.as_ref() != assets::facts::block::RESERVED_BLOCK_NAME)
        {
            let support = facts.get(record.sequential_id);
            let expected = record.name.as_ref() != "minecraft:air";
            for full_cube in [false, true] {
                assert_eq!(
                    super::support_acceptance(carpet, support, full_cube),
                    Some(expected),
                    "{} {} full_cube={full_cube}",
                    record.name,
                    record.canonical_state
                );
            }
        }
        for name in [
            "minecraft:ice",
            "minecraft:hopper",
            "minecraft:redstone_lamp",
        ] {
            let support = block_fact(name);
            assert_eq!(support.family, F::Unknown);
            assert!(support.identity.is_some());
            assert_eq!(
                super::support_acceptance(carpet, support, false),
                Some(true)
            );
        }
        assert_eq!(
            super::support_acceptance(carpet, facts.get(u32::MAX), true),
            None
        );
        for record in registry()
            .iter()
            .filter(|r| r.name.as_ref() == assets::facts::block::RESERVED_BLOCK_NAME)
        {
            assert_eq!(
                super::support_acceptance(carpet, facts.get(record.sequential_id), true),
                Some(true)
            );
        }
    }

    #[test]
    fn unknown_survival_families_stay_server_confirmed() {
        for identifier in [
            "minecraft:cactus",
            "minecraft:rail",
            "minecraft:oak_sapling",
            "minecraft:bed",
        ] {
            assert_eq!(
                support_requirement(identifier, &Map::new()),
                SupportRequirement::Unsupported
            );
        }
        for identifier in [
            "minecraft:oak_fence",
            "minecraft:glass_pane",
            "minecraft:oak_stairs",
        ] {
            assert_eq!(
                support_requirement(identifier, &Map::new()),
                SupportRequirement::Independent
            );
        }
    }
}
