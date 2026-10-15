//! Retained oracle: Attachment state and support requirements for local block placement.

use crate::placement_state::PlacementInput;
use crate::placement_support::SupportRequirement;
use serde_json::{Map, Value};

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
        return crate::placement_oracle::placement_vines::attachment_states(
            canonical, input.face, supports,
        );
    }
    if crate::placement_oracle::placement_multiface::is_multiface(canonical) {
        return crate::placement_oracle::placement_multiface::attachment_states(
            canonical, input.face, supports,
        );
    }
    if canonical.contains_key("torch_facing_direction") {
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
                if crate::placement_oracle::placement_state::yaw_quadrant(input.yaw)? & 1 == 0 {
                    "down_north_south"
                } else {
                    "down_east_west"
                }
            }
            1 => {
                if crate::placement_oracle::placement_state::yaw_quadrant(input.yaw)? & 1 == 0 {
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
        crate::placement_oracle::placement_state::set_value(
            &mut states,
            "lever_direction",
            Value::from(direction),
        )?;
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
    } else if known_keys(canonical, &["hanging"]) {
        state_bit(canonical, "hanging")?;
        let below = supports[0]?;
        let hanging = !below || (input.face == 0 && supports[1]?);
        if hanging && !supports[1]? {
            return None;
        }
        set_state(&mut states, "hanging", Value::from(u8::from(hanging)))?;
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
        return crate::placement_oracle::placement_vines::support_face(states)
            .map_or(SupportRequirement::Unsupported, SupportRequirement::Face);
    }
    if crate::placement_oracle::placement_multiface::is_multiface(states) {
        return crate::placement_oracle::placement_multiface::support_face(states)
            .map_or(SupportRequirement::Unsupported, SupportRequirement::Face);
    }
    if let Some(face) = crate::placement_oracle::placement_signs::support_face(identifier, states) {
        return SupportRequirement::Face(face);
    }
    if states.contains_key("torch_facing_direction") {
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
    if known_keys(states, &["hanging"]) {
        return match state_bit(states, "hanging") {
            Some(false) => SupportRequirement::Face(0),
            Some(true) => SupportRequirement::Face(1),
            None => SupportRequirement::Unsupported,
        };
    }
    if is_carpet(identifier) {
        return SupportRequirement::Face(0);
    }
    if crate::placement_oracle::placement_doors::is_door(identifier) {
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

const UNRESOLVED_SURVIVAL_SUFFIXES: &[&str] = &[
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
    "_bush",
    "_skull",
    "_head",
    "_chain",
];
const UNRESOLVED_SURVIVAL_IDENTIFIERS: &[&str] = &[
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
    "minecraft:golden_dandelion",
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
    "minecraft:moss_carpet",
    "minecraft:pale_moss_carpet",
    "minecraft:hanging_roots",
    "minecraft:nether_sprouts",
    "minecraft:weeping_vines",
    "minecraft:twisting_vines",
    "minecraft:pointed_dripstone",
    "minecraft:small_dripleaf_block",
    "minecraft:big_dripleaf",
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
    "minecraft:tripwire_hook",
    "minecraft:trip_wire",
    "minecraft:flower_pot",
    "minecraft:lantern",
    "minecraft:grindstone",
    "minecraft:brewing_stand",
];

/// Substrate, fluid, growth and multi-cell rules require additional verified world facts.
fn survival_is_unresolved(identifier: &str) -> bool {
    UNRESOLVED_SURVIVAL_IDENTIFIERS.contains(&identifier)
        || UNRESOLVED_SURVIVAL_SUFFIXES
            .iter()
            .any(|suffix| identifier.ends_with(suffix))
}
