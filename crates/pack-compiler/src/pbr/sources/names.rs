use std::path::Path;

fn java_name(stem: &str) -> String {
    fn wood(name: &str) -> &str {
        match name {
            "big_oak" | "roofed_oak" => "dark_oak",
            "wood" => "oak",
            _ => name,
        }
    }
    for (prefix, suffix) in [
        ("concrete_powder_", "concrete_powder"),
        ("concrete_", "concrete"),
        ("wool_colored_", "wool"),
        ("stained_glass_", "stained_glass"),
        ("glass_pane_top_", "stained_glass_pane_top"),
        ("glass_", "stained_glass"),
        ("hardened_clay_stained_", "terracotta"),
        ("glazed_terracotta_", "glazed_terracotta"),
    ] {
        if let Some(color) = stem.strip_prefix(prefix) {
            let color = if color == "silver" {
                "light_gray"
            } else {
                color
            };
            return format!("{color}_{suffix}");
        }
    }
    if let Some(value) = stem.strip_prefix("door_") {
        for (ending, face) in [("_lower", "bottom"), ("_upper", "top")] {
            if let Some(name) = value.strip_suffix(ending) {
                return format!("{}_door_{face}", wood(name));
            }
        }
    }
    if let Some(value) = stem.strip_prefix("log_") {
        let (name, top) = value
            .strip_suffix("_top")
            .map_or((value, false), |name| (name, true));
        return format!("{}_log{}", wood(name), if top { "_top" } else { "" });
    }
    for (prefix, suffix) in [
        ("planks_", "planks"),
        ("leaves_", "leaves"),
        ("sapling_", "sapling"),
    ] {
        if let Some(value) = stem.strip_prefix(prefix) {
            let value = value.strip_suffix("_carried").unwrap_or(value);
            return format!("{}_{suffix}", wood(value));
        }
    }
    if stem.ends_with("_log_side") {
        return stem.trim_end_matches("_side").to_owned();
    }
    if let Some(name) = stem.strip_suffix("_door_lower") {
        return format!("{name}_door_bottom");
    }
    if let Some(name) = stem.strip_suffix("_stem_side") {
        return format!("{name}_stem");
    }
    match stem {
        "grass_carried" | "grass_top" => "grass_block_top",
        "grass_side" | "grass_side_carried" => "grass_block_side",
        "grass_side_overlay" => "grass_block_side_overlay",
        "grass_side_snow" => "grass_block_snow",
        "grass_path_top" => "dirt_path_top",
        "grass_path_side" => "dirt_path_side",
        "stonebrick" => "stone_bricks",
        "stonebrick_cracked" => "cracked_stone_bricks",
        "stonebrick_mossy" => "mossy_stone_bricks",
        "stonebrick_carved" => "chiseled_stone_bricks",
        "cobblestone_mossy" => "mossy_cobblestone",
        "sandstone_carved" => "chiseled_sandstone",
        "sandstone_smooth" => "cut_sandstone",
        "red_sandstone_carved" => "chiseled_red_sandstone",
        "red_sandstone_smooth" => "cut_red_sandstone",
        "brick" => "bricks",
        "furnace_front_off" => "furnace_front",
        "sandstone_normal" => "sandstone",
        "red_sandstone_normal" => "red_sandstone",
        "stone_granite" => "granite",
        "stone_diorite" => "diorite",
        "stone_andesite" => "andesite",
        "stone_granite_smooth" => "polished_granite",
        "stone_diorite_smooth" => "polished_diorite",
        "stone_andesite_smooth" => "polished_andesite",
        "stone_slab_top" => "smooth_stone",
        "stone_slab_side" => "smooth_stone_slab_side",
        "rail_normal" => "rail",
        "rail_normal_turned" => "rail_corner",
        "rail_golden" => "powered_rail",
        "rail_golden_powered" => "powered_rail_on",
        "rail_activator" => "activator_rail",
        "rail_activator_powered" => "activator_rail_on",
        "rail_detector" => "detector_rail",
        "rail_detector_powered" => "detector_rail_on",
        "ice_packed" => "packed_ice",
        "sponge_wet" => "wet_sponge",
        "trapdoor" => "oak_trapdoor",
        "nether_brick" => "nether_bricks",
        "red_nether_brick" => "red_nether_bricks",
        "end_bricks" => "end_stone_bricks",
        "prismarine_dark" => "dark_prismarine",
        "prismarine_rough" => "prismarine",
        "hardened_clay" => "terracotta",
        "crimson_log_side" => "crimson_stem",
        "crimson_log_top" => "crimson_stem_top",
        "bamboo_leaf" => "bamboo_large_leaves",
        "bamboo_small_leaf" => "bamboo_small_leaves",
        "quartz_block_side" => "quartz_block_side",
        "quartz_block_lines" => "quartz_pillar",
        "quartz_block_lines_top" => "quartz_pillar_top",
        "tallgrass" => "grass",
        "reeds" => "sugar_cane",
        "web" => "cobweb",
        _ => stem,
    }
    .to_owned()
}

pub(super) fn variants(alias: &str) -> Vec<String> {
    let alias = alias.replace('\\', "/");
    let alias = alias
        .strip_suffix(".png")
        .or_else(|| alias.strip_suffix(".tga"))
        .unwrap_or(&alias);
    let path = Path::new(alias);
    let stem = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(alias);
    let java = java_name(stem);
    let mut result = vec![
        alias.to_owned(),
        alias.replace("textures/blocks/", "textures/block/"),
        alias.replace("textures/block/", "textures/blocks/"),
    ];
    let directory = if alias.starts_with("textures/items/") {
        "item"
    } else {
        "block"
    };
    result.push(format!("textures/{directory}/{java}"));
    result.push(format!("textures/{directory}s/{java}"));
    if stem == "tallgrass" {
        result.push("textures/block/short_grass".to_owned());
    }
    if stem == "snow" {
        result.push("textures/blocks/snow_block".to_owned());
    }
    if java == "stripped_crimson_stem_top" {
        result.push("textures/block/stripped_crimson_log_top".to_owned());
    }
    if java.starts_with("sandstone") {
        result.push(format!(
            "textures/block/{}",
            java.replacen("sandstone", "santstone", 1)
        ));
    }
    if java == "furnace_front" {
        result.push("textures/block/furnace_front_off".to_owned());
    }
    result.dedup();
    result
}
