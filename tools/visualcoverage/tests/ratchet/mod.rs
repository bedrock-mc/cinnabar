use assets::{
    ANIMATION_FLAG_BLEND, Animation, BiomeRule, BlockFlags, BlockVisual, CompiledAssets,
    CompiledBiomeAssets, ContributorRole, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_WATER_TINT, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
    MODEL_TEMPLATE_FLAG_FENCE_WOOD, MODEL_TEMPLATE_FLAG_PANE, MODEL_TEMPLATE_FLAG_STAIR, Material,
    ModelFamily, ModelQuad, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, RegistryProvenance,
    RegistryRecord, RuntimeAssets, TINT_MAP_BYTES, TextureArray, TextureMip, TexturePage,
    TextureRef, TintSource, VisualKind, encode_blob, read_registry,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use visualcoverage::{
    AllowlistEntry, Baseline, Counts, CoverageError, GALLERY_INVENTORY_SCHEMA,
    GALLERY_PAGE_CAPACITY, RenderStream, StateIdentity, analyze_bytes, analyze_records,
    baseline_from_snapshot, deterministic_json, gallery_inventory_bytes, parse_baseline, ratchet,
    ratchet_protocol_1001, strict_bytes, strict_records, write_deterministic_json_atomic,
};

const STAINED_GLASS_REMOVALS: [(u32, &str); 16] = [
    (360, "minecraft:lime_stained_glass"),
    (2_052, "minecraft:light_gray_stained_glass"),
    (2_703, "minecraft:brown_stained_glass"),
    (3_972, "minecraft:purple_stained_glass"),
    (5_455, "minecraft:gray_stained_glass"),
    (6_552, "minecraft:green_stained_glass"),
    (6_811, "minecraft:pink_stained_glass"),
    (7_091, "minecraft:orange_stained_glass"),
    (8_485, "minecraft:white_stained_glass"),
    (9_070, "minecraft:red_stained_glass"),
    (10_393, "minecraft:blue_stained_glass"),
    (10_431, "minecraft:light_blue_stained_glass"),
    (11_571, "minecraft:cyan_stained_glass"),
    (11_572, "minecraft:black_stained_glass"),
    (14_572, "minecraft:magenta_stained_glass"),
    (15_165, "minecraft:yellow_stained_glass"),
];

const COPPER_GRATE_REMOVALS: [(u32, &str); 8] = [
    (1_963, "minecraft:copper_grate"),
    (2_219, "minecraft:waxed_exposed_copper_grate"),
    (5_474, "minecraft:weathered_copper_grate"),
    (6_812, "minecraft:waxed_weathered_copper_grate"),
    (9_255, "minecraft:exposed_copper_grate"),
    (9_256, "minecraft:waxed_copper_grate"),
    (13_058, "minecraft:oxidized_copper_grate"),
    (16_113, "minecraft:waxed_oxidized_copper_grate"),
];

const SELECTOR_ALIAS_CUBE_REMOVALS: [u32; 27] = [
    2_908, 2_909, 2_910, 2_912, 2_913, 2_914, 2_916, 2_917, 2_918, 5_443, 5_444, 6_466, 6_467,
    6_468, 6_470, 6_471, 6_472, 6_474, 6_475, 6_476, 7_082, 7_083, 13_113, 14_686, 14_687, 15_345,
    15_346,
];

mod baseline;
mod gallery;
mod production;
mod strict_graph;
mod strict_routes;
mod strict_writes;
mod support;
