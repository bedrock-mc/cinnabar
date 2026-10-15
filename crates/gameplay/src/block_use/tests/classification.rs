//! Compares retained block-use classifications with facts over the pinned palette.

use assets::{RegistryRecord, facts::BlockFacts, registry::BlockTraits as T};
use serde_json::{Map, Value};

/// Loads real state variants, including the namespace spelling shipped in the registry.
fn palette(legacy: bool) -> Box<[RegistryRecord]> {
    if legacy {
        return assets::read_registry(include_bytes!(
            "../../../../assets/data/block-registry-v1001.bin"
        ))
        .unwrap();
    }
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
}

/// Retains the earlier orientation classifier as an independent comparison.
fn earlier_orientation(states: &Map<String, Value>) -> bool {
    states.keys().any(|key| {
        matches!(
            key.strip_prefix("minecraft:").unwrap_or(key),
            "cardinal_direction"
                | "facing_direction"
                | "vertical_half"
                | "direction"
                | "weirdo_direction"
                | "orientation"
                | "upside_down_bit"
                | "top_slot_bit"
                | "ground_sign_direction"
                | "hanging"
                | "rotation"
                | "torch_facing_direction"
                | "vine_direction_bits"
                | "coral_direction"
        )
    })
}

#[test]
fn orientation_facts_preserve_every_pinned_placement_classification() {
    for legacy in [false, true] {
        let records = palette(legacy);
        let facts = BlockFacts::new(&records).unwrap();
        for record in &records {
            let states: Map<String, Value> = serde_json::from_str(&record.canonical_state).unwrap();
            // GP-20: attachment and rail orientation were added to the retained placement classes.
            let attachment = [
                "rail_direction",
                "lever_direction",
                "multi_face_direction_bits",
                "minecraft:block_face",
            ]
            .iter()
            .any(|key| states.contains_key(*key));
            assert_eq!(
                super::super::orientation_sensitive(facts.get(record.sequential_id)),
                earlier_orientation(&states) || attachment,
                "{} {}",
                record.name,
                record.canonical_state
            );
        }
    }
}

#[test]
fn every_observer_state_keeps_the_first_held_placement_hit() {
    let records = palette(false);
    let facts = BlockFacts::new(&records).unwrap();
    let observers: Vec<_> = records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:observer")
        .collect();
    assert_eq!(observers.len(), 12);
    for observer in observers {
        let states: Map<String, Value> = serde_json::from_str(&observer.canonical_state).unwrap();
        assert!(states.contains_key("minecraft:facing_direction"));
        let mut intention = super::super::BuildIntention::default();
        let first_hit = [0.75, 64.0, 0.25];
        intention.record(
            false,
            [0, 63, 0],
            super::LocalUse::Place,
            true,
            false,
            first_hit,
        );
        intention.record(
            true,
            [1, 63, 0],
            super::LocalUse::Place,
            true,
            false,
            [1.5, 64.0, 0.5],
        );
        let orientation_hit =
            super::super::orientation_sensitive(facts.get(observer.sequential_id))
                .then(|| intention.first_world_hit())
                .flatten();
        assert_eq!(
            orientation_hit,
            Some(first_hit),
            "{}",
            observer.canonical_state
        );
        assert!(
            facts
                .get(observer.sequential_id)
                .traits
                .contains(T::ORIENTATION_SENSITIVE)
        );
    }
}

/// Retains the earlier destination replacement membership for comparison.
const REPLACEABLE_BLOCKS: &[&str] = &[
    "minecraft:air",
    "minecraft:short_grass",
    "minecraft:red_shrub",
    "minecraft:tall_grass",
    "minecraft:fern",
    "minecraft:large_fern",
    "minecraft:deadbush",
    "minecraft:vine",
    "minecraft:glow_lichen",
    "minecraft:seagrass",
    "minecraft:water",
    "minecraft:flowing_water",
    "minecraft:lava",
    "minecraft:flowing_lava",
    "minecraft:fire",
    "minecraft:soul_fire",
    "minecraft:structure_void",
    "minecraft:crimson_roots",
    "minecraft:warped_roots",
    "minecraft:nether_sprouts",
    "minecraft:hanging_roots",
];

