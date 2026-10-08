//! Attachment state and support requirements for local block placement.

use crate::placement_state::PlacementInput;
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
    identifier: &str,
    canonical: &Map<String, Value>,
    input: PlacementInput,
    supports: [Option<bool>; 6],
) -> Option<Map<String, Value>> {
    if input.face > 5 || !identifier.starts_with("minecraft:") {
        return None;
    }
    let mut states = canonical.clone();
    if identifier == "minecraft:vine" {
        return crate::placement_vines::attachment_states(canonical, input.face, supports);
    }
    if crate::placement_multiface::is_multiface(identifier) {
        return crate::placement_multiface::attachment_states(canonical, input.face, supports);
    }
    if is_torch(identifier) {
        if !known_keys(canonical, &["torch_facing_direction"]) {
            return None;
        }
        if !["unknown", "top", "north", "south", "west", "east"]
            .contains(&state_value(canonical, "torch_facing_direction")?.as_str()?)
        {
            return None;
        }
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
        set_state(&mut states, "torch_facing_direction", Value::from(name))?;
    } else if identifier == "minecraft:lever" {
        if !known_keys(canonical, &["lever_direction", "open_bit"])
            || state_bit(canonical, "open_bit").is_none()
            || ![
                "down_east_west",
                "east",
                "west",
                "south",
                "north",
                "up_north_south",
                "up_east_west",
                "down_north_south",
            ]
            .contains(&state_value(canonical, "lever_direction")?.as_str()?)
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
        crate::placement_state::set_value(&mut states, "lever_direction", Value::from(direction))?;
    } else if identifier.ends_with("_button") {
        if !known_keys(canonical, &["facing_direction", "button_pressed_bit"])
            || state_value(canonical, "facing_direction")?.as_u64()? > 5
            || state_bit(canonical, "button_pressed_bit").is_none()
        {
            return None;
        }
        if !supports[usize::from(input.face ^ 1)]? {
            return None;
        }
        set_state(&mut states, "facing_direction", Value::from(input.face))?;
        set_state(&mut states, "button_pressed_bit", Value::from(0))?;
    } else if is_lantern(identifier) {
        if !known_keys(canonical, &["hanging_bit"]) || state_bit(canonical, "hanging_bit").is_none()
        {
            return None;
        }
        let below = supports[0]?;
        let hanging = !below || (input.face == 0 && supports[1]?);
        if hanging && !supports[1]? {
            return None;
        }
        set_state(&mut states, "hanging_bit", Value::from(u8::from(hanging)))?;
    } else {
        return None;
    }
    Some(states)
}

/// Refuses additional state keys whose placement behavior has not been established.
fn known_keys(states: &Map<String, Value>, allowed: &[&str]) -> bool {
    states.len() == allowed.len() && states.keys().all(|key| allowed.contains(&key.as_str()))
}

/// Identifies the support needed by a resolved state; unimplemented survival rules fail closed.
pub(crate) fn support_requirement(
    identifier: &str,
    states: &Map<String, Value>,
) -> SupportRequirement {
    if !identifier.starts_with("minecraft:") {
        return SupportRequirement::Unsupported;
    }
    if identifier == "minecraft:vine" {
        return crate::placement_vines::support_face(states)
            .map_or(SupportRequirement::Unsupported, SupportRequirement::Face);
    }
    if crate::placement_multiface::is_multiface(identifier) {
        return crate::placement_multiface::support_face(states)
            .map_or(SupportRequirement::Unsupported, SupportRequirement::Face);
    }
    if let Some(face) = crate::placement_signs::support_face(identifier, states) {
        return SupportRequirement::Face(face);
    }
    if is_torch(identifier) {
        return match state_value(states, "torch_facing_direction").and_then(Value::as_str) {
            Some("top") => SupportRequirement::Face(0),
            Some("north") => SupportRequirement::Face(2),
            Some("south") => SupportRequirement::Face(3),
            Some("west") => SupportRequirement::Face(4),
            Some("east") => SupportRequirement::Face(5),
            _ => SupportRequirement::Unsupported,
        };
    }
    if identifier.ends_with("_button") {
        return match state_value(states, "facing_direction").and_then(Value::as_u64) {
            Some(face @ 0..=5) => SupportRequirement::Face((face as u8) ^ 1),
            _ => SupportRequirement::Unsupported,
        };
    }
    if identifier == "minecraft:lever" {
        return match state_value(states, "lever_direction").and_then(Value::as_str) {
            Some("down_east_west" | "down_north_south") => SupportRequirement::Face(1),
            Some("up_east_west" | "up_north_south") => SupportRequirement::Face(0),
            Some("north") => SupportRequirement::Face(3),
            Some("south") => SupportRequirement::Face(2),
            Some("west") => SupportRequirement::Face(5),
            Some("east") => SupportRequirement::Face(4),
            _ => SupportRequirement::Unsupported,
        };
    }
    if is_lantern(identifier) {
        return match state_bit(states, "hanging_bit") {
            Some(false) => SupportRequirement::Face(0),
            Some(true) => SupportRequirement::Face(1),
            None => SupportRequirement::Unsupported,
        };
    }
    if is_carpet(identifier) {
        return SupportRequirement::Face(0);
    }
    if crate::placement_doors::is_door(identifier) {
        return SupportRequirement::Face(0);
    }
    if is_candle(identifier)
        || matches!(identifier, "minecraft:sea_pickle" | "minecraft:snow_layer")
    {
        return SupportRequirement::Face(0);
    }
    if survival_is_unresolved(identifier) {
        SupportRequirement::Unsupported
    } else {
        SupportRequirement::Independent
    }
}

