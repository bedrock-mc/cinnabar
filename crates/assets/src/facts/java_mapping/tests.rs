use super::*;
use crate::{read_biome_registry, read_registry_for_protocol};

fn mapping() -> &'static JavaBlockMapping {
    JavaBlockMapping::pinned().expect("the checked-in Java mapping validates")
}

/// The sequential id of the registry state `name` whose canonical state has exactly these
/// property values (`byte`/`int` as numbers, `string` as text).
fn state(name: &str, properties: &[(&str, serde_json::Value)]) -> u32 {
    let records = read_registry_for_protocol(
        crate::pinned_content::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    records
        .iter()
        .find(|record| {
            let state: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&record.canonical_state).unwrap();
            record.name.as_ref() == name
                && state.len() == properties.len()
                && properties
                    .iter()
                    .all(|(key, value)| state.get(*key).map(|typed| &typed["value"]) == Some(value))
        })
        .unwrap_or_else(|| panic!("no registry state {name} {properties:?}"))
        .sequential_id
}

fn java(name: &str, properties: &[(&str, serde_json::Value)]) -> String {
    mapping()
        .java_block_state(state(name, properties))
        .unwrap_or_else(|| panic!("{name} is unmapped"))
        .to_string()
}

#[test]
fn leaves_logs_water_and_stairs_map_to_their_java_states() {
    use serde_json::json;
    assert_eq!(
        java(
            "minecraft:oak_leaves",
            &[("persistent_bit", json!(0)), ("update_bit", json!(0))]
        ),
        "minecraft:oak_leaves[distance=1,persistent=false,waterlogged=false]"
    );
    assert_eq!(
        java(
            "minecraft:oak_leaves",
            &[("persistent_bit", json!(1)), ("update_bit", json!(0))]
        ),
        "minecraft:oak_leaves[distance=1,persistent=true,waterlogged=false]"
    );
    for axis in ["x", "y", "z"] {
        assert_eq!(
            java("minecraft:oak_log", &[("pillar_axis", json!(axis))]),
            format!("minecraft:oak_log[axis={axis}]")
        );
    }
    assert_eq!(
        java("minecraft:water", &[("liquid_depth", json!(0))]),
        "minecraft:water[level=0]"
    );
    assert_eq!(
        java("minecraft:flowing_water", &[("liquid_depth", json!(3))]),
        "minecraft:water[level=3]"
    );
    let stairs = mapping()
        .java_block_state(state(
            "minecraft:oak_stairs",
            &[
                ("minecraft:corner", json!("none")),
                ("upside_down_bit", json!(1)),
                ("weirdo_direction", json!(0)),
            ],
        ))
        .unwrap();
    assert_eq!(stairs.name(), "minecraft:oak_stairs");
    assert_eq!(stairs.property("facing"), Some("east"));
    assert_eq!(stairs.property("half"), Some("top"));
    assert_eq!(stairs.property("waterlogged"), Some("false"));
    assert_eq!(stairs.property("shape"), Some("straight"));
    assert_eq!(stairs.property("open"), None);
}

#[test]
fn waterloggable_blocks_map_to_their_dry_java_state() {
    use serde_json::json;
    assert_eq!(
        java("minecraft:lantern", &[("hanging", json!(1))]),
        "minecraft:lantern[hanging=true,waterlogged=false]"
    );
    assert_eq!(java("minecraft:air", &[]), "minecraft:air");
    let connections = [
        "minecraft:connection_east",
        "minecraft:connection_north",
        "minecraft:connection_south",
        "minecraft:connection_west",
    ]
    .map(|key| (key, serde_json::json!(0)));
    assert_eq!(
        java("minecraft:oak_fence", &connections),
        "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=false,west=false]"
    );
}

#[test]
fn states_geyser_never_produces_take_the_closest_state_of_their_block() {
    use serde_json::json;
    let updating = state(
        "minecraft:oak_leaves",
        &[("persistent_bit", json!(1)), ("update_bit", json!(1))],
    );
    assert!(mapping().is_approximate(updating));
    assert_eq!(
        mapping().java_block_state(updating).unwrap().to_string(),
        "minecraft:oak_leaves[distance=1,persistent=true,waterlogged=false]"
    );
    let exact = state(
        "minecraft:oak_leaves",
        &[("persistent_bit", json!(1)), ("update_bit", json!(0))],
    );
    assert!(!mapping().is_approximate(exact));
}