/// Which ability a block's own use needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interaction {
    /// Doors, trapdoors, fence gates, buttons and levers.
    Switch,
    Container,
    /// Uses no ability gates.
    Other,
}

/// Retains the earlier container ability-gate membership for comparison.
const CONTAINER_BLOCKS: &[&str] = &[
    "minecraft:crafting_table",
    "minecraft:furnace",
    "minecraft:lit_furnace",
    "minecraft:blast_furnace",
    "minecraft:lit_blast_furnace",
    "minecraft:smoker",
    "minecraft:lit_smoker",
    "minecraft:barrel",
    "minecraft:anvil",
    "minecraft:enchanting_table",
    "minecraft:brewing_stand",
    "minecraft:hopper",
    "minecraft:dropper",
    "minecraft:dispenser",
    "minecraft:crafter",
    "minecraft:loom",
    "minecraft:stonecutter_block",
    "minecraft:grindstone",
    "minecraft:cartography_table",
    "minecraft:smithing_table",
    "minecraft:beacon",
];

/// Retains the earlier local interaction membership for comparison.
const OTHER_INTERACTIVE_BLOCKS: &[&str] = &[
    "minecraft:noteblock",
    "minecraft:unpowered_repeater",
    "minecraft:powered_repeater",
    "minecraft:unpowered_comparator",
    "minecraft:powered_comparator",
    "minecraft:daylight_detector",
    "minecraft:daylight_detector_inverted",
    "minecraft:bell",
    "minecraft:bed",
];

/// Classifies the earlier identifier rules independently of compiled facts.
fn earlier_interaction(identifier: &str) -> Option<Interaction> {
    let metal = identifier == "minecraft:iron_door" || identifier == "minecraft:iron_trapdoor";
    let ends = |suffixes: &[&str]| suffixes.iter().any(|suffix| identifier.ends_with(suffix));
    if identifier == "minecraft:lever"
        || (!metal && ends(&["_door", "_trapdoor", "_button", "fence_gate"]))
    {
        Some(Interaction::Switch)
    } else if CONTAINER_BLOCKS.contains(&identifier) || ends(&["chest", "shulker_box"]) {
        Some(Interaction::Container)
    } else if OTHER_INTERACTIVE_BLOCKS.contains(&identifier) || identifier.ends_with("_bed") {
        Some(Interaction::Other)
    } else {
        None
    }
}

#[test]
fn interaction_and_replacement_facts_preserve_pinned_classification() {
    for legacy in [false, true] {
        let records = palette(legacy);
        let facts = BlockFacts::new(&records).unwrap();
        let components: std::collections::BTreeSet<_> =
            include_str!("../../../../assets/data/block-components-v2193.tsv")
                .lines()
                .skip(1)
                .filter_map(|row| {
                    let (name, values) = row.split_once('\t').unwrap();
                    values
                        .split(',')
                        .any(|value| value == "minecraft:replaceable")
                        .then_some(name)
                })
                .collect();
        for record in &records {
            if record.name.as_ref() == assets::facts::block::RESERVED_BLOCK_NAME {
                continue;
            }
            let name = record.name.as_ref();
            let states: Map<String, Value> = serde_json::from_str(&record.canonical_state).unwrap();
            let traits = facts.get(record.sequential_id).traits;
            let interaction = earlier_interaction(name);
            // GP-18: the canonical trapdoor lacks the suffix used by the earlier switch list.
            let switch = interaction == Some(Interaction::Switch) || name == "minecraft:trapdoor";
            // GP-15 / PC-29: native replacement components and the state-dependent overrides are intentional additions.
            let replacement = REPLACEABLE_BLOCKS.contains(&name)
                || components.contains(name)
                || record.model_family == assets::registry::ModelFamily::FlowerBed
                || name.starts_with("minecraft:light_block_")
                || (name == "minecraft:snow_layer" && states["height"]["value"] == 0);
            for (trait_, expected) in [
                (T::TOGGLES_BY_HAND, switch),
                (
                    T::OPENS_CONTAINER,
                    interaction == Some(Interaction::Container),
                ),
                (T::USES_LOCALLY, interaction == Some(Interaction::Other)),
                (T::REPLACEABLE, replacement),
            ] {
                assert_eq!(
                    traits.contains(trait_),
                    expected,
                    "{trait_:?} {name} {}",
                    record.canonical_state
                );
            }
        }
    }
}