/// Accepts proven full support and carpet's non-air support, without guessing partial shapes.
pub(crate) fn accepts_support(identifier: &str, support_identifier: &str, full_cube: bool) -> bool {
    if is_carpet(identifier) {
        return support_identifier != "minecraft:air";
    }
    full_cube
        && support_identifier.starts_with("minecraft:")
        && !is_leaves(support_identifier)
        && support_identifier != "minecraft:powder_snow"
}

/// Distinguishes a rejected support from a shape whose attachment behavior is not implemented.
pub(crate) fn support_acceptance(
    identifier: &str,
    support_identifier: &str,
    full_cube: bool,
) -> Option<bool> {
    if identifier == "minecraft:vine" {
        return match support_identifier {
            "minecraft:stone" => Some(true),
            "minecraft:air" => Some(false),
            _ => None,
        };
    }
    if identifier == "minecraft:snow_layer" {
        return match support_identifier {
            "minecraft:stone" => Some(true),
            name if is_leaves(name) => Some(true),
            "minecraft:snow_layer" if full_cube => Some(true),
            "minecraft:air"
            | "minecraft:water"
            | "minecraft:flowing_water"
            | "minecraft:lava"
            | "minecraft:flowing_lava" => Some(false),
            _ => None,
        };
    }
    if accepts_support(identifier, support_identifier, full_cube) {
        return Some(true);
    }
    if is_carpet(identifier)
        || is_leaves(support_identifier)
        || matches!(
            support_identifier,
            "minecraft:air"
                | "minecraft:water"
                | "minecraft:flowing_water"
                | "minecraft:lava"
                | "minecraft:flowing_lava"
                | "minecraft:powder_snow"
                | "minecraft:fire"
                | "minecraft:soul_fire"
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

/// These variants share the wall and upright torch placement rule.
fn is_torch(identifier: &str) -> bool {
    matches!(
        identifier,
        "minecraft:torch"
            | "minecraft:soul_torch"
            | "minecraft:redstone_torch"
            | "minecraft:unlit_redstone_torch"
    )
}

/// Copper and soul variants keep lantern support and hanging-state behavior.
fn is_lantern(identifier: &str) -> bool {
    identifier == "minecraft:lantern"
        || identifier == "minecraft:soul_lantern"
        || identifier.ends_with("copper_lantern")
}

/// Ordinary candles share count and support rules; candle cakes use another placement path.
pub(crate) fn is_candle(identifier: &str) -> bool {
    identifier == "minecraft:candle"
        || (identifier.starts_with("minecraft:") && identifier.ends_with("_candle"))
}

/// Moss carpets have additional growth rules and are not ordinary colored carpet.
fn is_carpet(identifier: &str) -> bool {
    (identifier == "minecraft:carpet" || identifier.ends_with("_carpet"))
        && !matches!(
            identifier,
            "minecraft:moss_carpet" | "minecraft:pale_moss_carpet"
        )
}

/// Leaves explicitly reject attachment support despite their full collision box.
fn is_leaves(identifier: &str) -> bool {
    matches!(identifier, "minecraft:leaves" | "minecraft:leaves2")
        || identifier.ends_with("_leaves")
}

/// Substrate, fluid, growth and multi-cell rules require additional verified world facts.
fn survival_is_unresolved(identifier: &str) -> bool {
    const SUFFIXES: &[&str] = &[
        "_shrub",
        "_grass",
        "_shulker_box",
        "_flower",
        "_tulip",
        "_sapling",
        "_mushroom",
        "_roots",
        "_fungus",
        "_rail",
        "_pressure_plate",
        "_sign",
        "_wall_sign",
        "_hanging_sign",
        "_coral",
        "_coral_fan",
        "_coral_wall_fan",
        "_candle",
        "_candle_cake",
        "_door",
        "_bed",
        "_bush",
    ];
    const IDENTIFIERS: &[&str] = &[
        "minecraft:bush",
        "minecraft:unlit_redstone_torch",
        "minecraft:lever",
        "minecraft:rail",
        "minecraft:redstone_wire",
        "minecraft:unpowered_repeater",
        "minecraft:powered_repeater",
        "minecraft:unpowered_comparator",
        "minecraft:powered_comparator",
        "minecraft:snow_layer",
        "minecraft:candle",
        "minecraft:sea_pickle",
        "minecraft:vine",
        "minecraft:glow_lichen",
        "minecraft:sculk_vein",
        "minecraft:standing_sign",
        "minecraft:wall_sign",
        "minecraft:bed",
        "minecraft:wooden_door",
        "minecraft:iron_door",
        "minecraft:cactus",
        "minecraft:chorus_plant",
        "minecraft:chorus_flower",
        "minecraft:scaffolding",
        "minecraft:short_grass",
        "minecraft:tall_grass",
        "minecraft:fern",
        "minecraft:large_fern",
        "minecraft:deadbush",
        "minecraft:seagrass",
        "minecraft:kelp",
        "minecraft:reeds",
        "minecraft:bamboo",
        "minecraft:bamboo_sapling",
        "minecraft:azalea",
        "minecraft:flowering_azalea",
        "minecraft:dandelion",
        "minecraft:poppy",
        "minecraft:blue_orchid",
        "minecraft:allium",
        "minecraft:azure_bluet",
        "minecraft:oxeye_daisy",
        "minecraft:cornflower",
        "minecraft:lily_of_the_valley",
        "minecraft:closed_eyeblossom",
        "minecraft:open_eyeblossom",
        "minecraft:wither_rose",
        "minecraft:torchflower",
        "minecraft:sunflower",
        "minecraft:lilac",
        "minecraft:rose_bush",
        "minecraft:peony",
        "minecraft:pitcher_plant",
        "minecraft:pitcher_crop",
        "minecraft:double_plant",
        "minecraft:yellow_flower",
        "minecraft:red_flower",
        "minecraft:sapling",
        "minecraft:brown_mushroom",
        "minecraft:red_mushroom",
        "minecraft:wheat",
        "minecraft:carrots",
        "minecraft:potatoes",
        "minecraft:beetroot",
        "minecraft:melon_stem",
        "minecraft:pumpkin_stem",
        "minecraft:nether_wart",
        "minecraft:cocoa",
        "minecraft:waterlily",
        "minecraft:lily_pad",
        "minecraft:moss_carpet",
        "minecraft:pale_moss_carpet",
        "minecraft:hanging_roots",
        "minecraft:nether_sprouts",
        "minecraft:weeping_vines",
        "minecraft:twisting_vines",
        "minecraft:pointed_dripstone",
        "minecraft:small_dripleaf_block",
        "minecraft:big_dripleaf",
        "minecraft:big_dripleaf_stem",
        "minecraft:turtle_egg",
        "minecraft:sniffer_egg",
        "minecraft:frog_spawn",
        "minecraft:spore_blossom",
        "minecraft:pink_petals",
        "minecraft:wildflowers",
        "minecraft:leaf_litter",
        "minecraft:fire",
        "minecraft:soul_fire",
        "minecraft:ladder",
        "minecraft:bell",
        "minecraft:skull",
        "minecraft:tripwire_hook",
        "minecraft:trip_wire",
        "minecraft:tripwire",
        "minecraft:flower_pot",
        "minecraft:lantern",
        "minecraft:chain",
        "minecraft:grindstone",
        "minecraft:brewing_stand",
    ];
    IDENTIFIERS.contains(&identifier) || SUFFIXES.iter().any(|suffix| identifier.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Provides one placement input with only the tested face differing.
    fn input(face: u8) -> PlacementInput {
        PlacementInput {
            face,
            click_position: [0.5; 3],
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    /// Uses the same typed envelope as the block palette.
    fn torch_states() -> Map<String, Value> {
        serde_json::from_value(
            json!({"torch_facing_direction":{"type":"string","value":"unknown"}}),
        )
        .unwrap()
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
    fn torch_faces_point_toward_support() {
        for (face, expected, support) in [
            (1, "top", 0),
            (2, "south", 3),
            (3, "north", 2),
            (4, "east", 5),
            (5, "west", 4),
        ] {
            let mut supports = [Some(false); 6];
            supports[support] = Some(true);
            let states =
                attachment_states("minecraft:torch", &torch_states(), input(face), supports)
                    .unwrap();
            assert_eq!(
                state_value(&states, "torch_facing_direction"),
                Some(&Value::from(expected))
            );
            assert_eq!(
                support_requirement("minecraft:torch", &states),
                SupportRequirement::Face(support as u8)
            );
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
        let states =
            serde_json::from_value(json!({"hanging_bit":{"type":"byte","value":0}})).unwrap();
        for (below, above, face, hanging) in [
            (true, true, 1, false),
            (true, true, 0, true),
            (true, false, 0, false),
            (false, true, 1, true),
        ] {
            let mut supports = [Some(false); 6];
            supports[0] = Some(below);
            supports[1] = Some(above);
            let placed =
                attachment_states("minecraft:lantern", &states, input(face), supports).unwrap();
            assert_eq!(state_bit(&placed, "hanging_bit"), Some(hanging));
        }
        assert!(
            attachment_states("minecraft:lantern", &states, input(1), [Some(false); 6]).is_none()
        );
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
