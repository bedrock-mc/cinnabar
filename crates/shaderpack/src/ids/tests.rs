use std::collections::BTreeMap;

use super::*;

fn blocks(text: &str) -> IdMaps {
    IdMaps::parse(Some(text), None, None, &BTreeMap::new())
}

fn no_tags(_: &str) -> bool {
    false
}

/// Id of a state of a block that belongs to no tag.
fn id(map: &IdMaps, name: &str, properties: &[(&str, &str)]) -> Option<i32> {
    map.blocks.block_id(name, properties, &no_tags)
}

fn diagnostic_lines(map: &IdMaps) -> Vec<u32> {
    map.diagnostics.iter().map(|d| d.line).collect()
}

#[test]
fn names_default_to_the_minecraft_namespace() {
    let map = blocks("block.10=pebble_path cobalt:glow_moss\n");
    assert_eq!(id(&map, "minecraft:pebble_path", &[]), Some(10));
    assert_eq!(id(&map, "cobalt:glow_moss", &[]), Some(10));
    assert_eq!(id(&map, "minecraft:glow_moss", &[]), None);
    assert_eq!(id(&map, "cobalt:pebble_path", &[]), None);
    assert!(map.diagnostics.is_empty());
    assert!(!map.blocks.is_empty());
}

#[test]
fn state_filters_select_states_and_accept_alternatives() {
    let map = blocks(
        "block.20=reed_stalk:half=upper\n\
         block.21=cobalt:lantern_post:lit=true,flicker:facing=north\n",
    );
    assert_eq!(
        id(&map, "minecraft:reed_stalk", &[("half", "upper")]),
        Some(20)
    );
    assert_eq!(id(&map, "minecraft:reed_stalk", &[("half", "lower")]), None);
    let post = "cobalt:lantern_post";
    assert_eq!(
        id(&map, post, &[("facing", "north"), ("lit", "true")]),
        Some(21)
    );
    assert_eq!(
        id(&map, post, &[("lit", "flicker"), ("facing", "north")]),
        Some(21)
    );
    assert_eq!(
        id(&map, post, &[("lit", "false"), ("facing", "north")]),
        None
    );
    assert_eq!(
        id(&map, post, &[("lit", "true"), ("facing", "south")]),
        None
    );
}

#[test]
fn filters_on_properties_the_block_lacks_are_ignored() {
    let map = blocks("block.30=moss_tile:soggy=false\n");
    assert_eq!(id(&map, "minecraft:moss_tile", &[]), Some(30));
    assert_eq!(id(&map, "minecraft:moss_tile", &[("age", "3")]), Some(30));
    assert_eq!(id(&map, "minecraft:moss_tile", &[("soggy", "true")]), None);
}

#[test]
fn malformed_filters_are_reported_and_the_rest_of_the_entry_kept() {
    let map = blocks("block.31=fern_tile:age=2:loose\n");
    assert_eq!(id(&map, "minecraft:fern_tile", &[("age", "2")]), Some(31));
    assert_eq!(id(&map, "minecraft:fern_tile", &[("age", "1")]), None);
    assert_eq!(diagnostic_lines(&map), [1]);
    assert!(map.diagnostics[0].message.starts_with("block.properties"));
}

#[test]
fn the_first_matching_entry_wins_across_ids() {
    let map = blocks(
        "block.50=quartzite:smooth=true\n\
         block.51=quartzite\n\
         block.52=quartzite:smooth=false\n",
    );
    assert_eq!(
        id(&map, "minecraft:quartzite", &[("smooth", "true")]),
        Some(50)
    );
    assert_eq!(
        id(&map, "minecraft:quartzite", &[("smooth", "false")]),
        Some(51)
    );
}

#[test]
fn the_first_matching_entry_wins_within_one_value() {
    let map = blocks("block.55=chalk:wet=true chalk\nblock.56=chalk:wet=true\n");
    assert_eq!(id(&map, "minecraft:chalk", &[("wet", "true")]), Some(55));
    assert_eq!(id(&map, "minecraft:chalk", &[("wet", "false")]), Some(55));
}

#[test]
fn a_repeated_key_takes_the_later_value_at_the_first_position() {
    let map = blocks(
        "block.60=slate\n\
         block.61=slate basalt_tile\n\
         block.60=basalt_tile\n",
    );
    // block.60 now holds only basalt_tile, and still precedes block.61.
    assert_eq!(id(&map, "minecraft:basalt_tile", &[]), Some(60));
    assert_eq!(id(&map, "minecraft:slate", &[]), Some(61));
}

#[test]
fn plain_entries_beat_tags_regardless_of_file_order() {
    let map = blocks("block.70=%Cobalt:Rocky\nblock.71=chalk\n");
    let in_rocky = |tag: &str| tag == "cobalt:rocky";
    assert_eq!(
        map.blocks.block_id("minecraft:chalk", &[], &in_rocky),
        Some(71)
    );
    assert_eq!(
        map.blocks.block_id("minecraft:flint_tile", &[], &in_rocky),
        Some(70)
    );
    assert_eq!(id(&map, "minecraft:flint_tile", &[]), None);
    assert_eq!(map.blocks.tags().collect::<Vec<_>>(), ["cobalt:rocky"]);
}