#[test]
fn placement_traits_preserve_pinned_identity_and_state_classification() {
    for legacy in [false, true] {
        let records = palette(legacy);
        let facts = BlockFacts::new(&records).unwrap();
        for record in &records {
            if record.name.as_ref() == assets::facts::block::RESERVED_BLOCK_NAME {
                continue;
            }
            let name = record.name.as_ref();
            let states: Map<String, Value> = serde_json::from_str(&record.canonical_state).unwrap();
            let multipart = states.contains_key("head_piece_bit")
                || (states.contains_key("upper_block_bit")
                    && ![
                        "minecraft:pale_moss_carpet",
                        "minecraft:small_dripleaf_block",
                    ]
                    .contains(&name)
                    && (name != "minecraft:pitcher_crop"
                        || states["growth"]["value"].as_i64().unwrap() > 2));
            for (trait_, expected) in [
                (T::AIR, name == "minecraft:air"),
                (
                    T::WATER,
                    [
                        "minecraft:water",
                        "minecraft:flowing_water",
                        "minecraft:bubble_column",
                    ]
                    .contains(&name),
                ),
                (
                    T::LAVA,
                    ["minecraft:lava", "minecraft:flowing_lava"].contains(&name),
                ),
                (
                    T::FIRE,
                    ["minecraft:fire", "minecraft:soul_fire"].contains(&name),
                ),
                (T::RESPAWN_ANCHOR, name == "minecraft:respawn_anchor"),
                (T::GLOWSTONE, name == "minecraft:glowstone"),
                (
                    T::NETHER_BRICK_FENCE,
                    name == "minecraft:nether_brick_fence",
                ),
                (
                    T::DOOR,
                    name.ends_with("_door") && !name.ends_with("_trapdoor"),
                ),
                (
                    T::MULTIFACE,
                    states.contains_key("multi_face_direction_bits"),
                ),
                (T::TORCH, states.contains_key("torch_facing_direction")),
                (
                    T::LANTERN_LIKE,
                    states.len() == 1 && states.contains_key("hanging"),
                ),
                (T::SNOW_LAYER, name == "minecraft:snow_layer"),
                (
                    T::CANDLE,
                    name == "minecraft:candle" || name.ends_with("_candle"),
                ),
                (T::HOPPER, name == "minecraft:hopper"),
                (
                    T::BODY_RELATIVE_FACING,
                    [
                        "minecraft:dispenser",
                        "minecraft:piston",
                        "minecraft:sticky_piston",
                    ]
                    .contains(&name),
                ),
                // Piston arm entries share the compiled class; only body-relative facing is placed.
                (
                    T::PISTON,
                    [
                        "minecraft:piston",
                        "minecraft:sticky_piston",
                        "minecraft:piston_arm_collision",
                        "minecraft:sticky_piston_arm_collision",
                    ]
                    .contains(&name),
                ),
                (T::STANDING_SIGN, name.ends_with("standing_sign")),
                (T::WALL_SIGN, name.ends_with("wall_sign")),
                (
                    T::DOUBLE_SLAB,
                    name.ends_with("_slab") && name.contains("double_"),
                ),
                (
                    T::CONNECTS_THIN,
                    record.model_family == assets::registry::ModelFamily::Pane
                        || (record.model_family != assets::registry::ModelFamily::Fence
                            && states
                                .keys()
                                .any(|key| key.starts_with("minecraft:connection_"))),
                ),
                (
                    T::CONNECTS_FENCE,
                    record.model_family == assets::registry::ModelFamily::Fence,
                ),
                (T::MULTIPART, multipart),
            ] {
                assert_eq!(
                    facts.get(record.sequential_id).traits.contains(trait_),
                    expected,
                    "{trait_:?} {name} {}",
                    record.canonical_state
                );
            }
        }
    }
}
