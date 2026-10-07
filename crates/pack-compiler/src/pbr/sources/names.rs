use std::path::Path;

fn java_name(stem: &str) -> String {
    fn wood(name: &str) -> &str {
        if name == "big_oak" { "dark_oak" } else { name }
    }
    for (prefix, suffix) in [
        ("concrete_powder_", "concrete_powder"),
        ("concrete_", "concrete"),
        ("wool_colored_", "wool"),
        ("stained_glass_", "stained_glass"),
        ("glass_pane_top_", "stained_glass_pane_top"),
        ("glass_", "stained_glass"),
    ] {
        if let Some(color) = stem.strip_prefix(prefix) {
            return format!("{color}_{suffix}");
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
    match stem {
        "grass_carried" | "grass_top" => "grass_block_top",
        "grass_side" | "grass_side_carried" => "grass_block_side",
        "grass_side_overlay" => "grass_block_side_overlay",
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
    result.push(format!("textures/block/{java}"));
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