#[test]
fn properties_are_sorted_and_tags_list_the_block() {
    use serde_json::json;
    let leaves = mapping()
        .java_block_state(state(
            "minecraft:oak_leaves",
            &[("persistent_bit", json!(0)), ("update_bit", json!(0))],
        ))
        .unwrap();
    let keys: Vec<&str> = leaves.properties().map(|(key, _)| key).collect();
    assert_eq!(keys, ["distance", "persistent", "waterlogged"]);
    assert!(leaves.in_tag("minecraft:leaves"));
    assert!(!leaves.in_tag("minecraft:logs"));
    let log = mapping()
        .java_block_state(state("minecraft:oak_log", &[("pillar_axis", json!("y"))]))
        .unwrap();
    assert!(log.in_tag("minecraft:logs"));
}

#[test]
fn unmapped_states_and_biomes_return_none() {
    let mapping = mapping();
    assert!(mapping.java_block_state(u32::MAX).is_none());
    assert!(
        mapping
            .java_block_state(mapping.payload().states.len() as u32)
            .is_none()
    );
    if let Some(index) = mapping.block_states().position(|state| state.is_none()) {
        assert!(mapping.java_block_state(index as u32).is_none());
    }
    assert!(mapping.java_biome(u32::MAX).is_none());
}

#[test]
fn dense_states_cover_the_whole_block_registry() {
    let records = read_registry_for_protocol(
        crate::pinned_content::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    assert_eq!(mapping().block_states().len(), records.len());
}

#[test]
fn biomes_map_to_the_java_biome_of_the_same_name() {
    let records = read_biome_registry(BIOME_REGISTRY_BYTES).unwrap();
    let id = |name: &str| {
        records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .id
    };
    for name in ["minecraft:plains", "minecraft:the_end", "minecraft:river"] {
        assert_eq!(mapping().java_biome(id(name)), Some(name));
        assert_eq!(java_biome(id(name)), Some(name));
    }
}

/// The Java registry entry named `name`.
fn java_entry(name: &str) -> (u16, JavaBiomeEntry<'static>) {
    let (index, entry) = mapping()
        .java_biomes()
        .enumerate()
        .find(|(_, entry)| entry.name == name)
        .unwrap_or_else(|| panic!("no Java biome {name}"));
    (index as u16, entry)
}

#[test]
fn java_biomes_carry_their_registry_climate() {
    let (_, plains) = java_entry("minecraft:plains");
    assert_eq!(
        (
            plains.temperature,
            plains.downfall,
            plains.has_precipitation,
            plains.frozen
        ),
        (0.8, 0.4, true, false)
    );
    let (_, desert) = java_entry("minecraft:desert");
    assert_eq!(
        (
            desert.temperature,
            desert.downfall,
            desert.has_precipitation
        ),
        (2.0, 0.0, false)
    );
    assert!(java_entry("minecraft:frozen_ocean").1.frozen);
    assert!(!java_entry("minecraft:ocean").1.frozen);
}

#[test]
fn java_biome_indices_follow_registry_order() {
    let mapping = mapping();
    let count = mapping.java_biomes().len();
    assert_eq!(count, mapping.payload().java_biomes.len());
    for (index, entry) in mapping.java_biomes().enumerate() {
        assert_eq!(mapping.java_biome_entry(index as u16), Some(entry));
    }
    assert!(mapping.java_biome_entry(count as u16).is_none());
    let records = read_biome_registry(BIOME_REGISTRY_BYTES).unwrap();
    let plains = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:plains")
        .unwrap()
        .id;
    let index = mapping.java_biome_index(plains).unwrap();
    assert_eq!(
        mapping.java_biome_entry(index).unwrap().name,
        "minecraft:plains"
    );
    assert!(mapping.java_biome_index(u32::MAX).is_none());
}

#[test]
fn java_biome_tags_list_their_members() {
    let mapping = mapping();
    let (swamp, _) = java_entry("minecraft:swamp");
    let (plains, _) = java_entry("minecraft:plains");
    let (ocean, _) = java_entry("minecraft:ocean");
    assert!(mapping.java_biome_has_tag(swamp, "minecraft:allows_surface_slime_spawns"));
    assert!(!mapping.java_biome_has_tag(plains, "minecraft:allows_surface_slime_spawns"));
    assert!(mapping.java_biome_has_tag(ocean, "minecraft:is_ocean"));
    assert!(!mapping.java_biome_has_tag(ocean, "minecraft:no_such_tag"));
    assert!(!mapping.java_biome_has_tag(u16::MAX, "minecraft:is_ocean"));
}

#[test]
fn bedrock_entities_map_to_their_java_entity_types() {
    let mapping = mapping();
    for (bedrock, java) in [
        ("minecraft:ender_crystal", "minecraft:end_crystal"),
        ("minecraft:evocation_illager", "minecraft:evoker"),
        ("minecraft:zombie_pigman", "minecraft:zombified_piglin"),
        ("minecraft:cow", "minecraft:cow"),
        ("minecraft:xp_orb", "minecraft:experience_orb"),
        ("minecraft:minecart", "minecraft:minecart"),
    ] {
        assert_eq!(mapping.java_entity(bedrock), Some(java), "{bedrock}");
    }
    assert_eq!(
        mapping
            .java_entities("minecraft:evocation_illager")
            .collect::<Vec<_>>(),
        ["minecraft:evoker", "minecraft:illusioner"]
    );
    assert!(
        mapping
            .java_entities("minecraft:minecart")
            .any(|java| java == "minecraft:furnace_minecart")
    );
    assert_eq!(mapping.java_entity("minecraft:no_such_entity"), None);
    assert_eq!(mapping.java_entities("minecraft:no_such_entity").count(), 0);
}

#[test]
fn bedrock_items_map_to_their_java_items() {
    let mapping = mapping();
    assert_eq!(mapping.java_item("minecraft:web"), Some("minecraft:cobweb"));
    assert_eq!(
        mapping.java_item("minecraft:brick_block"),
        Some("minecraft:bricks")
    );
    assert_eq!(
        mapping.java_item("minecraft:diamond_sword"),
        Some("minecraft:diamond_sword")
    );
    assert_eq!(
        mapping.java_item("minecraft:filled_map"),
        Some("minecraft:filled_map")
    );
    let banners: Vec<&str> = mapping.java_items("minecraft:banner").collect();
    assert_eq!(
        mapping.java_item("minecraft:banner"),
        banners.first().copied()
    );
    assert!(banners.contains(&"minecraft:white_banner"));
    assert_eq!(mapping.java_item("minecraft:no_such_item"), None);
}

#[test]
fn corrupt_or_unpinned_bytes_are_rejected() {
    let protocol = crate::active_content_registry_protocol();
    let mut corrupt = SOURCE_BYTES.to_vec();
    corrupt[MAGIC.len() + 4] ^= 1;
    assert!(JavaBlockMapping::decode(&corrupt, protocol).is_err());
    assert!(JavaBlockMapping::validate_pinned(&corrupt).is_err());
    assert!(JavaBlockMapping::decode(SOURCE_BYTES, protocol + 1).is_err());
    assert!(JavaBlockMapping::decode(MAGIC, protocol).is_err());
}

#[test]
fn payload_validation_rejects_bad_order_and_indices() {
    let valid = mapping().payload().clone();
    assert!(JavaBlockMapping::encode(&valid).is_ok());
    let mut unsorted = valid.clone();
    unsorted.blocks.swap(0, 1);
    assert!(JavaBlockMapping::from_payload(unsorted).is_err());
    let mut out_of_range = valid.clone();
    out_of_range.states[0] = Some((u16::MAX, 0));
    assert!(JavaBlockMapping::from_payload(out_of_range).is_err());
    let mut bad_properties = valid.clone();
    bad_properties.property_sets = Box::from([Box::from("b=1,a=2")]);
    assert!(JavaBlockMapping::from_payload(bad_properties).is_err());
    let mut bad_biome = valid.clone();
    bad_biome.biomes[0].1 = valid.java_biomes.len() as u16;
    assert!(JavaBlockMapping::from_payload(bad_biome).is_err());
    let mut unsorted_tag = valid.clone();
    let members = &mut unsorted_tag
        .biome_tags
        .iter_mut()
        .find(|(_, m)| m.len() > 1)
        .unwrap()
        .1;
    members.swap(0, 1);
    assert!(JavaBlockMapping::from_payload(unsorted_tag).is_err());
    let mut duplicate_name = valid;
    duplicate_name.java_biomes[1].name = duplicate_name.java_biomes[0].name.clone();
    assert!(JavaBlockMapping::from_payload(duplicate_name).is_err());
    let mut unsorted_entities = mapping().payload().clone();
    unsorted_entities.entities.swap(0, 1);
    assert!(JavaBlockMapping::from_payload(unsorted_entities).is_err());
    let mut missing_inverse = mapping().payload().clone();
    let last = missing_inverse.bedrock_items.len() - 1;
    missing_inverse.bedrock_items = missing_inverse.bedrock_items[..last].into();
    assert!(JavaBlockMapping::from_payload(missing_inverse).is_err());
    let mut wrong_inverse = mapping().payload().clone();
    wrong_inverse.bedrock_entities.swap(0, 1);
    assert!(JavaBlockMapping::from_payload(wrong_inverse).is_err());
}
