use super::*;

pub(in crate::compiler) const fn is_terrestrial_cross(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Cross | ModelFamily::Crop)
}

pub(in crate::compiler) fn is_aquatic_cross(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Aquatic)
        && record.name.as_ref() == "minecraft:seagrass"
}

pub(in crate::compiler) fn is_cross_visual(record: &RegistryRecord) -> bool {
    is_terrestrial_cross(record) || is_aquatic_cross(record)
}

pub(in crate::compiler) fn is_kelp(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Aquatic) && record.name.as_ref() == "minecraft:kelp"
}

pub(in crate::compiler) fn is_flowerbed(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::FlowerBed)
        && matches!(
            record.name.as_ref(),
            "minecraft:wildflowers" | "minecraft:pink_petals"
        )
}

pub(in crate::compiler) fn is_vine(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Vine) && record.name.as_ref() == "minecraft:vine"
}

pub(in crate::compiler) fn is_glow_lichen(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::GlowLichen)
        && record.name.as_ref() == "minecraft:glow_lichen"
}

pub(in crate::compiler) fn is_sculk_vein(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::SculkVein)
        && record.name.as_ref() == "minecraft:sculk_vein"
}

pub(in crate::compiler) fn is_multiface(record: &RegistryRecord) -> bool {
    is_glow_lichen(record) || is_sculk_vein(record) || is_resin_clump(record)
}

pub(in crate::compiler) const fn is_door(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Door)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_trapdoor(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Trapdoor)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_wall(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Wall)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_pressure_plate(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::PressurePlate)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_button(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Button)
        && matches!(record.contributor_role, ContributorRole::Primary)
        && is_supported_button_name(&record.name)
}

pub(in crate::compiler) const fn is_supported_button_name(name: &str) -> bool {
    matches!(
        name.as_bytes(),
        b"minecraft:acacia_button"
            | b"minecraft:bamboo_button"
            | b"minecraft:birch_button"
            | b"minecraft:cherry_button"
            | b"minecraft:crimson_button"
            | b"minecraft:dark_oak_button"
            | b"minecraft:jungle_button"
            | b"minecraft:mangrove_button"
            | b"minecraft:pale_oak_button"
            | b"minecraft:polished_blackstone_button"
            | b"minecraft:spruce_button"
            | b"minecraft:stone_button"
            | b"minecraft:warped_button"
            | b"minecraft:wooden_button"
    )
}

pub(in crate::compiler) const fn is_carpet(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Carpet)
        && matches!(record.contributor_role, ContributorRole::Primary)
        && is_supported_carpet_name(&record.name)
}

pub(in crate::compiler) const fn is_supported_carpet_name(name: &str) -> bool {
    matches!(
        name.as_bytes(),
        b"minecraft:black_carpet"
            | b"minecraft:blue_carpet"
            | b"minecraft:brown_carpet"
            | b"minecraft:cyan_carpet"
            | b"minecraft:gray_carpet"
            | b"minecraft:green_carpet"
            | b"minecraft:light_blue_carpet"
            | b"minecraft:light_gray_carpet"
            | b"minecraft:lime_carpet"
            | b"minecraft:magenta_carpet"
            | b"minecraft:moss_carpet"
            | b"minecraft:orange_carpet"
            | b"minecraft:pale_moss_carpet"
            | b"minecraft:pink_carpet"
            | b"minecraft:purple_carpet"
            | b"minecraft:red_carpet"
            | b"minecraft:white_carpet"
            | b"minecraft:yellow_carpet"
    )
}

pub(in crate::compiler) fn is_pale_moss_carpet(record: &RegistryRecord) -> bool {
    is_carpet(record) && record.name.as_ref() == "minecraft:pale_moss_carpet"
}

pub(in crate::compiler) const fn is_gate(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Gate)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_pane(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Pane)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const ORDINARY_STAINED_GLASS_NAMES: [&str; 16] = [
    "minecraft:black_stained_glass",
    "minecraft:blue_stained_glass",
    "minecraft:brown_stained_glass",
    "minecraft:cyan_stained_glass",
    "minecraft:gray_stained_glass",
    "minecraft:green_stained_glass",
    "minecraft:light_blue_stained_glass",
    "minecraft:light_gray_stained_glass",
    "minecraft:lime_stained_glass",
    "minecraft:magenta_stained_glass",
    "minecraft:orange_stained_glass",
    "minecraft:pink_stained_glass",
    "minecraft:purple_stained_glass",
    "minecraft:red_stained_glass",
    "minecraft:white_stained_glass",
    "minecraft:yellow_stained_glass",
];

pub(in crate::compiler) const COPPER_GRATE_NAMES: [&str; 8] = [
    "minecraft:copper_grate",
    "minecraft:exposed_copper_grate",
    "minecraft:oxidized_copper_grate",
    "minecraft:waxed_copper_grate",
    "minecraft:waxed_exposed_copper_grate",
    "minecraft:waxed_oxidized_copper_grate",
    "minecraft:waxed_weathered_copper_grate",
    "minecraft:weathered_copper_grate",
];

pub(in crate::compiler) fn is_stained_glass_cube(record: &RegistryRecord) -> bool {
    record.canonical_state.as_ref() == "{}"
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
        && ORDINARY_STAINED_GLASS_NAMES
            .binary_search(&record.name.as_ref())
            .is_ok()
}