#[test]
fn tag_entries_filter_states_and_the_first_matching_tag_wins() {
    let map = blocks(
        "block.80=%leafy:kept=false\n\
         block.81=%leafy %cobalt:hedges\n",
    );
    let in_both = |tag: &str| tag == "minecraft:leafy" || tag == "cobalt:hedges";
    let only_hedges = |tag: &str| tag == "cobalt:hedges";
    let shrub = "cobalt:shrub";
    assert_eq!(
        map.blocks.block_id(shrub, &[("kept", "false")], &in_both),
        Some(80)
    );
    assert_eq!(
        map.blocks.block_id(shrub, &[("kept", "true")], &in_both),
        Some(81)
    );
    assert_eq!(
        map.blocks
            .block_id(shrub, &[("kept", "false")], &only_hedges),
        Some(81)
    );
    assert_eq!(
        map.blocks.tags().collect::<Vec<_>>(),
        ["minecraft:leafy", "cobalt:hedges"]
    );
}

#[test]
fn conditionals_select_entries() {
    let text = "#ifdef SWAY\nblock.90=tall_reed\n#else\nblock.91=tall_reed\n#endif\n";
    let defines = BTreeMap::from([("SWAY".to_owned(), String::new())]);
    let on = IdMaps::parse(Some(text), None, None, &defines);
    let off = blocks(text);
    assert_eq!(id(&on, "minecraft:tall_reed", &[]), Some(90));
    assert_eq!(id(&off, "minecraft:tall_reed", &[]), Some(91));
}

#[test]
fn continuation_lines_extend_a_value() {
    let map = blocks(
        "block.100=pebble_path \\\n    glow_tile \\\n    cobalt:dusk_bloom\nblock.101=ash_tile\n",
    );
    assert_eq!(id(&map, "minecraft:pebble_path", &[]), Some(100));
    assert_eq!(id(&map, "minecraft:glow_tile", &[]), Some(100));
    assert_eq!(id(&map, "cobalt:dusk_bloom", &[]), Some(100));
    assert_eq!(id(&map, "minecraft:ash_tile", &[]), Some(101));
}

#[test]
fn invalid_keys_are_reported_and_skipped() {
    let map = blocks("block.ten=mud_tile\nother.5=mud_tile\nblock.12=clay_tile\n");
    assert_eq!(id(&map, "minecraft:mud_tile", &[]), None);
    assert_eq!(id(&map, "minecraft:clay_tile", &[]), Some(12));
    assert_eq!(diagnostic_lines(&map), [1]);
}

#[test]
fn ids_outside_sixteen_bits_are_reported_but_kept() {
    let map = blocks("block.40000=vast_tile\n");
    assert_eq!(id(&map, "minecraft:vast_tile", &[]), Some(40000));
    assert_eq!(diagnostic_lines(&map), [1]);
}

#[test]
fn items_and_entities_map_names_and_reject_state_filters() {
    let map = IdMaps::parse(
        None,
        Some("item.5=iron_trowel cobalt:ember_rod\nitem.6=gadget:charge=2 copper_whistle\n"),
        Some("entity.300=drifter cobalt:wisp\nentity.301=drifter\n"),
        &BTreeMap::new(),
    );
    assert_eq!(map.items.id("minecraft:iron_trowel"), Some(5));
    assert_eq!(map.items.id("cobalt:ember_rod"), Some(5));
    assert_eq!(map.items.id("minecraft:copper_whistle"), Some(6));
    assert_eq!(map.items.id("minecraft:gadget"), None);
    assert_eq!(map.items.id("minecraft:ember_rod"), None);
    assert_eq!(map.entities.id("minecraft:drifter"), Some(301));
    assert_eq!(map.entities.id("cobalt:wisp"), Some(300));
    assert_eq!(map.entities.id("minecraft:iron_trowel"), None);
    assert_eq!(map.diagnostics.len(), 1);
    assert!(map.diagnostics[0].message.starts_with("item.properties"));
    assert_eq!(map.diagnostics[0].line, 2);
}

#[test]
fn missing_item_and_entity_files_give_empty_maps() {
    let map = blocks("block.1=chalk\n");
    assert!(map.items.is_empty());
    assert!(map.entities.is_empty());
    assert_eq!(map.items.id("minecraft:chalk"), None);
}

#[test]
fn without_block_properties_blocks_get_iris_legacy_ids() {
    let legacy = IdMaps::parse(None, None, None, &BTreeMap::new());
    assert!(legacy.diagnostics.is_empty());
    assert_eq!(id(&legacy, "minecraft:oak_leaves", &[]), Some(18));
    assert_eq!(id(&legacy, "minecraft:water", &[("level", "0")]), Some(9));
    assert_eq!(id(&legacy, "minecraft:emerald_block", &[]), Some(-123));
    assert_eq!(
        id(&legacy, "minecraft:red_stained_glass_pane", &[]),
        Some(160)
    );
    assert_eq!(id(&legacy, "minecraft:cherry_leaves", &[]), None);
    // An empty file is still a file: no legacy ids, as in Iris.
    let empty = blocks("");
    assert!(empty.blocks.is_empty());
    assert_eq!(id(&empty, "minecraft:oak_leaves", &[]), None);
    assert_eq!(empty.blocks.tags().count(), 0);
}

#[test]
fn layer_keys_override_render_layers_by_name_only() {
    let map = blocks(
        "layer.translucent=frost_glass cobalt:mist\n\
         layer.cutout=%leafy reed_stalk\n\
         layer.sparkly=chalk\n\
         layer.solid=cobalt:mist\n",
    );
    let layer = |name: &str| map.blocks.render_layer(name);
    assert_eq!(
        layer("minecraft:frost_glass"),
        Some(RenderLayer::Translucent)
    );
    assert_eq!(layer("cobalt:mist"), Some(RenderLayer::Solid));
    assert_eq!(layer("minecraft:reed_stalk"), Some(RenderLayer::Cutout));
    assert_eq!(layer("minecraft:chalk"), None);
    assert_eq!(diagnostic_lines(&map), [2, 3]);
    assert!(map.blocks.is_empty());
    assert_eq!(map.blocks.tags().count(), 0);
}