pub(in crate::compiler) fn is_ordinary_stained_glass_name(name: &str) -> bool {
    ORDINARY_STAINED_GLASS_NAMES.binary_search(&name).is_ok()
}

pub(in crate::compiler) fn is_copper_grate(record: &RegistryRecord) -> bool {
    record.canonical_state.as_ref() == "{}"
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.is_empty()
        && !record.flags.contains(BlockFlags::AIR)
        && record.flags.contains(BlockFlags::CUBE_GEOMETRY)
        && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
        && !record.flags.contains(BlockFlags::LEAF_MODEL)
        && is_copper_grate_name(&record.name)
}

pub(in crate::compiler) fn is_copper_grate_name(name: &str) -> bool {
    COPPER_GRATE_NAMES.binary_search(&name).is_ok()
}

pub(in crate::compiler) const fn is_fence(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Fence)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_sign(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Sign)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_slab(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Slab)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_stair(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Stair)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) fn is_cutout_model_visual(record: &RegistryRecord) -> bool {
    is_cross_visual(record)
        || is_kelp(record)
        || is_flowerbed(record)
        || is_vine(record)
        || is_multiface(record)
        || is_door(record)
        || is_trapdoor(record)
}

pub(in crate::compiler) fn is_model_visual(record: &RegistryRecord) -> bool {
    is_stained_glass_cube(record)
        || is_copper_grate(record)
        || is_cutout_model_visual(record)
        || is_slab(record)
        || is_stair(record)
        || is_wall(record)
        || is_pressure_plate(record)
        || is_button(record)
        || is_gate(record)
        || is_carpet(record)
        || is_pane(record)
        || is_fence(record)
        || is_sign(record)
}

pub(in crate::compiler) const fn is_liquid(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Liquid)
        && matches!(record.contributor_role, ContributorRole::LiquidAdditional)
}

pub(in crate::compiler) fn is_supported_liquid(record: &RegistryRecord) -> bool {
    is_liquid(record)
        && matches!(
            record.name.as_ref(),
            "minecraft:water"
                | "minecraft:flowing_water"
                | "minecraft:lava"
                | "minecraft:flowing_lava"
        )
}

pub(in crate::compiler) const fn liquid_material_flags(name: &str) -> u32 {
    match name.as_bytes() {
        b"minecraft:water" | b"minecraft:flowing_water" => {
            MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT
        }
        b"minecraft:lava" | b"minecraft:flowing_lava" => MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
        _ => 0,
    }
}

pub(in crate::compiler) fn cross_texture_face(record: &RegistryRecord) -> BlockFace {
    if canonical_state_u32(&record.canonical_state, "upper_block_bit") == Some(1) {
        BlockFace::Up
    } else {
        BlockFace::Down
    }
}

pub(in crate::compiler) fn canonical_state_u32(state: &str, property: &str) -> Option<u32> {
    let document =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(state).ok()?;
    let value = document.get(property)?;
    value
        .as_object()
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

pub(in crate::compiler) fn canonical_state_str(state: &str, property: &str) -> Option<Box<str>> {
    let document =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(state).ok()?;
    let value = document.get(property)?;
    value
        .as_object()
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
        .as_str()
        .map(Into::into)
}

pub(in crate::compiler) fn aquatic_cross_faces(record: &RegistryRecord) -> Option<[BlockFace; 2]> {
    match record.name.as_ref() {
        "minecraft:seagrass" => {
            match canonical_state_str(&record.canonical_state, "sea_grass_type")?.as_ref() {
                "default" => Some([BlockFace::Up, BlockFace::Up]),
                "double_bot" => Some([BlockFace::Down, BlockFace::South]),
                "double_top" => Some([BlockFace::East, BlockFace::West]),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(in crate::compiler) fn cutout_model_tint_flags(name: &str) -> u32 {
    match name {
        "minecraft:short_grass"
        | "minecraft:tall_grass"
        | "minecraft:fern"
        | "minecraft:large_fern" => MATERIAL_FLAG_GRASS_TINT,
        "minecraft:vine" => MATERIAL_FLAG_FOLIAGE_TINT,
        _ => 0,
    }
}

pub(in crate::compiler) fn leaf_tint_flags(name: &str) -> u32 {
    match name {
        "minecraft:oak_leaves"
        | "minecraft:dark_oak_leaves"
        | "minecraft:jungle_leaves"
        | "minecraft:acacia_leaves"
        | "minecraft:mangrove_leaves" => MATERIAL_FLAG_FOLIAGE_TINT,
        "minecraft:birch_leaves" => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_BIRCH_FOLIAGE,
        "minecraft:spruce_leaves" => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_EVERGREEN_FOLIAGE,
        _ => 0,
    }
}

pub(in crate::compiler) fn record_has_deferred_material(
    pack: &PackSources,
    record: &RegistryRecord,
) -> bool {
    BlockFace::ALL.into_iter().any(|face| {
        let TextureKey { key, .. } = resolve_texture_key(&pack.blocks, record, face);
        let Some(key) = key else {
            return false;
        };
        let Some(path) = pack.terrain.get_for_record(&key, record) else {
            return false;
        };
        source_is_deferred(pack, record, &key, path)
    })
}

pub(in crate::compiler) fn source_is_deferred(
    pack: &PackSources,
    record: &RegistryRecord,
    key: &str,
    _path: &str,
) -> bool {
    record.name.as_ref() != "minecraft:grass_block" && pack.terrain.requires_tint(key)
}
