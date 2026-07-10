//! Generated vanilla item definitions.
//! Do not edit: regenerate with valentine_gen.

use valentine_bedrock_core::item::{
    DurableItem, EnchantableItem, EnchantmentCategory, ItemDef, ItemDefDyn, ItemVariant,
    RepairableItem, VariantItem,
};

/// Air
pub struct Air;

impl ItemDef for Air {
    const ID: i32 = 0;
    const STRING_ID: &'static str = "minecraft:air";
    const NAME: &'static str = "Air";
    const STACK_SIZE: u8 = 64;
}

/// Stone
pub struct Stone;

impl ItemDef for Stone {
    const ID: i32 = 1;
    const STRING_ID: &'static str = "minecraft:stone";
    const NAME: &'static str = "Stone";
    const STACK_SIZE: u8 = 64;
}

/// Granite
pub struct Granite;

impl ItemDef for Granite {
    const ID: i32 = 2;
    const STRING_ID: &'static str = "minecraft:granite";
    const NAME: &'static str = "Granite";
    const STACK_SIZE: u8 = 64;
}

/// Polished Granite
pub struct PolishedGranite;

impl ItemDef for PolishedGranite {
    const ID: i32 = 3;
    const STRING_ID: &'static str = "minecraft:polished_granite";
    const NAME: &'static str = "Polished Granite";
    const STACK_SIZE: u8 = 64;
}

/// Diorite
pub struct Diorite;

impl ItemDef for Diorite {
    const ID: i32 = 4;
    const STRING_ID: &'static str = "minecraft:diorite";
    const NAME: &'static str = "Diorite";
    const STACK_SIZE: u8 = 64;
}

/// Polished Diorite
pub struct PolishedDiorite;

impl ItemDef for PolishedDiorite {
    const ID: i32 = 5;
    const STRING_ID: &'static str = "minecraft:polished_diorite";
    const NAME: &'static str = "Polished Diorite";
    const STACK_SIZE: u8 = 64;
}

/// Andesite
pub struct Andesite;

impl ItemDef for Andesite {
    const ID: i32 = 6;
    const STRING_ID: &'static str = "minecraft:andesite";
    const NAME: &'static str = "Andesite";
    const STACK_SIZE: u8 = 64;
}

/// Polished Andesite
pub struct PolishedAndesite;

impl ItemDef for PolishedAndesite {
    const ID: i32 = 7;
    const STRING_ID: &'static str = "minecraft:polished_andesite";
    const NAME: &'static str = "Polished Andesite";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate
pub struct Deepslate;

impl ItemDef for Deepslate {
    const ID: i32 = 8;
    const STRING_ID: &'static str = "minecraft:deepslate";
    const NAME: &'static str = "Deepslate";
    const STACK_SIZE: u8 = 64;
}

/// Cobbled Deepslate
pub struct CobbledDeepslate;

impl ItemDef for CobbledDeepslate {
    const ID: i32 = 9;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate";
    const NAME: &'static str = "Cobbled Deepslate";
    const STACK_SIZE: u8 = 64;
}

/// Polished Deepslate
pub struct PolishedDeepslate;

impl ItemDef for PolishedDeepslate {
    const ID: i32 = 10;
    const STRING_ID: &'static str = "minecraft:polished_deepslate";
    const NAME: &'static str = "Polished Deepslate";
    const STACK_SIZE: u8 = 64;
}

/// Calcite
pub struct Calcite;

impl ItemDef for Calcite {
    const ID: i32 = 11;
    const STRING_ID: &'static str = "minecraft:calcite";
    const NAME: &'static str = "Calcite";
    const STACK_SIZE: u8 = 64;
}

/// Tuff
pub struct Tuff;

impl ItemDef for Tuff {
    const ID: i32 = 12;
    const STRING_ID: &'static str = "minecraft:tuff";
    const NAME: &'static str = "Tuff";
    const STACK_SIZE: u8 = 64;
}

/// Tuff Slab
pub struct TuffSlab;

impl ItemDef for TuffSlab {
    const ID: i32 = 13;
    const STRING_ID: &'static str = "minecraft:tuff_slab";
    const NAME: &'static str = "Tuff Slab";
    const STACK_SIZE: u8 = 64;
}

/// Tuff Stairs
pub struct TuffStairs;

impl ItemDef for TuffStairs {
    const ID: i32 = 14;
    const STRING_ID: &'static str = "minecraft:tuff_stairs";
    const NAME: &'static str = "Tuff Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Tuff Wall
pub struct TuffWall;

impl ItemDef for TuffWall {
    const ID: i32 = 15;
    const STRING_ID: &'static str = "minecraft:tuff_wall";
    const NAME: &'static str = "Tuff Wall";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Tuff
pub struct ChiseledTuff;

impl ItemDef for ChiseledTuff {
    const ID: i32 = 16;
    const STRING_ID: &'static str = "minecraft:chiseled_tuff";
    const NAME: &'static str = "Chiseled Tuff";
    const STACK_SIZE: u8 = 64;
}

/// Polished Tuff
pub struct PolishedTuff;

impl ItemDef for PolishedTuff {
    const ID: i32 = 17;
    const STRING_ID: &'static str = "minecraft:polished_tuff";
    const NAME: &'static str = "Polished Tuff";
    const STACK_SIZE: u8 = 64;
}

/// Polished Tuff Slab
pub struct PolishedTuffSlab;

impl ItemDef for PolishedTuffSlab {
    const ID: i32 = 18;
    const STRING_ID: &'static str = "minecraft:polished_tuff_slab";
    const NAME: &'static str = "Polished Tuff Slab";
    const STACK_SIZE: u8 = 64;
}

/// Polished Tuff Stairs
pub struct PolishedTuffStairs;

impl ItemDef for PolishedTuffStairs {
    const ID: i32 = 19;
    const STRING_ID: &'static str = "minecraft:polished_tuff_stairs";
    const NAME: &'static str = "Polished Tuff Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Polished Tuff Wall
pub struct PolishedTuffWall;

impl ItemDef for PolishedTuffWall {
    const ID: i32 = 20;
    const STRING_ID: &'static str = "minecraft:polished_tuff_wall";
    const NAME: &'static str = "Polished Tuff Wall";
    const STACK_SIZE: u8 = 64;
}

/// Tuff Bricks
pub struct TuffBricks;

impl ItemDef for TuffBricks {
    const ID: i32 = 21;
    const STRING_ID: &'static str = "minecraft:tuff_bricks";
    const NAME: &'static str = "Tuff Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Tuff Brick Slab
pub struct TuffBrickSlab;

impl ItemDef for TuffBrickSlab {
    const ID: i32 = 22;
    const STRING_ID: &'static str = "minecraft:tuff_brick_slab";
    const NAME: &'static str = "Tuff Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Tuff Brick Stairs
pub struct TuffBrickStairs;

impl ItemDef for TuffBrickStairs {
    const ID: i32 = 23;
    const STRING_ID: &'static str = "minecraft:tuff_brick_stairs";
    const NAME: &'static str = "Tuff Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Tuff Brick Wall
pub struct TuffBrickWall;

impl ItemDef for TuffBrickWall {
    const ID: i32 = 24;
    const STRING_ID: &'static str = "minecraft:tuff_brick_wall";
    const NAME: &'static str = "Tuff Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Tuff Bricks
pub struct ChiseledTuffBricks;

impl ItemDef for ChiseledTuffBricks {
    const ID: i32 = 25;
    const STRING_ID: &'static str = "minecraft:chiseled_tuff_bricks";
    const NAME: &'static str = "Chiseled Tuff Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Dripstone Block
pub struct DripstoneBlock;

impl ItemDef for DripstoneBlock {
    const ID: i32 = 26;
    const STRING_ID: &'static str = "minecraft:dripstone_block";
    const NAME: &'static str = "Dripstone Block";
    const STACK_SIZE: u8 = 64;
}

/// Grass Block
pub struct GrassBlock;

impl ItemDef for GrassBlock {
    const ID: i32 = 27;
    const STRING_ID: &'static str = "minecraft:grass_block";
    const NAME: &'static str = "Grass Block";
    const STACK_SIZE: u8 = 64;
}

/// Dirt
pub struct Dirt;

impl ItemDef for Dirt {
    const ID: i32 = 28;
    const STRING_ID: &'static str = "minecraft:dirt";
    const NAME: &'static str = "Dirt";
    const STACK_SIZE: u8 = 64;
}

/// Coarse Dirt
pub struct CoarseDirt;

impl ItemDef for CoarseDirt {
    const ID: i32 = 29;
    const STRING_ID: &'static str = "minecraft:coarse_dirt";
    const NAME: &'static str = "Coarse Dirt";
    const STACK_SIZE: u8 = 64;
}

/// Podzol
pub struct Podzol;

impl ItemDef for Podzol {
    const ID: i32 = 30;
    const STRING_ID: &'static str = "minecraft:podzol";
    const NAME: &'static str = "Podzol";
    const STACK_SIZE: u8 = 64;
}

/// Rooted Dirt
pub struct DirtWithRoots;

impl ItemDef for DirtWithRoots {
    const ID: i32 = 31;
    const STRING_ID: &'static str = "minecraft:dirt_with_roots";
    const NAME: &'static str = "Rooted Dirt";
    const STACK_SIZE: u8 = 64;
}

/// Mud
pub struct Mud;

impl ItemDef for Mud {
    const ID: i32 = 32;
    const STRING_ID: &'static str = "minecraft:mud";
    const NAME: &'static str = "Mud";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Nylium
pub struct CrimsonNylium;

impl ItemDef for CrimsonNylium {
    const ID: i32 = 33;
    const STRING_ID: &'static str = "minecraft:crimson_nylium";
    const NAME: &'static str = "Crimson Nylium";
    const STACK_SIZE: u8 = 64;
}

/// Warped Nylium
pub struct WarpedNylium;

impl ItemDef for WarpedNylium {
    const ID: i32 = 34;
    const STRING_ID: &'static str = "minecraft:warped_nylium";
    const NAME: &'static str = "Warped Nylium";
    const STACK_SIZE: u8 = 64;
}

/// Cobblestone
pub struct Cobblestone;

impl ItemDef for Cobblestone {
    const ID: i32 = 35;
    const STRING_ID: &'static str = "minecraft:cobblestone";
    const NAME: &'static str = "Cobblestone";
    const STACK_SIZE: u8 = 64;
}

/// Oak Planks
pub struct OakPlanks;

impl ItemDef for OakPlanks {
    const ID: i32 = 36;
    const STRING_ID: &'static str = "minecraft:oak_planks";
    const NAME: &'static str = "Oak Planks";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Planks
pub struct SprucePlanks;

impl ItemDef for SprucePlanks {
    const ID: i32 = 37;
    const STRING_ID: &'static str = "minecraft:spruce_planks";
    const NAME: &'static str = "Spruce Planks";
    const STACK_SIZE: u8 = 64;
}

/// Birch Planks
pub struct BirchPlanks;

impl ItemDef for BirchPlanks {
    const ID: i32 = 38;
    const STRING_ID: &'static str = "minecraft:birch_planks";
    const NAME: &'static str = "Birch Planks";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Planks
pub struct JunglePlanks;

impl ItemDef for JunglePlanks {
    const ID: i32 = 39;
    const STRING_ID: &'static str = "minecraft:jungle_planks";
    const NAME: &'static str = "Jungle Planks";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Planks
pub struct AcaciaPlanks;

impl ItemDef for AcaciaPlanks {
    const ID: i32 = 40;
    const STRING_ID: &'static str = "minecraft:acacia_planks";
    const NAME: &'static str = "Acacia Planks";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Planks
pub struct CherryPlanks;

impl ItemDef for CherryPlanks {
    const ID: i32 = 41;
    const STRING_ID: &'static str = "minecraft:cherry_planks";
    const NAME: &'static str = "Cherry Planks";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Planks
pub struct DarkOakPlanks;

impl ItemDef for DarkOakPlanks {
    const ID: i32 = 42;
    const STRING_ID: &'static str = "minecraft:dark_oak_planks";
    const NAME: &'static str = "Dark Oak Planks";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Planks
pub struct PaleOakPlanks;

impl ItemDef for PaleOakPlanks {
    const ID: i32 = 43;
    const STRING_ID: &'static str = "minecraft:pale_oak_planks";
    const NAME: &'static str = "Pale Oak Planks";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Planks
pub struct MangrovePlanks;

impl ItemDef for MangrovePlanks {
    const ID: i32 = 44;
    const STRING_ID: &'static str = "minecraft:mangrove_planks";
    const NAME: &'static str = "Mangrove Planks";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Planks
pub struct BambooPlanks;

impl ItemDef for BambooPlanks {
    const ID: i32 = 45;
    const STRING_ID: &'static str = "minecraft:bamboo_planks";
    const NAME: &'static str = "Bamboo Planks";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Planks
pub struct CrimsonPlanks;

impl ItemDef for CrimsonPlanks {
    const ID: i32 = 46;
    const STRING_ID: &'static str = "minecraft:crimson_planks";
    const NAME: &'static str = "Crimson Planks";
    const STACK_SIZE: u8 = 64;
}

/// Warped Planks
pub struct WarpedPlanks;

impl ItemDef for WarpedPlanks {
    const ID: i32 = 47;
    const STRING_ID: &'static str = "minecraft:warped_planks";
    const NAME: &'static str = "Warped Planks";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Mosaic
pub struct BambooMosaic;

impl ItemDef for BambooMosaic {
    const ID: i32 = 48;
    const STRING_ID: &'static str = "minecraft:bamboo_mosaic";
    const NAME: &'static str = "Bamboo Mosaic";
    const STACK_SIZE: u8 = 64;
}

/// Oak Sapling
pub struct OakSapling;

impl ItemDef for OakSapling {
    const ID: i32 = 49;
    const STRING_ID: &'static str = "minecraft:oak_sapling";
    const NAME: &'static str = "Oak Sapling";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Sapling
pub struct SpruceSapling;

impl ItemDef for SpruceSapling {
    const ID: i32 = 50;
    const STRING_ID: &'static str = "minecraft:spruce_sapling";
    const NAME: &'static str = "Spruce Sapling";
    const STACK_SIZE: u8 = 64;
}

/// Birch Sapling
pub struct BirchSapling;

impl ItemDef for BirchSapling {
    const ID: i32 = 51;
    const STRING_ID: &'static str = "minecraft:birch_sapling";
    const NAME: &'static str = "Birch Sapling";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Sapling
pub struct JungleSapling;

impl ItemDef for JungleSapling {
    const ID: i32 = 52;
    const STRING_ID: &'static str = "minecraft:jungle_sapling";
    const NAME: &'static str = "Jungle Sapling";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Sapling
pub struct AcaciaSapling;

impl ItemDef for AcaciaSapling {
    const ID: i32 = 53;
    const STRING_ID: &'static str = "minecraft:acacia_sapling";
    const NAME: &'static str = "Acacia Sapling";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Sapling
pub struct CherrySapling;

impl ItemDef for CherrySapling {
    const ID: i32 = 54;
    const STRING_ID: &'static str = "minecraft:cherry_sapling";
    const NAME: &'static str = "Cherry Sapling";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Sapling
pub struct DarkOakSapling;

impl ItemDef for DarkOakSapling {
    const ID: i32 = 55;
    const STRING_ID: &'static str = "minecraft:dark_oak_sapling";
    const NAME: &'static str = "Dark Oak Sapling";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Sapling
pub struct PaleOakSapling;

impl ItemDef for PaleOakSapling {
    const ID: i32 = 56;
    const STRING_ID: &'static str = "minecraft:pale_oak_sapling";
    const NAME: &'static str = "Pale Oak Sapling";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Propagule
pub struct MangrovePropagule;

impl ItemDef for MangrovePropagule {
    const ID: i32 = 57;
    const STRING_ID: &'static str = "minecraft:mangrove_propagule";
    const NAME: &'static str = "Mangrove Propagule";
    const STACK_SIZE: u8 = 64;
}

/// Bedrock
pub struct Bedrock;

impl ItemDef for Bedrock {
    const ID: i32 = 58;
    const STRING_ID: &'static str = "minecraft:bedrock";
    const NAME: &'static str = "Bedrock";
    const STACK_SIZE: u8 = 64;
}

/// Sand
pub struct Sand;

impl ItemDef for Sand {
    const ID: i32 = 59;
    const STRING_ID: &'static str = "minecraft:sand";
    const NAME: &'static str = "Sand";
    const STACK_SIZE: u8 = 64;
}

/// Suspicious Sand
pub struct SuspiciousSand;

impl ItemDef for SuspiciousSand {
    const ID: i32 = 60;
    const STRING_ID: &'static str = "minecraft:suspicious_sand";
    const NAME: &'static str = "Suspicious Sand";
    const STACK_SIZE: u8 = 64;
}

/// Suspicious Gravel
pub struct SuspiciousGravel;

impl ItemDef for SuspiciousGravel {
    const ID: i32 = 61;
    const STRING_ID: &'static str = "minecraft:suspicious_gravel";
    const NAME: &'static str = "Suspicious Gravel";
    const STACK_SIZE: u8 = 64;
}

/// Red Sand
pub struct RedSand;

impl ItemDef for RedSand {
    const ID: i32 = 62;
    const STRING_ID: &'static str = "minecraft:red_sand";
    const NAME: &'static str = "Red Sand";
    const STACK_SIZE: u8 = 64;
}

/// Gravel
pub struct Gravel;

impl ItemDef for Gravel {
    const ID: i32 = 63;
    const STRING_ID: &'static str = "minecraft:gravel";
    const NAME: &'static str = "Gravel";
    const STACK_SIZE: u8 = 64;
}

/// Coal Ore
pub struct CoalOre;

impl ItemDef for CoalOre {
    const ID: i32 = 64;
    const STRING_ID: &'static str = "minecraft:coal_ore";
    const NAME: &'static str = "Coal Ore";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Coal Ore
pub struct DeepslateCoalOre;

impl ItemDef for DeepslateCoalOre {
    const ID: i32 = 65;
    const STRING_ID: &'static str = "minecraft:deepslate_coal_ore";
    const NAME: &'static str = "Deepslate Coal Ore";
    const STACK_SIZE: u8 = 64;
}

/// Iron Ore
pub struct IronOre;

impl ItemDef for IronOre {
    const ID: i32 = 66;
    const STRING_ID: &'static str = "minecraft:iron_ore";
    const NAME: &'static str = "Iron Ore";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Iron Ore
pub struct DeepslateIronOre;

impl ItemDef for DeepslateIronOre {
    const ID: i32 = 67;
    const STRING_ID: &'static str = "minecraft:deepslate_iron_ore";
    const NAME: &'static str = "Deepslate Iron Ore";
    const STACK_SIZE: u8 = 64;
}

/// Copper Ore
pub struct CopperOre;

impl ItemDef for CopperOre {
    const ID: i32 = 68;
    const STRING_ID: &'static str = "minecraft:copper_ore";
    const NAME: &'static str = "Copper Ore";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Copper Ore
pub struct DeepslateCopperOre;

impl ItemDef for DeepslateCopperOre {
    const ID: i32 = 69;
    const STRING_ID: &'static str = "minecraft:deepslate_copper_ore";
    const NAME: &'static str = "Deepslate Copper Ore";
    const STACK_SIZE: u8 = 64;
}

/// Gold Ore
pub struct GoldOre;

impl ItemDef for GoldOre {
    const ID: i32 = 70;
    const STRING_ID: &'static str = "minecraft:gold_ore";
    const NAME: &'static str = "Gold Ore";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Gold Ore
pub struct DeepslateGoldOre;

impl ItemDef for DeepslateGoldOre {
    const ID: i32 = 71;
    const STRING_ID: &'static str = "minecraft:deepslate_gold_ore";
    const NAME: &'static str = "Deepslate Gold Ore";
    const STACK_SIZE: u8 = 64;
}

/// Redstone Ore
pub struct RedstoneOre;

impl ItemDef for RedstoneOre {
    const ID: i32 = 72;
    const STRING_ID: &'static str = "minecraft:redstone_ore";
    const NAME: &'static str = "Redstone Ore";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Redstone Ore
pub struct DeepslateRedstoneOre;

impl ItemDef for DeepslateRedstoneOre {
    const ID: i32 = 73;
    const STRING_ID: &'static str = "minecraft:deepslate_redstone_ore";
    const NAME: &'static str = "Deepslate Redstone Ore";
    const STACK_SIZE: u8 = 64;
}

/// Emerald Ore
pub struct EmeraldOre;

impl ItemDef for EmeraldOre {
    const ID: i32 = 74;
    const STRING_ID: &'static str = "minecraft:emerald_ore";
    const NAME: &'static str = "Emerald Ore";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Emerald Ore
pub struct DeepslateEmeraldOre;

impl ItemDef for DeepslateEmeraldOre {
    const ID: i32 = 75;
    const STRING_ID: &'static str = "minecraft:deepslate_emerald_ore";
    const NAME: &'static str = "Deepslate Emerald Ore";
    const STACK_SIZE: u8 = 64;
}

/// Lapis Lazuli Ore
pub struct LapisOre;

impl ItemDef for LapisOre {
    const ID: i32 = 76;
    const STRING_ID: &'static str = "minecraft:lapis_ore";
    const NAME: &'static str = "Lapis Lazuli Ore";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Lapis Lazuli Ore
pub struct DeepslateLapisOre;

impl ItemDef for DeepslateLapisOre {
    const ID: i32 = 77;
    const STRING_ID: &'static str = "minecraft:deepslate_lapis_ore";
    const NAME: &'static str = "Deepslate Lapis Lazuli Ore";
    const STACK_SIZE: u8 = 64;
}

/// Diamond Ore
pub struct DiamondOre;

impl ItemDef for DiamondOre {
    const ID: i32 = 78;
    const STRING_ID: &'static str = "minecraft:diamond_ore";
    const NAME: &'static str = "Diamond Ore";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Diamond Ore
pub struct DeepslateDiamondOre;

impl ItemDef for DeepslateDiamondOre {
    const ID: i32 = 79;
    const STRING_ID: &'static str = "minecraft:deepslate_diamond_ore";
    const NAME: &'static str = "Deepslate Diamond Ore";
    const STACK_SIZE: u8 = 64;
}

/// Nether Gold Ore
pub struct NetherGoldOre;

impl ItemDef for NetherGoldOre {
    const ID: i32 = 80;
    const STRING_ID: &'static str = "minecraft:nether_gold_ore";
    const NAME: &'static str = "Nether Gold Ore";
    const STACK_SIZE: u8 = 64;
}

/// Nether Quartz Ore
pub struct QuartzOre;

impl ItemDef for QuartzOre {
    const ID: i32 = 81;
    const STRING_ID: &'static str = "minecraft:quartz_ore";
    const NAME: &'static str = "Nether Quartz Ore";
    const STACK_SIZE: u8 = 64;
}

/// Ancient Debris
pub struct AncientDebris;

impl ItemDef for AncientDebris {
    const ID: i32 = 82;
    const STRING_ID: &'static str = "minecraft:ancient_debris";
    const NAME: &'static str = "Ancient Debris";
    const STACK_SIZE: u8 = 64;
}

/// Block of Coal
pub struct CoalBlock;

impl ItemDef for CoalBlock {
    const ID: i32 = 83;
    const STRING_ID: &'static str = "minecraft:coal_block";
    const NAME: &'static str = "Block of Coal";
    const STACK_SIZE: u8 = 64;
}

/// Block of Raw Iron
pub struct RawIronBlock;

impl ItemDef for RawIronBlock {
    const ID: i32 = 84;
    const STRING_ID: &'static str = "minecraft:raw_iron_block";
    const NAME: &'static str = "Block of Raw Iron";
    const STACK_SIZE: u8 = 64;
}

/// Block of Raw Copper
pub struct RawCopperBlock;

impl ItemDef for RawCopperBlock {
    const ID: i32 = 85;
    const STRING_ID: &'static str = "minecraft:raw_copper_block";
    const NAME: &'static str = "Block of Raw Copper";
    const STACK_SIZE: u8 = 64;
}

/// Block of Raw Gold
pub struct RawGoldBlock;

impl ItemDef for RawGoldBlock {
    const ID: i32 = 86;
    const STRING_ID: &'static str = "minecraft:raw_gold_block";
    const NAME: &'static str = "Block of Raw Gold";
    const STACK_SIZE: u8 = 64;
}

/// Heavy Core
pub struct HeavyCore;

impl ItemDef for HeavyCore {
    const ID: i32 = 87;
    const STRING_ID: &'static str = "minecraft:heavy_core";
    const NAME: &'static str = "Heavy Core";
    const STACK_SIZE: u8 = 64;
}

/// Block of Amethyst
pub struct AmethystBlock;

impl ItemDef for AmethystBlock {
    const ID: i32 = 88;
    const STRING_ID: &'static str = "minecraft:amethyst_block";
    const NAME: &'static str = "Block of Amethyst";
    const STACK_SIZE: u8 = 64;
}

/// Budding Amethyst
pub struct BuddingAmethyst;

impl ItemDef for BuddingAmethyst {
    const ID: i32 = 89;
    const STRING_ID: &'static str = "minecraft:budding_amethyst";
    const NAME: &'static str = "Budding Amethyst";
    const STACK_SIZE: u8 = 64;
}

/// Block of Iron
pub struct IronBlock;

impl ItemDef for IronBlock {
    const ID: i32 = 90;
    const STRING_ID: &'static str = "minecraft:iron_block";
    const NAME: &'static str = "Block of Iron";
    const STACK_SIZE: u8 = 64;
}

/// Block of Copper
pub struct CopperBlock;

impl ItemDef for CopperBlock {
    const ID: i32 = 91;
    const STRING_ID: &'static str = "minecraft:copper_block";
    const NAME: &'static str = "Block of Copper";
    const STACK_SIZE: u8 = 64;
}

/// Block of Gold
pub struct GoldBlock;

impl ItemDef for GoldBlock {
    const ID: i32 = 92;
    const STRING_ID: &'static str = "minecraft:gold_block";
    const NAME: &'static str = "Block of Gold";
    const STACK_SIZE: u8 = 64;
}

/// Block of Diamond
pub struct DiamondBlock;

impl ItemDef for DiamondBlock {
    const ID: i32 = 93;
    const STRING_ID: &'static str = "minecraft:diamond_block";
    const NAME: &'static str = "Block of Diamond";
    const STACK_SIZE: u8 = 64;
}

/// Block of Netherite
pub struct NetheriteBlock;

impl ItemDef for NetheriteBlock {
    const ID: i32 = 94;
    const STRING_ID: &'static str = "minecraft:netherite_block";
    const NAME: &'static str = "Block of Netherite";
    const STACK_SIZE: u8 = 64;
}

/// Exposed Copper
pub struct ExposedCopper;

impl ItemDef for ExposedCopper {
    const ID: i32 = 95;
    const STRING_ID: &'static str = "minecraft:exposed_copper";
    const NAME: &'static str = "Exposed Copper";
    const STACK_SIZE: u8 = 64;
}

/// Weathered Copper
pub struct WeatheredCopper;

impl ItemDef for WeatheredCopper {
    const ID: i32 = 96;
    const STRING_ID: &'static str = "minecraft:weathered_copper";
    const NAME: &'static str = "Weathered Copper";
    const STACK_SIZE: u8 = 64;
}

/// Oxidized Copper
pub struct OxidizedCopper;

impl ItemDef for OxidizedCopper {
    const ID: i32 = 97;
    const STRING_ID: &'static str = "minecraft:oxidized_copper";
    const NAME: &'static str = "Oxidized Copper";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Copper
pub struct ChiseledCopper;

impl ItemDef for ChiseledCopper {
    const ID: i32 = 98;
    const STRING_ID: &'static str = "minecraft:chiseled_copper";
    const NAME: &'static str = "Chiseled Copper";
    const STACK_SIZE: u8 = 64;
}

/// Exposed Chiseled Copper
pub struct ExposedChiseledCopper;

impl ItemDef for ExposedChiseledCopper {
    const ID: i32 = 99;
    const STRING_ID: &'static str = "minecraft:exposed_chiseled_copper";
    const NAME: &'static str = "Exposed Chiseled Copper";
    const STACK_SIZE: u8 = 64;
}

/// Weathered Chiseled Copper
pub struct WeatheredChiseledCopper;

impl ItemDef for WeatheredChiseledCopper {
    const ID: i32 = 100;
    const STRING_ID: &'static str = "minecraft:weathered_chiseled_copper";
    const NAME: &'static str = "Weathered Chiseled Copper";
    const STACK_SIZE: u8 = 64;
}

/// Oxidized Chiseled Copper
pub struct OxidizedChiseledCopper;

impl ItemDef for OxidizedChiseledCopper {
    const ID: i32 = 101;
    const STRING_ID: &'static str = "minecraft:oxidized_chiseled_copper";
    const NAME: &'static str = "Oxidized Chiseled Copper";
    const STACK_SIZE: u8 = 64;
}

/// Cut Copper
pub struct CutCopper;

impl ItemDef for CutCopper {
    const ID: i32 = 102;
    const STRING_ID: &'static str = "minecraft:cut_copper";
    const NAME: &'static str = "Cut Copper";
    const STACK_SIZE: u8 = 64;
}

/// Exposed Cut Copper
pub struct ExposedCutCopper;

impl ItemDef for ExposedCutCopper {
    const ID: i32 = 103;
    const STRING_ID: &'static str = "minecraft:exposed_cut_copper";
    const NAME: &'static str = "Exposed Cut Copper";
    const STACK_SIZE: u8 = 64;
}

/// Weathered Cut Copper
pub struct WeatheredCutCopper;

impl ItemDef for WeatheredCutCopper {
    const ID: i32 = 104;
    const STRING_ID: &'static str = "minecraft:weathered_cut_copper";
    const NAME: &'static str = "Weathered Cut Copper";
    const STACK_SIZE: u8 = 64;
}

/// Oxidized Cut Copper
pub struct OxidizedCutCopper;

impl ItemDef for OxidizedCutCopper {
    const ID: i32 = 105;
    const STRING_ID: &'static str = "minecraft:oxidized_cut_copper";
    const NAME: &'static str = "Oxidized Cut Copper";
    const STACK_SIZE: u8 = 64;
}

/// Cut Copper Stairs
pub struct CutCopperStairs;

impl ItemDef for CutCopperStairs {
    const ID: i32 = 106;
    const STRING_ID: &'static str = "minecraft:cut_copper_stairs";
    const NAME: &'static str = "Cut Copper Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Exposed Cut Copper Stairs
pub struct ExposedCutCopperStairs;

impl ItemDef for ExposedCutCopperStairs {
    const ID: i32 = 107;
    const STRING_ID: &'static str = "minecraft:exposed_cut_copper_stairs";
    const NAME: &'static str = "Exposed Cut Copper Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Weathered Cut Copper Stairs
pub struct WeatheredCutCopperStairs;

impl ItemDef for WeatheredCutCopperStairs {
    const ID: i32 = 108;
    const STRING_ID: &'static str = "minecraft:weathered_cut_copper_stairs";
    const NAME: &'static str = "Weathered Cut Copper Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Oxidized Cut Copper Stairs
pub struct OxidizedCutCopperStairs;

impl ItemDef for OxidizedCutCopperStairs {
    const ID: i32 = 109;
    const STRING_ID: &'static str = "minecraft:oxidized_cut_copper_stairs";
    const NAME: &'static str = "Oxidized Cut Copper Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Cut Copper Slab
pub struct CutCopperSlab;

impl ItemDef for CutCopperSlab {
    const ID: i32 = 110;
    const STRING_ID: &'static str = "minecraft:cut_copper_slab";
    const NAME: &'static str = "Cut Copper Slab";
    const STACK_SIZE: u8 = 64;
}

/// Exposed Cut Copper Slab
pub struct ExposedCutCopperSlab;

impl ItemDef for ExposedCutCopperSlab {
    const ID: i32 = 111;
    const STRING_ID: &'static str = "minecraft:exposed_cut_copper_slab";
    const NAME: &'static str = "Exposed Cut Copper Slab";
    const STACK_SIZE: u8 = 64;
}

/// Weathered Cut Copper Slab
pub struct WeatheredCutCopperSlab;

impl ItemDef for WeatheredCutCopperSlab {
    const ID: i32 = 112;
    const STRING_ID: &'static str = "minecraft:weathered_cut_copper_slab";
    const NAME: &'static str = "Weathered Cut Copper Slab";
    const STACK_SIZE: u8 = 64;
}

/// Oxidized Cut Copper Slab
pub struct OxidizedCutCopperSlab;

impl ItemDef for OxidizedCutCopperSlab {
    const ID: i32 = 113;
    const STRING_ID: &'static str = "minecraft:oxidized_cut_copper_slab";
    const NAME: &'static str = "Oxidized Cut Copper Slab";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Block of Copper
pub struct WaxedCopper;

impl ItemDef for WaxedCopper {
    const ID: i32 = 114;
    const STRING_ID: &'static str = "minecraft:waxed_copper";
    const NAME: &'static str = "Waxed Block of Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Exposed Copper
pub struct WaxedExposedCopper;

impl ItemDef for WaxedExposedCopper {
    const ID: i32 = 115;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper";
    const NAME: &'static str = "Waxed Exposed Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Weathered Copper
pub struct WaxedWeatheredCopper;

impl ItemDef for WaxedWeatheredCopper {
    const ID: i32 = 116;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper";
    const NAME: &'static str = "Waxed Weathered Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Oxidized Copper
pub struct WaxedOxidizedCopper;

impl ItemDef for WaxedOxidizedCopper {
    const ID: i32 = 117;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper";
    const NAME: &'static str = "Waxed Oxidized Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Chiseled Copper
pub struct WaxedChiseledCopper;

impl ItemDef for WaxedChiseledCopper {
    const ID: i32 = 118;
    const STRING_ID: &'static str = "minecraft:waxed_chiseled_copper";
    const NAME: &'static str = "Waxed Chiseled Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Exposed Chiseled Copper
pub struct WaxedExposedChiseledCopper;

impl ItemDef for WaxedExposedChiseledCopper {
    const ID: i32 = 119;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_chiseled_copper";
    const NAME: &'static str = "Waxed Exposed Chiseled Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Weathered Chiseled Copper
pub struct WaxedWeatheredChiseledCopper;

impl ItemDef for WaxedWeatheredChiseledCopper {
    const ID: i32 = 120;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_chiseled_copper";
    const NAME: &'static str = "Waxed Weathered Chiseled Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Oxidized Chiseled Copper
pub struct WaxedOxidizedChiseledCopper;

impl ItemDef for WaxedOxidizedChiseledCopper {
    const ID: i32 = 121;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_chiseled_copper";
    const NAME: &'static str = "Waxed Oxidized Chiseled Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Cut Copper
pub struct WaxedCutCopper;

impl ItemDef for WaxedCutCopper {
    const ID: i32 = 122;
    const STRING_ID: &'static str = "minecraft:waxed_cut_copper";
    const NAME: &'static str = "Waxed Cut Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Exposed Cut Copper
pub struct WaxedExposedCutCopper;

impl ItemDef for WaxedExposedCutCopper {
    const ID: i32 = 123;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_cut_copper";
    const NAME: &'static str = "Waxed Exposed Cut Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Weathered Cut Copper
pub struct WaxedWeatheredCutCopper;

impl ItemDef for WaxedWeatheredCutCopper {
    const ID: i32 = 124;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_cut_copper";
    const NAME: &'static str = "Waxed Weathered Cut Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Oxidized Cut Copper
pub struct WaxedOxidizedCutCopper;

impl ItemDef for WaxedOxidizedCutCopper {
    const ID: i32 = 125;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_cut_copper";
    const NAME: &'static str = "Waxed Oxidized Cut Copper";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Cut Copper Stairs
pub struct WaxedCutCopperStairs;

impl ItemDef for WaxedCutCopperStairs {
    const ID: i32 = 126;
    const STRING_ID: &'static str = "minecraft:waxed_cut_copper_stairs";
    const NAME: &'static str = "Waxed Cut Copper Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Exposed Cut Copper Stairs
pub struct WaxedExposedCutCopperStairs;

impl ItemDef for WaxedExposedCutCopperStairs {
    const ID: i32 = 127;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_cut_copper_stairs";
    const NAME: &'static str = "Waxed Exposed Cut Copper Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Weathered Cut Copper Stairs
pub struct WaxedWeatheredCutCopperStairs;

impl ItemDef for WaxedWeatheredCutCopperStairs {
    const ID: i32 = 128;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_cut_copper_stairs";
    const NAME: &'static str = "Waxed Weathered Cut Copper Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Oxidized Cut Copper Stairs
pub struct WaxedOxidizedCutCopperStairs;

impl ItemDef for WaxedOxidizedCutCopperStairs {
    const ID: i32 = 129;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_cut_copper_stairs";
    const NAME: &'static str = "Waxed Oxidized Cut Copper Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Cut Copper Slab
pub struct WaxedCutCopperSlab;

impl ItemDef for WaxedCutCopperSlab {
    const ID: i32 = 130;
    const STRING_ID: &'static str = "minecraft:waxed_cut_copper_slab";
    const NAME: &'static str = "Waxed Cut Copper Slab";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Exposed Cut Copper Slab
pub struct WaxedExposedCutCopperSlab;

impl ItemDef for WaxedExposedCutCopperSlab {
    const ID: i32 = 131;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_cut_copper_slab";
    const NAME: &'static str = "Waxed Exposed Cut Copper Slab";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Weathered Cut Copper Slab
pub struct WaxedWeatheredCutCopperSlab;

impl ItemDef for WaxedWeatheredCutCopperSlab {
    const ID: i32 = 132;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_cut_copper_slab";
    const NAME: &'static str = "Waxed Weathered Cut Copper Slab";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Oxidized Cut Copper Slab
pub struct WaxedOxidizedCutCopperSlab;

impl ItemDef for WaxedOxidizedCutCopperSlab {
    const ID: i32 = 133;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_cut_copper_slab";
    const NAME: &'static str = "Waxed Oxidized Cut Copper Slab";
    const STACK_SIZE: u8 = 64;
}

/// Oak Log
pub struct OakLog;

impl ItemDef for OakLog {
    const ID: i32 = 134;
    const STRING_ID: &'static str = "minecraft:oak_log";
    const NAME: &'static str = "Oak Log";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Log
pub struct SpruceLog;

impl ItemDef for SpruceLog {
    const ID: i32 = 135;
    const STRING_ID: &'static str = "minecraft:spruce_log";
    const NAME: &'static str = "Spruce Log";
    const STACK_SIZE: u8 = 64;
}

/// Birch Log
pub struct BirchLog;

impl ItemDef for BirchLog {
    const ID: i32 = 136;
    const STRING_ID: &'static str = "minecraft:birch_log";
    const NAME: &'static str = "Birch Log";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Log
pub struct JungleLog;

impl ItemDef for JungleLog {
    const ID: i32 = 137;
    const STRING_ID: &'static str = "minecraft:jungle_log";
    const NAME: &'static str = "Jungle Log";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Log
pub struct AcaciaLog;

impl ItemDef for AcaciaLog {
    const ID: i32 = 138;
    const STRING_ID: &'static str = "minecraft:acacia_log";
    const NAME: &'static str = "Acacia Log";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Log
pub struct CherryLog;

impl ItemDef for CherryLog {
    const ID: i32 = 139;
    const STRING_ID: &'static str = "minecraft:cherry_log";
    const NAME: &'static str = "Cherry Log";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Log
pub struct PaleOakLog;

impl ItemDef for PaleOakLog {
    const ID: i32 = 140;
    const STRING_ID: &'static str = "minecraft:pale_oak_log";
    const NAME: &'static str = "Pale Oak Log";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Log
pub struct DarkOakLog;

impl ItemDef for DarkOakLog {
    const ID: i32 = 141;
    const STRING_ID: &'static str = "minecraft:dark_oak_log";
    const NAME: &'static str = "Dark Oak Log";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Log
pub struct MangroveLog;

impl ItemDef for MangroveLog {
    const ID: i32 = 142;
    const STRING_ID: &'static str = "minecraft:mangrove_log";
    const NAME: &'static str = "Mangrove Log";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Roots
pub struct MangroveRoots;

impl ItemDef for MangroveRoots {
    const ID: i32 = 143;
    const STRING_ID: &'static str = "minecraft:mangrove_roots";
    const NAME: &'static str = "Mangrove Roots";
    const STACK_SIZE: u8 = 64;
}

/// Muddy Mangrove Roots
pub struct MuddyMangroveRoots;

impl ItemDef for MuddyMangroveRoots {
    const ID: i32 = 144;
    const STRING_ID: &'static str = "minecraft:muddy_mangrove_roots";
    const NAME: &'static str = "Muddy Mangrove Roots";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Stem
pub struct CrimsonStem;

impl ItemDef for CrimsonStem {
    const ID: i32 = 145;
    const STRING_ID: &'static str = "minecraft:crimson_stem";
    const NAME: &'static str = "Crimson Stem";
    const STACK_SIZE: u8 = 64;
}

/// Warped Stem
pub struct WarpedStem;

impl ItemDef for WarpedStem {
    const ID: i32 = 146;
    const STRING_ID: &'static str = "minecraft:warped_stem";
    const NAME: &'static str = "Warped Stem";
    const STACK_SIZE: u8 = 64;
}

/// Block of Bamboo
pub struct BambooBlock;

impl ItemDef for BambooBlock {
    const ID: i32 = 147;
    const STRING_ID: &'static str = "minecraft:bamboo_block";
    const NAME: &'static str = "Block of Bamboo";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Oak Log
pub struct StrippedOakLog;

impl ItemDef for StrippedOakLog {
    const ID: i32 = 148;
    const STRING_ID: &'static str = "minecraft:stripped_oak_log";
    const NAME: &'static str = "Stripped Oak Log";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Spruce Log
pub struct StrippedSpruceLog;

impl ItemDef for StrippedSpruceLog {
    const ID: i32 = 149;
    const STRING_ID: &'static str = "minecraft:stripped_spruce_log";
    const NAME: &'static str = "Stripped Spruce Log";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Birch Log
pub struct StrippedBirchLog;

impl ItemDef for StrippedBirchLog {
    const ID: i32 = 150;
    const STRING_ID: &'static str = "minecraft:stripped_birch_log";
    const NAME: &'static str = "Stripped Birch Log";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Jungle Log
pub struct StrippedJungleLog;

impl ItemDef for StrippedJungleLog {
    const ID: i32 = 151;
    const STRING_ID: &'static str = "minecraft:stripped_jungle_log";
    const NAME: &'static str = "Stripped Jungle Log";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Acacia Log
pub struct StrippedAcaciaLog;

impl ItemDef for StrippedAcaciaLog {
    const ID: i32 = 152;
    const STRING_ID: &'static str = "minecraft:stripped_acacia_log";
    const NAME: &'static str = "Stripped Acacia Log";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Cherry Log
pub struct StrippedCherryLog;

impl ItemDef for StrippedCherryLog {
    const ID: i32 = 153;
    const STRING_ID: &'static str = "minecraft:stripped_cherry_log";
    const NAME: &'static str = "Stripped Cherry Log";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Dark Oak Log
pub struct StrippedDarkOakLog;

impl ItemDef for StrippedDarkOakLog {
    const ID: i32 = 154;
    const STRING_ID: &'static str = "minecraft:stripped_dark_oak_log";
    const NAME: &'static str = "Stripped Dark Oak Log";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Pale Oak Log
pub struct StrippedPaleOakLog;

impl ItemDef for StrippedPaleOakLog {
    const ID: i32 = 155;
    const STRING_ID: &'static str = "minecraft:stripped_pale_oak_log";
    const NAME: &'static str = "Stripped Pale Oak Log";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Mangrove Log
pub struct StrippedMangroveLog;

impl ItemDef for StrippedMangroveLog {
    const ID: i32 = 156;
    const STRING_ID: &'static str = "minecraft:stripped_mangrove_log";
    const NAME: &'static str = "Stripped Mangrove Log";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Crimson Stem
pub struct StrippedCrimsonStem;

impl ItemDef for StrippedCrimsonStem {
    const ID: i32 = 157;
    const STRING_ID: &'static str = "minecraft:stripped_crimson_stem";
    const NAME: &'static str = "Stripped Crimson Stem";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Warped Stem
pub struct StrippedWarpedStem;

impl ItemDef for StrippedWarpedStem {
    const ID: i32 = 158;
    const STRING_ID: &'static str = "minecraft:stripped_warped_stem";
    const NAME: &'static str = "Stripped Warped Stem";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Oak Wood
pub struct StrippedOakWood;

impl ItemDef for StrippedOakWood {
    const ID: i32 = 159;
    const STRING_ID: &'static str = "minecraft:stripped_oak_wood";
    const NAME: &'static str = "Stripped Oak Wood";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Spruce Wood
pub struct StrippedSpruceWood;

impl ItemDef for StrippedSpruceWood {
    const ID: i32 = 160;
    const STRING_ID: &'static str = "minecraft:stripped_spruce_wood";
    const NAME: &'static str = "Stripped Spruce Wood";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Birch Wood
pub struct StrippedBirchWood;

impl ItemDef for StrippedBirchWood {
    const ID: i32 = 161;
    const STRING_ID: &'static str = "minecraft:stripped_birch_wood";
    const NAME: &'static str = "Stripped Birch Wood";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Jungle Wood
pub struct StrippedJungleWood;

impl ItemDef for StrippedJungleWood {
    const ID: i32 = 162;
    const STRING_ID: &'static str = "minecraft:stripped_jungle_wood";
    const NAME: &'static str = "Stripped Jungle Wood";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Acacia Wood
pub struct StrippedAcaciaWood;

impl ItemDef for StrippedAcaciaWood {
    const ID: i32 = 163;
    const STRING_ID: &'static str = "minecraft:stripped_acacia_wood";
    const NAME: &'static str = "Stripped Acacia Wood";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Cherry Wood
pub struct StrippedCherryWood;

impl ItemDef for StrippedCherryWood {
    const ID: i32 = 164;
    const STRING_ID: &'static str = "minecraft:stripped_cherry_wood";
    const NAME: &'static str = "Stripped Cherry Wood";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Dark Oak Wood
pub struct StrippedDarkOakWood;

impl ItemDef for StrippedDarkOakWood {
    const ID: i32 = 165;
    const STRING_ID: &'static str = "minecraft:stripped_dark_oak_wood";
    const NAME: &'static str = "Stripped Dark Oak Wood";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Pale Oak Wood
pub struct StrippedPaleOakWood;

impl ItemDef for StrippedPaleOakWood {
    const ID: i32 = 166;
    const STRING_ID: &'static str = "minecraft:stripped_pale_oak_wood";
    const NAME: &'static str = "Stripped Pale Oak Wood";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Mangrove Wood
pub struct StrippedMangroveWood;

impl ItemDef for StrippedMangroveWood {
    const ID: i32 = 167;
    const STRING_ID: &'static str = "minecraft:stripped_mangrove_wood";
    const NAME: &'static str = "Stripped Mangrove Wood";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Crimson Hyphae
pub struct StrippedCrimsonHyphae;

impl ItemDef for StrippedCrimsonHyphae {
    const ID: i32 = 168;
    const STRING_ID: &'static str = "minecraft:stripped_crimson_hyphae";
    const NAME: &'static str = "Stripped Crimson Hyphae";
    const STACK_SIZE: u8 = 64;
}

/// Stripped Warped Hyphae
pub struct StrippedWarpedHyphae;

impl ItemDef for StrippedWarpedHyphae {
    const ID: i32 = 169;
    const STRING_ID: &'static str = "minecraft:stripped_warped_hyphae";
    const NAME: &'static str = "Stripped Warped Hyphae";
    const STACK_SIZE: u8 = 64;
}

/// Block of Stripped Bamboo
pub struct StrippedBambooBlock;

impl ItemDef for StrippedBambooBlock {
    const ID: i32 = 170;
    const STRING_ID: &'static str = "minecraft:stripped_bamboo_block";
    const NAME: &'static str = "Block of Stripped Bamboo";
    const STACK_SIZE: u8 = 64;
}

/// Oak Wood
pub struct OakWood;

impl ItemDef for OakWood {
    const ID: i32 = 171;
    const STRING_ID: &'static str = "minecraft:oak_wood";
    const NAME: &'static str = "Oak Wood";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Wood
pub struct SpruceWood;

impl ItemDef for SpruceWood {
    const ID: i32 = 172;
    const STRING_ID: &'static str = "minecraft:spruce_wood";
    const NAME: &'static str = "Spruce Wood";
    const STACK_SIZE: u8 = 64;
}

/// Birch Wood
pub struct BirchWood;

impl ItemDef for BirchWood {
    const ID: i32 = 173;
    const STRING_ID: &'static str = "minecraft:birch_wood";
    const NAME: &'static str = "Birch Wood";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Wood
pub struct JungleWood;

impl ItemDef for JungleWood {
    const ID: i32 = 174;
    const STRING_ID: &'static str = "minecraft:jungle_wood";
    const NAME: &'static str = "Jungle Wood";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Wood
pub struct AcaciaWood;

impl ItemDef for AcaciaWood {
    const ID: i32 = 175;
    const STRING_ID: &'static str = "minecraft:acacia_wood";
    const NAME: &'static str = "Acacia Wood";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Wood
pub struct CherryWood;

impl ItemDef for CherryWood {
    const ID: i32 = 176;
    const STRING_ID: &'static str = "minecraft:cherry_wood";
    const NAME: &'static str = "Cherry Wood";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Wood
pub struct PaleOakWood;

impl ItemDef for PaleOakWood {
    const ID: i32 = 177;
    const STRING_ID: &'static str = "minecraft:pale_oak_wood";
    const NAME: &'static str = "Pale Oak Wood";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Wood
pub struct DarkOakWood;

impl ItemDef for DarkOakWood {
    const ID: i32 = 178;
    const STRING_ID: &'static str = "minecraft:dark_oak_wood";
    const NAME: &'static str = "Dark Oak Wood";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Wood
pub struct MangroveWood;

impl ItemDef for MangroveWood {
    const ID: i32 = 179;
    const STRING_ID: &'static str = "minecraft:mangrove_wood";
    const NAME: &'static str = "Mangrove Wood";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Hyphae
pub struct CrimsonHyphae;

impl ItemDef for CrimsonHyphae {
    const ID: i32 = 180;
    const STRING_ID: &'static str = "minecraft:crimson_hyphae";
    const NAME: &'static str = "Crimson Hyphae";
    const STACK_SIZE: u8 = 64;
}

/// Warped Hyphae
pub struct WarpedHyphae;

impl ItemDef for WarpedHyphae {
    const ID: i32 = 181;
    const STRING_ID: &'static str = "minecraft:warped_hyphae";
    const NAME: &'static str = "Warped Hyphae";
    const STACK_SIZE: u8 = 64;
}

/// Oak Leaves
pub struct OakLeaves;

impl ItemDef for OakLeaves {
    const ID: i32 = 182;
    const STRING_ID: &'static str = "minecraft:oak_leaves";
    const NAME: &'static str = "Oak Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Leaves
pub struct SpruceLeaves;

impl ItemDef for SpruceLeaves {
    const ID: i32 = 183;
    const STRING_ID: &'static str = "minecraft:spruce_leaves";
    const NAME: &'static str = "Spruce Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Birch Leaves
pub struct BirchLeaves;

impl ItemDef for BirchLeaves {
    const ID: i32 = 184;
    const STRING_ID: &'static str = "minecraft:birch_leaves";
    const NAME: &'static str = "Birch Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Leaves
pub struct JungleLeaves;

impl ItemDef for JungleLeaves {
    const ID: i32 = 185;
    const STRING_ID: &'static str = "minecraft:jungle_leaves";
    const NAME: &'static str = "Jungle Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Leaves
pub struct AcaciaLeaves;

impl ItemDef for AcaciaLeaves {
    const ID: i32 = 186;
    const STRING_ID: &'static str = "minecraft:acacia_leaves";
    const NAME: &'static str = "Acacia Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Leaves
pub struct CherryLeaves;

impl ItemDef for CherryLeaves {
    const ID: i32 = 187;
    const STRING_ID: &'static str = "minecraft:cherry_leaves";
    const NAME: &'static str = "Cherry Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Leaves
pub struct DarkOakLeaves;

impl ItemDef for DarkOakLeaves {
    const ID: i32 = 188;
    const STRING_ID: &'static str = "minecraft:dark_oak_leaves";
    const NAME: &'static str = "Dark Oak Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Leaves
pub struct PaleOakLeaves;

impl ItemDef for PaleOakLeaves {
    const ID: i32 = 189;
    const STRING_ID: &'static str = "minecraft:pale_oak_leaves";
    const NAME: &'static str = "Pale Oak Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Leaves
pub struct MangroveLeaves;

impl ItemDef for MangroveLeaves {
    const ID: i32 = 190;
    const STRING_ID: &'static str = "minecraft:mangrove_leaves";
    const NAME: &'static str = "Mangrove Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Azalea Leaves
pub struct AzaleaLeaves;

impl ItemDef for AzaleaLeaves {
    const ID: i32 = 191;
    const STRING_ID: &'static str = "minecraft:azalea_leaves";
    const NAME: &'static str = "Azalea Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Flowering Azalea Leaves
pub struct AzaleaLeavesFlowered;

impl ItemDef for AzaleaLeavesFlowered {
    const ID: i32 = 192;
    const STRING_ID: &'static str = "minecraft:azalea_leaves_flowered";
    const NAME: &'static str = "Flowering Azalea Leaves";
    const STACK_SIZE: u8 = 64;
}

/// Sponge
pub struct Sponge;

impl ItemDef for Sponge {
    const ID: i32 = 193;
    const STRING_ID: &'static str = "minecraft:sponge";
    const NAME: &'static str = "Sponge";
    const STACK_SIZE: u8 = 64;
}

/// Wet Sponge
pub struct WetSponge;

impl ItemDef for WetSponge {
    const ID: i32 = 194;
    const STRING_ID: &'static str = "minecraft:wet_sponge";
    const NAME: &'static str = "Wet Sponge";
    const STACK_SIZE: u8 = 64;
}

/// Glass
pub struct Glass;

impl ItemDef for Glass {
    const ID: i32 = 195;
    const STRING_ID: &'static str = "minecraft:glass";
    const NAME: &'static str = "Glass";
    const STACK_SIZE: u8 = 64;
}

/// Tinted Glass
pub struct TintedGlass;

impl ItemDef for TintedGlass {
    const ID: i32 = 196;
    const STRING_ID: &'static str = "minecraft:tinted_glass";
    const NAME: &'static str = "Tinted Glass";
    const STACK_SIZE: u8 = 64;
}

/// Block of Lapis Lazuli
pub struct LapisBlock;

impl ItemDef for LapisBlock {
    const ID: i32 = 197;
    const STRING_ID: &'static str = "minecraft:lapis_block";
    const NAME: &'static str = "Block of Lapis Lazuli";
    const STACK_SIZE: u8 = 64;
}

/// Sandstone
pub struct Sandstone;

impl ItemDef for Sandstone {
    const ID: i32 = 198;
    const STRING_ID: &'static str = "minecraft:sandstone";
    const NAME: &'static str = "Sandstone";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Sandstone
pub struct ChiseledSandstone;

impl ItemDef for ChiseledSandstone {
    const ID: i32 = 199;
    const STRING_ID: &'static str = "minecraft:chiseled_sandstone";
    const NAME: &'static str = "Chiseled Sandstone";
    const STACK_SIZE: u8 = 64;
}

/// Cut Sandstone
pub struct CutSandstone;

impl ItemDef for CutSandstone {
    const ID: i32 = 200;
    const STRING_ID: &'static str = "minecraft:cut_sandstone";
    const NAME: &'static str = "Cut Sandstone";
    const STACK_SIZE: u8 = 64;
}

/// Cobweb
pub struct Web;

impl ItemDef for Web {
    const ID: i32 = 201;
    const STRING_ID: &'static str = "minecraft:web";
    const NAME: &'static str = "Cobweb";
    const STACK_SIZE: u8 = 64;
}

/// Short Grass
pub struct ShortGrass;

impl ItemDef for ShortGrass {
    const ID: i32 = 202;
    const STRING_ID: &'static str = "minecraft:short_grass";
    const NAME: &'static str = "Short Grass";
    const STACK_SIZE: u8 = 64;
}

/// Fern
pub struct Fern;

impl ItemDef for Fern {
    const ID: i32 = 203;
    const STRING_ID: &'static str = "minecraft:fern";
    const NAME: &'static str = "Fern";
    const STACK_SIZE: u8 = 64;
}

/// Bush
pub struct Bush;

impl ItemDef for Bush {
    const ID: i32 = 204;
    const STRING_ID: &'static str = "minecraft:bush";
    const NAME: &'static str = "Bush";
    const STACK_SIZE: u8 = 64;
}

/// Azalea
pub struct Azalea;

impl ItemDef for Azalea {
    const ID: i32 = 205;
    const STRING_ID: &'static str = "minecraft:azalea";
    const NAME: &'static str = "Azalea";
    const STACK_SIZE: u8 = 64;
}

/// Flowering Azalea
pub struct FloweringAzalea;

impl ItemDef for FloweringAzalea {
    const ID: i32 = 206;
    const STRING_ID: &'static str = "minecraft:flowering_azalea";
    const NAME: &'static str = "Flowering Azalea";
    const STACK_SIZE: u8 = 64;
}

/// Dead Bush
pub struct Deadbush;

impl ItemDef for Deadbush {
    const ID: i32 = 207;
    const STRING_ID: &'static str = "minecraft:deadbush";
    const NAME: &'static str = "Dead Bush";
    const STACK_SIZE: u8 = 64;
}

/// Firefly Bush
pub struct FireflyBush;

impl ItemDef for FireflyBush {
    const ID: i32 = 208;
    const STRING_ID: &'static str = "minecraft:firefly_bush";
    const NAME: &'static str = "Firefly Bush";
    const STACK_SIZE: u8 = 64;
}

/// Short Dry Grass
pub struct ShortDryGrass;

impl ItemDef for ShortDryGrass {
    const ID: i32 = 209;
    const STRING_ID: &'static str = "minecraft:short_dry_grass";
    const NAME: &'static str = "Short Dry Grass";
    const STACK_SIZE: u8 = 64;
}

/// Tall Dry Grass
pub struct TallDryGrass;

impl ItemDef for TallDryGrass {
    const ID: i32 = 210;
    const STRING_ID: &'static str = "minecraft:tall_dry_grass";
    const NAME: &'static str = "Tall Dry Grass";
    const STACK_SIZE: u8 = 64;
}

/// Seagrass
pub struct Seagrass;

impl ItemDef for Seagrass {
    const ID: i32 = 211;
    const STRING_ID: &'static str = "minecraft:seagrass";
    const NAME: &'static str = "Seagrass";
    const STACK_SIZE: u8 = 64;
}

/// Sea Pickle
pub struct SeaPickle;

impl ItemDef for SeaPickle {
    const ID: i32 = 212;
    const STRING_ID: &'static str = "minecraft:sea_pickle";
    const NAME: &'static str = "Sea Pickle";
    const STACK_SIZE: u8 = 64;
}

/// White Wool
pub struct WhiteWool;

impl ItemDef for WhiteWool {
    const ID: i32 = 213;
    const STRING_ID: &'static str = "minecraft:white_wool";
    const NAME: &'static str = "White Wool";
    const STACK_SIZE: u8 = 64;
}

/// Orange Wool
pub struct OrangeWool;

impl ItemDef for OrangeWool {
    const ID: i32 = 214;
    const STRING_ID: &'static str = "minecraft:orange_wool";
    const NAME: &'static str = "Orange Wool";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Wool
pub struct MagentaWool;

impl ItemDef for MagentaWool {
    const ID: i32 = 215;
    const STRING_ID: &'static str = "minecraft:magenta_wool";
    const NAME: &'static str = "Magenta Wool";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Wool
pub struct LightBlueWool;

impl ItemDef for LightBlueWool {
    const ID: i32 = 216;
    const STRING_ID: &'static str = "minecraft:light_blue_wool";
    const NAME: &'static str = "Light Blue Wool";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Wool
pub struct YellowWool;

impl ItemDef for YellowWool {
    const ID: i32 = 217;
    const STRING_ID: &'static str = "minecraft:yellow_wool";
    const NAME: &'static str = "Yellow Wool";
    const STACK_SIZE: u8 = 64;
}

/// Lime Wool
pub struct LimeWool;

impl ItemDef for LimeWool {
    const ID: i32 = 218;
    const STRING_ID: &'static str = "minecraft:lime_wool";
    const NAME: &'static str = "Lime Wool";
    const STACK_SIZE: u8 = 64;
}

/// Pink Wool
pub struct PinkWool;

impl ItemDef for PinkWool {
    const ID: i32 = 219;
    const STRING_ID: &'static str = "minecraft:pink_wool";
    const NAME: &'static str = "Pink Wool";
    const STACK_SIZE: u8 = 64;
}

/// Gray Wool
pub struct GrayWool;

impl ItemDef for GrayWool {
    const ID: i32 = 220;
    const STRING_ID: &'static str = "minecraft:gray_wool";
    const NAME: &'static str = "Gray Wool";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Wool
pub struct LightGrayWool;

impl ItemDef for LightGrayWool {
    const ID: i32 = 221;
    const STRING_ID: &'static str = "minecraft:light_gray_wool";
    const NAME: &'static str = "Light Gray Wool";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Wool
pub struct CyanWool;

impl ItemDef for CyanWool {
    const ID: i32 = 222;
    const STRING_ID: &'static str = "minecraft:cyan_wool";
    const NAME: &'static str = "Cyan Wool";
    const STACK_SIZE: u8 = 64;
}

/// Purple Wool
pub struct PurpleWool;

impl ItemDef for PurpleWool {
    const ID: i32 = 223;
    const STRING_ID: &'static str = "minecraft:purple_wool";
    const NAME: &'static str = "Purple Wool";
    const STACK_SIZE: u8 = 64;
}

/// Blue Wool
pub struct BlueWool;

impl ItemDef for BlueWool {
    const ID: i32 = 224;
    const STRING_ID: &'static str = "minecraft:blue_wool";
    const NAME: &'static str = "Blue Wool";
    const STACK_SIZE: u8 = 64;
}

/// Brown Wool
pub struct BrownWool;

impl ItemDef for BrownWool {
    const ID: i32 = 225;
    const STRING_ID: &'static str = "minecraft:brown_wool";
    const NAME: &'static str = "Brown Wool";
    const STACK_SIZE: u8 = 64;
}

/// Green Wool
pub struct GreenWool;

impl ItemDef for GreenWool {
    const ID: i32 = 226;
    const STRING_ID: &'static str = "minecraft:green_wool";
    const NAME: &'static str = "Green Wool";
    const STACK_SIZE: u8 = 64;
}

/// Red Wool
pub struct RedWool;

impl ItemDef for RedWool {
    const ID: i32 = 227;
    const STRING_ID: &'static str = "minecraft:red_wool";
    const NAME: &'static str = "Red Wool";
    const STACK_SIZE: u8 = 64;
}

/// Black Wool
pub struct BlackWool;

impl ItemDef for BlackWool {
    const ID: i32 = 228;
    const STRING_ID: &'static str = "minecraft:black_wool";
    const NAME: &'static str = "Black Wool";
    const STACK_SIZE: u8 = 64;
}

/// Dandelion
pub struct Dandelion;

impl ItemDef for Dandelion {
    const ID: i32 = 229;
    const STRING_ID: &'static str = "minecraft:dandelion";
    const NAME: &'static str = "Dandelion";
    const STACK_SIZE: u8 = 64;
}

/// Open Eyeblossom
pub struct OpenEyeblossom;

impl ItemDef for OpenEyeblossom {
    const ID: i32 = 230;
    const STRING_ID: &'static str = "minecraft:open_eyeblossom";
    const NAME: &'static str = "Open Eyeblossom";
    const STACK_SIZE: u8 = 64;
}

/// Closed Eyeblossom
pub struct ClosedEyeblossom;

impl ItemDef for ClosedEyeblossom {
    const ID: i32 = 231;
    const STRING_ID: &'static str = "minecraft:closed_eyeblossom";
    const NAME: &'static str = "Closed Eyeblossom";
    const STACK_SIZE: u8 = 64;
}

/// Poppy
pub struct Poppy;

impl ItemDef for Poppy {
    const ID: i32 = 232;
    const STRING_ID: &'static str = "minecraft:poppy";
    const NAME: &'static str = "Poppy";
    const STACK_SIZE: u8 = 64;
}

/// Blue Orchid
pub struct BlueOrchid;

impl ItemDef for BlueOrchid {
    const ID: i32 = 233;
    const STRING_ID: &'static str = "minecraft:blue_orchid";
    const NAME: &'static str = "Blue Orchid";
    const STACK_SIZE: u8 = 64;
}

/// Allium
pub struct Allium;

impl ItemDef for Allium {
    const ID: i32 = 234;
    const STRING_ID: &'static str = "minecraft:allium";
    const NAME: &'static str = "Allium";
    const STACK_SIZE: u8 = 64;
}

/// Azure Bluet
pub struct AzureBluet;

impl ItemDef for AzureBluet {
    const ID: i32 = 235;
    const STRING_ID: &'static str = "minecraft:azure_bluet";
    const NAME: &'static str = "Azure Bluet";
    const STACK_SIZE: u8 = 64;
}

/// Red Tulip
pub struct RedTulip;

impl ItemDef for RedTulip {
    const ID: i32 = 236;
    const STRING_ID: &'static str = "minecraft:red_tulip";
    const NAME: &'static str = "Red Tulip";
    const STACK_SIZE: u8 = 64;
}

/// Orange Tulip
pub struct OrangeTulip;

impl ItemDef for OrangeTulip {
    const ID: i32 = 237;
    const STRING_ID: &'static str = "minecraft:orange_tulip";
    const NAME: &'static str = "Orange Tulip";
    const STACK_SIZE: u8 = 64;
}

/// White Tulip
pub struct WhiteTulip;

impl ItemDef for WhiteTulip {
    const ID: i32 = 238;
    const STRING_ID: &'static str = "minecraft:white_tulip";
    const NAME: &'static str = "White Tulip";
    const STACK_SIZE: u8 = 64;
}

/// Pink Tulip
pub struct PinkTulip;

impl ItemDef for PinkTulip {
    const ID: i32 = 239;
    const STRING_ID: &'static str = "minecraft:pink_tulip";
    const NAME: &'static str = "Pink Tulip";
    const STACK_SIZE: u8 = 64;
}

/// Oxeye Daisy
pub struct OxeyeDaisy;

impl ItemDef for OxeyeDaisy {
    const ID: i32 = 240;
    const STRING_ID: &'static str = "minecraft:oxeye_daisy";
    const NAME: &'static str = "Oxeye Daisy";
    const STACK_SIZE: u8 = 64;
}

/// Cornflower
pub struct Cornflower;

impl ItemDef for Cornflower {
    const ID: i32 = 241;
    const STRING_ID: &'static str = "minecraft:cornflower";
    const NAME: &'static str = "Cornflower";
    const STACK_SIZE: u8 = 64;
}

/// Lily of the Valley
pub struct LilyOfTheValley;

impl ItemDef for LilyOfTheValley {
    const ID: i32 = 242;
    const STRING_ID: &'static str = "minecraft:lily_of_the_valley";
    const NAME: &'static str = "Lily of the Valley";
    const STACK_SIZE: u8 = 64;
}

/// Wither Rose
pub struct WitherRose;

impl ItemDef for WitherRose {
    const ID: i32 = 243;
    const STRING_ID: &'static str = "minecraft:wither_rose";
    const NAME: &'static str = "Wither Rose";
    const STACK_SIZE: u8 = 64;
}

/// Torchflower
pub struct Torchflower;

impl ItemDef for Torchflower {
    const ID: i32 = 244;
    const STRING_ID: &'static str = "minecraft:torchflower";
    const NAME: &'static str = "Torchflower";
    const STACK_SIZE: u8 = 64;
}

/// Pitcher Plant
pub struct PitcherPlant;

impl ItemDef for PitcherPlant {
    const ID: i32 = 245;
    const STRING_ID: &'static str = "minecraft:pitcher_plant";
    const NAME: &'static str = "Pitcher Plant";
    const STACK_SIZE: u8 = 64;
}

/// Spore Blossom
pub struct SporeBlossom;

impl ItemDef for SporeBlossom {
    const ID: i32 = 246;
    const STRING_ID: &'static str = "minecraft:spore_blossom";
    const NAME: &'static str = "Spore Blossom";
    const STACK_SIZE: u8 = 64;
}

/// Brown Mushroom
pub struct BrownMushroom;

impl ItemDef for BrownMushroom {
    const ID: i32 = 247;
    const STRING_ID: &'static str = "minecraft:brown_mushroom";
    const NAME: &'static str = "Brown Mushroom";
    const STACK_SIZE: u8 = 64;
}

/// Red Mushroom
pub struct RedMushroom;

impl ItemDef for RedMushroom {
    const ID: i32 = 248;
    const STRING_ID: &'static str = "minecraft:red_mushroom";
    const NAME: &'static str = "Red Mushroom";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Fungus
pub struct CrimsonFungus;

impl ItemDef for CrimsonFungus {
    const ID: i32 = 249;
    const STRING_ID: &'static str = "minecraft:crimson_fungus";
    const NAME: &'static str = "Crimson Fungus";
    const STACK_SIZE: u8 = 64;
}

/// Warped Fungus
pub struct WarpedFungus;

impl ItemDef for WarpedFungus {
    const ID: i32 = 250;
    const STRING_ID: &'static str = "minecraft:warped_fungus";
    const NAME: &'static str = "Warped Fungus";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Roots
pub struct CrimsonRoots;

impl ItemDef for CrimsonRoots {
    const ID: i32 = 251;
    const STRING_ID: &'static str = "minecraft:crimson_roots";
    const NAME: &'static str = "Crimson Roots";
    const STACK_SIZE: u8 = 64;
}

/// Warped Roots
pub struct WarpedRoots;

impl ItemDef for WarpedRoots {
    const ID: i32 = 252;
    const STRING_ID: &'static str = "minecraft:warped_roots";
    const NAME: &'static str = "Warped Roots";
    const STACK_SIZE: u8 = 64;
}

/// Nether Sprouts
pub struct NetherSprouts;

impl ItemDef for NetherSprouts {
    const ID: i32 = 253;
    const STRING_ID: &'static str = "minecraft:nether_sprouts";
    const NAME: &'static str = "Nether Sprouts";
    const STACK_SIZE: u8 = 64;
}

/// Weeping Vines
pub struct WeepingVines;

impl ItemDef for WeepingVines {
    const ID: i32 = 254;
    const STRING_ID: &'static str = "minecraft:weeping_vines";
    const NAME: &'static str = "Weeping Vines";
    const STACK_SIZE: u8 = 64;
}

/// Twisting Vines
pub struct TwistingVines;

impl ItemDef for TwistingVines {
    const ID: i32 = 255;
    const STRING_ID: &'static str = "minecraft:twisting_vines";
    const NAME: &'static str = "Twisting Vines";
    const STACK_SIZE: u8 = 64;
}

/// Sugar Cane
pub struct SugarCane;

impl ItemDef for SugarCane {
    const ID: i32 = 256;
    const STRING_ID: &'static str = "minecraft:sugar_cane";
    const NAME: &'static str = "Sugar Cane";
    const STACK_SIZE: u8 = 64;
}

/// Kelp
pub struct Kelp;

impl ItemDef for Kelp {
    const ID: i32 = 257;
    const STRING_ID: &'static str = "minecraft:kelp";
    const NAME: &'static str = "Kelp";
    const STACK_SIZE: u8 = 64;
}

/// Pink Petals
pub struct PinkPetals;

impl ItemDef for PinkPetals {
    const ID: i32 = 258;
    const STRING_ID: &'static str = "minecraft:pink_petals";
    const NAME: &'static str = "Pink Petals";
    const STACK_SIZE: u8 = 64;
}

/// Wildflowers
pub struct Wildflowers;

impl ItemDef for Wildflowers {
    const ID: i32 = 259;
    const STRING_ID: &'static str = "minecraft:wildflowers";
    const NAME: &'static str = "Wildflowers";
    const STACK_SIZE: u8 = 64;
}

/// Leaf Litter
pub struct LeafLitter;

impl ItemDef for LeafLitter {
    const ID: i32 = 260;
    const STRING_ID: &'static str = "minecraft:leaf_litter";
    const NAME: &'static str = "Leaf Litter";
    const STACK_SIZE: u8 = 64;
}

/// Moss Carpet
pub struct MossCarpet;

impl ItemDef for MossCarpet {
    const ID: i32 = 261;
    const STRING_ID: &'static str = "minecraft:moss_carpet";
    const NAME: &'static str = "Moss Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Moss Block
pub struct MossBlock;

impl ItemDef for MossBlock {
    const ID: i32 = 262;
    const STRING_ID: &'static str = "minecraft:moss_block";
    const NAME: &'static str = "Moss Block";
    const STACK_SIZE: u8 = 64;
}

/// Pale Moss Carpet
pub struct PaleMossCarpet;

impl ItemDef for PaleMossCarpet {
    const ID: i32 = 263;
    const STRING_ID: &'static str = "minecraft:pale_moss_carpet";
    const NAME: &'static str = "Pale Moss Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Pale Hanging Moss
pub struct PaleHangingMoss;

impl ItemDef for PaleHangingMoss {
    const ID: i32 = 264;
    const STRING_ID: &'static str = "minecraft:pale_hanging_moss";
    const NAME: &'static str = "Pale Hanging Moss";
    const STACK_SIZE: u8 = 64;
}

/// Pale Moss Block
pub struct PaleMossBlock;

impl ItemDef for PaleMossBlock {
    const ID: i32 = 265;
    const STRING_ID: &'static str = "minecraft:pale_moss_block";
    const NAME: &'static str = "Pale Moss Block";
    const STACK_SIZE: u8 = 64;
}

/// Hanging Roots
pub struct HangingRoots;

impl ItemDef for HangingRoots {
    const ID: i32 = 266;
    const STRING_ID: &'static str = "minecraft:hanging_roots";
    const NAME: &'static str = "Hanging Roots";
    const STACK_SIZE: u8 = 64;
}

/// Big Dripleaf
pub struct BigDripleaf;

impl ItemDef for BigDripleaf {
    const ID: i32 = 267;
    const STRING_ID: &'static str = "minecraft:big_dripleaf";
    const NAME: &'static str = "Big Dripleaf";
    const STACK_SIZE: u8 = 64;
}

/// Small Dripleaf
pub struct SmallDripleafBlock;

impl ItemDef for SmallDripleafBlock {
    const ID: i32 = 268;
    const STRING_ID: &'static str = "minecraft:small_dripleaf_block";
    const NAME: &'static str = "Small Dripleaf";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo
pub struct Bamboo;

impl ItemDef for Bamboo {
    const ID: i32 = 269;
    const STRING_ID: &'static str = "minecraft:bamboo";
    const NAME: &'static str = "Bamboo";
    const STACK_SIZE: u8 = 64;
}

/// Oak Slab
pub struct OakSlab;

impl ItemDef for OakSlab {
    const ID: i32 = 270;
    const STRING_ID: &'static str = "minecraft:oak_slab";
    const NAME: &'static str = "Oak Slab";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Slab
pub struct SpruceSlab;

impl ItemDef for SpruceSlab {
    const ID: i32 = 271;
    const STRING_ID: &'static str = "minecraft:spruce_slab";
    const NAME: &'static str = "Spruce Slab";
    const STACK_SIZE: u8 = 64;
}

/// Birch Slab
pub struct BirchSlab;

impl ItemDef for BirchSlab {
    const ID: i32 = 272;
    const STRING_ID: &'static str = "minecraft:birch_slab";
    const NAME: &'static str = "Birch Slab";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Slab
pub struct JungleSlab;

impl ItemDef for JungleSlab {
    const ID: i32 = 273;
    const STRING_ID: &'static str = "minecraft:jungle_slab";
    const NAME: &'static str = "Jungle Slab";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Slab
pub struct AcaciaSlab;

impl ItemDef for AcaciaSlab {
    const ID: i32 = 274;
    const STRING_ID: &'static str = "minecraft:acacia_slab";
    const NAME: &'static str = "Acacia Slab";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Slab
pub struct CherrySlab;

impl ItemDef for CherrySlab {
    const ID: i32 = 275;
    const STRING_ID: &'static str = "minecraft:cherry_slab";
    const NAME: &'static str = "Cherry Slab";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Slab
pub struct DarkOakSlab;

impl ItemDef for DarkOakSlab {
    const ID: i32 = 276;
    const STRING_ID: &'static str = "minecraft:dark_oak_slab";
    const NAME: &'static str = "Dark Oak Slab";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Slab
pub struct PaleOakSlab;

impl ItemDef for PaleOakSlab {
    const ID: i32 = 277;
    const STRING_ID: &'static str = "minecraft:pale_oak_slab";
    const NAME: &'static str = "Pale Oak Slab";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Slab
pub struct MangroveSlab;

impl ItemDef for MangroveSlab {
    const ID: i32 = 278;
    const STRING_ID: &'static str = "minecraft:mangrove_slab";
    const NAME: &'static str = "Mangrove Slab";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Slab
pub struct BambooSlab;

impl ItemDef for BambooSlab {
    const ID: i32 = 279;
    const STRING_ID: &'static str = "minecraft:bamboo_slab";
    const NAME: &'static str = "Bamboo Slab";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Mosaic Slab
pub struct BambooMosaicSlab;

impl ItemDef for BambooMosaicSlab {
    const ID: i32 = 280;
    const STRING_ID: &'static str = "minecraft:bamboo_mosaic_slab";
    const NAME: &'static str = "Bamboo Mosaic Slab";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Slab
pub struct CrimsonSlab;

impl ItemDef for CrimsonSlab {
    const ID: i32 = 281;
    const STRING_ID: &'static str = "minecraft:crimson_slab";
    const NAME: &'static str = "Crimson Slab";
    const STACK_SIZE: u8 = 64;
}

/// Warped Slab
pub struct WarpedSlab;

impl ItemDef for WarpedSlab {
    const ID: i32 = 282;
    const STRING_ID: &'static str = "minecraft:warped_slab";
    const NAME: &'static str = "Warped Slab";
    const STACK_SIZE: u8 = 64;
}

/// Stone Slab
pub struct NormalStoneSlab;

impl ItemDef for NormalStoneSlab {
    const ID: i32 = 283;
    const STRING_ID: &'static str = "minecraft:normal_stone_slab";
    const NAME: &'static str = "Stone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Stone Slab
pub struct SmoothStoneSlab;

impl ItemDef for SmoothStoneSlab {
    const ID: i32 = 284;
    const STRING_ID: &'static str = "minecraft:smooth_stone_slab";
    const NAME: &'static str = "Smooth Stone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Sandstone Slab
pub struct SandstoneSlab;

impl ItemDef for SandstoneSlab {
    const ID: i32 = 285;
    const STRING_ID: &'static str = "minecraft:sandstone_slab";
    const NAME: &'static str = "Sandstone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Cut Sandstone Slab
pub struct CutSandstoneSlab;

impl ItemDef for CutSandstoneSlab {
    const ID: i32 = 286;
    const STRING_ID: &'static str = "minecraft:cut_sandstone_slab";
    const NAME: &'static str = "Cut Sandstone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Petrified Oak Slab
pub struct PetrifiedOakSlab;

impl ItemDef for PetrifiedOakSlab {
    const ID: i32 = 287;
    const STRING_ID: &'static str = "minecraft:petrified_oak_slab";
    const NAME: &'static str = "Petrified Oak Slab";
    const STACK_SIZE: u8 = 64;
}

/// Cobblestone Slab
pub struct CobblestoneSlab;

impl ItemDef for CobblestoneSlab {
    const ID: i32 = 288;
    const STRING_ID: &'static str = "minecraft:cobblestone_slab";
    const NAME: &'static str = "Cobblestone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Brick Slab
pub struct BrickSlab;

impl ItemDef for BrickSlab {
    const ID: i32 = 289;
    const STRING_ID: &'static str = "minecraft:brick_slab";
    const NAME: &'static str = "Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Stone Brick Slab
pub struct StoneBrickSlab;

impl ItemDef for StoneBrickSlab {
    const ID: i32 = 290;
    const STRING_ID: &'static str = "minecraft:stone_brick_slab";
    const NAME: &'static str = "Stone Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Mud Brick Slab
pub struct MudBrickSlab;

impl ItemDef for MudBrickSlab {
    const ID: i32 = 291;
    const STRING_ID: &'static str = "minecraft:mud_brick_slab";
    const NAME: &'static str = "Mud Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Nether Brick Slab
pub struct NetherBrickSlab;

impl ItemDef for NetherBrickSlab {
    const ID: i32 = 292;
    const STRING_ID: &'static str = "minecraft:nether_brick_slab";
    const NAME: &'static str = "Nether Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Quartz Slab
pub struct QuartzSlab;

impl ItemDef for QuartzSlab {
    const ID: i32 = 293;
    const STRING_ID: &'static str = "minecraft:quartz_slab";
    const NAME: &'static str = "Quartz Slab";
    const STACK_SIZE: u8 = 64;
}

/// Red Sandstone Slab
pub struct RedSandstoneSlab;

impl ItemDef for RedSandstoneSlab {
    const ID: i32 = 294;
    const STRING_ID: &'static str = "minecraft:red_sandstone_slab";
    const NAME: &'static str = "Red Sandstone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Cut Red Sandstone Slab
pub struct CutRedSandstoneSlab;

impl ItemDef for CutRedSandstoneSlab {
    const ID: i32 = 295;
    const STRING_ID: &'static str = "minecraft:cut_red_sandstone_slab";
    const NAME: &'static str = "Cut Red Sandstone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Purpur Slab
pub struct PurpurSlab;

impl ItemDef for PurpurSlab {
    const ID: i32 = 296;
    const STRING_ID: &'static str = "minecraft:purpur_slab";
    const NAME: &'static str = "Purpur Slab";
    const STACK_SIZE: u8 = 64;
}

/// Prismarine Slab
pub struct PrismarineSlab;

impl ItemDef for PrismarineSlab {
    const ID: i32 = 297;
    const STRING_ID: &'static str = "minecraft:prismarine_slab";
    const NAME: &'static str = "Prismarine Slab";
    const STACK_SIZE: u8 = 64;
}

/// Prismarine Brick Slab
pub struct PrismarineBrickSlab;

impl ItemDef for PrismarineBrickSlab {
    const ID: i32 = 298;
    const STRING_ID: &'static str = "minecraft:prismarine_brick_slab";
    const NAME: &'static str = "Prismarine Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Dark Prismarine Slab
pub struct DarkPrismarineSlab;

impl ItemDef for DarkPrismarineSlab {
    const ID: i32 = 299;
    const STRING_ID: &'static str = "minecraft:dark_prismarine_slab";
    const NAME: &'static str = "Dark Prismarine Slab";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Quartz Block
pub struct SmoothQuartz;

impl ItemDef for SmoothQuartz {
    const ID: i32 = 300;
    const STRING_ID: &'static str = "minecraft:smooth_quartz";
    const NAME: &'static str = "Smooth Quartz Block";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Red Sandstone
pub struct SmoothRedSandstone;

impl ItemDef for SmoothRedSandstone {
    const ID: i32 = 301;
    const STRING_ID: &'static str = "minecraft:smooth_red_sandstone";
    const NAME: &'static str = "Smooth Red Sandstone";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Sandstone
pub struct SmoothSandstone;

impl ItemDef for SmoothSandstone {
    const ID: i32 = 302;
    const STRING_ID: &'static str = "minecraft:smooth_sandstone";
    const NAME: &'static str = "Smooth Sandstone";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Stone
pub struct SmoothStone;

impl ItemDef for SmoothStone {
    const ID: i32 = 303;
    const STRING_ID: &'static str = "minecraft:smooth_stone";
    const NAME: &'static str = "Smooth Stone";
    const STACK_SIZE: u8 = 64;
}

/// Bricks
pub struct BrickBlock;

impl ItemDef for BrickBlock {
    const ID: i32 = 304;
    const STRING_ID: &'static str = "minecraft:brick_block";
    const NAME: &'static str = "Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Bookshelf
pub struct Bookshelf;

impl ItemDef for Bookshelf {
    const ID: i32 = 305;
    const STRING_ID: &'static str = "minecraft:bookshelf";
    const NAME: &'static str = "Bookshelf";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Bookshelf
pub struct ChiseledBookshelf;

impl ItemDef for ChiseledBookshelf {
    const ID: i32 = 306;
    const STRING_ID: &'static str = "minecraft:chiseled_bookshelf";
    const NAME: &'static str = "Chiseled Bookshelf";
    const STACK_SIZE: u8 = 64;
}

/// Decorated Pot
pub struct DecoratedPot;

impl ItemDef for DecoratedPot {
    const ID: i32 = 307;
    const STRING_ID: &'static str = "minecraft:decorated_pot";
    const NAME: &'static str = "Decorated Pot";
    const STACK_SIZE: u8 = 64;
}

/// Mossy Cobblestone
pub struct MossyCobblestone;

impl ItemDef for MossyCobblestone {
    const ID: i32 = 308;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone";
    const NAME: &'static str = "Mossy Cobblestone";
    const STACK_SIZE: u8 = 64;
}

/// Obsidian
pub struct Obsidian;

impl ItemDef for Obsidian {
    const ID: i32 = 309;
    const STRING_ID: &'static str = "minecraft:obsidian";
    const NAME: &'static str = "Obsidian";
    const STACK_SIZE: u8 = 64;
}

/// Torch
pub struct Torch;

impl ItemDef for Torch {
    const ID: i32 = 310;
    const STRING_ID: &'static str = "minecraft:torch";
    const NAME: &'static str = "Torch";
    const STACK_SIZE: u8 = 64;
}

/// End Rod
pub struct EndRod;

impl ItemDef for EndRod {
    const ID: i32 = 311;
    const STRING_ID: &'static str = "minecraft:end_rod";
    const NAME: &'static str = "End Rod";
    const STACK_SIZE: u8 = 64;
}

/// Chorus Plant
pub struct ChorusPlant;

impl ItemDef for ChorusPlant {
    const ID: i32 = 312;
    const STRING_ID: &'static str = "minecraft:chorus_plant";
    const NAME: &'static str = "Chorus Plant";
    const STACK_SIZE: u8 = 64;
}

/// Chorus Flower
pub struct ChorusFlower;

impl ItemDef for ChorusFlower {
    const ID: i32 = 313;
    const STRING_ID: &'static str = "minecraft:chorus_flower";
    const NAME: &'static str = "Chorus Flower";
    const STACK_SIZE: u8 = 64;
}

/// Purpur Block
pub struct PurpurBlock;

impl ItemDef for PurpurBlock {
    const ID: i32 = 314;
    const STRING_ID: &'static str = "minecraft:purpur_block";
    const NAME: &'static str = "Purpur Block";
    const STACK_SIZE: u8 = 64;
}

/// Purpur Pillar
pub struct PurpurPillar;

impl ItemDef for PurpurPillar {
    const ID: i32 = 315;
    const STRING_ID: &'static str = "minecraft:purpur_pillar";
    const NAME: &'static str = "Purpur Pillar";
    const STACK_SIZE: u8 = 64;
}

/// Purpur Stairs
pub struct PurpurStairs;

impl ItemDef for PurpurStairs {
    const ID: i32 = 316;
    const STRING_ID: &'static str = "minecraft:purpur_stairs";
    const NAME: &'static str = "Purpur Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Monster Spawner
pub struct MobSpawner;

impl ItemDef for MobSpawner {
    const ID: i32 = 317;
    const STRING_ID: &'static str = "minecraft:mob_spawner";
    const NAME: &'static str = "Monster Spawner";
    const STACK_SIZE: u8 = 64;
}

/// Creaking Heart
pub struct CreakingHeart;

impl ItemDef for CreakingHeart {
    const ID: i32 = 318;
    const STRING_ID: &'static str = "minecraft:creaking_heart";
    const NAME: &'static str = "Creaking Heart";
    const STACK_SIZE: u8 = 64;
}

/// Chest
pub struct Chest;

impl ItemDef for Chest {
    const ID: i32 = 319;
    const STRING_ID: &'static str = "minecraft:chest";
    const NAME: &'static str = "Chest";
    const STACK_SIZE: u8 = 64;
}

/// Crafting Table
pub struct CraftingTable;

impl ItemDef for CraftingTable {
    const ID: i32 = 320;
    const STRING_ID: &'static str = "minecraft:crafting_table";
    const NAME: &'static str = "Crafting Table";
    const STACK_SIZE: u8 = 64;
}

/// Farmland
pub struct Farmland;

impl ItemDef for Farmland {
    const ID: i32 = 321;
    const STRING_ID: &'static str = "minecraft:farmland";
    const NAME: &'static str = "Farmland";
    const STACK_SIZE: u8 = 64;
}

/// Furnace
pub struct Furnace;

impl ItemDef for Furnace {
    const ID: i32 = 322;
    const STRING_ID: &'static str = "minecraft:furnace";
    const NAME: &'static str = "Furnace";
    const STACK_SIZE: u8 = 64;
}

/// Ladder
pub struct Ladder;

impl ItemDef for Ladder {
    const ID: i32 = 323;
    const STRING_ID: &'static str = "minecraft:ladder";
    const NAME: &'static str = "Ladder";
    const STACK_SIZE: u8 = 64;
}

/// Cobblestone Stairs
pub struct StoneStairs;

impl ItemDef for StoneStairs {
    const ID: i32 = 324;
    const STRING_ID: &'static str = "minecraft:stone_stairs";
    const NAME: &'static str = "Cobblestone Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Snow
pub struct SnowLayer;

impl ItemDef for SnowLayer {
    const ID: i32 = 325;
    const STRING_ID: &'static str = "minecraft:snow_layer";
    const NAME: &'static str = "Snow";
    const STACK_SIZE: u8 = 64;
}

/// Ice
pub struct Ice;

impl ItemDef for Ice {
    const ID: i32 = 326;
    const STRING_ID: &'static str = "minecraft:ice";
    const NAME: &'static str = "Ice";
    const STACK_SIZE: u8 = 64;
}

/// Snow Block
pub struct Snow;

impl ItemDef for Snow {
    const ID: i32 = 327;
    const STRING_ID: &'static str = "minecraft:snow";
    const NAME: &'static str = "Snow Block";
    const STACK_SIZE: u8 = 64;
}

/// Cactus
pub struct Cactus;

impl ItemDef for Cactus {
    const ID: i32 = 328;
    const STRING_ID: &'static str = "minecraft:cactus";
    const NAME: &'static str = "Cactus";
    const STACK_SIZE: u8 = 64;
}

/// Cactus Flower
pub struct CactusFlower;

impl ItemDef for CactusFlower {
    const ID: i32 = 329;
    const STRING_ID: &'static str = "minecraft:cactus_flower";
    const NAME: &'static str = "Cactus Flower";
    const STACK_SIZE: u8 = 64;
}

/// Clay
pub struct Clay;

impl ItemDef for Clay {
    const ID: i32 = 330;
    const STRING_ID: &'static str = "minecraft:clay";
    const NAME: &'static str = "Clay";
    const STACK_SIZE: u8 = 64;
}

/// Jukebox
pub struct Jukebox;

impl ItemDef for Jukebox {
    const ID: i32 = 331;
    const STRING_ID: &'static str = "minecraft:jukebox";
    const NAME: &'static str = "Jukebox";
    const STACK_SIZE: u8 = 64;
}

/// Oak Fence
pub struct OakFence;

impl ItemDef for OakFence {
    const ID: i32 = 332;
    const STRING_ID: &'static str = "minecraft:oak_fence";
    const NAME: &'static str = "Oak Fence";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Fence
pub struct SpruceFence;

impl ItemDef for SpruceFence {
    const ID: i32 = 333;
    const STRING_ID: &'static str = "minecraft:spruce_fence";
    const NAME: &'static str = "Spruce Fence";
    const STACK_SIZE: u8 = 64;
}

/// Birch Fence
pub struct BirchFence;

impl ItemDef for BirchFence {
    const ID: i32 = 334;
    const STRING_ID: &'static str = "minecraft:birch_fence";
    const NAME: &'static str = "Birch Fence";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Fence
pub struct JungleFence;

impl ItemDef for JungleFence {
    const ID: i32 = 335;
    const STRING_ID: &'static str = "minecraft:jungle_fence";
    const NAME: &'static str = "Jungle Fence";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Fence
pub struct AcaciaFence;

impl ItemDef for AcaciaFence {
    const ID: i32 = 336;
    const STRING_ID: &'static str = "minecraft:acacia_fence";
    const NAME: &'static str = "Acacia Fence";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Fence
pub struct CherryFence;

impl ItemDef for CherryFence {
    const ID: i32 = 337;
    const STRING_ID: &'static str = "minecraft:cherry_fence";
    const NAME: &'static str = "Cherry Fence";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Fence
pub struct DarkOakFence;

impl ItemDef for DarkOakFence {
    const ID: i32 = 338;
    const STRING_ID: &'static str = "minecraft:dark_oak_fence";
    const NAME: &'static str = "Dark Oak Fence";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Fence
pub struct PaleOakFence;

impl ItemDef for PaleOakFence {
    const ID: i32 = 339;
    const STRING_ID: &'static str = "minecraft:pale_oak_fence";
    const NAME: &'static str = "Pale Oak Fence";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Fence
pub struct MangroveFence;

impl ItemDef for MangroveFence {
    const ID: i32 = 340;
    const STRING_ID: &'static str = "minecraft:mangrove_fence";
    const NAME: &'static str = "Mangrove Fence";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Fence
pub struct BambooFence;

impl ItemDef for BambooFence {
    const ID: i32 = 341;
    const STRING_ID: &'static str = "minecraft:bamboo_fence";
    const NAME: &'static str = "Bamboo Fence";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Fence
pub struct CrimsonFence;

impl ItemDef for CrimsonFence {
    const ID: i32 = 342;
    const STRING_ID: &'static str = "minecraft:crimson_fence";
    const NAME: &'static str = "Crimson Fence";
    const STACK_SIZE: u8 = 64;
}

/// Warped Fence
pub struct WarpedFence;

impl ItemDef for WarpedFence {
    const ID: i32 = 343;
    const STRING_ID: &'static str = "minecraft:warped_fence";
    const NAME: &'static str = "Warped Fence";
    const STACK_SIZE: u8 = 64;
}

/// Pumpkin
pub struct Pumpkin;

impl ItemDef for Pumpkin {
    const ID: i32 = 344;
    const STRING_ID: &'static str = "minecraft:pumpkin";
    const NAME: &'static str = "Pumpkin";
    const STACK_SIZE: u8 = 64;
}

/// Carved Pumpkin
pub struct CarvedPumpkin;

impl ItemDef for CarvedPumpkin {
    const ID: i32 = 345;
    const STRING_ID: &'static str = "minecraft:carved_pumpkin";
    const NAME: &'static str = "Carved Pumpkin";
    const STACK_SIZE: u8 = 64;
}

impl EnchantableItem for CarvedPumpkin {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Jack o'Lantern
pub struct LitPumpkin;

impl ItemDef for LitPumpkin {
    const ID: i32 = 346;
    const STRING_ID: &'static str = "minecraft:lit_pumpkin";
    const NAME: &'static str = "Jack o'Lantern";
    const STACK_SIZE: u8 = 64;
}

/// Netherrack
pub struct Netherrack;

impl ItemDef for Netherrack {
    const ID: i32 = 347;
    const STRING_ID: &'static str = "minecraft:netherrack";
    const NAME: &'static str = "Netherrack";
    const STACK_SIZE: u8 = 64;
}

/// Soul Sand
pub struct SoulSand;

impl ItemDef for SoulSand {
    const ID: i32 = 348;
    const STRING_ID: &'static str = "minecraft:soul_sand";
    const NAME: &'static str = "Soul Sand";
    const STACK_SIZE: u8 = 64;
}

/// Soul Soil
pub struct SoulSoil;

impl ItemDef for SoulSoil {
    const ID: i32 = 349;
    const STRING_ID: &'static str = "minecraft:soul_soil";
    const NAME: &'static str = "Soul Soil";
    const STACK_SIZE: u8 = 64;
}

/// Basalt
pub struct Basalt;

impl ItemDef for Basalt {
    const ID: i32 = 350;
    const STRING_ID: &'static str = "minecraft:basalt";
    const NAME: &'static str = "Basalt";
    const STACK_SIZE: u8 = 64;
}

/// Polished Basalt
pub struct PolishedBasalt;

impl ItemDef for PolishedBasalt {
    const ID: i32 = 351;
    const STRING_ID: &'static str = "minecraft:polished_basalt";
    const NAME: &'static str = "Polished Basalt";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Basalt
pub struct SmoothBasalt;

impl ItemDef for SmoothBasalt {
    const ID: i32 = 352;
    const STRING_ID: &'static str = "minecraft:smooth_basalt";
    const NAME: &'static str = "Smooth Basalt";
    const STACK_SIZE: u8 = 64;
}

/// Soul Torch
pub struct SoulTorch;

impl ItemDef for SoulTorch {
    const ID: i32 = 353;
    const STRING_ID: &'static str = "minecraft:soul_torch";
    const NAME: &'static str = "Soul Torch";
    const STACK_SIZE: u8 = 64;
}

/// Glowstone
pub struct Glowstone;

impl ItemDef for Glowstone {
    const ID: i32 = 354;
    const STRING_ID: &'static str = "minecraft:glowstone";
    const NAME: &'static str = "Glowstone";
    const STACK_SIZE: u8 = 64;
}

/// Infested Stone
pub struct InfestedStone;

impl ItemDef for InfestedStone {
    const ID: i32 = 355;
    const STRING_ID: &'static str = "minecraft:infested_stone";
    const NAME: &'static str = "Infested Stone";
    const STACK_SIZE: u8 = 64;
}

/// Infested Cobblestone
pub struct InfestedCobblestone;

impl ItemDef for InfestedCobblestone {
    const ID: i32 = 356;
    const STRING_ID: &'static str = "minecraft:infested_cobblestone";
    const NAME: &'static str = "Infested Cobblestone";
    const STACK_SIZE: u8 = 64;
}

/// Infested Stone Bricks
pub struct InfestedStoneBricks;

impl ItemDef for InfestedStoneBricks {
    const ID: i32 = 357;
    const STRING_ID: &'static str = "minecraft:infested_stone_bricks";
    const NAME: &'static str = "Infested Stone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Infested Mossy Stone Bricks
pub struct InfestedMossyStoneBricks;

impl ItemDef for InfestedMossyStoneBricks {
    const ID: i32 = 358;
    const STRING_ID: &'static str = "minecraft:infested_mossy_stone_bricks";
    const NAME: &'static str = "Infested Mossy Stone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Infested Cracked Stone Bricks
pub struct InfestedCrackedStoneBricks;

impl ItemDef for InfestedCrackedStoneBricks {
    const ID: i32 = 359;
    const STRING_ID: &'static str = "minecraft:infested_cracked_stone_bricks";
    const NAME: &'static str = "Infested Cracked Stone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Infested Chiseled Stone Bricks
pub struct InfestedChiseledStoneBricks;

impl ItemDef for InfestedChiseledStoneBricks {
    const ID: i32 = 360;
    const STRING_ID: &'static str = "minecraft:infested_chiseled_stone_bricks";
    const NAME: &'static str = "Infested Chiseled Stone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Infested Deepslate
pub struct InfestedDeepslate;

impl ItemDef for InfestedDeepslate {
    const ID: i32 = 361;
    const STRING_ID: &'static str = "minecraft:infested_deepslate";
    const NAME: &'static str = "Infested Deepslate";
    const STACK_SIZE: u8 = 64;
}

/// Stone Bricks
pub struct StoneBricks;

impl ItemDef for StoneBricks {
    const ID: i32 = 362;
    const STRING_ID: &'static str = "minecraft:stone_bricks";
    const NAME: &'static str = "Stone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Mossy Stone Bricks
pub struct MossyStoneBricks;

impl ItemDef for MossyStoneBricks {
    const ID: i32 = 363;
    const STRING_ID: &'static str = "minecraft:mossy_stone_bricks";
    const NAME: &'static str = "Mossy Stone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Cracked Stone Bricks
pub struct CrackedStoneBricks;

impl ItemDef for CrackedStoneBricks {
    const ID: i32 = 364;
    const STRING_ID: &'static str = "minecraft:cracked_stone_bricks";
    const NAME: &'static str = "Cracked Stone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Stone Bricks
pub struct ChiseledStoneBricks;

impl ItemDef for ChiseledStoneBricks {
    const ID: i32 = 365;
    const STRING_ID: &'static str = "minecraft:chiseled_stone_bricks";
    const NAME: &'static str = "Chiseled Stone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Packed Mud
pub struct PackedMud;

impl ItemDef for PackedMud {
    const ID: i32 = 366;
    const STRING_ID: &'static str = "minecraft:packed_mud";
    const NAME: &'static str = "Packed Mud";
    const STACK_SIZE: u8 = 64;
}

/// Mud Bricks
pub struct MudBricks;

impl ItemDef for MudBricks {
    const ID: i32 = 367;
    const STRING_ID: &'static str = "minecraft:mud_bricks";
    const NAME: &'static str = "Mud Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Bricks
pub struct DeepslateBricks;

impl ItemDef for DeepslateBricks {
    const ID: i32 = 368;
    const STRING_ID: &'static str = "minecraft:deepslate_bricks";
    const NAME: &'static str = "Deepslate Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Cracked Deepslate Bricks
pub struct CrackedDeepslateBricks;

impl ItemDef for CrackedDeepslateBricks {
    const ID: i32 = 369;
    const STRING_ID: &'static str = "minecraft:cracked_deepslate_bricks";
    const NAME: &'static str = "Cracked Deepslate Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Tiles
pub struct DeepslateTiles;

impl ItemDef for DeepslateTiles {
    const ID: i32 = 370;
    const STRING_ID: &'static str = "minecraft:deepslate_tiles";
    const NAME: &'static str = "Deepslate Tiles";
    const STACK_SIZE: u8 = 64;
}

/// Cracked Deepslate Tiles
pub struct CrackedDeepslateTiles;

impl ItemDef for CrackedDeepslateTiles {
    const ID: i32 = 371;
    const STRING_ID: &'static str = "minecraft:cracked_deepslate_tiles";
    const NAME: &'static str = "Cracked Deepslate Tiles";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Deepslate
pub struct ChiseledDeepslate;

impl ItemDef for ChiseledDeepslate {
    const ID: i32 = 372;
    const STRING_ID: &'static str = "minecraft:chiseled_deepslate";
    const NAME: &'static str = "Chiseled Deepslate";
    const STACK_SIZE: u8 = 64;
}

/// Reinforced Deepslate
pub struct ReinforcedDeepslate;

impl ItemDef for ReinforcedDeepslate {
    const ID: i32 = 373;
    const STRING_ID: &'static str = "minecraft:reinforced_deepslate";
    const NAME: &'static str = "Reinforced Deepslate";
    const STACK_SIZE: u8 = 64;
}

/// Brown Mushroom Block
pub struct BrownMushroomBlock;

impl ItemDef for BrownMushroomBlock {
    const ID: i32 = 374;
    const STRING_ID: &'static str = "minecraft:brown_mushroom_block";
    const NAME: &'static str = "Brown Mushroom Block";
    const STACK_SIZE: u8 = 64;
}

/// Red Mushroom Block
pub struct RedMushroomBlock;

impl ItemDef for RedMushroomBlock {
    const ID: i32 = 375;
    const STRING_ID: &'static str = "minecraft:red_mushroom_block";
    const NAME: &'static str = "Red Mushroom Block";
    const STACK_SIZE: u8 = 64;
}

/// Mushroom Stem
pub struct MushroomStem;

impl ItemDef for MushroomStem {
    const ID: i32 = 376;
    const STRING_ID: &'static str = "minecraft:mushroom_stem";
    const NAME: &'static str = "Mushroom Stem";
    const STACK_SIZE: u8 = 64;
}

/// Iron Bars
pub struct IronBars;

impl ItemDef for IronBars {
    const ID: i32 = 377;
    const STRING_ID: &'static str = "minecraft:iron_bars";
    const NAME: &'static str = "Iron Bars";
    const STACK_SIZE: u8 = 64;
}

/// Chain
pub struct IronChain;

impl ItemDef for IronChain {
    const ID: i32 = 378;
    const STRING_ID: &'static str = "minecraft:iron_chain";
    const NAME: &'static str = "Chain";
    const STACK_SIZE: u8 = 64;
}

/// Glass Pane
pub struct GlassPane;

impl ItemDef for GlassPane {
    const ID: i32 = 379;
    const STRING_ID: &'static str = "minecraft:glass_pane";
    const NAME: &'static str = "Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Melon
pub struct MelonBlock;

impl ItemDef for MelonBlock {
    const ID: i32 = 380;
    const STRING_ID: &'static str = "minecraft:melon_block";
    const NAME: &'static str = "Melon";
    const STACK_SIZE: u8 = 64;
}

/// Vines
pub struct Vine;

impl ItemDef for Vine {
    const ID: i32 = 381;
    const STRING_ID: &'static str = "minecraft:vine";
    const NAME: &'static str = "Vines";
    const STACK_SIZE: u8 = 64;
}

/// Glow Lichen
pub struct GlowLichen;

impl ItemDef for GlowLichen {
    const ID: i32 = 382;
    const STRING_ID: &'static str = "minecraft:glow_lichen";
    const NAME: &'static str = "Glow Lichen";
    const STACK_SIZE: u8 = 64;
}

/// Resin Clump
pub struct ResinClump;

impl ItemDef for ResinClump {
    const ID: i32 = 383;
    const STRING_ID: &'static str = "minecraft:resin_clump";
    const NAME: &'static str = "Resin Clump";
    const STACK_SIZE: u8 = 64;
}

/// Block of Resin
pub struct ResinBlock;

impl ItemDef for ResinBlock {
    const ID: i32 = 384;
    const STRING_ID: &'static str = "minecraft:resin_block";
    const NAME: &'static str = "Block of Resin";
    const STACK_SIZE: u8 = 64;
}

/// Resin Bricks
pub struct ResinBricks;

impl ItemDef for ResinBricks {
    const ID: i32 = 385;
    const STRING_ID: &'static str = "minecraft:resin_bricks";
    const NAME: &'static str = "Resin Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Resin Brick Stairs
pub struct ResinBrickStairs;

impl ItemDef for ResinBrickStairs {
    const ID: i32 = 386;
    const STRING_ID: &'static str = "minecraft:resin_brick_stairs";
    const NAME: &'static str = "Resin Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Resin Brick Slab
pub struct ResinBrickSlab;

impl ItemDef for ResinBrickSlab {
    const ID: i32 = 387;
    const STRING_ID: &'static str = "minecraft:resin_brick_slab";
    const NAME: &'static str = "Resin Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Resin Brick Wall
pub struct ResinBrickWall;

impl ItemDef for ResinBrickWall {
    const ID: i32 = 388;
    const STRING_ID: &'static str = "minecraft:resin_brick_wall";
    const NAME: &'static str = "Resin Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Resin Bricks
pub struct ChiseledResinBricks;

impl ItemDef for ChiseledResinBricks {
    const ID: i32 = 389;
    const STRING_ID: &'static str = "minecraft:chiseled_resin_bricks";
    const NAME: &'static str = "Chiseled Resin Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Brick Stairs
pub struct BrickStairs;

impl ItemDef for BrickStairs {
    const ID: i32 = 390;
    const STRING_ID: &'static str = "minecraft:brick_stairs";
    const NAME: &'static str = "Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Stone Brick Stairs
pub struct StoneBrickStairs;

impl ItemDef for StoneBrickStairs {
    const ID: i32 = 391;
    const STRING_ID: &'static str = "minecraft:stone_brick_stairs";
    const NAME: &'static str = "Stone Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Mud Brick Stairs
pub struct MudBrickStairs;

impl ItemDef for MudBrickStairs {
    const ID: i32 = 392;
    const STRING_ID: &'static str = "minecraft:mud_brick_stairs";
    const NAME: &'static str = "Mud Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Mycelium
pub struct Mycelium;

impl ItemDef for Mycelium {
    const ID: i32 = 393;
    const STRING_ID: &'static str = "minecraft:mycelium";
    const NAME: &'static str = "Mycelium";
    const STACK_SIZE: u8 = 64;
}

/// Lily Pad
pub struct Waterlily;

impl ItemDef for Waterlily {
    const ID: i32 = 394;
    const STRING_ID: &'static str = "minecraft:waterlily";
    const NAME: &'static str = "Lily Pad";
    const STACK_SIZE: u8 = 64;
}

/// Nether Bricks
pub struct NetherBrick;

impl ItemDef for NetherBrick {
    const ID: i32 = 395;
    const STRING_ID: &'static str = "minecraft:nether_brick";
    const NAME: &'static str = "Nether Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Cracked Nether Bricks
pub struct CrackedNetherBricks;

impl ItemDef for CrackedNetherBricks {
    const ID: i32 = 396;
    const STRING_ID: &'static str = "minecraft:cracked_nether_bricks";
    const NAME: &'static str = "Cracked Nether Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Nether Bricks
pub struct ChiseledNetherBricks;

impl ItemDef for ChiseledNetherBricks {
    const ID: i32 = 397;
    const STRING_ID: &'static str = "minecraft:chiseled_nether_bricks";
    const NAME: &'static str = "Chiseled Nether Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Nether Brick Fence
pub struct NetherBrickFence;

impl ItemDef for NetherBrickFence {
    const ID: i32 = 398;
    const STRING_ID: &'static str = "minecraft:nether_brick_fence";
    const NAME: &'static str = "Nether Brick Fence";
    const STACK_SIZE: u8 = 64;
}

/// Nether Brick Stairs
pub struct NetherBrickStairs;

impl ItemDef for NetherBrickStairs {
    const ID: i32 = 399;
    const STRING_ID: &'static str = "minecraft:nether_brick_stairs";
    const NAME: &'static str = "Nether Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Sculk
pub struct Sculk;

impl ItemDef for Sculk {
    const ID: i32 = 400;
    const STRING_ID: &'static str = "minecraft:sculk";
    const NAME: &'static str = "Sculk";
    const STACK_SIZE: u8 = 64;
}

/// Sculk Vein
pub struct SculkVein;

impl ItemDef for SculkVein {
    const ID: i32 = 401;
    const STRING_ID: &'static str = "minecraft:sculk_vein";
    const NAME: &'static str = "Sculk Vein";
    const STACK_SIZE: u8 = 64;
}

/// Sculk Catalyst
pub struct SculkCatalyst;

impl ItemDef for SculkCatalyst {
    const ID: i32 = 402;
    const STRING_ID: &'static str = "minecraft:sculk_catalyst";
    const NAME: &'static str = "Sculk Catalyst";
    const STACK_SIZE: u8 = 64;
}

/// Sculk Shrieker
pub struct SculkShrieker;

impl ItemDef for SculkShrieker {
    const ID: i32 = 403;
    const STRING_ID: &'static str = "minecraft:sculk_shrieker";
    const NAME: &'static str = "Sculk Shrieker";
    const STACK_SIZE: u8 = 64;
}

/// Enchanting Table
pub struct EnchantingTable;

impl ItemDef for EnchantingTable {
    const ID: i32 = 404;
    const STRING_ID: &'static str = "minecraft:enchanting_table";
    const NAME: &'static str = "Enchanting Table";
    const STACK_SIZE: u8 = 64;
}

/// End Portal Frame
pub struct EndPortalFrame;

impl ItemDef for EndPortalFrame {
    const ID: i32 = 405;
    const STRING_ID: &'static str = "minecraft:end_portal_frame";
    const NAME: &'static str = "End Portal Frame";
    const STACK_SIZE: u8 = 64;
}

/// End Stone
pub struct EndStone;

impl ItemDef for EndStone {
    const ID: i32 = 406;
    const STRING_ID: &'static str = "minecraft:end_stone";
    const NAME: &'static str = "End Stone";
    const STACK_SIZE: u8 = 64;
}

/// End Stone Bricks
pub struct EndBricks;

impl ItemDef for EndBricks {
    const ID: i32 = 407;
    const STRING_ID: &'static str = "minecraft:end_bricks";
    const NAME: &'static str = "End Stone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Dragon Egg
pub struct DragonEgg;

impl ItemDef for DragonEgg {
    const ID: i32 = 408;
    const STRING_ID: &'static str = "minecraft:dragon_egg";
    const NAME: &'static str = "Dragon Egg";
    const STACK_SIZE: u8 = 64;
}

/// Sandstone Stairs
pub struct SandstoneStairs;

impl ItemDef for SandstoneStairs {
    const ID: i32 = 409;
    const STRING_ID: &'static str = "minecraft:sandstone_stairs";
    const NAME: &'static str = "Sandstone Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Ender Chest
pub struct EnderChest;

impl ItemDef for EnderChest {
    const ID: i32 = 410;
    const STRING_ID: &'static str = "minecraft:ender_chest";
    const NAME: &'static str = "Ender Chest";
    const STACK_SIZE: u8 = 64;
}

/// Block of Emerald
pub struct EmeraldBlock;

impl ItemDef for EmeraldBlock {
    const ID: i32 = 411;
    const STRING_ID: &'static str = "minecraft:emerald_block";
    const NAME: &'static str = "Block of Emerald";
    const STACK_SIZE: u8 = 64;
}

/// Oak Stairs
pub struct OakStairs;

impl ItemDef for OakStairs {
    const ID: i32 = 412;
    const STRING_ID: &'static str = "minecraft:oak_stairs";
    const NAME: &'static str = "Oak Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Stairs
pub struct SpruceStairs;

impl ItemDef for SpruceStairs {
    const ID: i32 = 413;
    const STRING_ID: &'static str = "minecraft:spruce_stairs";
    const NAME: &'static str = "Spruce Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Birch Stairs
pub struct BirchStairs;

impl ItemDef for BirchStairs {
    const ID: i32 = 414;
    const STRING_ID: &'static str = "minecraft:birch_stairs";
    const NAME: &'static str = "Birch Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Stairs
pub struct JungleStairs;

impl ItemDef for JungleStairs {
    const ID: i32 = 415;
    const STRING_ID: &'static str = "minecraft:jungle_stairs";
    const NAME: &'static str = "Jungle Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Stairs
pub struct AcaciaStairs;

impl ItemDef for AcaciaStairs {
    const ID: i32 = 416;
    const STRING_ID: &'static str = "minecraft:acacia_stairs";
    const NAME: &'static str = "Acacia Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Stairs
pub struct CherryStairs;

impl ItemDef for CherryStairs {
    const ID: i32 = 417;
    const STRING_ID: &'static str = "minecraft:cherry_stairs";
    const NAME: &'static str = "Cherry Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Stairs
pub struct DarkOakStairs;

impl ItemDef for DarkOakStairs {
    const ID: i32 = 418;
    const STRING_ID: &'static str = "minecraft:dark_oak_stairs";
    const NAME: &'static str = "Dark Oak Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Stairs
pub struct PaleOakStairs;

impl ItemDef for PaleOakStairs {
    const ID: i32 = 419;
    const STRING_ID: &'static str = "minecraft:pale_oak_stairs";
    const NAME: &'static str = "Pale Oak Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Stairs
pub struct MangroveStairs;

impl ItemDef for MangroveStairs {
    const ID: i32 = 420;
    const STRING_ID: &'static str = "minecraft:mangrove_stairs";
    const NAME: &'static str = "Mangrove Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Stairs
pub struct BambooStairs;

impl ItemDef for BambooStairs {
    const ID: i32 = 421;
    const STRING_ID: &'static str = "minecraft:bamboo_stairs";
    const NAME: &'static str = "Bamboo Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Mosaic Stairs
pub struct BambooMosaicStairs;

impl ItemDef for BambooMosaicStairs {
    const ID: i32 = 422;
    const STRING_ID: &'static str = "minecraft:bamboo_mosaic_stairs";
    const NAME: &'static str = "Bamboo Mosaic Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Stairs
pub struct CrimsonStairs;

impl ItemDef for CrimsonStairs {
    const ID: i32 = 423;
    const STRING_ID: &'static str = "minecraft:crimson_stairs";
    const NAME: &'static str = "Crimson Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Warped Stairs
pub struct WarpedStairs;

impl ItemDef for WarpedStairs {
    const ID: i32 = 424;
    const STRING_ID: &'static str = "minecraft:warped_stairs";
    const NAME: &'static str = "Warped Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Command Block
pub struct CommandBlock;

impl ItemDef for CommandBlock {
    const ID: i32 = 425;
    const STRING_ID: &'static str = "minecraft:command_block";
    const NAME: &'static str = "Command Block";
    const STACK_SIZE: u8 = 64;
}

/// Beacon
pub struct Beacon;

impl ItemDef for Beacon {
    const ID: i32 = 426;
    const STRING_ID: &'static str = "minecraft:beacon";
    const NAME: &'static str = "Beacon";
    const STACK_SIZE: u8 = 64;
}

/// Cobblestone Wall
pub struct CobblestoneWall;

impl ItemDef for CobblestoneWall {
    const ID: i32 = 427;
    const STRING_ID: &'static str = "minecraft:cobblestone_wall";
    const NAME: &'static str = "Cobblestone Wall";
    const STACK_SIZE: u8 = 64;
}

/// Mossy Cobblestone Wall
pub struct MossyCobblestoneWall;

impl ItemDef for MossyCobblestoneWall {
    const ID: i32 = 428;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone_wall";
    const NAME: &'static str = "Mossy Cobblestone Wall";
    const STACK_SIZE: u8 = 64;
}

/// Brick Wall
pub struct BrickWall;

impl ItemDef for BrickWall {
    const ID: i32 = 429;
    const STRING_ID: &'static str = "minecraft:brick_wall";
    const NAME: &'static str = "Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Prismarine Wall
pub struct PrismarineWall;

impl ItemDef for PrismarineWall {
    const ID: i32 = 430;
    const STRING_ID: &'static str = "minecraft:prismarine_wall";
    const NAME: &'static str = "Prismarine Wall";
    const STACK_SIZE: u8 = 64;
}

/// Red Sandstone Wall
pub struct RedSandstoneWall;

impl ItemDef for RedSandstoneWall {
    const ID: i32 = 431;
    const STRING_ID: &'static str = "minecraft:red_sandstone_wall";
    const NAME: &'static str = "Red Sandstone Wall";
    const STACK_SIZE: u8 = 64;
}

/// Mossy Stone Brick Wall
pub struct MossyStoneBrickWall;

impl ItemDef for MossyStoneBrickWall {
    const ID: i32 = 432;
    const STRING_ID: &'static str = "minecraft:mossy_stone_brick_wall";
    const NAME: &'static str = "Mossy Stone Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Granite Wall
pub struct GraniteWall;

impl ItemDef for GraniteWall {
    const ID: i32 = 433;
    const STRING_ID: &'static str = "minecraft:granite_wall";
    const NAME: &'static str = "Granite Wall";
    const STACK_SIZE: u8 = 64;
}

/// Stone Brick Wall
pub struct StoneBrickWall;

impl ItemDef for StoneBrickWall {
    const ID: i32 = 434;
    const STRING_ID: &'static str = "minecraft:stone_brick_wall";
    const NAME: &'static str = "Stone Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Mud Brick Wall
pub struct MudBrickWall;

impl ItemDef for MudBrickWall {
    const ID: i32 = 435;
    const STRING_ID: &'static str = "minecraft:mud_brick_wall";
    const NAME: &'static str = "Mud Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Nether Brick Wall
pub struct NetherBrickWall;

impl ItemDef for NetherBrickWall {
    const ID: i32 = 436;
    const STRING_ID: &'static str = "minecraft:nether_brick_wall";
    const NAME: &'static str = "Nether Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Andesite Wall
pub struct AndesiteWall;

impl ItemDef for AndesiteWall {
    const ID: i32 = 437;
    const STRING_ID: &'static str = "minecraft:andesite_wall";
    const NAME: &'static str = "Andesite Wall";
    const STACK_SIZE: u8 = 64;
}

/// Red Nether Brick Wall
pub struct RedNetherBrickWall;

impl ItemDef for RedNetherBrickWall {
    const ID: i32 = 438;
    const STRING_ID: &'static str = "minecraft:red_nether_brick_wall";
    const NAME: &'static str = "Red Nether Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Sandstone Wall
pub struct SandstoneWall;

impl ItemDef for SandstoneWall {
    const ID: i32 = 439;
    const STRING_ID: &'static str = "minecraft:sandstone_wall";
    const NAME: &'static str = "Sandstone Wall";
    const STACK_SIZE: u8 = 64;
}

/// End Stone Brick Wall
pub struct EndStoneBrickWall;

impl ItemDef for EndStoneBrickWall {
    const ID: i32 = 440;
    const STRING_ID: &'static str = "minecraft:end_stone_brick_wall";
    const NAME: &'static str = "End Stone Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Diorite Wall
pub struct DioriteWall;

impl ItemDef for DioriteWall {
    const ID: i32 = 441;
    const STRING_ID: &'static str = "minecraft:diorite_wall";
    const NAME: &'static str = "Diorite Wall";
    const STACK_SIZE: u8 = 64;
}

/// Blackstone Wall
pub struct BlackstoneWall;

impl ItemDef for BlackstoneWall {
    const ID: i32 = 442;
    const STRING_ID: &'static str = "minecraft:blackstone_wall";
    const NAME: &'static str = "Blackstone Wall";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone Wall
pub struct PolishedBlackstoneWall;

impl ItemDef for PolishedBlackstoneWall {
    const ID: i32 = 443;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_wall";
    const NAME: &'static str = "Polished Blackstone Wall";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone Brick Wall
pub struct PolishedBlackstoneBrickWall;

impl ItemDef for PolishedBlackstoneBrickWall {
    const ID: i32 = 444;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_brick_wall";
    const NAME: &'static str = "Polished Blackstone Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Cobbled Deepslate Wall
pub struct CobbledDeepslateWall;

impl ItemDef for CobbledDeepslateWall {
    const ID: i32 = 445;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate_wall";
    const NAME: &'static str = "Cobbled Deepslate Wall";
    const STACK_SIZE: u8 = 64;
}

/// Polished Deepslate Wall
pub struct PolishedDeepslateWall;

impl ItemDef for PolishedDeepslateWall {
    const ID: i32 = 446;
    const STRING_ID: &'static str = "minecraft:polished_deepslate_wall";
    const NAME: &'static str = "Polished Deepslate Wall";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Brick Wall
pub struct DeepslateBrickWall;

impl ItemDef for DeepslateBrickWall {
    const ID: i32 = 447;
    const STRING_ID: &'static str = "minecraft:deepslate_brick_wall";
    const NAME: &'static str = "Deepslate Brick Wall";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Tile Wall
pub struct DeepslateTileWall;

impl ItemDef for DeepslateTileWall {
    const ID: i32 = 448;
    const STRING_ID: &'static str = "minecraft:deepslate_tile_wall";
    const NAME: &'static str = "Deepslate Tile Wall";
    const STACK_SIZE: u8 = 64;
}

/// Anvil
pub struct Anvil;

impl ItemDef for Anvil {
    const ID: i32 = 449;
    const STRING_ID: &'static str = "minecraft:anvil";
    const NAME: &'static str = "Anvil";
    const STACK_SIZE: u8 = 64;
}

/// Chipped Anvil
pub struct ChippedAnvil;

impl ItemDef for ChippedAnvil {
    const ID: i32 = 450;
    const STRING_ID: &'static str = "minecraft:chipped_anvil";
    const NAME: &'static str = "Chipped Anvil";
    const STACK_SIZE: u8 = 64;
}

/// Damaged Anvil
pub struct DamagedAnvil;

impl ItemDef for DamagedAnvil {
    const ID: i32 = 451;
    const STRING_ID: &'static str = "minecraft:damaged_anvil";
    const NAME: &'static str = "Damaged Anvil";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Quartz Block
pub struct ChiseledQuartzBlock;

impl ItemDef for ChiseledQuartzBlock {
    const ID: i32 = 452;
    const STRING_ID: &'static str = "minecraft:chiseled_quartz_block";
    const NAME: &'static str = "Chiseled Quartz Block";
    const STACK_SIZE: u8 = 64;
}

/// Block of Quartz
pub struct QuartzBlock;

impl ItemDef for QuartzBlock {
    const ID: i32 = 453;
    const STRING_ID: &'static str = "minecraft:quartz_block";
    const NAME: &'static str = "Block of Quartz";
    const STACK_SIZE: u8 = 64;
}

/// Quartz Bricks
pub struct QuartzBricks;

impl ItemDef for QuartzBricks {
    const ID: i32 = 454;
    const STRING_ID: &'static str = "minecraft:quartz_bricks";
    const NAME: &'static str = "Quartz Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Quartz Pillar
pub struct QuartzPillar;

impl ItemDef for QuartzPillar {
    const ID: i32 = 455;
    const STRING_ID: &'static str = "minecraft:quartz_pillar";
    const NAME: &'static str = "Quartz Pillar";
    const STACK_SIZE: u8 = 64;
}

/// Quartz Stairs
pub struct QuartzStairs;

impl ItemDef for QuartzStairs {
    const ID: i32 = 456;
    const STRING_ID: &'static str = "minecraft:quartz_stairs";
    const NAME: &'static str = "Quartz Stairs";
    const STACK_SIZE: u8 = 64;
}

/// White Terracotta
pub struct WhiteTerracotta;

impl ItemDef for WhiteTerracotta {
    const ID: i32 = 457;
    const STRING_ID: &'static str = "minecraft:white_terracotta";
    const NAME: &'static str = "White Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Orange Terracotta
pub struct OrangeTerracotta;

impl ItemDef for OrangeTerracotta {
    const ID: i32 = 458;
    const STRING_ID: &'static str = "minecraft:orange_terracotta";
    const NAME: &'static str = "Orange Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Terracotta
pub struct MagentaTerracotta;

impl ItemDef for MagentaTerracotta {
    const ID: i32 = 459;
    const STRING_ID: &'static str = "minecraft:magenta_terracotta";
    const NAME: &'static str = "Magenta Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Terracotta
pub struct LightBlueTerracotta;

impl ItemDef for LightBlueTerracotta {
    const ID: i32 = 460;
    const STRING_ID: &'static str = "minecraft:light_blue_terracotta";
    const NAME: &'static str = "Light Blue Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Terracotta
pub struct YellowTerracotta;

impl ItemDef for YellowTerracotta {
    const ID: i32 = 461;
    const STRING_ID: &'static str = "minecraft:yellow_terracotta";
    const NAME: &'static str = "Yellow Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Lime Terracotta
pub struct LimeTerracotta;

impl ItemDef for LimeTerracotta {
    const ID: i32 = 462;
    const STRING_ID: &'static str = "minecraft:lime_terracotta";
    const NAME: &'static str = "Lime Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Pink Terracotta
pub struct PinkTerracotta;

impl ItemDef for PinkTerracotta {
    const ID: i32 = 463;
    const STRING_ID: &'static str = "minecraft:pink_terracotta";
    const NAME: &'static str = "Pink Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Gray Terracotta
pub struct GrayTerracotta;

impl ItemDef for GrayTerracotta {
    const ID: i32 = 464;
    const STRING_ID: &'static str = "minecraft:gray_terracotta";
    const NAME: &'static str = "Gray Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Terracotta
pub struct LightGrayTerracotta;

impl ItemDef for LightGrayTerracotta {
    const ID: i32 = 465;
    const STRING_ID: &'static str = "minecraft:light_gray_terracotta";
    const NAME: &'static str = "Light Gray Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Terracotta
pub struct CyanTerracotta;

impl ItemDef for CyanTerracotta {
    const ID: i32 = 466;
    const STRING_ID: &'static str = "minecraft:cyan_terracotta";
    const NAME: &'static str = "Cyan Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Purple Terracotta
pub struct PurpleTerracotta;

impl ItemDef for PurpleTerracotta {
    const ID: i32 = 467;
    const STRING_ID: &'static str = "minecraft:purple_terracotta";
    const NAME: &'static str = "Purple Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Blue Terracotta
pub struct BlueTerracotta;

impl ItemDef for BlueTerracotta {
    const ID: i32 = 468;
    const STRING_ID: &'static str = "minecraft:blue_terracotta";
    const NAME: &'static str = "Blue Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Brown Terracotta
pub struct BrownTerracotta;

impl ItemDef for BrownTerracotta {
    const ID: i32 = 469;
    const STRING_ID: &'static str = "minecraft:brown_terracotta";
    const NAME: &'static str = "Brown Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Green Terracotta
pub struct GreenTerracotta;

impl ItemDef for GreenTerracotta {
    const ID: i32 = 470;
    const STRING_ID: &'static str = "minecraft:green_terracotta";
    const NAME: &'static str = "Green Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Red Terracotta
pub struct RedTerracotta;

impl ItemDef for RedTerracotta {
    const ID: i32 = 471;
    const STRING_ID: &'static str = "minecraft:red_terracotta";
    const NAME: &'static str = "Red Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Black Terracotta
pub struct BlackTerracotta;

impl ItemDef for BlackTerracotta {
    const ID: i32 = 472;
    const STRING_ID: &'static str = "minecraft:black_terracotta";
    const NAME: &'static str = "Black Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Barrier
pub struct Barrier;

impl ItemDef for Barrier {
    const ID: i32 = 473;
    const STRING_ID: &'static str = "minecraft:barrier";
    const NAME: &'static str = "Barrier";
    const STACK_SIZE: u8 = 64;
}

/// Light
pub struct LightBlock;

impl ItemDef for LightBlock {
    const ID: i32 = 474;
    const STRING_ID: &'static str = "minecraft:light_block";
    const NAME: &'static str = "Light";
    const STACK_SIZE: u8 = 64;
}

/// Hay Bale
pub struct HayBlock;

impl ItemDef for HayBlock {
    const ID: i32 = 475;
    const STRING_ID: &'static str = "minecraft:hay_block";
    const NAME: &'static str = "Hay Bale";
    const STACK_SIZE: u8 = 64;
}

/// White Carpet
pub struct WhiteCarpet;

impl ItemDef for WhiteCarpet {
    const ID: i32 = 476;
    const STRING_ID: &'static str = "minecraft:white_carpet";
    const NAME: &'static str = "White Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Orange Carpet
pub struct OrangeCarpet;

impl ItemDef for OrangeCarpet {
    const ID: i32 = 477;
    const STRING_ID: &'static str = "minecraft:orange_carpet";
    const NAME: &'static str = "Orange Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Carpet
pub struct MagentaCarpet;

impl ItemDef for MagentaCarpet {
    const ID: i32 = 478;
    const STRING_ID: &'static str = "minecraft:magenta_carpet";
    const NAME: &'static str = "Magenta Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Carpet
pub struct LightBlueCarpet;

impl ItemDef for LightBlueCarpet {
    const ID: i32 = 479;
    const STRING_ID: &'static str = "minecraft:light_blue_carpet";
    const NAME: &'static str = "Light Blue Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Carpet
pub struct YellowCarpet;

impl ItemDef for YellowCarpet {
    const ID: i32 = 480;
    const STRING_ID: &'static str = "minecraft:yellow_carpet";
    const NAME: &'static str = "Yellow Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Lime Carpet
pub struct LimeCarpet;

impl ItemDef for LimeCarpet {
    const ID: i32 = 481;
    const STRING_ID: &'static str = "minecraft:lime_carpet";
    const NAME: &'static str = "Lime Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Pink Carpet
pub struct PinkCarpet;

impl ItemDef for PinkCarpet {
    const ID: i32 = 482;
    const STRING_ID: &'static str = "minecraft:pink_carpet";
    const NAME: &'static str = "Pink Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Gray Carpet
pub struct GrayCarpet;

impl ItemDef for GrayCarpet {
    const ID: i32 = 483;
    const STRING_ID: &'static str = "minecraft:gray_carpet";
    const NAME: &'static str = "Gray Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Carpet
pub struct LightGrayCarpet;

impl ItemDef for LightGrayCarpet {
    const ID: i32 = 484;
    const STRING_ID: &'static str = "minecraft:light_gray_carpet";
    const NAME: &'static str = "Light Gray Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Carpet
pub struct CyanCarpet;

impl ItemDef for CyanCarpet {
    const ID: i32 = 485;
    const STRING_ID: &'static str = "minecraft:cyan_carpet";
    const NAME: &'static str = "Cyan Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Purple Carpet
pub struct PurpleCarpet;

impl ItemDef for PurpleCarpet {
    const ID: i32 = 486;
    const STRING_ID: &'static str = "minecraft:purple_carpet";
    const NAME: &'static str = "Purple Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Blue Carpet
pub struct BlueCarpet;

impl ItemDef for BlueCarpet {
    const ID: i32 = 487;
    const STRING_ID: &'static str = "minecraft:blue_carpet";
    const NAME: &'static str = "Blue Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Brown Carpet
pub struct BrownCarpet;

impl ItemDef for BrownCarpet {
    const ID: i32 = 488;
    const STRING_ID: &'static str = "minecraft:brown_carpet";
    const NAME: &'static str = "Brown Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Green Carpet
pub struct GreenCarpet;

impl ItemDef for GreenCarpet {
    const ID: i32 = 489;
    const STRING_ID: &'static str = "minecraft:green_carpet";
    const NAME: &'static str = "Green Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Red Carpet
pub struct RedCarpet;

impl ItemDef for RedCarpet {
    const ID: i32 = 490;
    const STRING_ID: &'static str = "minecraft:red_carpet";
    const NAME: &'static str = "Red Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Black Carpet
pub struct BlackCarpet;

impl ItemDef for BlackCarpet {
    const ID: i32 = 491;
    const STRING_ID: &'static str = "minecraft:black_carpet";
    const NAME: &'static str = "Black Carpet";
    const STACK_SIZE: u8 = 64;
}

/// Terracotta
pub struct HardenedClay;

impl ItemDef for HardenedClay {
    const ID: i32 = 492;
    const STRING_ID: &'static str = "minecraft:hardened_clay";
    const NAME: &'static str = "Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Packed Ice
pub struct PackedIce;

impl ItemDef for PackedIce {
    const ID: i32 = 493;
    const STRING_ID: &'static str = "minecraft:packed_ice";
    const NAME: &'static str = "Packed Ice";
    const STACK_SIZE: u8 = 64;
}

/// Dirt Path
pub struct GrassPath;

impl ItemDef for GrassPath {
    const ID: i32 = 494;
    const STRING_ID: &'static str = "minecraft:grass_path";
    const NAME: &'static str = "Dirt Path";
    const STACK_SIZE: u8 = 64;
}

/// Sunflower
pub struct Sunflower;

impl ItemDef for Sunflower {
    const ID: i32 = 495;
    const STRING_ID: &'static str = "minecraft:sunflower";
    const NAME: &'static str = "Sunflower";
    const STACK_SIZE: u8 = 64;
}

/// Lilac
pub struct Lilac;

impl ItemDef for Lilac {
    const ID: i32 = 496;
    const STRING_ID: &'static str = "minecraft:lilac";
    const NAME: &'static str = "Lilac";
    const STACK_SIZE: u8 = 64;
}

/// Rose Bush
pub struct RoseBush;

impl ItemDef for RoseBush {
    const ID: i32 = 497;
    const STRING_ID: &'static str = "minecraft:rose_bush";
    const NAME: &'static str = "Rose Bush";
    const STACK_SIZE: u8 = 64;
}

/// Peony
pub struct Peony;

impl ItemDef for Peony {
    const ID: i32 = 498;
    const STRING_ID: &'static str = "minecraft:peony";
    const NAME: &'static str = "Peony";
    const STACK_SIZE: u8 = 64;
}

/// Tall Grass
pub struct TallGrass;

impl ItemDef for TallGrass {
    const ID: i32 = 499;
    const STRING_ID: &'static str = "minecraft:tall_grass";
    const NAME: &'static str = "Tall Grass";
    const STACK_SIZE: u8 = 64;
}

/// Large Fern
pub struct LargeFern;

impl ItemDef for LargeFern {
    const ID: i32 = 500;
    const STRING_ID: &'static str = "minecraft:large_fern";
    const NAME: &'static str = "Large Fern";
    const STACK_SIZE: u8 = 64;
}

/// White Stained Glass
pub struct WhiteStainedGlass;

impl ItemDef for WhiteStainedGlass {
    const ID: i32 = 501;
    const STRING_ID: &'static str = "minecraft:white_stained_glass";
    const NAME: &'static str = "White Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Orange Stained Glass
pub struct OrangeStainedGlass;

impl ItemDef for OrangeStainedGlass {
    const ID: i32 = 502;
    const STRING_ID: &'static str = "minecraft:orange_stained_glass";
    const NAME: &'static str = "Orange Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Stained Glass
pub struct MagentaStainedGlass;

impl ItemDef for MagentaStainedGlass {
    const ID: i32 = 503;
    const STRING_ID: &'static str = "minecraft:magenta_stained_glass";
    const NAME: &'static str = "Magenta Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Stained Glass
pub struct LightBlueStainedGlass;

impl ItemDef for LightBlueStainedGlass {
    const ID: i32 = 504;
    const STRING_ID: &'static str = "minecraft:light_blue_stained_glass";
    const NAME: &'static str = "Light Blue Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Stained Glass
pub struct YellowStainedGlass;

impl ItemDef for YellowStainedGlass {
    const ID: i32 = 505;
    const STRING_ID: &'static str = "minecraft:yellow_stained_glass";
    const NAME: &'static str = "Yellow Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Lime Stained Glass
pub struct LimeStainedGlass;

impl ItemDef for LimeStainedGlass {
    const ID: i32 = 506;
    const STRING_ID: &'static str = "minecraft:lime_stained_glass";
    const NAME: &'static str = "Lime Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Pink Stained Glass
pub struct PinkStainedGlass;

impl ItemDef for PinkStainedGlass {
    const ID: i32 = 507;
    const STRING_ID: &'static str = "minecraft:pink_stained_glass";
    const NAME: &'static str = "Pink Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Gray Stained Glass
pub struct GrayStainedGlass;

impl ItemDef for GrayStainedGlass {
    const ID: i32 = 508;
    const STRING_ID: &'static str = "minecraft:gray_stained_glass";
    const NAME: &'static str = "Gray Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Stained Glass
pub struct LightGrayStainedGlass;

impl ItemDef for LightGrayStainedGlass {
    const ID: i32 = 509;
    const STRING_ID: &'static str = "minecraft:light_gray_stained_glass";
    const NAME: &'static str = "Light Gray Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Stained Glass
pub struct CyanStainedGlass;

impl ItemDef for CyanStainedGlass {
    const ID: i32 = 510;
    const STRING_ID: &'static str = "minecraft:cyan_stained_glass";
    const NAME: &'static str = "Cyan Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Purple Stained Glass
pub struct PurpleStainedGlass;

impl ItemDef for PurpleStainedGlass {
    const ID: i32 = 511;
    const STRING_ID: &'static str = "minecraft:purple_stained_glass";
    const NAME: &'static str = "Purple Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Blue Stained Glass
pub struct BlueStainedGlass;

impl ItemDef for BlueStainedGlass {
    const ID: i32 = 512;
    const STRING_ID: &'static str = "minecraft:blue_stained_glass";
    const NAME: &'static str = "Blue Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Brown Stained Glass
pub struct BrownStainedGlass;

impl ItemDef for BrownStainedGlass {
    const ID: i32 = 513;
    const STRING_ID: &'static str = "minecraft:brown_stained_glass";
    const NAME: &'static str = "Brown Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Green Stained Glass
pub struct GreenStainedGlass;

impl ItemDef for GreenStainedGlass {
    const ID: i32 = 514;
    const STRING_ID: &'static str = "minecraft:green_stained_glass";
    const NAME: &'static str = "Green Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Red Stained Glass
pub struct RedStainedGlass;

impl ItemDef for RedStainedGlass {
    const ID: i32 = 515;
    const STRING_ID: &'static str = "minecraft:red_stained_glass";
    const NAME: &'static str = "Red Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// Black Stained Glass
pub struct BlackStainedGlass;

impl ItemDef for BlackStainedGlass {
    const ID: i32 = 516;
    const STRING_ID: &'static str = "minecraft:black_stained_glass";
    const NAME: &'static str = "Black Stained Glass";
    const STACK_SIZE: u8 = 64;
}

/// White Stained Glass Pane
pub struct WhiteStainedGlassPane;

impl ItemDef for WhiteStainedGlassPane {
    const ID: i32 = 517;
    const STRING_ID: &'static str = "minecraft:white_stained_glass_pane";
    const NAME: &'static str = "White Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Orange Stained Glass Pane
pub struct OrangeStainedGlassPane;

impl ItemDef for OrangeStainedGlassPane {
    const ID: i32 = 518;
    const STRING_ID: &'static str = "minecraft:orange_stained_glass_pane";
    const NAME: &'static str = "Orange Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Stained Glass Pane
pub struct MagentaStainedGlassPane;

impl ItemDef for MagentaStainedGlassPane {
    const ID: i32 = 519;
    const STRING_ID: &'static str = "minecraft:magenta_stained_glass_pane";
    const NAME: &'static str = "Magenta Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Stained Glass Pane
pub struct LightBlueStainedGlassPane;

impl ItemDef for LightBlueStainedGlassPane {
    const ID: i32 = 520;
    const STRING_ID: &'static str = "minecraft:light_blue_stained_glass_pane";
    const NAME: &'static str = "Light Blue Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Stained Glass Pane
pub struct YellowStainedGlassPane;

impl ItemDef for YellowStainedGlassPane {
    const ID: i32 = 521;
    const STRING_ID: &'static str = "minecraft:yellow_stained_glass_pane";
    const NAME: &'static str = "Yellow Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Lime Stained Glass Pane
pub struct LimeStainedGlassPane;

impl ItemDef for LimeStainedGlassPane {
    const ID: i32 = 522;
    const STRING_ID: &'static str = "minecraft:lime_stained_glass_pane";
    const NAME: &'static str = "Lime Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Pink Stained Glass Pane
pub struct PinkStainedGlassPane;

impl ItemDef for PinkStainedGlassPane {
    const ID: i32 = 523;
    const STRING_ID: &'static str = "minecraft:pink_stained_glass_pane";
    const NAME: &'static str = "Pink Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Gray Stained Glass Pane
pub struct GrayStainedGlassPane;

impl ItemDef for GrayStainedGlassPane {
    const ID: i32 = 524;
    const STRING_ID: &'static str = "minecraft:gray_stained_glass_pane";
    const NAME: &'static str = "Gray Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Stained Glass Pane
pub struct LightGrayStainedGlassPane;

impl ItemDef for LightGrayStainedGlassPane {
    const ID: i32 = 525;
    const STRING_ID: &'static str = "minecraft:light_gray_stained_glass_pane";
    const NAME: &'static str = "Light Gray Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Stained Glass Pane
pub struct CyanStainedGlassPane;

impl ItemDef for CyanStainedGlassPane {
    const ID: i32 = 526;
    const STRING_ID: &'static str = "minecraft:cyan_stained_glass_pane";
    const NAME: &'static str = "Cyan Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Purple Stained Glass Pane
pub struct PurpleStainedGlassPane;

impl ItemDef for PurpleStainedGlassPane {
    const ID: i32 = 527;
    const STRING_ID: &'static str = "minecraft:purple_stained_glass_pane";
    const NAME: &'static str = "Purple Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Blue Stained Glass Pane
pub struct BlueStainedGlassPane;

impl ItemDef for BlueStainedGlassPane {
    const ID: i32 = 528;
    const STRING_ID: &'static str = "minecraft:blue_stained_glass_pane";
    const NAME: &'static str = "Blue Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Brown Stained Glass Pane
pub struct BrownStainedGlassPane;

impl ItemDef for BrownStainedGlassPane {
    const ID: i32 = 529;
    const STRING_ID: &'static str = "minecraft:brown_stained_glass_pane";
    const NAME: &'static str = "Brown Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Green Stained Glass Pane
pub struct GreenStainedGlassPane;

impl ItemDef for GreenStainedGlassPane {
    const ID: i32 = 530;
    const STRING_ID: &'static str = "minecraft:green_stained_glass_pane";
    const NAME: &'static str = "Green Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Red Stained Glass Pane
pub struct RedStainedGlassPane;

impl ItemDef for RedStainedGlassPane {
    const ID: i32 = 531;
    const STRING_ID: &'static str = "minecraft:red_stained_glass_pane";
    const NAME: &'static str = "Red Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Black Stained Glass Pane
pub struct BlackStainedGlassPane;

impl ItemDef for BlackStainedGlassPane {
    const ID: i32 = 532;
    const STRING_ID: &'static str = "minecraft:black_stained_glass_pane";
    const NAME: &'static str = "Black Stained Glass Pane";
    const STACK_SIZE: u8 = 64;
}

/// Prismarine
pub struct Prismarine;

impl ItemDef for Prismarine {
    const ID: i32 = 533;
    const STRING_ID: &'static str = "minecraft:prismarine";
    const NAME: &'static str = "Prismarine";
    const STACK_SIZE: u8 = 64;
}

/// Prismarine Bricks
pub struct PrismarineBricks;

impl ItemDef for PrismarineBricks {
    const ID: i32 = 534;
    const STRING_ID: &'static str = "minecraft:prismarine_bricks";
    const NAME: &'static str = "Prismarine Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Dark Prismarine
pub struct DarkPrismarine;

impl ItemDef for DarkPrismarine {
    const ID: i32 = 535;
    const STRING_ID: &'static str = "minecraft:dark_prismarine";
    const NAME: &'static str = "Dark Prismarine";
    const STACK_SIZE: u8 = 64;
}

/// Prismarine Stairs
pub struct PrismarineStairs;

impl ItemDef for PrismarineStairs {
    const ID: i32 = 536;
    const STRING_ID: &'static str = "minecraft:prismarine_stairs";
    const NAME: &'static str = "Prismarine Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Prismarine Brick Stairs
pub struct PrismarineBricksStairs;

impl ItemDef for PrismarineBricksStairs {
    const ID: i32 = 537;
    const STRING_ID: &'static str = "minecraft:prismarine_bricks_stairs";
    const NAME: &'static str = "Prismarine Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Dark Prismarine Stairs
pub struct DarkPrismarineStairs;

impl ItemDef for DarkPrismarineStairs {
    const ID: i32 = 538;
    const STRING_ID: &'static str = "minecraft:dark_prismarine_stairs";
    const NAME: &'static str = "Dark Prismarine Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Sea Lantern
pub struct SeaLantern;

impl ItemDef for SeaLantern {
    const ID: i32 = 539;
    const STRING_ID: &'static str = "minecraft:sea_lantern";
    const NAME: &'static str = "Sea Lantern";
    const STACK_SIZE: u8 = 64;
}

/// Red Sandstone
pub struct RedSandstone;

impl ItemDef for RedSandstone {
    const ID: i32 = 540;
    const STRING_ID: &'static str = "minecraft:red_sandstone";
    const NAME: &'static str = "Red Sandstone";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Red Sandstone
pub struct ChiseledRedSandstone;

impl ItemDef for ChiseledRedSandstone {
    const ID: i32 = 541;
    const STRING_ID: &'static str = "minecraft:chiseled_red_sandstone";
    const NAME: &'static str = "Chiseled Red Sandstone";
    const STACK_SIZE: u8 = 64;
}

/// Cut Red Sandstone
pub struct CutRedSandstone;

impl ItemDef for CutRedSandstone {
    const ID: i32 = 542;
    const STRING_ID: &'static str = "minecraft:cut_red_sandstone";
    const NAME: &'static str = "Cut Red Sandstone";
    const STACK_SIZE: u8 = 64;
}

/// Red Sandstone Stairs
pub struct RedSandstoneStairs;

impl ItemDef for RedSandstoneStairs {
    const ID: i32 = 543;
    const STRING_ID: &'static str = "minecraft:red_sandstone_stairs";
    const NAME: &'static str = "Red Sandstone Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Repeating Command Block
pub struct RepeatingCommandBlock;

impl ItemDef for RepeatingCommandBlock {
    const ID: i32 = 544;
    const STRING_ID: &'static str = "minecraft:repeating_command_block";
    const NAME: &'static str = "Repeating Command Block";
    const STACK_SIZE: u8 = 64;
}

/// Chain Command Block
pub struct ChainCommandBlock;

impl ItemDef for ChainCommandBlock {
    const ID: i32 = 545;
    const STRING_ID: &'static str = "minecraft:chain_command_block";
    const NAME: &'static str = "Chain Command Block";
    const STACK_SIZE: u8 = 64;
}

/// Magma Block
pub struct Magma;

impl ItemDef for Magma {
    const ID: i32 = 546;
    const STRING_ID: &'static str = "minecraft:magma";
    const NAME: &'static str = "Magma Block";
    const STACK_SIZE: u8 = 64;
}

/// Nether Wart Block
pub struct NetherWartBlock;

impl ItemDef for NetherWartBlock {
    const ID: i32 = 547;
    const STRING_ID: &'static str = "minecraft:nether_wart_block";
    const NAME: &'static str = "Nether Wart Block";
    const STACK_SIZE: u8 = 64;
}

/// Warped Wart Block
pub struct WarpedWartBlock;

impl ItemDef for WarpedWartBlock {
    const ID: i32 = 548;
    const STRING_ID: &'static str = "minecraft:warped_wart_block";
    const NAME: &'static str = "Warped Wart Block";
    const STACK_SIZE: u8 = 64;
}

/// Red Nether Bricks
pub struct RedNetherBrick;

impl ItemDef for RedNetherBrick {
    const ID: i32 = 549;
    const STRING_ID: &'static str = "minecraft:red_nether_brick";
    const NAME: &'static str = "Red Nether Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Bone Block
pub struct BoneBlock;

impl ItemDef for BoneBlock {
    const ID: i32 = 550;
    const STRING_ID: &'static str = "minecraft:bone_block";
    const NAME: &'static str = "Bone Block";
    const STACK_SIZE: u8 = 64;
}

/// Structure Void
pub struct StructureVoid;

impl ItemDef for StructureVoid {
    const ID: i32 = 551;
    const STRING_ID: &'static str = "minecraft:structure_void";
    const NAME: &'static str = "Structure Void";
    const STACK_SIZE: u8 = 64;
}

/// Shulker Box
pub struct UndyedShulkerBox;

impl ItemDef for UndyedShulkerBox {
    const ID: i32 = 552;
    const STRING_ID: &'static str = "minecraft:undyed_shulker_box";
    const NAME: &'static str = "Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// White Shulker Box
pub struct WhiteShulkerBox;

impl ItemDef for WhiteShulkerBox {
    const ID: i32 = 553;
    const STRING_ID: &'static str = "minecraft:white_shulker_box";
    const NAME: &'static str = "White Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Orange Shulker Box
pub struct OrangeShulkerBox;

impl ItemDef for OrangeShulkerBox {
    const ID: i32 = 554;
    const STRING_ID: &'static str = "minecraft:orange_shulker_box";
    const NAME: &'static str = "Orange Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Magenta Shulker Box
pub struct MagentaShulkerBox;

impl ItemDef for MagentaShulkerBox {
    const ID: i32 = 555;
    const STRING_ID: &'static str = "minecraft:magenta_shulker_box";
    const NAME: &'static str = "Magenta Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Light Blue Shulker Box
pub struct LightBlueShulkerBox;

impl ItemDef for LightBlueShulkerBox {
    const ID: i32 = 556;
    const STRING_ID: &'static str = "minecraft:light_blue_shulker_box";
    const NAME: &'static str = "Light Blue Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Yellow Shulker Box
pub struct YellowShulkerBox;

impl ItemDef for YellowShulkerBox {
    const ID: i32 = 557;
    const STRING_ID: &'static str = "minecraft:yellow_shulker_box";
    const NAME: &'static str = "Yellow Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Lime Shulker Box
pub struct LimeShulkerBox;

impl ItemDef for LimeShulkerBox {
    const ID: i32 = 558;
    const STRING_ID: &'static str = "minecraft:lime_shulker_box";
    const NAME: &'static str = "Lime Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Pink Shulker Box
pub struct PinkShulkerBox;

impl ItemDef for PinkShulkerBox {
    const ID: i32 = 559;
    const STRING_ID: &'static str = "minecraft:pink_shulker_box";
    const NAME: &'static str = "Pink Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Gray Shulker Box
pub struct GrayShulkerBox;

impl ItemDef for GrayShulkerBox {
    const ID: i32 = 560;
    const STRING_ID: &'static str = "minecraft:gray_shulker_box";
    const NAME: &'static str = "Gray Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Light Gray Shulker Box
pub struct LightGrayShulkerBox;

impl ItemDef for LightGrayShulkerBox {
    const ID: i32 = 561;
    const STRING_ID: &'static str = "minecraft:light_gray_shulker_box";
    const NAME: &'static str = "Light Gray Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Cyan Shulker Box
pub struct CyanShulkerBox;

impl ItemDef for CyanShulkerBox {
    const ID: i32 = 562;
    const STRING_ID: &'static str = "minecraft:cyan_shulker_box";
    const NAME: &'static str = "Cyan Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Purple Shulker Box
pub struct PurpleShulkerBox;

impl ItemDef for PurpleShulkerBox {
    const ID: i32 = 563;
    const STRING_ID: &'static str = "minecraft:purple_shulker_box";
    const NAME: &'static str = "Purple Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Blue Shulker Box
pub struct BlueShulkerBox;

impl ItemDef for BlueShulkerBox {
    const ID: i32 = 564;
    const STRING_ID: &'static str = "minecraft:blue_shulker_box";
    const NAME: &'static str = "Blue Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Brown Shulker Box
pub struct BrownShulkerBox;

impl ItemDef for BrownShulkerBox {
    const ID: i32 = 565;
    const STRING_ID: &'static str = "minecraft:brown_shulker_box";
    const NAME: &'static str = "Brown Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Green Shulker Box
pub struct GreenShulkerBox;

impl ItemDef for GreenShulkerBox {
    const ID: i32 = 566;
    const STRING_ID: &'static str = "minecraft:green_shulker_box";
    const NAME: &'static str = "Green Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Red Shulker Box
pub struct RedShulkerBox;

impl ItemDef for RedShulkerBox {
    const ID: i32 = 567;
    const STRING_ID: &'static str = "minecraft:red_shulker_box";
    const NAME: &'static str = "Red Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Black Shulker Box
pub struct BlackShulkerBox;

impl ItemDef for BlackShulkerBox {
    const ID: i32 = 568;
    const STRING_ID: &'static str = "minecraft:black_shulker_box";
    const NAME: &'static str = "Black Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// White Glazed Terracotta
pub struct WhiteGlazedTerracotta;

impl ItemDef for WhiteGlazedTerracotta {
    const ID: i32 = 569;
    const STRING_ID: &'static str = "minecraft:white_glazed_terracotta";
    const NAME: &'static str = "White Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Orange Glazed Terracotta
pub struct OrangeGlazedTerracotta;

impl ItemDef for OrangeGlazedTerracotta {
    const ID: i32 = 570;
    const STRING_ID: &'static str = "minecraft:orange_glazed_terracotta";
    const NAME: &'static str = "Orange Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Glazed Terracotta
pub struct MagentaGlazedTerracotta;

impl ItemDef for MagentaGlazedTerracotta {
    const ID: i32 = 571;
    const STRING_ID: &'static str = "minecraft:magenta_glazed_terracotta";
    const NAME: &'static str = "Magenta Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Glazed Terracotta
pub struct LightBlueGlazedTerracotta;

impl ItemDef for LightBlueGlazedTerracotta {
    const ID: i32 = 572;
    const STRING_ID: &'static str = "minecraft:light_blue_glazed_terracotta";
    const NAME: &'static str = "Light Blue Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Glazed Terracotta
pub struct YellowGlazedTerracotta;

impl ItemDef for YellowGlazedTerracotta {
    const ID: i32 = 573;
    const STRING_ID: &'static str = "minecraft:yellow_glazed_terracotta";
    const NAME: &'static str = "Yellow Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Lime Glazed Terracotta
pub struct LimeGlazedTerracotta;

impl ItemDef for LimeGlazedTerracotta {
    const ID: i32 = 574;
    const STRING_ID: &'static str = "minecraft:lime_glazed_terracotta";
    const NAME: &'static str = "Lime Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Pink Glazed Terracotta
pub struct PinkGlazedTerracotta;

impl ItemDef for PinkGlazedTerracotta {
    const ID: i32 = 575;
    const STRING_ID: &'static str = "minecraft:pink_glazed_terracotta";
    const NAME: &'static str = "Pink Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Gray Glazed Terracotta
pub struct GrayGlazedTerracotta;

impl ItemDef for GrayGlazedTerracotta {
    const ID: i32 = 576;
    const STRING_ID: &'static str = "minecraft:gray_glazed_terracotta";
    const NAME: &'static str = "Gray Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Glazed Terracotta
pub struct SilverGlazedTerracotta;

impl ItemDef for SilverGlazedTerracotta {
    const ID: i32 = 577;
    const STRING_ID: &'static str = "minecraft:silver_glazed_terracotta";
    const NAME: &'static str = "Light Gray Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Glazed Terracotta
pub struct CyanGlazedTerracotta;

impl ItemDef for CyanGlazedTerracotta {
    const ID: i32 = 578;
    const STRING_ID: &'static str = "minecraft:cyan_glazed_terracotta";
    const NAME: &'static str = "Cyan Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Purple Glazed Terracotta
pub struct PurpleGlazedTerracotta;

impl ItemDef for PurpleGlazedTerracotta {
    const ID: i32 = 579;
    const STRING_ID: &'static str = "minecraft:purple_glazed_terracotta";
    const NAME: &'static str = "Purple Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Blue Glazed Terracotta
pub struct BlueGlazedTerracotta;

impl ItemDef for BlueGlazedTerracotta {
    const ID: i32 = 580;
    const STRING_ID: &'static str = "minecraft:blue_glazed_terracotta";
    const NAME: &'static str = "Blue Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Brown Glazed Terracotta
pub struct BrownGlazedTerracotta;

impl ItemDef for BrownGlazedTerracotta {
    const ID: i32 = 581;
    const STRING_ID: &'static str = "minecraft:brown_glazed_terracotta";
    const NAME: &'static str = "Brown Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Green Glazed Terracotta
pub struct GreenGlazedTerracotta;

impl ItemDef for GreenGlazedTerracotta {
    const ID: i32 = 582;
    const STRING_ID: &'static str = "minecraft:green_glazed_terracotta";
    const NAME: &'static str = "Green Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Red Glazed Terracotta
pub struct RedGlazedTerracotta;

impl ItemDef for RedGlazedTerracotta {
    const ID: i32 = 583;
    const STRING_ID: &'static str = "minecraft:red_glazed_terracotta";
    const NAME: &'static str = "Red Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// Black Glazed Terracotta
pub struct BlackGlazedTerracotta;

impl ItemDef for BlackGlazedTerracotta {
    const ID: i32 = 584;
    const STRING_ID: &'static str = "minecraft:black_glazed_terracotta";
    const NAME: &'static str = "Black Glazed Terracotta";
    const STACK_SIZE: u8 = 64;
}

/// White Concrete
pub struct WhiteConcrete;

impl ItemDef for WhiteConcrete {
    const ID: i32 = 585;
    const STRING_ID: &'static str = "minecraft:white_concrete";
    const NAME: &'static str = "White Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Orange Concrete
pub struct OrangeConcrete;

impl ItemDef for OrangeConcrete {
    const ID: i32 = 586;
    const STRING_ID: &'static str = "minecraft:orange_concrete";
    const NAME: &'static str = "Orange Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Concrete
pub struct MagentaConcrete;

impl ItemDef for MagentaConcrete {
    const ID: i32 = 587;
    const STRING_ID: &'static str = "minecraft:magenta_concrete";
    const NAME: &'static str = "Magenta Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Concrete
pub struct LightBlueConcrete;

impl ItemDef for LightBlueConcrete {
    const ID: i32 = 588;
    const STRING_ID: &'static str = "minecraft:light_blue_concrete";
    const NAME: &'static str = "Light Blue Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Concrete
pub struct YellowConcrete;

impl ItemDef for YellowConcrete {
    const ID: i32 = 589;
    const STRING_ID: &'static str = "minecraft:yellow_concrete";
    const NAME: &'static str = "Yellow Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Lime Concrete
pub struct LimeConcrete;

impl ItemDef for LimeConcrete {
    const ID: i32 = 590;
    const STRING_ID: &'static str = "minecraft:lime_concrete";
    const NAME: &'static str = "Lime Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Pink Concrete
pub struct PinkConcrete;

impl ItemDef for PinkConcrete {
    const ID: i32 = 591;
    const STRING_ID: &'static str = "minecraft:pink_concrete";
    const NAME: &'static str = "Pink Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Gray Concrete
pub struct GrayConcrete;

impl ItemDef for GrayConcrete {
    const ID: i32 = 592;
    const STRING_ID: &'static str = "minecraft:gray_concrete";
    const NAME: &'static str = "Gray Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Concrete
pub struct LightGrayConcrete;

impl ItemDef for LightGrayConcrete {
    const ID: i32 = 593;
    const STRING_ID: &'static str = "minecraft:light_gray_concrete";
    const NAME: &'static str = "Light Gray Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Concrete
pub struct CyanConcrete;

impl ItemDef for CyanConcrete {
    const ID: i32 = 594;
    const STRING_ID: &'static str = "minecraft:cyan_concrete";
    const NAME: &'static str = "Cyan Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Purple Concrete
pub struct PurpleConcrete;

impl ItemDef for PurpleConcrete {
    const ID: i32 = 595;
    const STRING_ID: &'static str = "minecraft:purple_concrete";
    const NAME: &'static str = "Purple Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Blue Concrete
pub struct BlueConcrete;

impl ItemDef for BlueConcrete {
    const ID: i32 = 596;
    const STRING_ID: &'static str = "minecraft:blue_concrete";
    const NAME: &'static str = "Blue Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Brown Concrete
pub struct BrownConcrete;

impl ItemDef for BrownConcrete {
    const ID: i32 = 597;
    const STRING_ID: &'static str = "minecraft:brown_concrete";
    const NAME: &'static str = "Brown Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Green Concrete
pub struct GreenConcrete;

impl ItemDef for GreenConcrete {
    const ID: i32 = 598;
    const STRING_ID: &'static str = "minecraft:green_concrete";
    const NAME: &'static str = "Green Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Red Concrete
pub struct RedConcrete;

impl ItemDef for RedConcrete {
    const ID: i32 = 599;
    const STRING_ID: &'static str = "minecraft:red_concrete";
    const NAME: &'static str = "Red Concrete";
    const STACK_SIZE: u8 = 64;
}

/// Black Concrete
pub struct BlackConcrete;

impl ItemDef for BlackConcrete {
    const ID: i32 = 600;
    const STRING_ID: &'static str = "minecraft:black_concrete";
    const NAME: &'static str = "Black Concrete";
    const STACK_SIZE: u8 = 64;
}

/// White Concrete Powder
pub struct WhiteConcretePowder;

impl ItemDef for WhiteConcretePowder {
    const ID: i32 = 601;
    const STRING_ID: &'static str = "minecraft:white_concrete_powder";
    const NAME: &'static str = "White Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Orange Concrete Powder
pub struct OrangeConcretePowder;

impl ItemDef for OrangeConcretePowder {
    const ID: i32 = 602;
    const STRING_ID: &'static str = "minecraft:orange_concrete_powder";
    const NAME: &'static str = "Orange Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Concrete Powder
pub struct MagentaConcretePowder;

impl ItemDef for MagentaConcretePowder {
    const ID: i32 = 603;
    const STRING_ID: &'static str = "minecraft:magenta_concrete_powder";
    const NAME: &'static str = "Magenta Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Concrete Powder
pub struct LightBlueConcretePowder;

impl ItemDef for LightBlueConcretePowder {
    const ID: i32 = 604;
    const STRING_ID: &'static str = "minecraft:light_blue_concrete_powder";
    const NAME: &'static str = "Light Blue Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Concrete Powder
pub struct YellowConcretePowder;

impl ItemDef for YellowConcretePowder {
    const ID: i32 = 605;
    const STRING_ID: &'static str = "minecraft:yellow_concrete_powder";
    const NAME: &'static str = "Yellow Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Lime Concrete Powder
pub struct LimeConcretePowder;

impl ItemDef for LimeConcretePowder {
    const ID: i32 = 606;
    const STRING_ID: &'static str = "minecraft:lime_concrete_powder";
    const NAME: &'static str = "Lime Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Pink Concrete Powder
pub struct PinkConcretePowder;

impl ItemDef for PinkConcretePowder {
    const ID: i32 = 607;
    const STRING_ID: &'static str = "minecraft:pink_concrete_powder";
    const NAME: &'static str = "Pink Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Gray Concrete Powder
pub struct GrayConcretePowder;

impl ItemDef for GrayConcretePowder {
    const ID: i32 = 608;
    const STRING_ID: &'static str = "minecraft:gray_concrete_powder";
    const NAME: &'static str = "Gray Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Concrete Powder
pub struct LightGrayConcretePowder;

impl ItemDef for LightGrayConcretePowder {
    const ID: i32 = 609;
    const STRING_ID: &'static str = "minecraft:light_gray_concrete_powder";
    const NAME: &'static str = "Light Gray Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Concrete Powder
pub struct CyanConcretePowder;

impl ItemDef for CyanConcretePowder {
    const ID: i32 = 610;
    const STRING_ID: &'static str = "minecraft:cyan_concrete_powder";
    const NAME: &'static str = "Cyan Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Purple Concrete Powder
pub struct PurpleConcretePowder;

impl ItemDef for PurpleConcretePowder {
    const ID: i32 = 611;
    const STRING_ID: &'static str = "minecraft:purple_concrete_powder";
    const NAME: &'static str = "Purple Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Blue Concrete Powder
pub struct BlueConcretePowder;

impl ItemDef for BlueConcretePowder {
    const ID: i32 = 612;
    const STRING_ID: &'static str = "minecraft:blue_concrete_powder";
    const NAME: &'static str = "Blue Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Brown Concrete Powder
pub struct BrownConcretePowder;

impl ItemDef for BrownConcretePowder {
    const ID: i32 = 613;
    const STRING_ID: &'static str = "minecraft:brown_concrete_powder";
    const NAME: &'static str = "Brown Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Green Concrete Powder
pub struct GreenConcretePowder;

impl ItemDef for GreenConcretePowder {
    const ID: i32 = 614;
    const STRING_ID: &'static str = "minecraft:green_concrete_powder";
    const NAME: &'static str = "Green Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Red Concrete Powder
pub struct RedConcretePowder;

impl ItemDef for RedConcretePowder {
    const ID: i32 = 615;
    const STRING_ID: &'static str = "minecraft:red_concrete_powder";
    const NAME: &'static str = "Red Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Black Concrete Powder
pub struct BlackConcretePowder;

impl ItemDef for BlackConcretePowder {
    const ID: i32 = 616;
    const STRING_ID: &'static str = "minecraft:black_concrete_powder";
    const NAME: &'static str = "Black Concrete Powder";
    const STACK_SIZE: u8 = 64;
}

/// Turtle Egg
pub struct TurtleEgg;

impl ItemDef for TurtleEgg {
    const ID: i32 = 617;
    const STRING_ID: &'static str = "minecraft:turtle_egg";
    const NAME: &'static str = "Turtle Egg";
    const STACK_SIZE: u8 = 64;
}

/// Sniffer Egg
pub struct SnifferEgg;

impl ItemDef for SnifferEgg {
    const ID: i32 = 618;
    const STRING_ID: &'static str = "minecraft:sniffer_egg";
    const NAME: &'static str = "Sniffer Egg";
    const STACK_SIZE: u8 = 64;
}

/// Dried Ghast
pub struct DriedGhast;

impl ItemDef for DriedGhast {
    const ID: i32 = 619;
    const STRING_ID: &'static str = "minecraft:dried_ghast";
    const NAME: &'static str = "Dried Ghast";
    const STACK_SIZE: u8 = 64;
}

/// Dead Tube Coral Block
pub struct DeadTubeCoralBlock;

impl ItemDef for DeadTubeCoralBlock {
    const ID: i32 = 620;
    const STRING_ID: &'static str = "minecraft:dead_tube_coral_block";
    const NAME: &'static str = "Dead Tube Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Dead Brain Coral Block
pub struct DeadBrainCoralBlock;

impl ItemDef for DeadBrainCoralBlock {
    const ID: i32 = 621;
    const STRING_ID: &'static str = "minecraft:dead_brain_coral_block";
    const NAME: &'static str = "Dead Brain Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Dead Bubble Coral Block
pub struct DeadBubbleCoralBlock;

impl ItemDef for DeadBubbleCoralBlock {
    const ID: i32 = 622;
    const STRING_ID: &'static str = "minecraft:dead_bubble_coral_block";
    const NAME: &'static str = "Dead Bubble Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Dead Fire Coral Block
pub struct DeadFireCoralBlock;

impl ItemDef for DeadFireCoralBlock {
    const ID: i32 = 623;
    const STRING_ID: &'static str = "minecraft:dead_fire_coral_block";
    const NAME: &'static str = "Dead Fire Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Dead Horn Coral Block
pub struct DeadHornCoralBlock;

impl ItemDef for DeadHornCoralBlock {
    const ID: i32 = 624;
    const STRING_ID: &'static str = "minecraft:dead_horn_coral_block";
    const NAME: &'static str = "Dead Horn Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Tube Coral Block
pub struct TubeCoralBlock;

impl ItemDef for TubeCoralBlock {
    const ID: i32 = 625;
    const STRING_ID: &'static str = "minecraft:tube_coral_block";
    const NAME: &'static str = "Tube Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Brain Coral Block
pub struct BrainCoralBlock;

impl ItemDef for BrainCoralBlock {
    const ID: i32 = 626;
    const STRING_ID: &'static str = "minecraft:brain_coral_block";
    const NAME: &'static str = "Brain Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Bubble Coral Block
pub struct BubbleCoralBlock;

impl ItemDef for BubbleCoralBlock {
    const ID: i32 = 627;
    const STRING_ID: &'static str = "minecraft:bubble_coral_block";
    const NAME: &'static str = "Bubble Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Fire Coral Block
pub struct FireCoralBlock;

impl ItemDef for FireCoralBlock {
    const ID: i32 = 628;
    const STRING_ID: &'static str = "minecraft:fire_coral_block";
    const NAME: &'static str = "Fire Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Horn Coral Block
pub struct HornCoralBlock;

impl ItemDef for HornCoralBlock {
    const ID: i32 = 629;
    const STRING_ID: &'static str = "minecraft:horn_coral_block";
    const NAME: &'static str = "Horn Coral Block";
    const STACK_SIZE: u8 = 64;
}

/// Tube Coral
pub struct TubeCoral;

impl ItemDef for TubeCoral {
    const ID: i32 = 630;
    const STRING_ID: &'static str = "minecraft:tube_coral";
    const NAME: &'static str = "Tube Coral";
    const STACK_SIZE: u8 = 64;
}

/// Brain Coral
pub struct BrainCoral;

impl ItemDef for BrainCoral {
    const ID: i32 = 631;
    const STRING_ID: &'static str = "minecraft:brain_coral";
    const NAME: &'static str = "Brain Coral";
    const STACK_SIZE: u8 = 64;
}

/// Bubble Coral
pub struct BubbleCoral;

impl ItemDef for BubbleCoral {
    const ID: i32 = 632;
    const STRING_ID: &'static str = "minecraft:bubble_coral";
    const NAME: &'static str = "Bubble Coral";
    const STACK_SIZE: u8 = 64;
}

/// Fire Coral
pub struct FireCoral;

impl ItemDef for FireCoral {
    const ID: i32 = 633;
    const STRING_ID: &'static str = "minecraft:fire_coral";
    const NAME: &'static str = "Fire Coral";
    const STACK_SIZE: u8 = 64;
}

/// Horn Coral
pub struct HornCoral;

impl ItemDef for HornCoral {
    const ID: i32 = 634;
    const STRING_ID: &'static str = "minecraft:horn_coral";
    const NAME: &'static str = "Horn Coral";
    const STACK_SIZE: u8 = 64;
}

/// Dead Brain Coral
pub struct DeadBrainCoral;

impl ItemDef for DeadBrainCoral {
    const ID: i32 = 635;
    const STRING_ID: &'static str = "minecraft:dead_brain_coral";
    const NAME: &'static str = "Dead Brain Coral";
    const STACK_SIZE: u8 = 64;
}

/// Dead Bubble Coral
pub struct DeadBubbleCoral;

impl ItemDef for DeadBubbleCoral {
    const ID: i32 = 636;
    const STRING_ID: &'static str = "minecraft:dead_bubble_coral";
    const NAME: &'static str = "Dead Bubble Coral";
    const STACK_SIZE: u8 = 64;
}

/// Dead Fire Coral
pub struct DeadFireCoral;

impl ItemDef for DeadFireCoral {
    const ID: i32 = 637;
    const STRING_ID: &'static str = "minecraft:dead_fire_coral";
    const NAME: &'static str = "Dead Fire Coral";
    const STACK_SIZE: u8 = 64;
}

/// Dead Horn Coral
pub struct DeadHornCoral;

impl ItemDef for DeadHornCoral {
    const ID: i32 = 638;
    const STRING_ID: &'static str = "minecraft:dead_horn_coral";
    const NAME: &'static str = "Dead Horn Coral";
    const STACK_SIZE: u8 = 64;
}

/// Dead Tube Coral
pub struct DeadTubeCoral;

impl ItemDef for DeadTubeCoral {
    const ID: i32 = 639;
    const STRING_ID: &'static str = "minecraft:dead_tube_coral";
    const NAME: &'static str = "Dead Tube Coral";
    const STACK_SIZE: u8 = 64;
}

/// Tube Coral Fan
pub struct TubeCoralFan;

impl ItemDef for TubeCoralFan {
    const ID: i32 = 640;
    const STRING_ID: &'static str = "minecraft:tube_coral_fan";
    const NAME: &'static str = "Tube Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Brain Coral Fan
pub struct BrainCoralFan;

impl ItemDef for BrainCoralFan {
    const ID: i32 = 641;
    const STRING_ID: &'static str = "minecraft:brain_coral_fan";
    const NAME: &'static str = "Brain Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Bubble Coral Fan
pub struct BubbleCoralFan;

impl ItemDef for BubbleCoralFan {
    const ID: i32 = 642;
    const STRING_ID: &'static str = "minecraft:bubble_coral_fan";
    const NAME: &'static str = "Bubble Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Fire Coral Fan
pub struct FireCoralFan;

impl ItemDef for FireCoralFan {
    const ID: i32 = 643;
    const STRING_ID: &'static str = "minecraft:fire_coral_fan";
    const NAME: &'static str = "Fire Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Horn Coral Fan
pub struct HornCoralFan;

impl ItemDef for HornCoralFan {
    const ID: i32 = 644;
    const STRING_ID: &'static str = "minecraft:horn_coral_fan";
    const NAME: &'static str = "Horn Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Dead Tube Coral Fan
pub struct DeadTubeCoralFan;

impl ItemDef for DeadTubeCoralFan {
    const ID: i32 = 645;
    const STRING_ID: &'static str = "minecraft:dead_tube_coral_fan";
    const NAME: &'static str = "Dead Tube Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Dead Brain Coral Fan
pub struct DeadBrainCoralFan;

impl ItemDef for DeadBrainCoralFan {
    const ID: i32 = 646;
    const STRING_ID: &'static str = "minecraft:dead_brain_coral_fan";
    const NAME: &'static str = "Dead Brain Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Dead Bubble Coral Fan
pub struct DeadBubbleCoralFan;

impl ItemDef for DeadBubbleCoralFan {
    const ID: i32 = 647;
    const STRING_ID: &'static str = "minecraft:dead_bubble_coral_fan";
    const NAME: &'static str = "Dead Bubble Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Dead Fire Coral Fan
pub struct DeadFireCoralFan;

impl ItemDef for DeadFireCoralFan {
    const ID: i32 = 648;
    const STRING_ID: &'static str = "minecraft:dead_fire_coral_fan";
    const NAME: &'static str = "Dead Fire Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Dead Horn Coral Fan
pub struct DeadHornCoralFan;

impl ItemDef for DeadHornCoralFan {
    const ID: i32 = 649;
    const STRING_ID: &'static str = "minecraft:dead_horn_coral_fan";
    const NAME: &'static str = "Dead Horn Coral Fan";
    const STACK_SIZE: u8 = 64;
}

/// Blue Ice
pub struct BlueIce;

impl ItemDef for BlueIce {
    const ID: i32 = 650;
    const STRING_ID: &'static str = "minecraft:blue_ice";
    const NAME: &'static str = "Blue Ice";
    const STACK_SIZE: u8 = 64;
}

/// Conduit
pub struct Conduit;

impl ItemDef for Conduit {
    const ID: i32 = 651;
    const STRING_ID: &'static str = "minecraft:conduit";
    const NAME: &'static str = "Conduit";
    const STACK_SIZE: u8 = 64;
}

/// Polished Granite Stairs
pub struct PolishedGraniteStairs;

impl ItemDef for PolishedGraniteStairs {
    const ID: i32 = 652;
    const STRING_ID: &'static str = "minecraft:polished_granite_stairs";
    const NAME: &'static str = "Polished Granite Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Red Sandstone Stairs
pub struct SmoothRedSandstoneStairs;

impl ItemDef for SmoothRedSandstoneStairs {
    const ID: i32 = 653;
    const STRING_ID: &'static str = "minecraft:smooth_red_sandstone_stairs";
    const NAME: &'static str = "Smooth Red Sandstone Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Mossy Stone Brick Stairs
pub struct MossyStoneBrickStairs;

impl ItemDef for MossyStoneBrickStairs {
    const ID: i32 = 654;
    const STRING_ID: &'static str = "minecraft:mossy_stone_brick_stairs";
    const NAME: &'static str = "Mossy Stone Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Polished Diorite Stairs
pub struct PolishedDioriteStairs;

impl ItemDef for PolishedDioriteStairs {
    const ID: i32 = 655;
    const STRING_ID: &'static str = "minecraft:polished_diorite_stairs";
    const NAME: &'static str = "Polished Diorite Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Mossy Cobblestone Stairs
pub struct MossyCobblestoneStairs;

impl ItemDef for MossyCobblestoneStairs {
    const ID: i32 = 656;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone_stairs";
    const NAME: &'static str = "Mossy Cobblestone Stairs";
    const STACK_SIZE: u8 = 64;
}

/// End Stone Brick Stairs
pub struct EndBrickStairs;

impl ItemDef for EndBrickStairs {
    const ID: i32 = 657;
    const STRING_ID: &'static str = "minecraft:end_brick_stairs";
    const NAME: &'static str = "End Stone Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Stone Stairs
pub struct NormalStoneStairs;

impl ItemDef for NormalStoneStairs {
    const ID: i32 = 658;
    const STRING_ID: &'static str = "minecraft:normal_stone_stairs";
    const NAME: &'static str = "Stone Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Sandstone Stairs
pub struct SmoothSandstoneStairs;

impl ItemDef for SmoothSandstoneStairs {
    const ID: i32 = 659;
    const STRING_ID: &'static str = "minecraft:smooth_sandstone_stairs";
    const NAME: &'static str = "Smooth Sandstone Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Quartz Stairs
pub struct SmoothQuartzStairs;

impl ItemDef for SmoothQuartzStairs {
    const ID: i32 = 660;
    const STRING_ID: &'static str = "minecraft:smooth_quartz_stairs";
    const NAME: &'static str = "Smooth Quartz Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Granite Stairs
pub struct GraniteStairs;

impl ItemDef for GraniteStairs {
    const ID: i32 = 661;
    const STRING_ID: &'static str = "minecraft:granite_stairs";
    const NAME: &'static str = "Granite Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Andesite Stairs
pub struct AndesiteStairs;

impl ItemDef for AndesiteStairs {
    const ID: i32 = 662;
    const STRING_ID: &'static str = "minecraft:andesite_stairs";
    const NAME: &'static str = "Andesite Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Red Nether Brick Stairs
pub struct RedNetherBrickStairs;

impl ItemDef for RedNetherBrickStairs {
    const ID: i32 = 663;
    const STRING_ID: &'static str = "minecraft:red_nether_brick_stairs";
    const NAME: &'static str = "Red Nether Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Polished Andesite Stairs
pub struct PolishedAndesiteStairs;

impl ItemDef for PolishedAndesiteStairs {
    const ID: i32 = 664;
    const STRING_ID: &'static str = "minecraft:polished_andesite_stairs";
    const NAME: &'static str = "Polished Andesite Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Diorite Stairs
pub struct DioriteStairs;

impl ItemDef for DioriteStairs {
    const ID: i32 = 665;
    const STRING_ID: &'static str = "minecraft:diorite_stairs";
    const NAME: &'static str = "Diorite Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Cobbled Deepslate Stairs
pub struct CobbledDeepslateStairs;

impl ItemDef for CobbledDeepslateStairs {
    const ID: i32 = 666;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate_stairs";
    const NAME: &'static str = "Cobbled Deepslate Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Polished Deepslate Stairs
pub struct PolishedDeepslateStairs;

impl ItemDef for PolishedDeepslateStairs {
    const ID: i32 = 667;
    const STRING_ID: &'static str = "minecraft:polished_deepslate_stairs";
    const NAME: &'static str = "Polished Deepslate Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Brick Stairs
pub struct DeepslateBrickStairs;

impl ItemDef for DeepslateBrickStairs {
    const ID: i32 = 668;
    const STRING_ID: &'static str = "minecraft:deepslate_brick_stairs";
    const NAME: &'static str = "Deepslate Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Tile Stairs
pub struct DeepslateTileStairs;

impl ItemDef for DeepslateTileStairs {
    const ID: i32 = 669;
    const STRING_ID: &'static str = "minecraft:deepslate_tile_stairs";
    const NAME: &'static str = "Deepslate Tile Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Polished Granite Slab
pub struct PolishedGraniteSlab;

impl ItemDef for PolishedGraniteSlab {
    const ID: i32 = 670;
    const STRING_ID: &'static str = "minecraft:polished_granite_slab";
    const NAME: &'static str = "Polished Granite Slab";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Red Sandstone Slab
pub struct SmoothRedSandstoneSlab;

impl ItemDef for SmoothRedSandstoneSlab {
    const ID: i32 = 671;
    const STRING_ID: &'static str = "minecraft:smooth_red_sandstone_slab";
    const NAME: &'static str = "Smooth Red Sandstone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Mossy Stone Brick Slab
pub struct MossyStoneBrickSlab;

impl ItemDef for MossyStoneBrickSlab {
    const ID: i32 = 672;
    const STRING_ID: &'static str = "minecraft:mossy_stone_brick_slab";
    const NAME: &'static str = "Mossy Stone Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Polished Diorite Slab
pub struct PolishedDioriteSlab;

impl ItemDef for PolishedDioriteSlab {
    const ID: i32 = 673;
    const STRING_ID: &'static str = "minecraft:polished_diorite_slab";
    const NAME: &'static str = "Polished Diorite Slab";
    const STACK_SIZE: u8 = 64;
}

/// Mossy Cobblestone Slab
pub struct MossyCobblestoneSlab;

impl ItemDef for MossyCobblestoneSlab {
    const ID: i32 = 674;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone_slab";
    const NAME: &'static str = "Mossy Cobblestone Slab";
    const STACK_SIZE: u8 = 64;
}

/// End Stone Brick Slab
pub struct EndStoneBrickSlab;

impl ItemDef for EndStoneBrickSlab {
    const ID: i32 = 675;
    const STRING_ID: &'static str = "minecraft:end_stone_brick_slab";
    const NAME: &'static str = "End Stone Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Sandstone Slab
pub struct SmoothSandstoneSlab;

impl ItemDef for SmoothSandstoneSlab {
    const ID: i32 = 676;
    const STRING_ID: &'static str = "minecraft:smooth_sandstone_slab";
    const NAME: &'static str = "Smooth Sandstone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Smooth Quartz Slab
pub struct SmoothQuartzSlab;

impl ItemDef for SmoothQuartzSlab {
    const ID: i32 = 677;
    const STRING_ID: &'static str = "minecraft:smooth_quartz_slab";
    const NAME: &'static str = "Smooth Quartz Slab";
    const STACK_SIZE: u8 = 64;
}

/// Granite Slab
pub struct GraniteSlab;

impl ItemDef for GraniteSlab {
    const ID: i32 = 678;
    const STRING_ID: &'static str = "minecraft:granite_slab";
    const NAME: &'static str = "Granite Slab";
    const STACK_SIZE: u8 = 64;
}

/// Andesite Slab
pub struct AndesiteSlab;

impl ItemDef for AndesiteSlab {
    const ID: i32 = 679;
    const STRING_ID: &'static str = "minecraft:andesite_slab";
    const NAME: &'static str = "Andesite Slab";
    const STACK_SIZE: u8 = 64;
}

/// Red Nether Brick Slab
pub struct RedNetherBrickSlab;

impl ItemDef for RedNetherBrickSlab {
    const ID: i32 = 680;
    const STRING_ID: &'static str = "minecraft:red_nether_brick_slab";
    const NAME: &'static str = "Red Nether Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Polished Andesite Slab
pub struct PolishedAndesiteSlab;

impl ItemDef for PolishedAndesiteSlab {
    const ID: i32 = 681;
    const STRING_ID: &'static str = "minecraft:polished_andesite_slab";
    const NAME: &'static str = "Polished Andesite Slab";
    const STACK_SIZE: u8 = 64;
}

/// Diorite Slab
pub struct DioriteSlab;

impl ItemDef for DioriteSlab {
    const ID: i32 = 682;
    const STRING_ID: &'static str = "minecraft:diorite_slab";
    const NAME: &'static str = "Diorite Slab";
    const STACK_SIZE: u8 = 64;
}

/// Cobbled Deepslate Slab
pub struct CobbledDeepslateSlab;

impl ItemDef for CobbledDeepslateSlab {
    const ID: i32 = 683;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate_slab";
    const NAME: &'static str = "Cobbled Deepslate Slab";
    const STACK_SIZE: u8 = 64;
}

/// Polished Deepslate Slab
pub struct PolishedDeepslateSlab;

impl ItemDef for PolishedDeepslateSlab {
    const ID: i32 = 684;
    const STRING_ID: &'static str = "minecraft:polished_deepslate_slab";
    const NAME: &'static str = "Polished Deepslate Slab";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Brick Slab
pub struct DeepslateBrickSlab;

impl ItemDef for DeepslateBrickSlab {
    const ID: i32 = 685;
    const STRING_ID: &'static str = "minecraft:deepslate_brick_slab";
    const NAME: &'static str = "Deepslate Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Deepslate Tile Slab
pub struct DeepslateTileSlab;

impl ItemDef for DeepslateTileSlab {
    const ID: i32 = 686;
    const STRING_ID: &'static str = "minecraft:deepslate_tile_slab";
    const NAME: &'static str = "Deepslate Tile Slab";
    const STACK_SIZE: u8 = 64;
}

/// Scaffolding
pub struct Scaffolding;

impl ItemDef for Scaffolding {
    const ID: i32 = 687;
    const STRING_ID: &'static str = "minecraft:scaffolding";
    const NAME: &'static str = "Scaffolding";
    const STACK_SIZE: u8 = 64;
}

/// Redstone Dust
pub struct Redstone;

impl ItemDef for Redstone {
    const ID: i32 = 688;
    const STRING_ID: &'static str = "minecraft:redstone";
    const NAME: &'static str = "Redstone Dust";
    const STACK_SIZE: u8 = 64;
}

/// Redstone Torch
pub struct RedstoneTorch;

impl ItemDef for RedstoneTorch {
    const ID: i32 = 689;
    const STRING_ID: &'static str = "minecraft:redstone_torch";
    const NAME: &'static str = "Redstone Torch";
    const STACK_SIZE: u8 = 64;
}

/// Block of Redstone
pub struct RedstoneBlock;

impl ItemDef for RedstoneBlock {
    const ID: i32 = 690;
    const STRING_ID: &'static str = "minecraft:redstone_block";
    const NAME: &'static str = "Block of Redstone";
    const STACK_SIZE: u8 = 64;
}

/// Redstone Repeater
pub struct Repeater;

impl ItemDef for Repeater {
    const ID: i32 = 691;
    const STRING_ID: &'static str = "minecraft:repeater";
    const NAME: &'static str = "Redstone Repeater";
    const STACK_SIZE: u8 = 64;
}

/// Redstone Comparator
pub struct Comparator;

impl ItemDef for Comparator {
    const ID: i32 = 692;
    const STRING_ID: &'static str = "minecraft:comparator";
    const NAME: &'static str = "Redstone Comparator";
    const STACK_SIZE: u8 = 64;
}

/// Piston
pub struct Piston;

impl ItemDef for Piston {
    const ID: i32 = 693;
    const STRING_ID: &'static str = "minecraft:piston";
    const NAME: &'static str = "Piston";
    const STACK_SIZE: u8 = 64;
}

/// Sticky Piston
pub struct StickyPiston;

impl ItemDef for StickyPiston {
    const ID: i32 = 694;
    const STRING_ID: &'static str = "minecraft:sticky_piston";
    const NAME: &'static str = "Sticky Piston";
    const STACK_SIZE: u8 = 64;
}

/// Slime Block
pub struct Slime;

impl ItemDef for Slime {
    const ID: i32 = 695;
    const STRING_ID: &'static str = "minecraft:slime";
    const NAME: &'static str = "Slime Block";
    const STACK_SIZE: u8 = 64;
}

/// Honey Block
pub struct HoneyBlock;

impl ItemDef for HoneyBlock {
    const ID: i32 = 696;
    const STRING_ID: &'static str = "minecraft:honey_block";
    const NAME: &'static str = "Honey Block";
    const STACK_SIZE: u8 = 64;
}

/// Observer
pub struct Observer;

impl ItemDef for Observer {
    const ID: i32 = 697;
    const STRING_ID: &'static str = "minecraft:observer";
    const NAME: &'static str = "Observer";
    const STACK_SIZE: u8 = 64;
}

/// Hopper
pub struct Hopper;

impl ItemDef for Hopper {
    const ID: i32 = 698;
    const STRING_ID: &'static str = "minecraft:hopper";
    const NAME: &'static str = "Hopper";
    const STACK_SIZE: u8 = 64;
}

/// Dispenser
pub struct Dispenser;

impl ItemDef for Dispenser {
    const ID: i32 = 699;
    const STRING_ID: &'static str = "minecraft:dispenser";
    const NAME: &'static str = "Dispenser";
    const STACK_SIZE: u8 = 64;
}

/// Dropper
pub struct Dropper;

impl ItemDef for Dropper {
    const ID: i32 = 700;
    const STRING_ID: &'static str = "minecraft:dropper";
    const NAME: &'static str = "Dropper";
    const STACK_SIZE: u8 = 64;
}

/// Lectern
pub struct Lectern;

impl ItemDef for Lectern {
    const ID: i32 = 701;
    const STRING_ID: &'static str = "minecraft:lectern";
    const NAME: &'static str = "Lectern";
    const STACK_SIZE: u8 = 64;
}

/// Target
pub struct Target;

impl ItemDef for Target {
    const ID: i32 = 702;
    const STRING_ID: &'static str = "minecraft:target";
    const NAME: &'static str = "Target";
    const STACK_SIZE: u8 = 64;
}

/// Lever
pub struct Lever;

impl ItemDef for Lever {
    const ID: i32 = 703;
    const STRING_ID: &'static str = "minecraft:lever";
    const NAME: &'static str = "Lever";
    const STACK_SIZE: u8 = 64;
}

/// Lightning Rod
pub struct LightningRod;

impl ItemDef for LightningRod {
    const ID: i32 = 704;
    const STRING_ID: &'static str = "minecraft:lightning_rod";
    const NAME: &'static str = "Lightning Rod";
    const STACK_SIZE: u8 = 64;
}

/// Daylight Detector
pub struct DaylightDetector;

impl ItemDef for DaylightDetector {
    const ID: i32 = 705;
    const STRING_ID: &'static str = "minecraft:daylight_detector";
    const NAME: &'static str = "Daylight Detector";
    const STACK_SIZE: u8 = 64;
}

/// Sculk Sensor
pub struct SculkSensor;

impl ItemDef for SculkSensor {
    const ID: i32 = 706;
    const STRING_ID: &'static str = "minecraft:sculk_sensor";
    const NAME: &'static str = "Sculk Sensor";
    const STACK_SIZE: u8 = 64;
}

/// Calibrated Sculk Sensor
pub struct CalibratedSculkSensor;

impl ItemDef for CalibratedSculkSensor {
    const ID: i32 = 707;
    const STRING_ID: &'static str = "minecraft:calibrated_sculk_sensor";
    const NAME: &'static str = "Calibrated Sculk Sensor";
    const STACK_SIZE: u8 = 64;
}

/// Tripwire Hook
pub struct TripwireHook;

impl ItemDef for TripwireHook {
    const ID: i32 = 708;
    const STRING_ID: &'static str = "minecraft:tripwire_hook";
    const NAME: &'static str = "Tripwire Hook";
    const STACK_SIZE: u8 = 64;
}

/// Trapped Chest
pub struct TrappedChest;

impl ItemDef for TrappedChest {
    const ID: i32 = 709;
    const STRING_ID: &'static str = "minecraft:trapped_chest";
    const NAME: &'static str = "Trapped Chest";
    const STACK_SIZE: u8 = 64;
}

/// TNT
pub struct Tnt;

impl ItemDef for Tnt {
    const ID: i32 = 710;
    const STRING_ID: &'static str = "minecraft:tnt";
    const NAME: &'static str = "TNT";
    const STACK_SIZE: u8 = 64;
}

/// Redstone Lamp
pub struct RedstoneLamp;

impl ItemDef for RedstoneLamp {
    const ID: i32 = 711;
    const STRING_ID: &'static str = "minecraft:redstone_lamp";
    const NAME: &'static str = "Redstone Lamp";
    const STACK_SIZE: u8 = 64;
}

/// Note Block
pub struct Noteblock;

impl ItemDef for Noteblock {
    const ID: i32 = 712;
    const STRING_ID: &'static str = "minecraft:noteblock";
    const NAME: &'static str = "Note Block";
    const STACK_SIZE: u8 = 64;
}

/// Stone Button
pub struct StoneButton;

impl ItemDef for StoneButton {
    const ID: i32 = 713;
    const STRING_ID: &'static str = "minecraft:stone_button";
    const NAME: &'static str = "Stone Button";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone Button
pub struct PolishedBlackstoneButton;

impl ItemDef for PolishedBlackstoneButton {
    const ID: i32 = 714;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_button";
    const NAME: &'static str = "Polished Blackstone Button";
    const STACK_SIZE: u8 = 64;
}

/// Oak Button
pub struct WoodenButton;

impl ItemDef for WoodenButton {
    const ID: i32 = 715;
    const STRING_ID: &'static str = "minecraft:wooden_button";
    const NAME: &'static str = "Oak Button";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Button
pub struct SpruceButton;

impl ItemDef for SpruceButton {
    const ID: i32 = 716;
    const STRING_ID: &'static str = "minecraft:spruce_button";
    const NAME: &'static str = "Spruce Button";
    const STACK_SIZE: u8 = 64;
}

/// Birch Button
pub struct BirchButton;

impl ItemDef for BirchButton {
    const ID: i32 = 717;
    const STRING_ID: &'static str = "minecraft:birch_button";
    const NAME: &'static str = "Birch Button";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Button
pub struct JungleButton;

impl ItemDef for JungleButton {
    const ID: i32 = 718;
    const STRING_ID: &'static str = "minecraft:jungle_button";
    const NAME: &'static str = "Jungle Button";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Button
pub struct AcaciaButton;

impl ItemDef for AcaciaButton {
    const ID: i32 = 719;
    const STRING_ID: &'static str = "minecraft:acacia_button";
    const NAME: &'static str = "Acacia Button";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Button
pub struct CherryButton;

impl ItemDef for CherryButton {
    const ID: i32 = 720;
    const STRING_ID: &'static str = "minecraft:cherry_button";
    const NAME: &'static str = "Cherry Button";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Button
pub struct DarkOakButton;

impl ItemDef for DarkOakButton {
    const ID: i32 = 721;
    const STRING_ID: &'static str = "minecraft:dark_oak_button";
    const NAME: &'static str = "Dark Oak Button";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Button
pub struct PaleOakButton;

impl ItemDef for PaleOakButton {
    const ID: i32 = 722;
    const STRING_ID: &'static str = "minecraft:pale_oak_button";
    const NAME: &'static str = "Pale Oak Button";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Button
pub struct MangroveButton;

impl ItemDef for MangroveButton {
    const ID: i32 = 723;
    const STRING_ID: &'static str = "minecraft:mangrove_button";
    const NAME: &'static str = "Mangrove Button";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Button
pub struct BambooButton;

impl ItemDef for BambooButton {
    const ID: i32 = 724;
    const STRING_ID: &'static str = "minecraft:bamboo_button";
    const NAME: &'static str = "Bamboo Button";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Button
pub struct CrimsonButton;

impl ItemDef for CrimsonButton {
    const ID: i32 = 725;
    const STRING_ID: &'static str = "minecraft:crimson_button";
    const NAME: &'static str = "Crimson Button";
    const STACK_SIZE: u8 = 64;
}

/// Warped Button
pub struct WarpedButton;

impl ItemDef for WarpedButton {
    const ID: i32 = 726;
    const STRING_ID: &'static str = "minecraft:warped_button";
    const NAME: &'static str = "Warped Button";
    const STACK_SIZE: u8 = 64;
}

/// Stone Pressure Plate
pub struct StonePressurePlate;

impl ItemDef for StonePressurePlate {
    const ID: i32 = 727;
    const STRING_ID: &'static str = "minecraft:stone_pressure_plate";
    const NAME: &'static str = "Stone Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone Pressure Plate
pub struct PolishedBlackstonePressurePlate;

impl ItemDef for PolishedBlackstonePressurePlate {
    const ID: i32 = 728;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_pressure_plate";
    const NAME: &'static str = "Polished Blackstone Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Light Weighted Pressure Plate
pub struct LightWeightedPressurePlate;

impl ItemDef for LightWeightedPressurePlate {
    const ID: i32 = 729;
    const STRING_ID: &'static str = "minecraft:light_weighted_pressure_plate";
    const NAME: &'static str = "Light Weighted Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Heavy Weighted Pressure Plate
pub struct HeavyWeightedPressurePlate;

impl ItemDef for HeavyWeightedPressurePlate {
    const ID: i32 = 730;
    const STRING_ID: &'static str = "minecraft:heavy_weighted_pressure_plate";
    const NAME: &'static str = "Heavy Weighted Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Oak Pressure Plate
pub struct WoodenPressurePlate;

impl ItemDef for WoodenPressurePlate {
    const ID: i32 = 731;
    const STRING_ID: &'static str = "minecraft:wooden_pressure_plate";
    const NAME: &'static str = "Oak Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Pressure Plate
pub struct SprucePressurePlate;

impl ItemDef for SprucePressurePlate {
    const ID: i32 = 732;
    const STRING_ID: &'static str = "minecraft:spruce_pressure_plate";
    const NAME: &'static str = "Spruce Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Birch Pressure Plate
pub struct BirchPressurePlate;

impl ItemDef for BirchPressurePlate {
    const ID: i32 = 733;
    const STRING_ID: &'static str = "minecraft:birch_pressure_plate";
    const NAME: &'static str = "Birch Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Pressure Plate
pub struct JunglePressurePlate;

impl ItemDef for JunglePressurePlate {
    const ID: i32 = 734;
    const STRING_ID: &'static str = "minecraft:jungle_pressure_plate";
    const NAME: &'static str = "Jungle Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Pressure Plate
pub struct AcaciaPressurePlate;

impl ItemDef for AcaciaPressurePlate {
    const ID: i32 = 735;
    const STRING_ID: &'static str = "minecraft:acacia_pressure_plate";
    const NAME: &'static str = "Acacia Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Pressure Plate
pub struct CherryPressurePlate;

impl ItemDef for CherryPressurePlate {
    const ID: i32 = 736;
    const STRING_ID: &'static str = "minecraft:cherry_pressure_plate";
    const NAME: &'static str = "Cherry Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Pressure Plate
pub struct DarkOakPressurePlate;

impl ItemDef for DarkOakPressurePlate {
    const ID: i32 = 737;
    const STRING_ID: &'static str = "minecraft:dark_oak_pressure_plate";
    const NAME: &'static str = "Dark Oak Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Pressure Plate
pub struct PaleOakPressurePlate;

impl ItemDef for PaleOakPressurePlate {
    const ID: i32 = 738;
    const STRING_ID: &'static str = "minecraft:pale_oak_pressure_plate";
    const NAME: &'static str = "Pale Oak Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Pressure Plate
pub struct MangrovePressurePlate;

impl ItemDef for MangrovePressurePlate {
    const ID: i32 = 739;
    const STRING_ID: &'static str = "minecraft:mangrove_pressure_plate";
    const NAME: &'static str = "Mangrove Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Pressure Plate
pub struct BambooPressurePlate;

impl ItemDef for BambooPressurePlate {
    const ID: i32 = 740;
    const STRING_ID: &'static str = "minecraft:bamboo_pressure_plate";
    const NAME: &'static str = "Bamboo Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Pressure Plate
pub struct CrimsonPressurePlate;

impl ItemDef for CrimsonPressurePlate {
    const ID: i32 = 741;
    const STRING_ID: &'static str = "minecraft:crimson_pressure_plate";
    const NAME: &'static str = "Crimson Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Warped Pressure Plate
pub struct WarpedPressurePlate;

impl ItemDef for WarpedPressurePlate {
    const ID: i32 = 742;
    const STRING_ID: &'static str = "minecraft:warped_pressure_plate";
    const NAME: &'static str = "Warped Pressure Plate";
    const STACK_SIZE: u8 = 64;
}

/// Iron Door
pub struct IronDoor;

impl ItemDef for IronDoor {
    const ID: i32 = 743;
    const STRING_ID: &'static str = "minecraft:iron_door";
    const NAME: &'static str = "Iron Door";
    const STACK_SIZE: u8 = 64;
}

/// Oak Door
pub struct WoodenDoor;

impl ItemDef for WoodenDoor {
    const ID: i32 = 744;
    const STRING_ID: &'static str = "minecraft:wooden_door";
    const NAME: &'static str = "Oak Door";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Door
pub struct SpruceDoor;

impl ItemDef for SpruceDoor {
    const ID: i32 = 745;
    const STRING_ID: &'static str = "minecraft:spruce_door";
    const NAME: &'static str = "Spruce Door";
    const STACK_SIZE: u8 = 64;
}

/// Birch Door
pub struct BirchDoor;

impl ItemDef for BirchDoor {
    const ID: i32 = 746;
    const STRING_ID: &'static str = "minecraft:birch_door";
    const NAME: &'static str = "Birch Door";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Door
pub struct JungleDoor;

impl ItemDef for JungleDoor {
    const ID: i32 = 747;
    const STRING_ID: &'static str = "minecraft:jungle_door";
    const NAME: &'static str = "Jungle Door";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Door
pub struct AcaciaDoor;

impl ItemDef for AcaciaDoor {
    const ID: i32 = 748;
    const STRING_ID: &'static str = "minecraft:acacia_door";
    const NAME: &'static str = "Acacia Door";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Door
pub struct CherryDoor;

impl ItemDef for CherryDoor {
    const ID: i32 = 749;
    const STRING_ID: &'static str = "minecraft:cherry_door";
    const NAME: &'static str = "Cherry Door";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Door
pub struct DarkOakDoor;

impl ItemDef for DarkOakDoor {
    const ID: i32 = 750;
    const STRING_ID: &'static str = "minecraft:dark_oak_door";
    const NAME: &'static str = "Dark Oak Door";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Door
pub struct PaleOakDoor;

impl ItemDef for PaleOakDoor {
    const ID: i32 = 751;
    const STRING_ID: &'static str = "minecraft:pale_oak_door";
    const NAME: &'static str = "Pale Oak Door";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Door
pub struct MangroveDoor;

impl ItemDef for MangroveDoor {
    const ID: i32 = 752;
    const STRING_ID: &'static str = "minecraft:mangrove_door";
    const NAME: &'static str = "Mangrove Door";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Door
pub struct BambooDoor;

impl ItemDef for BambooDoor {
    const ID: i32 = 753;
    const STRING_ID: &'static str = "minecraft:bamboo_door";
    const NAME: &'static str = "Bamboo Door";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Door
pub struct CrimsonDoor;

impl ItemDef for CrimsonDoor {
    const ID: i32 = 754;
    const STRING_ID: &'static str = "minecraft:crimson_door";
    const NAME: &'static str = "Crimson Door";
    const STACK_SIZE: u8 = 64;
}

/// Warped Door
pub struct WarpedDoor;

impl ItemDef for WarpedDoor {
    const ID: i32 = 755;
    const STRING_ID: &'static str = "minecraft:warped_door";
    const NAME: &'static str = "Warped Door";
    const STACK_SIZE: u8 = 64;
}

/// Copper Door
pub struct CopperDoor;

impl ItemDef for CopperDoor {
    const ID: i32 = 756;
    const STRING_ID: &'static str = "minecraft:copper_door";
    const NAME: &'static str = "Copper Door";
    const STACK_SIZE: u8 = 64;
}

/// Exposed Copper Door
pub struct ExposedCopperDoor;

impl ItemDef for ExposedCopperDoor {
    const ID: i32 = 757;
    const STRING_ID: &'static str = "minecraft:exposed_copper_door";
    const NAME: &'static str = "Exposed Copper Door";
    const STACK_SIZE: u8 = 64;
}

/// Weathered Copper Door
pub struct WeatheredCopperDoor;

impl ItemDef for WeatheredCopperDoor {
    const ID: i32 = 758;
    const STRING_ID: &'static str = "minecraft:weathered_copper_door";
    const NAME: &'static str = "Weathered Copper Door";
    const STACK_SIZE: u8 = 64;
}

/// Oxidized Copper Door
pub struct OxidizedCopperDoor;

impl ItemDef for OxidizedCopperDoor {
    const ID: i32 = 759;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_door";
    const NAME: &'static str = "Oxidized Copper Door";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Copper Door
pub struct WaxedCopperDoor;

impl ItemDef for WaxedCopperDoor {
    const ID: i32 = 760;
    const STRING_ID: &'static str = "minecraft:waxed_copper_door";
    const NAME: &'static str = "Waxed Copper Door";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Exposed Copper Door
pub struct WaxedExposedCopperDoor;

impl ItemDef for WaxedExposedCopperDoor {
    const ID: i32 = 761;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_door";
    const NAME: &'static str = "Waxed Exposed Copper Door";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Weathered Copper Door
pub struct WaxedWeatheredCopperDoor;

impl ItemDef for WaxedWeatheredCopperDoor {
    const ID: i32 = 762;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_door";
    const NAME: &'static str = "Waxed Weathered Copper Door";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Oxidized Copper Door
pub struct WaxedOxidizedCopperDoor;

impl ItemDef for WaxedOxidizedCopperDoor {
    const ID: i32 = 763;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_door";
    const NAME: &'static str = "Waxed Oxidized Copper Door";
    const STACK_SIZE: u8 = 64;
}

/// Iron Trapdoor
pub struct IronTrapdoor;

impl ItemDef for IronTrapdoor {
    const ID: i32 = 764;
    const STRING_ID: &'static str = "minecraft:iron_trapdoor";
    const NAME: &'static str = "Iron Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Oak Trapdoor
pub struct Trapdoor;

impl ItemDef for Trapdoor {
    const ID: i32 = 765;
    const STRING_ID: &'static str = "minecraft:trapdoor";
    const NAME: &'static str = "Oak Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Trapdoor
pub struct SpruceTrapdoor;

impl ItemDef for SpruceTrapdoor {
    const ID: i32 = 766;
    const STRING_ID: &'static str = "minecraft:spruce_trapdoor";
    const NAME: &'static str = "Spruce Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Birch Trapdoor
pub struct BirchTrapdoor;

impl ItemDef for BirchTrapdoor {
    const ID: i32 = 767;
    const STRING_ID: &'static str = "minecraft:birch_trapdoor";
    const NAME: &'static str = "Birch Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Trapdoor
pub struct JungleTrapdoor;

impl ItemDef for JungleTrapdoor {
    const ID: i32 = 768;
    const STRING_ID: &'static str = "minecraft:jungle_trapdoor";
    const NAME: &'static str = "Jungle Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Trapdoor
pub struct AcaciaTrapdoor;

impl ItemDef for AcaciaTrapdoor {
    const ID: i32 = 769;
    const STRING_ID: &'static str = "minecraft:acacia_trapdoor";
    const NAME: &'static str = "Acacia Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Trapdoor
pub struct CherryTrapdoor;

impl ItemDef for CherryTrapdoor {
    const ID: i32 = 770;
    const STRING_ID: &'static str = "minecraft:cherry_trapdoor";
    const NAME: &'static str = "Cherry Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Trapdoor
pub struct DarkOakTrapdoor;

impl ItemDef for DarkOakTrapdoor {
    const ID: i32 = 771;
    const STRING_ID: &'static str = "minecraft:dark_oak_trapdoor";
    const NAME: &'static str = "Dark Oak Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Trapdoor
pub struct PaleOakTrapdoor;

impl ItemDef for PaleOakTrapdoor {
    const ID: i32 = 772;
    const STRING_ID: &'static str = "minecraft:pale_oak_trapdoor";
    const NAME: &'static str = "Pale Oak Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Trapdoor
pub struct MangroveTrapdoor;

impl ItemDef for MangroveTrapdoor {
    const ID: i32 = 773;
    const STRING_ID: &'static str = "minecraft:mangrove_trapdoor";
    const NAME: &'static str = "Mangrove Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Trapdoor
pub struct BambooTrapdoor;

impl ItemDef for BambooTrapdoor {
    const ID: i32 = 774;
    const STRING_ID: &'static str = "minecraft:bamboo_trapdoor";
    const NAME: &'static str = "Bamboo Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Trapdoor
pub struct CrimsonTrapdoor;

impl ItemDef for CrimsonTrapdoor {
    const ID: i32 = 775;
    const STRING_ID: &'static str = "minecraft:crimson_trapdoor";
    const NAME: &'static str = "Crimson Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Warped Trapdoor
pub struct WarpedTrapdoor;

impl ItemDef for WarpedTrapdoor {
    const ID: i32 = 776;
    const STRING_ID: &'static str = "minecraft:warped_trapdoor";
    const NAME: &'static str = "Warped Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Copper Trapdoor
pub struct CopperTrapdoor;

impl ItemDef for CopperTrapdoor {
    const ID: i32 = 777;
    const STRING_ID: &'static str = "minecraft:copper_trapdoor";
    const NAME: &'static str = "Copper Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Exposed Copper Trapdoor
pub struct ExposedCopperTrapdoor;

impl ItemDef for ExposedCopperTrapdoor {
    const ID: i32 = 778;
    const STRING_ID: &'static str = "minecraft:exposed_copper_trapdoor";
    const NAME: &'static str = "Exposed Copper Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Weathered Copper Trapdoor
pub struct WeatheredCopperTrapdoor;

impl ItemDef for WeatheredCopperTrapdoor {
    const ID: i32 = 779;
    const STRING_ID: &'static str = "minecraft:weathered_copper_trapdoor";
    const NAME: &'static str = "Weathered Copper Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Oxidized Copper Trapdoor
pub struct OxidizedCopperTrapdoor;

impl ItemDef for OxidizedCopperTrapdoor {
    const ID: i32 = 780;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_trapdoor";
    const NAME: &'static str = "Oxidized Copper Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Copper Trapdoor
pub struct WaxedCopperTrapdoor;

impl ItemDef for WaxedCopperTrapdoor {
    const ID: i32 = 781;
    const STRING_ID: &'static str = "minecraft:waxed_copper_trapdoor";
    const NAME: &'static str = "Waxed Copper Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Exposed Copper Trapdoor
pub struct WaxedExposedCopperTrapdoor;

impl ItemDef for WaxedExposedCopperTrapdoor {
    const ID: i32 = 782;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_trapdoor";
    const NAME: &'static str = "Waxed Exposed Copper Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Weathered Copper Trapdoor
pub struct WaxedWeatheredCopperTrapdoor;

impl ItemDef for WaxedWeatheredCopperTrapdoor {
    const ID: i32 = 783;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_trapdoor";
    const NAME: &'static str = "Waxed Weathered Copper Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Oxidized Copper Trapdoor
pub struct WaxedOxidizedCopperTrapdoor;

impl ItemDef for WaxedOxidizedCopperTrapdoor {
    const ID: i32 = 784;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_trapdoor";
    const NAME: &'static str = "Waxed Oxidized Copper Trapdoor";
    const STACK_SIZE: u8 = 64;
}

/// Oak Fence Gate
pub struct FenceGate;

impl ItemDef for FenceGate {
    const ID: i32 = 785;
    const STRING_ID: &'static str = "minecraft:fence_gate";
    const NAME: &'static str = "Oak Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Spruce Fence Gate
pub struct SpruceFenceGate;

impl ItemDef for SpruceFenceGate {
    const ID: i32 = 786;
    const STRING_ID: &'static str = "minecraft:spruce_fence_gate";
    const NAME: &'static str = "Spruce Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Birch Fence Gate
pub struct BirchFenceGate;

impl ItemDef for BirchFenceGate {
    const ID: i32 = 787;
    const STRING_ID: &'static str = "minecraft:birch_fence_gate";
    const NAME: &'static str = "Birch Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Jungle Fence Gate
pub struct JungleFenceGate;

impl ItemDef for JungleFenceGate {
    const ID: i32 = 788;
    const STRING_ID: &'static str = "minecraft:jungle_fence_gate";
    const NAME: &'static str = "Jungle Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Acacia Fence Gate
pub struct AcaciaFenceGate;

impl ItemDef for AcaciaFenceGate {
    const ID: i32 = 789;
    const STRING_ID: &'static str = "minecraft:acacia_fence_gate";
    const NAME: &'static str = "Acacia Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Cherry Fence Gate
pub struct CherryFenceGate;

impl ItemDef for CherryFenceGate {
    const ID: i32 = 790;
    const STRING_ID: &'static str = "minecraft:cherry_fence_gate";
    const NAME: &'static str = "Cherry Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Dark Oak Fence Gate
pub struct DarkOakFenceGate;

impl ItemDef for DarkOakFenceGate {
    const ID: i32 = 791;
    const STRING_ID: &'static str = "minecraft:dark_oak_fence_gate";
    const NAME: &'static str = "Dark Oak Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Pale Oak Fence Gate
pub struct PaleOakFenceGate;

impl ItemDef for PaleOakFenceGate {
    const ID: i32 = 792;
    const STRING_ID: &'static str = "minecraft:pale_oak_fence_gate";
    const NAME: &'static str = "Pale Oak Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Fence Gate
pub struct MangroveFenceGate;

impl ItemDef for MangroveFenceGate {
    const ID: i32 = 793;
    const STRING_ID: &'static str = "minecraft:mangrove_fence_gate";
    const NAME: &'static str = "Mangrove Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Bamboo Fence Gate
pub struct BambooFenceGate;

impl ItemDef for BambooFenceGate {
    const ID: i32 = 794;
    const STRING_ID: &'static str = "minecraft:bamboo_fence_gate";
    const NAME: &'static str = "Bamboo Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Crimson Fence Gate
pub struct CrimsonFenceGate;

impl ItemDef for CrimsonFenceGate {
    const ID: i32 = 795;
    const STRING_ID: &'static str = "minecraft:crimson_fence_gate";
    const NAME: &'static str = "Crimson Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Warped Fence Gate
pub struct WarpedFenceGate;

impl ItemDef for WarpedFenceGate {
    const ID: i32 = 796;
    const STRING_ID: &'static str = "minecraft:warped_fence_gate";
    const NAME: &'static str = "Warped Fence Gate";
    const STACK_SIZE: u8 = 64;
}

/// Powered Rail
pub struct GoldenRail;

impl ItemDef for GoldenRail {
    const ID: i32 = 797;
    const STRING_ID: &'static str = "minecraft:golden_rail";
    const NAME: &'static str = "Powered Rail";
    const STACK_SIZE: u8 = 64;
}

/// Detector Rail
pub struct DetectorRail;

impl ItemDef for DetectorRail {
    const ID: i32 = 798;
    const STRING_ID: &'static str = "minecraft:detector_rail";
    const NAME: &'static str = "Detector Rail";
    const STACK_SIZE: u8 = 64;
}

/// Rail
pub struct Rail;

impl ItemDef for Rail {
    const ID: i32 = 799;
    const STRING_ID: &'static str = "minecraft:rail";
    const NAME: &'static str = "Rail";
    const STACK_SIZE: u8 = 64;
}

/// Activator Rail
pub struct ActivatorRail;

impl ItemDef for ActivatorRail {
    const ID: i32 = 800;
    const STRING_ID: &'static str = "minecraft:activator_rail";
    const NAME: &'static str = "Activator Rail";
    const STACK_SIZE: u8 = 64;
}

/// Saddle
pub struct Saddle;

impl ItemDef for Saddle {
    const ID: i32 = 801;
    const STRING_ID: &'static str = "minecraft:saddle";
    const NAME: &'static str = "Saddle";
    const STACK_SIZE: u8 = 1;
}

/// White Harness
pub struct WhiteHarness;

impl ItemDef for WhiteHarness {
    const ID: i32 = 802;
    const STRING_ID: &'static str = "minecraft:white_harness";
    const NAME: &'static str = "White Harness";
    const STACK_SIZE: u8 = 1;
}

/// Orange Harness
pub struct OrangeHarness;

impl ItemDef for OrangeHarness {
    const ID: i32 = 803;
    const STRING_ID: &'static str = "minecraft:orange_harness";
    const NAME: &'static str = "Orange Harness";
    const STACK_SIZE: u8 = 1;
}

/// Magenta Harness
pub struct MagentaHarness;

impl ItemDef for MagentaHarness {
    const ID: i32 = 804;
    const STRING_ID: &'static str = "minecraft:magenta_harness";
    const NAME: &'static str = "Magenta Harness";
    const STACK_SIZE: u8 = 1;
}

/// Light Blue Harness
pub struct LightBlueHarness;

impl ItemDef for LightBlueHarness {
    const ID: i32 = 805;
    const STRING_ID: &'static str = "minecraft:light_blue_harness";
    const NAME: &'static str = "Light Blue Harness";
    const STACK_SIZE: u8 = 1;
}

/// Yellow Harness
pub struct YellowHarness;

impl ItemDef for YellowHarness {
    const ID: i32 = 806;
    const STRING_ID: &'static str = "minecraft:yellow_harness";
    const NAME: &'static str = "Yellow Harness";
    const STACK_SIZE: u8 = 1;
}

/// Lime Harness
pub struct LimeHarness;

impl ItemDef for LimeHarness {
    const ID: i32 = 807;
    const STRING_ID: &'static str = "minecraft:lime_harness";
    const NAME: &'static str = "Lime Harness";
    const STACK_SIZE: u8 = 1;
}

/// Pink Harness
pub struct PinkHarness;

impl ItemDef for PinkHarness {
    const ID: i32 = 808;
    const STRING_ID: &'static str = "minecraft:pink_harness";
    const NAME: &'static str = "Pink Harness";
    const STACK_SIZE: u8 = 1;
}

/// Gray Harness
pub struct GrayHarness;

impl ItemDef for GrayHarness {
    const ID: i32 = 809;
    const STRING_ID: &'static str = "minecraft:gray_harness";
    const NAME: &'static str = "Gray Harness";
    const STACK_SIZE: u8 = 1;
}

/// Light Gray Harness
pub struct LightGrayHarness;

impl ItemDef for LightGrayHarness {
    const ID: i32 = 810;
    const STRING_ID: &'static str = "minecraft:light_gray_harness";
    const NAME: &'static str = "Light Gray Harness";
    const STACK_SIZE: u8 = 1;
}

/// Cyan Harness
pub struct CyanHarness;

impl ItemDef for CyanHarness {
    const ID: i32 = 811;
    const STRING_ID: &'static str = "minecraft:cyan_harness";
    const NAME: &'static str = "Cyan Harness";
    const STACK_SIZE: u8 = 1;
}

/// Purple Harness
pub struct PurpleHarness;

impl ItemDef for PurpleHarness {
    const ID: i32 = 812;
    const STRING_ID: &'static str = "minecraft:purple_harness";
    const NAME: &'static str = "Purple Harness";
    const STACK_SIZE: u8 = 1;
}

/// Blue Harness
pub struct BlueHarness;

impl ItemDef for BlueHarness {
    const ID: i32 = 813;
    const STRING_ID: &'static str = "minecraft:blue_harness";
    const NAME: &'static str = "Blue Harness";
    const STACK_SIZE: u8 = 1;
}

/// Brown Harness
pub struct BrownHarness;

impl ItemDef for BrownHarness {
    const ID: i32 = 814;
    const STRING_ID: &'static str = "minecraft:brown_harness";
    const NAME: &'static str = "Brown Harness";
    const STACK_SIZE: u8 = 1;
}

/// Green Harness
pub struct GreenHarness;

impl ItemDef for GreenHarness {
    const ID: i32 = 815;
    const STRING_ID: &'static str = "minecraft:green_harness";
    const NAME: &'static str = "Green Harness";
    const STACK_SIZE: u8 = 1;
}

/// Red Harness
pub struct RedHarness;

impl ItemDef for RedHarness {
    const ID: i32 = 816;
    const STRING_ID: &'static str = "minecraft:red_harness";
    const NAME: &'static str = "Red Harness";
    const STACK_SIZE: u8 = 1;
}

/// Black Harness
pub struct BlackHarness;

impl ItemDef for BlackHarness {
    const ID: i32 = 817;
    const STRING_ID: &'static str = "minecraft:black_harness";
    const NAME: &'static str = "Black Harness";
    const STACK_SIZE: u8 = 1;
}

/// Minecart
pub struct Minecart;

impl ItemDef for Minecart {
    const ID: i32 = 818;
    const STRING_ID: &'static str = "minecraft:minecart";
    const NAME: &'static str = "Minecart";
    const STACK_SIZE: u8 = 1;
}

/// Minecart with Chest
pub struct ChestMinecart;

impl ItemDef for ChestMinecart {
    const ID: i32 = 819;
    const STRING_ID: &'static str = "minecraft:chest_minecart";
    const NAME: &'static str = "Minecart with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Minecart with Furnace
pub struct HopperMinecart;

impl ItemDef for HopperMinecart {
    const ID: i32 = 820;
    const STRING_ID: &'static str = "minecraft:hopper_minecart";
    const NAME: &'static str = "Minecart with Furnace";
    const STACK_SIZE: u8 = 1;
}

impl VariantItem for HopperMinecart {
    fn variants() -> &'static [ItemVariant] {
        &[ItemVariant {
            id: 822,
            metadata: 0,
            name: "hopper_minecart",
            display_name: "Minecart with Hopper",
            stack_size: 1,
        }]
    }
}

/// Minecart with TNT
pub struct TntMinecart;

impl ItemDef for TntMinecart {
    const ID: i32 = 821;
    const STRING_ID: &'static str = "minecraft:tnt_minecart";
    const NAME: &'static str = "Minecart with TNT";
    const STACK_SIZE: u8 = 1;
}

/// Carrot on a Stick
pub struct CarrotOnAStick;

impl ItemDef for CarrotOnAStick {
    const ID: i32 = 823;
    const STRING_ID: &'static str = "minecraft:carrot_on_a_stick";
    const NAME: &'static str = "Carrot on a Stick";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for CarrotOnAStick {
    const MAX_DURABILITY: u16 = 25;
}

impl EnchantableItem for CarrotOnAStick {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Warped Fungus on a Stick
pub struct WarpedFungusOnAStick;

impl ItemDef for WarpedFungusOnAStick {
    const ID: i32 = 824;
    const STRING_ID: &'static str = "minecraft:warped_fungus_on_a_stick";
    const NAME: &'static str = "Warped Fungus on a Stick";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for WarpedFungusOnAStick {
    const MAX_DURABILITY: u16 = 100;
}

impl EnchantableItem for WarpedFungusOnAStick {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Phantom Membrane
pub struct PhantomMembrane;

impl ItemDef for PhantomMembrane {
    const ID: i32 = 825;
    const STRING_ID: &'static str = "minecraft:phantom_membrane";
    const NAME: &'static str = "Phantom Membrane";
    const STACK_SIZE: u8 = 64;
}

/// Elytra
pub struct Elytra;

impl ItemDef for Elytra {
    const ID: i32 = 826;
    const STRING_ID: &'static str = "minecraft:elytra";
    const NAME: &'static str = "Elytra";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for Elytra {
    const MAX_DURABILITY: u16 = 432;
}

impl RepairableItem for Elytra {
    fn repair_items() -> &'static [i32] {
        &[825]
    }
}

impl EnchantableItem for Elytra {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Oak Boat
pub struct OakBoat;

impl ItemDef for OakBoat {
    const ID: i32 = 827;
    const STRING_ID: &'static str = "minecraft:oak_boat";
    const NAME: &'static str = "Oak Boat";
    const STACK_SIZE: u8 = 1;
}

/// Oak Boat with Chest
pub struct OakChestBoat;

impl ItemDef for OakChestBoat {
    const ID: i32 = 828;
    const STRING_ID: &'static str = "minecraft:oak_chest_boat";
    const NAME: &'static str = "Oak Boat with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Spruce Boat
pub struct SpruceBoat;

impl ItemDef for SpruceBoat {
    const ID: i32 = 829;
    const STRING_ID: &'static str = "minecraft:spruce_boat";
    const NAME: &'static str = "Spruce Boat";
    const STACK_SIZE: u8 = 1;
}

/// Spruce Boat with Chest
pub struct SpruceChestBoat;

impl ItemDef for SpruceChestBoat {
    const ID: i32 = 830;
    const STRING_ID: &'static str = "minecraft:spruce_chest_boat";
    const NAME: &'static str = "Spruce Boat with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Birch Boat
pub struct BirchBoat;

impl ItemDef for BirchBoat {
    const ID: i32 = 831;
    const STRING_ID: &'static str = "minecraft:birch_boat";
    const NAME: &'static str = "Birch Boat";
    const STACK_SIZE: u8 = 1;
}

/// Birch Boat with Chest
pub struct BirchChestBoat;

impl ItemDef for BirchChestBoat {
    const ID: i32 = 832;
    const STRING_ID: &'static str = "minecraft:birch_chest_boat";
    const NAME: &'static str = "Birch Boat with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Jungle Boat
pub struct JungleBoat;

impl ItemDef for JungleBoat {
    const ID: i32 = 833;
    const STRING_ID: &'static str = "minecraft:jungle_boat";
    const NAME: &'static str = "Jungle Boat";
    const STACK_SIZE: u8 = 1;
}

/// Jungle Boat with Chest
pub struct JungleChestBoat;

impl ItemDef for JungleChestBoat {
    const ID: i32 = 834;
    const STRING_ID: &'static str = "minecraft:jungle_chest_boat";
    const NAME: &'static str = "Jungle Boat with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Acacia Boat
pub struct AcaciaBoat;

impl ItemDef for AcaciaBoat {
    const ID: i32 = 835;
    const STRING_ID: &'static str = "minecraft:acacia_boat";
    const NAME: &'static str = "Acacia Boat";
    const STACK_SIZE: u8 = 1;
}

/// Acacia Boat with Chest
pub struct AcaciaChestBoat;

impl ItemDef for AcaciaChestBoat {
    const ID: i32 = 836;
    const STRING_ID: &'static str = "minecraft:acacia_chest_boat";
    const NAME: &'static str = "Acacia Boat with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Cherry Boat
pub struct CherryBoat;

impl ItemDef for CherryBoat {
    const ID: i32 = 837;
    const STRING_ID: &'static str = "minecraft:cherry_boat";
    const NAME: &'static str = "Cherry Boat";
    const STACK_SIZE: u8 = 1;
}

/// Cherry Boat with Chest
pub struct CherryChestBoat;

impl ItemDef for CherryChestBoat {
    const ID: i32 = 838;
    const STRING_ID: &'static str = "minecraft:cherry_chest_boat";
    const NAME: &'static str = "Cherry Boat with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Dark Oak Boat
pub struct DarkOakBoat;

impl ItemDef for DarkOakBoat {
    const ID: i32 = 839;
    const STRING_ID: &'static str = "minecraft:dark_oak_boat";
    const NAME: &'static str = "Dark Oak Boat";
    const STACK_SIZE: u8 = 1;
}

/// Dark Oak Boat with Chest
pub struct DarkOakChestBoat;

impl ItemDef for DarkOakChestBoat {
    const ID: i32 = 840;
    const STRING_ID: &'static str = "minecraft:dark_oak_chest_boat";
    const NAME: &'static str = "Dark Oak Boat with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Pale Oak Boat
pub struct PaleOakBoat;

impl ItemDef for PaleOakBoat {
    const ID: i32 = 841;
    const STRING_ID: &'static str = "minecraft:pale_oak_boat";
    const NAME: &'static str = "Pale Oak Boat";
    const STACK_SIZE: u8 = 1;
}

/// Pale Oak Boat with Chest
pub struct PaleOakChestBoat;

impl ItemDef for PaleOakChestBoat {
    const ID: i32 = 842;
    const STRING_ID: &'static str = "minecraft:pale_oak_chest_boat";
    const NAME: &'static str = "Pale Oak Boat with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Mangrove Boat
pub struct MangroveBoat;

impl ItemDef for MangroveBoat {
    const ID: i32 = 843;
    const STRING_ID: &'static str = "minecraft:mangrove_boat";
    const NAME: &'static str = "Mangrove Boat";
    const STACK_SIZE: u8 = 1;
}

/// Mangrove Boat with Chest
pub struct MangroveChestBoat;

impl ItemDef for MangroveChestBoat {
    const ID: i32 = 844;
    const STRING_ID: &'static str = "minecraft:mangrove_chest_boat";
    const NAME: &'static str = "Mangrove Boat with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Bamboo Raft
pub struct BambooRaft;

impl ItemDef for BambooRaft {
    const ID: i32 = 845;
    const STRING_ID: &'static str = "minecraft:bamboo_raft";
    const NAME: &'static str = "Bamboo Raft";
    const STACK_SIZE: u8 = 1;
}

/// Bamboo Raft with Chest
pub struct BambooChestRaft;

impl ItemDef for BambooChestRaft {
    const ID: i32 = 846;
    const STRING_ID: &'static str = "minecraft:bamboo_chest_raft";
    const NAME: &'static str = "Bamboo Raft with Chest";
    const STACK_SIZE: u8 = 1;
}

/// Structure Block
pub struct StructureBlock;

impl ItemDef for StructureBlock {
    const ID: i32 = 847;
    const STRING_ID: &'static str = "minecraft:structure_block";
    const NAME: &'static str = "Structure Block";
    const STACK_SIZE: u8 = 64;
}

/// Jigsaw Block
pub struct Jigsaw;

impl ItemDef for Jigsaw {
    const ID: i32 = 848;
    const STRING_ID: &'static str = "minecraft:jigsaw";
    const NAME: &'static str = "Jigsaw Block";
    const STACK_SIZE: u8 = 64;
}

/// Test Block
pub struct Unknown;

impl ItemDef for Unknown {
    const ID: i32 = 849;
    const STRING_ID: &'static str = "minecraft:unknown";
    const NAME: &'static str = "Test Block";
    const STACK_SIZE: u8 = 64;
}

impl VariantItem for Unknown {
    fn variants() -> &'static [ItemVariant] {
        &[ItemVariant {
            id: 850,
            metadata: 0,
            name: "test_instance_block",
            display_name: "Test Instance Block",
            stack_size: 64,
        }]
    }
}

/// Turtle Shell
pub struct TurtleHelmet;

impl ItemDef for TurtleHelmet {
    const ID: i32 = 851;
    const STRING_ID: &'static str = "minecraft:turtle_helmet";
    const NAME: &'static str = "Turtle Shell";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for TurtleHelmet {
    const MAX_DURABILITY: u16 = 275;
}

impl RepairableItem for TurtleHelmet {
    fn repair_items() -> &'static [i32] {
        &[852]
    }
}

impl EnchantableItem for TurtleHelmet {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::HeadArmor,
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Turtle Scute
pub struct TurtleScute;

impl ItemDef for TurtleScute {
    const ID: i32 = 852;
    const STRING_ID: &'static str = "minecraft:turtle_scute";
    const NAME: &'static str = "Turtle Scute";
    const STACK_SIZE: u8 = 64;
}

/// Armadillo Scute
pub struct ArmadilloScute;

impl ItemDef for ArmadilloScute {
    const ID: i32 = 853;
    const STRING_ID: &'static str = "minecraft:armadillo_scute";
    const NAME: &'static str = "Armadillo Scute";
    const STACK_SIZE: u8 = 64;
}

/// Wolf Armor
pub struct WolfArmor;

impl ItemDef for WolfArmor {
    const ID: i32 = 854;
    const STRING_ID: &'static str = "minecraft:wolf_armor";
    const NAME: &'static str = "Wolf Armor";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for WolfArmor {
    const MAX_DURABILITY: u16 = 64;
}

impl RepairableItem for WolfArmor {
    fn repair_items() -> &'static [i32] {
        &[853]
    }
}

/// Flint and Steel
pub struct FlintAndSteel;

impl ItemDef for FlintAndSteel {
    const ID: i32 = 855;
    const STRING_ID: &'static str = "minecraft:flint_and_steel";
    const NAME: &'static str = "Flint and Steel";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for FlintAndSteel {
    const MAX_DURABILITY: u16 = 64;
}

impl EnchantableItem for FlintAndSteel {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Bowl
pub struct Bowl;

impl ItemDef for Bowl {
    const ID: i32 = 856;
    const STRING_ID: &'static str = "minecraft:bowl";
    const NAME: &'static str = "Bowl";
    const STACK_SIZE: u8 = 64;
}

/// Apple
pub struct Apple;

impl ItemDef for Apple {
    const ID: i32 = 857;
    const STRING_ID: &'static str = "minecraft:apple";
    const NAME: &'static str = "Apple";
    const STACK_SIZE: u8 = 64;
}

/// Bow
pub struct Bow;

impl ItemDef for Bow {
    const ID: i32 = 858;
    const STRING_ID: &'static str = "minecraft:bow";
    const NAME: &'static str = "Bow";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for Bow {
    const MAX_DURABILITY: u16 = 384;
}

impl EnchantableItem for Bow {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Bow,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Arrow
pub struct Arrow;

impl ItemDef for Arrow {
    const ID: i32 = 859;
    const STRING_ID: &'static str = "minecraft:arrow";
    const NAME: &'static str = "Arrow";
    const STACK_SIZE: u8 = 64;
}

impl VariantItem for Arrow {
    fn variants() -> &'static [ItemVariant] {
        &[
            ItemVariant {
                id: 1240,
                metadata: 0,
                name: "spectral_arrow",
                display_name: "Spectral Arrow",
                stack_size: 64,
            },
            ItemVariant {
                id: 1241,
                metadata: 0,
                name: "tipped_arrow",
                display_name: "Tipped Arrow",
                stack_size: 64,
            },
        ]
    }
}

/// Coal
pub struct Coal;

impl ItemDef for Coal {
    const ID: i32 = 860;
    const STRING_ID: &'static str = "minecraft:coal";
    const NAME: &'static str = "Coal";
    const STACK_SIZE: u8 = 64;
}

/// Charcoal
pub struct Charcoal;

impl ItemDef for Charcoal {
    const ID: i32 = 861;
    const STRING_ID: &'static str = "minecraft:charcoal";
    const NAME: &'static str = "Charcoal";
    const STACK_SIZE: u8 = 64;
}

/// Diamond
pub struct Diamond;

impl ItemDef for Diamond {
    const ID: i32 = 862;
    const STRING_ID: &'static str = "minecraft:diamond";
    const NAME: &'static str = "Diamond";
    const STACK_SIZE: u8 = 64;
}

/// Emerald
pub struct Emerald;

impl ItemDef for Emerald {
    const ID: i32 = 863;
    const STRING_ID: &'static str = "minecraft:emerald";
    const NAME: &'static str = "Emerald";
    const STACK_SIZE: u8 = 64;
}

/// Lapis Lazuli
pub struct LapisLazuli;

impl ItemDef for LapisLazuli {
    const ID: i32 = 864;
    const STRING_ID: &'static str = "minecraft:lapis_lazuli";
    const NAME: &'static str = "Lapis Lazuli";
    const STACK_SIZE: u8 = 64;
}

/// Nether Quartz
pub struct Quartz;

impl ItemDef for Quartz {
    const ID: i32 = 865;
    const STRING_ID: &'static str = "minecraft:quartz";
    const NAME: &'static str = "Nether Quartz";
    const STACK_SIZE: u8 = 64;
}

/// Amethyst Shard
pub struct AmethystShard;

impl ItemDef for AmethystShard {
    const ID: i32 = 866;
    const STRING_ID: &'static str = "minecraft:amethyst_shard";
    const NAME: &'static str = "Amethyst Shard";
    const STACK_SIZE: u8 = 64;
}

/// Raw Iron
pub struct RawIron;

impl ItemDef for RawIron {
    const ID: i32 = 867;
    const STRING_ID: &'static str = "minecraft:raw_iron";
    const NAME: &'static str = "Raw Iron";
    const STACK_SIZE: u8 = 64;
}

/// Iron Ingot
pub struct IronIngot;

impl ItemDef for IronIngot {
    const ID: i32 = 868;
    const STRING_ID: &'static str = "minecraft:iron_ingot";
    const NAME: &'static str = "Iron Ingot";
    const STACK_SIZE: u8 = 64;
}

/// Raw Copper
pub struct RawCopper;

impl ItemDef for RawCopper {
    const ID: i32 = 869;
    const STRING_ID: &'static str = "minecraft:raw_copper";
    const NAME: &'static str = "Raw Copper";
    const STACK_SIZE: u8 = 64;
}

/// Copper Ingot
pub struct CopperIngot;

impl ItemDef for CopperIngot {
    const ID: i32 = 870;
    const STRING_ID: &'static str = "minecraft:copper_ingot";
    const NAME: &'static str = "Copper Ingot";
    const STACK_SIZE: u8 = 64;
}

/// Raw Gold
pub struct RawGold;

impl ItemDef for RawGold {
    const ID: i32 = 871;
    const STRING_ID: &'static str = "minecraft:raw_gold";
    const NAME: &'static str = "Raw Gold";
    const STACK_SIZE: u8 = 64;
}

/// Gold Ingot
pub struct GoldIngot;

impl ItemDef for GoldIngot {
    const ID: i32 = 872;
    const STRING_ID: &'static str = "minecraft:gold_ingot";
    const NAME: &'static str = "Gold Ingot";
    const STACK_SIZE: u8 = 64;
}

/// Netherite Ingot
pub struct NetheriteIngot;

impl ItemDef for NetheriteIngot {
    const ID: i32 = 873;
    const STRING_ID: &'static str = "minecraft:netherite_ingot";
    const NAME: &'static str = "Netherite Ingot";
    const STACK_SIZE: u8 = 64;
}

/// Netherite Scrap
pub struct NetheriteScrap;

impl ItemDef for NetheriteScrap {
    const ID: i32 = 874;
    const STRING_ID: &'static str = "minecraft:netherite_scrap";
    const NAME: &'static str = "Netherite Scrap";
    const STACK_SIZE: u8 = 64;
}

/// Wooden Sword
pub struct WoodenSword;

impl ItemDef for WoodenSword {
    const ID: i32 = 875;
    const STRING_ID: &'static str = "minecraft:wooden_sword";
    const NAME: &'static str = "Wooden Sword";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for WoodenSword {
    const MAX_DURABILITY: u16 = 59;
}

impl RepairableItem for WoodenSword {
    fn repair_items() -> &'static [i32] {
        &[36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47]
    }
}

impl EnchantableItem for WoodenSword {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Sword,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Wooden Shovel
pub struct WoodenShovel;

impl ItemDef for WoodenShovel {
    const ID: i32 = 876;
    const STRING_ID: &'static str = "minecraft:wooden_shovel";
    const NAME: &'static str = "Wooden Shovel";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for WoodenShovel {
    const MAX_DURABILITY: u16 = 59;
}

impl RepairableItem for WoodenShovel {
    fn repair_items() -> &'static [i32] {
        &[36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47]
    }
}

impl EnchantableItem for WoodenShovel {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Wooden Pickaxe
pub struct WoodenPickaxe;

impl ItemDef for WoodenPickaxe {
    const ID: i32 = 877;
    const STRING_ID: &'static str = "minecraft:wooden_pickaxe";
    const NAME: &'static str = "Wooden Pickaxe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for WoodenPickaxe {
    const MAX_DURABILITY: u16 = 59;
}

impl RepairableItem for WoodenPickaxe {
    fn repair_items() -> &'static [i32] {
        &[36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47]
    }
}

impl EnchantableItem for WoodenPickaxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Wooden Axe
pub struct WoodenAxe;

impl ItemDef for WoodenAxe {
    const ID: i32 = 878;
    const STRING_ID: &'static str = "minecraft:wooden_axe";
    const NAME: &'static str = "Wooden Axe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for WoodenAxe {
    const MAX_DURABILITY: u16 = 59;
}

impl RepairableItem for WoodenAxe {
    fn repair_items() -> &'static [i32] {
        &[36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47]
    }
}

impl EnchantableItem for WoodenAxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Wooden Hoe
pub struct WoodenHoe;

impl ItemDef for WoodenHoe {
    const ID: i32 = 879;
    const STRING_ID: &'static str = "minecraft:wooden_hoe";
    const NAME: &'static str = "Wooden Hoe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for WoodenHoe {
    const MAX_DURABILITY: u16 = 59;
}

impl RepairableItem for WoodenHoe {
    fn repair_items() -> &'static [i32] {
        &[36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47]
    }
}

impl EnchantableItem for WoodenHoe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Stone Sword
pub struct StoneSword;

impl ItemDef for StoneSword {
    const ID: i32 = 880;
    const STRING_ID: &'static str = "minecraft:stone_sword";
    const NAME: &'static str = "Stone Sword";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for StoneSword {
    const MAX_DURABILITY: u16 = 131;
}

impl RepairableItem for StoneSword {
    fn repair_items() -> &'static [i32] {
        &[9, 35, 1312]
    }
}

impl EnchantableItem for StoneSword {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Sword,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Stone Shovel
pub struct StoneShovel;

impl ItemDef for StoneShovel {
    const ID: i32 = 881;
    const STRING_ID: &'static str = "minecraft:stone_shovel";
    const NAME: &'static str = "Stone Shovel";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for StoneShovel {
    const MAX_DURABILITY: u16 = 131;
}

impl RepairableItem for StoneShovel {
    fn repair_items() -> &'static [i32] {
        &[9, 35, 1312]
    }
}

impl EnchantableItem for StoneShovel {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Stone Pickaxe
pub struct StonePickaxe;

impl ItemDef for StonePickaxe {
    const ID: i32 = 882;
    const STRING_ID: &'static str = "minecraft:stone_pickaxe";
    const NAME: &'static str = "Stone Pickaxe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for StonePickaxe {
    const MAX_DURABILITY: u16 = 131;
}

impl RepairableItem for StonePickaxe {
    fn repair_items() -> &'static [i32] {
        &[9, 35, 1312]
    }
}

impl EnchantableItem for StonePickaxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Stone Axe
pub struct StoneAxe;

impl ItemDef for StoneAxe {
    const ID: i32 = 883;
    const STRING_ID: &'static str = "minecraft:stone_axe";
    const NAME: &'static str = "Stone Axe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for StoneAxe {
    const MAX_DURABILITY: u16 = 131;
}

impl RepairableItem for StoneAxe {
    fn repair_items() -> &'static [i32] {
        &[9, 35, 1312]
    }
}

impl EnchantableItem for StoneAxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Stone Hoe
pub struct StoneHoe;

impl ItemDef for StoneHoe {
    const ID: i32 = 884;
    const STRING_ID: &'static str = "minecraft:stone_hoe";
    const NAME: &'static str = "Stone Hoe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for StoneHoe {
    const MAX_DURABILITY: u16 = 131;
}

impl RepairableItem for StoneHoe {
    fn repair_items() -> &'static [i32] {
        &[9, 35, 1312]
    }
}

impl EnchantableItem for StoneHoe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Golden Sword
pub struct GoldenSword;

impl ItemDef for GoldenSword {
    const ID: i32 = 885;
    const STRING_ID: &'static str = "minecraft:golden_sword";
    const NAME: &'static str = "Golden Sword";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for GoldenSword {
    const MAX_DURABILITY: u16 = 32;
}

impl RepairableItem for GoldenSword {
    fn repair_items() -> &'static [i32] {
        &[872]
    }
}

impl EnchantableItem for GoldenSword {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Sword,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Golden Shovel
pub struct GoldenShovel;

impl ItemDef for GoldenShovel {
    const ID: i32 = 886;
    const STRING_ID: &'static str = "minecraft:golden_shovel";
    const NAME: &'static str = "Golden Shovel";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for GoldenShovel {
    const MAX_DURABILITY: u16 = 32;
}

impl RepairableItem for GoldenShovel {
    fn repair_items() -> &'static [i32] {
        &[872]
    }
}

impl EnchantableItem for GoldenShovel {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Golden Pickaxe
pub struct GoldenPickaxe;

impl ItemDef for GoldenPickaxe {
    const ID: i32 = 887;
    const STRING_ID: &'static str = "minecraft:golden_pickaxe";
    const NAME: &'static str = "Golden Pickaxe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for GoldenPickaxe {
    const MAX_DURABILITY: u16 = 32;
}

impl RepairableItem for GoldenPickaxe {
    fn repair_items() -> &'static [i32] {
        &[872]
    }
}

impl EnchantableItem for GoldenPickaxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Golden Axe
pub struct GoldenAxe;

impl ItemDef for GoldenAxe {
    const ID: i32 = 888;
    const STRING_ID: &'static str = "minecraft:golden_axe";
    const NAME: &'static str = "Golden Axe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for GoldenAxe {
    const MAX_DURABILITY: u16 = 32;
}

impl RepairableItem for GoldenAxe {
    fn repair_items() -> &'static [i32] {
        &[872]
    }
}

impl EnchantableItem for GoldenAxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Golden Hoe
pub struct GoldenHoe;

impl ItemDef for GoldenHoe {
    const ID: i32 = 889;
    const STRING_ID: &'static str = "minecraft:golden_hoe";
    const NAME: &'static str = "Golden Hoe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for GoldenHoe {
    const MAX_DURABILITY: u16 = 32;
}

impl RepairableItem for GoldenHoe {
    fn repair_items() -> &'static [i32] {
        &[872]
    }
}

impl EnchantableItem for GoldenHoe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Iron Sword
pub struct IronSword;

impl ItemDef for IronSword {
    const ID: i32 = 890;
    const STRING_ID: &'static str = "minecraft:iron_sword";
    const NAME: &'static str = "Iron Sword";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for IronSword {
    const MAX_DURABILITY: u16 = 250;
}

impl RepairableItem for IronSword {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for IronSword {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Sword,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Iron Shovel
pub struct IronShovel;

impl ItemDef for IronShovel {
    const ID: i32 = 891;
    const STRING_ID: &'static str = "minecraft:iron_shovel";
    const NAME: &'static str = "Iron Shovel";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for IronShovel {
    const MAX_DURABILITY: u16 = 250;
}

impl RepairableItem for IronShovel {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for IronShovel {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Iron Pickaxe
pub struct IronPickaxe;

impl ItemDef for IronPickaxe {
    const ID: i32 = 892;
    const STRING_ID: &'static str = "minecraft:iron_pickaxe";
    const NAME: &'static str = "Iron Pickaxe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for IronPickaxe {
    const MAX_DURABILITY: u16 = 250;
}

impl RepairableItem for IronPickaxe {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for IronPickaxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Iron Axe
pub struct IronAxe;

impl ItemDef for IronAxe {
    const ID: i32 = 893;
    const STRING_ID: &'static str = "minecraft:iron_axe";
    const NAME: &'static str = "Iron Axe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for IronAxe {
    const MAX_DURABILITY: u16 = 250;
}

impl RepairableItem for IronAxe {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for IronAxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Iron Hoe
pub struct IronHoe;

impl ItemDef for IronHoe {
    const ID: i32 = 894;
    const STRING_ID: &'static str = "minecraft:iron_hoe";
    const NAME: &'static str = "Iron Hoe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for IronHoe {
    const MAX_DURABILITY: u16 = 250;
}

impl RepairableItem for IronHoe {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for IronHoe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Diamond Sword
pub struct DiamondSword;

impl ItemDef for DiamondSword {
    const ID: i32 = 895;
    const STRING_ID: &'static str = "minecraft:diamond_sword";
    const NAME: &'static str = "Diamond Sword";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondSword {
    const MAX_DURABILITY: u16 = 1561;
}

impl RepairableItem for DiamondSword {
    fn repair_items() -> &'static [i32] {
        &[862]
    }
}

impl EnchantableItem for DiamondSword {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Sword,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Diamond Shovel
pub struct DiamondShovel;

impl ItemDef for DiamondShovel {
    const ID: i32 = 896;
    const STRING_ID: &'static str = "minecraft:diamond_shovel";
    const NAME: &'static str = "Diamond Shovel";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondShovel {
    const MAX_DURABILITY: u16 = 1561;
}

impl RepairableItem for DiamondShovel {
    fn repair_items() -> &'static [i32] {
        &[862]
    }
}

impl EnchantableItem for DiamondShovel {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Diamond Pickaxe
pub struct DiamondPickaxe;

impl ItemDef for DiamondPickaxe {
    const ID: i32 = 897;
    const STRING_ID: &'static str = "minecraft:diamond_pickaxe";
    const NAME: &'static str = "Diamond Pickaxe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondPickaxe {
    const MAX_DURABILITY: u16 = 1561;
}

impl RepairableItem for DiamondPickaxe {
    fn repair_items() -> &'static [i32] {
        &[862]
    }
}

impl EnchantableItem for DiamondPickaxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Diamond Axe
pub struct DiamondAxe;

impl ItemDef for DiamondAxe {
    const ID: i32 = 898;
    const STRING_ID: &'static str = "minecraft:diamond_axe";
    const NAME: &'static str = "Diamond Axe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondAxe {
    const MAX_DURABILITY: u16 = 1561;
}

impl RepairableItem for DiamondAxe {
    fn repair_items() -> &'static [i32] {
        &[862]
    }
}

impl EnchantableItem for DiamondAxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Diamond Hoe
pub struct DiamondHoe;

impl ItemDef for DiamondHoe {
    const ID: i32 = 899;
    const STRING_ID: &'static str = "minecraft:diamond_hoe";
    const NAME: &'static str = "Diamond Hoe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondHoe {
    const MAX_DURABILITY: u16 = 1561;
}

impl RepairableItem for DiamondHoe {
    fn repair_items() -> &'static [i32] {
        &[862]
    }
}

impl EnchantableItem for DiamondHoe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Sword
pub struct NetheriteSword;

impl ItemDef for NetheriteSword {
    const ID: i32 = 900;
    const STRING_ID: &'static str = "minecraft:netherite_sword";
    const NAME: &'static str = "Netherite Sword";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for NetheriteSword {
    const MAX_DURABILITY: u16 = 2031;
}

impl RepairableItem for NetheriteSword {
    fn repair_items() -> &'static [i32] {
        &[873]
    }
}

impl EnchantableItem for NetheriteSword {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Sword,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Shovel
pub struct NetheriteShovel;

impl ItemDef for NetheriteShovel {
    const ID: i32 = 901;
    const STRING_ID: &'static str = "minecraft:netherite_shovel";
    const NAME: &'static str = "Netherite Shovel";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for NetheriteShovel {
    const MAX_DURABILITY: u16 = 2031;
}

impl RepairableItem for NetheriteShovel {
    fn repair_items() -> &'static [i32] {
        &[873]
    }
}

impl EnchantableItem for NetheriteShovel {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Pickaxe
pub struct NetheritePickaxe;

impl ItemDef for NetheritePickaxe {
    const ID: i32 = 902;
    const STRING_ID: &'static str = "minecraft:netherite_pickaxe";
    const NAME: &'static str = "Netherite Pickaxe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for NetheritePickaxe {
    const MAX_DURABILITY: u16 = 2031;
}

impl RepairableItem for NetheritePickaxe {
    fn repair_items() -> &'static [i32] {
        &[873]
    }
}

impl EnchantableItem for NetheritePickaxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Axe
pub struct NetheriteAxe;

impl ItemDef for NetheriteAxe {
    const ID: i32 = 903;
    const STRING_ID: &'static str = "minecraft:netherite_axe";
    const NAME: &'static str = "Netherite Axe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for NetheriteAxe {
    const MAX_DURABILITY: u16 = 2031;
}

impl RepairableItem for NetheriteAxe {
    fn repair_items() -> &'static [i32] {
        &[873]
    }
}

impl EnchantableItem for NetheriteAxe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Hoe
pub struct NetheriteHoe;

impl ItemDef for NetheriteHoe {
    const ID: i32 = 904;
    const STRING_ID: &'static str = "minecraft:netherite_hoe";
    const NAME: &'static str = "Netherite Hoe";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for NetheriteHoe {
    const MAX_DURABILITY: u16 = 2031;
}

impl RepairableItem for NetheriteHoe {
    fn repair_items() -> &'static [i32] {
        &[873]
    }
}

impl EnchantableItem for NetheriteHoe {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Stick
pub struct Stick;

impl ItemDef for Stick {
    const ID: i32 = 905;
    const STRING_ID: &'static str = "minecraft:stick";
    const NAME: &'static str = "Stick";
    const STACK_SIZE: u8 = 64;
}

impl VariantItem for Stick {
    fn variants() -> &'static [ItemVariant] {
        &[ItemVariant {
            id: 1248,
            metadata: 0,
            name: "debug_stick",
            display_name: "Debug Stick",
            stack_size: 1,
        }]
    }
}

/// Mushroom Stew
pub struct MushroomStew;

impl ItemDef for MushroomStew {
    const ID: i32 = 906;
    const STRING_ID: &'static str = "minecraft:mushroom_stew";
    const NAME: &'static str = "Mushroom Stew";
    const STACK_SIZE: u8 = 1;
}

/// String
pub struct String;

impl ItemDef for String {
    const ID: i32 = 907;
    const STRING_ID: &'static str = "minecraft:string";
    const NAME: &'static str = "String";
    const STACK_SIZE: u8 = 64;
}

/// Feather
pub struct Feather;

impl ItemDef for Feather {
    const ID: i32 = 908;
    const STRING_ID: &'static str = "minecraft:feather";
    const NAME: &'static str = "Feather";
    const STACK_SIZE: u8 = 64;
}

/// Gunpowder
pub struct Gunpowder;

impl ItemDef for Gunpowder {
    const ID: i32 = 909;
    const STRING_ID: &'static str = "minecraft:gunpowder";
    const NAME: &'static str = "Gunpowder";
    const STACK_SIZE: u8 = 64;
}

/// Wheat Seeds
pub struct WheatSeeds;

impl ItemDef for WheatSeeds {
    const ID: i32 = 910;
    const STRING_ID: &'static str = "minecraft:wheat_seeds";
    const NAME: &'static str = "Wheat Seeds";
    const STACK_SIZE: u8 = 64;
}

/// Wheat
pub struct Wheat;

impl ItemDef for Wheat {
    const ID: i32 = 911;
    const STRING_ID: &'static str = "minecraft:wheat";
    const NAME: &'static str = "Wheat";
    const STACK_SIZE: u8 = 64;
}

/// Bread
pub struct Bread;

impl ItemDef for Bread {
    const ID: i32 = 912;
    const STRING_ID: &'static str = "minecraft:bread";
    const NAME: &'static str = "Bread";
    const STACK_SIZE: u8 = 64;
}

/// Leather Cap
pub struct LeatherHelmet;

impl ItemDef for LeatherHelmet {
    const ID: i32 = 913;
    const STRING_ID: &'static str = "minecraft:leather_helmet";
    const NAME: &'static str = "Leather Cap";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for LeatherHelmet {
    const MAX_DURABILITY: u16 = 55;
}

impl RepairableItem for LeatherHelmet {
    fn repair_items() -> &'static [i32] {
        &[972]
    }
}

impl EnchantableItem for LeatherHelmet {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::HeadArmor,
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Leather Tunic
pub struct LeatherChestplate;

impl ItemDef for LeatherChestplate {
    const ID: i32 = 914;
    const STRING_ID: &'static str = "minecraft:leather_chestplate";
    const NAME: &'static str = "Leather Tunic";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for LeatherChestplate {
    const MAX_DURABILITY: u16 = 80;
}

impl RepairableItem for LeatherChestplate {
    fn repair_items() -> &'static [i32] {
        &[972]
    }
}

impl EnchantableItem for LeatherChestplate {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Leather Pants
pub struct LeatherLeggings;

impl ItemDef for LeatherLeggings {
    const ID: i32 = 915;
    const STRING_ID: &'static str = "minecraft:leather_leggings";
    const NAME: &'static str = "Leather Pants";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for LeatherLeggings {
    const MAX_DURABILITY: u16 = 75;
}

impl RepairableItem for LeatherLeggings {
    fn repair_items() -> &'static [i32] {
        &[972]
    }
}

impl EnchantableItem for LeatherLeggings {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Leather Boots
pub struct LeatherBoots;

impl ItemDef for LeatherBoots {
    const ID: i32 = 916;
    const STRING_ID: &'static str = "minecraft:leather_boots";
    const NAME: &'static str = "Leather Boots";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for LeatherBoots {
    const MAX_DURABILITY: u16 = 65;
}

impl RepairableItem for LeatherBoots {
    fn repair_items() -> &'static [i32] {
        &[972]
    }
}

impl EnchantableItem for LeatherBoots {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Chainmail Helmet
pub struct ChainmailHelmet;

impl ItemDef for ChainmailHelmet {
    const ID: i32 = 917;
    const STRING_ID: &'static str = "minecraft:chainmail_helmet";
    const NAME: &'static str = "Chainmail Helmet";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for ChainmailHelmet {
    const MAX_DURABILITY: u16 = 165;
}

impl RepairableItem for ChainmailHelmet {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for ChainmailHelmet {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::HeadArmor,
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Chainmail Chestplate
pub struct ChainmailChestplate;

impl ItemDef for ChainmailChestplate {
    const ID: i32 = 918;
    const STRING_ID: &'static str = "minecraft:chainmail_chestplate";
    const NAME: &'static str = "Chainmail Chestplate";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for ChainmailChestplate {
    const MAX_DURABILITY: u16 = 240;
}

impl RepairableItem for ChainmailChestplate {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for ChainmailChestplate {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Chainmail Leggings
pub struct ChainmailLeggings;

impl ItemDef for ChainmailLeggings {
    const ID: i32 = 919;
    const STRING_ID: &'static str = "minecraft:chainmail_leggings";
    const NAME: &'static str = "Chainmail Leggings";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for ChainmailLeggings {
    const MAX_DURABILITY: u16 = 225;
}

impl RepairableItem for ChainmailLeggings {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for ChainmailLeggings {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Chainmail Boots
pub struct ChainmailBoots;

impl ItemDef for ChainmailBoots {
    const ID: i32 = 920;
    const STRING_ID: &'static str = "minecraft:chainmail_boots";
    const NAME: &'static str = "Chainmail Boots";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for ChainmailBoots {
    const MAX_DURABILITY: u16 = 195;
}

impl RepairableItem for ChainmailBoots {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for ChainmailBoots {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Iron Helmet
pub struct IronHelmet;

impl ItemDef for IronHelmet {
    const ID: i32 = 921;
    const STRING_ID: &'static str = "minecraft:iron_helmet";
    const NAME: &'static str = "Iron Helmet";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for IronHelmet {
    const MAX_DURABILITY: u16 = 165;
}

impl RepairableItem for IronHelmet {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for IronHelmet {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::HeadArmor,
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Iron Chestplate
pub struct IronChestplate;

impl ItemDef for IronChestplate {
    const ID: i32 = 922;
    const STRING_ID: &'static str = "minecraft:iron_chestplate";
    const NAME: &'static str = "Iron Chestplate";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for IronChestplate {
    const MAX_DURABILITY: u16 = 240;
}

impl RepairableItem for IronChestplate {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for IronChestplate {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Iron Leggings
pub struct IronLeggings;

impl ItemDef for IronLeggings {
    const ID: i32 = 923;
    const STRING_ID: &'static str = "minecraft:iron_leggings";
    const NAME: &'static str = "Iron Leggings";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for IronLeggings {
    const MAX_DURABILITY: u16 = 225;
}

impl RepairableItem for IronLeggings {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for IronLeggings {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Iron Boots
pub struct IronBoots;

impl ItemDef for IronBoots {
    const ID: i32 = 924;
    const STRING_ID: &'static str = "minecraft:iron_boots";
    const NAME: &'static str = "Iron Boots";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for IronBoots {
    const MAX_DURABILITY: u16 = 195;
}

impl RepairableItem for IronBoots {
    fn repair_items() -> &'static [i32] {
        &[868]
    }
}

impl EnchantableItem for IronBoots {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Diamond Helmet
pub struct DiamondHelmet;

impl ItemDef for DiamondHelmet {
    const ID: i32 = 925;
    const STRING_ID: &'static str = "minecraft:diamond_helmet";
    const NAME: &'static str = "Diamond Helmet";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondHelmet {
    const MAX_DURABILITY: u16 = 363;
}

impl RepairableItem for DiamondHelmet {
    fn repair_items() -> &'static [i32] {
        &[862]
    }
}

impl EnchantableItem for DiamondHelmet {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::HeadArmor,
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Diamond Chestplate
pub struct DiamondChestplate;

impl ItemDef for DiamondChestplate {
    const ID: i32 = 926;
    const STRING_ID: &'static str = "minecraft:diamond_chestplate";
    const NAME: &'static str = "Diamond Chestplate";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondChestplate {
    const MAX_DURABILITY: u16 = 528;
}

impl RepairableItem for DiamondChestplate {
    fn repair_items() -> &'static [i32] {
        &[862]
    }
}

impl EnchantableItem for DiamondChestplate {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Diamond Leggings
pub struct DiamondLeggings;

impl ItemDef for DiamondLeggings {
    const ID: i32 = 927;
    const STRING_ID: &'static str = "minecraft:diamond_leggings";
    const NAME: &'static str = "Diamond Leggings";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondLeggings {
    const MAX_DURABILITY: u16 = 495;
}

impl RepairableItem for DiamondLeggings {
    fn repair_items() -> &'static [i32] {
        &[862]
    }
}

impl EnchantableItem for DiamondLeggings {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Diamond Boots
pub struct DiamondBoots;

impl ItemDef for DiamondBoots {
    const ID: i32 = 928;
    const STRING_ID: &'static str = "minecraft:diamond_boots";
    const NAME: &'static str = "Diamond Boots";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondBoots {
    const MAX_DURABILITY: u16 = 429;
}

impl RepairableItem for DiamondBoots {
    fn repair_items() -> &'static [i32] {
        &[862]
    }
}

impl EnchantableItem for DiamondBoots {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Golden Helmet
pub struct GoldenHelmet;

impl ItemDef for GoldenHelmet {
    const ID: i32 = 929;
    const STRING_ID: &'static str = "minecraft:golden_helmet";
    const NAME: &'static str = "Golden Helmet";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for GoldenHelmet {
    const MAX_DURABILITY: u16 = 77;
}

impl RepairableItem for GoldenHelmet {
    fn repair_items() -> &'static [i32] {
        &[872]
    }
}

impl EnchantableItem for GoldenHelmet {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::HeadArmor,
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Golden Chestplate
pub struct GoldenChestplate;

impl ItemDef for GoldenChestplate {
    const ID: i32 = 930;
    const STRING_ID: &'static str = "minecraft:golden_chestplate";
    const NAME: &'static str = "Golden Chestplate";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for GoldenChestplate {
    const MAX_DURABILITY: u16 = 112;
}

impl RepairableItem for GoldenChestplate {
    fn repair_items() -> &'static [i32] {
        &[872]
    }
}

impl EnchantableItem for GoldenChestplate {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Golden Leggings
pub struct GoldenLeggings;

impl ItemDef for GoldenLeggings {
    const ID: i32 = 931;
    const STRING_ID: &'static str = "minecraft:golden_leggings";
    const NAME: &'static str = "Golden Leggings";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for GoldenLeggings {
    const MAX_DURABILITY: u16 = 105;
}

impl RepairableItem for GoldenLeggings {
    fn repair_items() -> &'static [i32] {
        &[872]
    }
}

impl EnchantableItem for GoldenLeggings {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Golden Boots
pub struct GoldenBoots;

impl ItemDef for GoldenBoots {
    const ID: i32 = 932;
    const STRING_ID: &'static str = "minecraft:golden_boots";
    const NAME: &'static str = "Golden Boots";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for GoldenBoots {
    const MAX_DURABILITY: u16 = 91;
}

impl RepairableItem for GoldenBoots {
    fn repair_items() -> &'static [i32] {
        &[872]
    }
}

impl EnchantableItem for GoldenBoots {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Helmet
pub struct NetheriteHelmet;

impl ItemDef for NetheriteHelmet {
    const ID: i32 = 933;
    const STRING_ID: &'static str = "minecraft:netherite_helmet";
    const NAME: &'static str = "Netherite Helmet";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for NetheriteHelmet {
    const MAX_DURABILITY: u16 = 407;
}

impl RepairableItem for NetheriteHelmet {
    fn repair_items() -> &'static [i32] {
        &[873]
    }
}

impl EnchantableItem for NetheriteHelmet {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::HeadArmor,
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Chestplate
pub struct NetheriteChestplate;

impl ItemDef for NetheriteChestplate {
    const ID: i32 = 934;
    const STRING_ID: &'static str = "minecraft:netherite_chestplate";
    const NAME: &'static str = "Netherite Chestplate";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for NetheriteChestplate {
    const MAX_DURABILITY: u16 = 592;
}

impl RepairableItem for NetheriteChestplate {
    fn repair_items() -> &'static [i32] {
        &[873]
    }
}

impl EnchantableItem for NetheriteChestplate {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Leggings
pub struct NetheriteLeggings;

impl ItemDef for NetheriteLeggings {
    const ID: i32 = 935;
    const STRING_ID: &'static str = "minecraft:netherite_leggings";
    const NAME: &'static str = "Netherite Leggings";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for NetheriteLeggings {
    const MAX_DURABILITY: u16 = 555;
}

impl RepairableItem for NetheriteLeggings {
    fn repair_items() -> &'static [i32] {
        &[873]
    }
}

impl EnchantableItem for NetheriteLeggings {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Boots
pub struct NetheriteBoots;

impl ItemDef for NetheriteBoots {
    const ID: i32 = 936;
    const STRING_ID: &'static str = "minecraft:netherite_boots";
    const NAME: &'static str = "Netherite Boots";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for NetheriteBoots {
    const MAX_DURABILITY: u16 = 481;
}

impl RepairableItem for NetheriteBoots {
    fn repair_items() -> &'static [i32] {
        &[873]
    }
}

impl EnchantableItem for NetheriteBoots {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Armor,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Flint
pub struct Flint;

impl ItemDef for Flint {
    const ID: i32 = 937;
    const STRING_ID: &'static str = "minecraft:flint";
    const NAME: &'static str = "Flint";
    const STACK_SIZE: u8 = 64;
}

/// Raw Porkchop
pub struct Porkchop;

impl ItemDef for Porkchop {
    const ID: i32 = 938;
    const STRING_ID: &'static str = "minecraft:porkchop";
    const NAME: &'static str = "Raw Porkchop";
    const STACK_SIZE: u8 = 64;
}

/// Cooked Porkchop
pub struct CookedPorkchop;

impl ItemDef for CookedPorkchop {
    const ID: i32 = 939;
    const STRING_ID: &'static str = "minecraft:cooked_porkchop";
    const NAME: &'static str = "Cooked Porkchop";
    const STACK_SIZE: u8 = 64;
}

/// Painting
pub struct Painting;

impl ItemDef for Painting {
    const ID: i32 = 940;
    const STRING_ID: &'static str = "minecraft:painting";
    const NAME: &'static str = "Painting";
    const STACK_SIZE: u8 = 64;
}

/// Golden Apple
pub struct GoldenApple;

impl ItemDef for GoldenApple {
    const ID: i32 = 941;
    const STRING_ID: &'static str = "minecraft:golden_apple";
    const NAME: &'static str = "Golden Apple";
    const STACK_SIZE: u8 = 64;
}

/// Enchanted Golden Apple
pub struct EnchantedGoldenApple;

impl ItemDef for EnchantedGoldenApple {
    const ID: i32 = 942;
    const STRING_ID: &'static str = "minecraft:enchanted_golden_apple";
    const NAME: &'static str = "Enchanted Golden Apple";
    const STACK_SIZE: u8 = 64;
}

/// Oak Sign
pub struct OakSign;

impl ItemDef for OakSign {
    const ID: i32 = 943;
    const STRING_ID: &'static str = "minecraft:oak_sign";
    const NAME: &'static str = "Oak Sign";
    const STACK_SIZE: u8 = 16;
}

/// Spruce Sign
pub struct SpruceSign;

impl ItemDef for SpruceSign {
    const ID: i32 = 944;
    const STRING_ID: &'static str = "minecraft:spruce_sign";
    const NAME: &'static str = "Spruce Sign";
    const STACK_SIZE: u8 = 16;
}

/// Birch Sign
pub struct BirchSign;

impl ItemDef for BirchSign {
    const ID: i32 = 945;
    const STRING_ID: &'static str = "minecraft:birch_sign";
    const NAME: &'static str = "Birch Sign";
    const STACK_SIZE: u8 = 16;
}

/// Jungle Sign
pub struct JungleSign;

impl ItemDef for JungleSign {
    const ID: i32 = 946;
    const STRING_ID: &'static str = "minecraft:jungle_sign";
    const NAME: &'static str = "Jungle Sign";
    const STACK_SIZE: u8 = 16;
}

/// Acacia Sign
pub struct AcaciaSign;

impl ItemDef for AcaciaSign {
    const ID: i32 = 947;
    const STRING_ID: &'static str = "minecraft:acacia_sign";
    const NAME: &'static str = "Acacia Sign";
    const STACK_SIZE: u8 = 16;
}

/// Cherry Sign
pub struct CherrySign;

impl ItemDef for CherrySign {
    const ID: i32 = 948;
    const STRING_ID: &'static str = "minecraft:cherry_sign";
    const NAME: &'static str = "Cherry Sign";
    const STACK_SIZE: u8 = 16;
}

/// Dark Oak Sign
pub struct DarkOakSign;

impl ItemDef for DarkOakSign {
    const ID: i32 = 949;
    const STRING_ID: &'static str = "minecraft:dark_oak_sign";
    const NAME: &'static str = "Dark Oak Sign";
    const STACK_SIZE: u8 = 16;
}

/// Pale Oak Sign
pub struct PaleOakSign;

impl ItemDef for PaleOakSign {
    const ID: i32 = 950;
    const STRING_ID: &'static str = "minecraft:pale_oak_sign";
    const NAME: &'static str = "Pale Oak Sign";
    const STACK_SIZE: u8 = 16;
}

/// Mangrove Sign
pub struct MangroveSign;

impl ItemDef for MangroveSign {
    const ID: i32 = 951;
    const STRING_ID: &'static str = "minecraft:mangrove_sign";
    const NAME: &'static str = "Mangrove Sign";
    const STACK_SIZE: u8 = 16;
}

/// Bamboo Sign
pub struct BambooSign;

impl ItemDef for BambooSign {
    const ID: i32 = 952;
    const STRING_ID: &'static str = "minecraft:bamboo_sign";
    const NAME: &'static str = "Bamboo Sign";
    const STACK_SIZE: u8 = 16;
}

/// Crimson Sign
pub struct CrimsonSign;

impl ItemDef for CrimsonSign {
    const ID: i32 = 953;
    const STRING_ID: &'static str = "minecraft:crimson_sign";
    const NAME: &'static str = "Crimson Sign";
    const STACK_SIZE: u8 = 16;
}

/// Warped Sign
pub struct WarpedSign;

impl ItemDef for WarpedSign {
    const ID: i32 = 954;
    const STRING_ID: &'static str = "minecraft:warped_sign";
    const NAME: &'static str = "Warped Sign";
    const STACK_SIZE: u8 = 16;
}

/// Oak Hanging Sign
pub struct OakHangingSign;

impl ItemDef for OakHangingSign {
    const ID: i32 = 955;
    const STRING_ID: &'static str = "minecraft:oak_hanging_sign";
    const NAME: &'static str = "Oak Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Spruce Hanging Sign
pub struct SpruceHangingSign;

impl ItemDef for SpruceHangingSign {
    const ID: i32 = 956;
    const STRING_ID: &'static str = "minecraft:spruce_hanging_sign";
    const NAME: &'static str = "Spruce Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Birch Hanging Sign
pub struct BirchHangingSign;

impl ItemDef for BirchHangingSign {
    const ID: i32 = 957;
    const STRING_ID: &'static str = "minecraft:birch_hanging_sign";
    const NAME: &'static str = "Birch Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Jungle Hanging Sign
pub struct JungleHangingSign;

impl ItemDef for JungleHangingSign {
    const ID: i32 = 958;
    const STRING_ID: &'static str = "minecraft:jungle_hanging_sign";
    const NAME: &'static str = "Jungle Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Acacia Hanging Sign
pub struct AcaciaHangingSign;

impl ItemDef for AcaciaHangingSign {
    const ID: i32 = 959;
    const STRING_ID: &'static str = "minecraft:acacia_hanging_sign";
    const NAME: &'static str = "Acacia Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Cherry Hanging Sign
pub struct CherryHangingSign;

impl ItemDef for CherryHangingSign {
    const ID: i32 = 960;
    const STRING_ID: &'static str = "minecraft:cherry_hanging_sign";
    const NAME: &'static str = "Cherry Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Dark Oak Hanging Sign
pub struct DarkOakHangingSign;

impl ItemDef for DarkOakHangingSign {
    const ID: i32 = 961;
    const STRING_ID: &'static str = "minecraft:dark_oak_hanging_sign";
    const NAME: &'static str = "Dark Oak Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Pale Oak Hanging Sign
pub struct PaleOakHangingSign;

impl ItemDef for PaleOakHangingSign {
    const ID: i32 = 962;
    const STRING_ID: &'static str = "minecraft:pale_oak_hanging_sign";
    const NAME: &'static str = "Pale Oak Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Mangrove Hanging Sign
pub struct MangroveHangingSign;

impl ItemDef for MangroveHangingSign {
    const ID: i32 = 963;
    const STRING_ID: &'static str = "minecraft:mangrove_hanging_sign";
    const NAME: &'static str = "Mangrove Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Bamboo Hanging Sign
pub struct BambooHangingSign;

impl ItemDef for BambooHangingSign {
    const ID: i32 = 964;
    const STRING_ID: &'static str = "minecraft:bamboo_hanging_sign";
    const NAME: &'static str = "Bamboo Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Crimson Hanging Sign
pub struct CrimsonHangingSign;

impl ItemDef for CrimsonHangingSign {
    const ID: i32 = 965;
    const STRING_ID: &'static str = "minecraft:crimson_hanging_sign";
    const NAME: &'static str = "Crimson Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Warped Hanging Sign
pub struct WarpedHangingSign;

impl ItemDef for WarpedHangingSign {
    const ID: i32 = 966;
    const STRING_ID: &'static str = "minecraft:warped_hanging_sign";
    const NAME: &'static str = "Warped Hanging Sign";
    const STACK_SIZE: u8 = 16;
}

/// Bucket
pub struct Bucket;

impl ItemDef for Bucket {
    const ID: i32 = 967;
    const STRING_ID: &'static str = "minecraft:bucket";
    const NAME: &'static str = "Bucket";
    const STACK_SIZE: u8 = 16;
}

/// Water Bucket
pub struct WaterBucket;

impl ItemDef for WaterBucket {
    const ID: i32 = 968;
    const STRING_ID: &'static str = "minecraft:water_bucket";
    const NAME: &'static str = "Water Bucket";
    const STACK_SIZE: u8 = 1;
}

/// Lava Bucket
pub struct LavaBucket;

impl ItemDef for LavaBucket {
    const ID: i32 = 969;
    const STRING_ID: &'static str = "minecraft:lava_bucket";
    const NAME: &'static str = "Lava Bucket";
    const STACK_SIZE: u8 = 1;
}

/// Powder Snow Bucket
pub struct PowderSnowBucket;

impl ItemDef for PowderSnowBucket {
    const ID: i32 = 970;
    const STRING_ID: &'static str = "minecraft:powder_snow_bucket";
    const NAME: &'static str = "Powder Snow Bucket";
    const STACK_SIZE: u8 = 1;
}

/// Snowball
pub struct Snowball;

impl ItemDef for Snowball {
    const ID: i32 = 971;
    const STRING_ID: &'static str = "minecraft:snowball";
    const NAME: &'static str = "Snowball";
    const STACK_SIZE: u8 = 16;
}

/// Leather
pub struct Leather;

impl ItemDef for Leather {
    const ID: i32 = 972;
    const STRING_ID: &'static str = "minecraft:leather";
    const NAME: &'static str = "Leather";
    const STACK_SIZE: u8 = 64;
}

/// Milk Bucket
pub struct MilkBucket;

impl ItemDef for MilkBucket {
    const ID: i32 = 973;
    const STRING_ID: &'static str = "minecraft:milk_bucket";
    const NAME: &'static str = "Milk Bucket";
    const STACK_SIZE: u8 = 1;
}

/// Bucket of Pufferfish
pub struct PufferfishBucket;

impl ItemDef for PufferfishBucket {
    const ID: i32 = 974;
    const STRING_ID: &'static str = "minecraft:pufferfish_bucket";
    const NAME: &'static str = "Bucket of Pufferfish";
    const STACK_SIZE: u8 = 1;
}

/// Bucket of Salmon
pub struct SalmonBucket;

impl ItemDef for SalmonBucket {
    const ID: i32 = 975;
    const STRING_ID: &'static str = "minecraft:salmon_bucket";
    const NAME: &'static str = "Bucket of Salmon";
    const STACK_SIZE: u8 = 1;
}

/// Bucket of Cod
pub struct CodBucket;

impl ItemDef for CodBucket {
    const ID: i32 = 976;
    const STRING_ID: &'static str = "minecraft:cod_bucket";
    const NAME: &'static str = "Bucket of Cod";
    const STACK_SIZE: u8 = 1;
}

/// Bucket of Tropical Fish
pub struct TropicalFishBucket;

impl ItemDef for TropicalFishBucket {
    const ID: i32 = 977;
    const STRING_ID: &'static str = "minecraft:tropical_fish_bucket";
    const NAME: &'static str = "Bucket of Tropical Fish";
    const STACK_SIZE: u8 = 1;
}

/// Bucket of Axolotl
pub struct AxolotlBucket;

impl ItemDef for AxolotlBucket {
    const ID: i32 = 978;
    const STRING_ID: &'static str = "minecraft:axolotl_bucket";
    const NAME: &'static str = "Bucket of Axolotl";
    const STACK_SIZE: u8 = 1;
}

/// Bucket of Tadpole
pub struct TadpoleBucket;

impl ItemDef for TadpoleBucket {
    const ID: i32 = 979;
    const STRING_ID: &'static str = "minecraft:tadpole_bucket";
    const NAME: &'static str = "Bucket of Tadpole";
    const STACK_SIZE: u8 = 1;
}

/// Brick
pub struct Brick;

impl ItemDef for Brick {
    const ID: i32 = 980;
    const STRING_ID: &'static str = "minecraft:brick";
    const NAME: &'static str = "Brick";
    const STACK_SIZE: u8 = 64;
}

/// Clay Ball
pub struct ClayBall;

impl ItemDef for ClayBall {
    const ID: i32 = 981;
    const STRING_ID: &'static str = "minecraft:clay_ball";
    const NAME: &'static str = "Clay Ball";
    const STACK_SIZE: u8 = 64;
}

/// Dried Kelp Block
pub struct DriedKelpBlock;

impl ItemDef for DriedKelpBlock {
    const ID: i32 = 982;
    const STRING_ID: &'static str = "minecraft:dried_kelp_block";
    const NAME: &'static str = "Dried Kelp Block";
    const STACK_SIZE: u8 = 64;
}

/// Paper
pub struct Paper;

impl ItemDef for Paper {
    const ID: i32 = 983;
    const STRING_ID: &'static str = "minecraft:paper";
    const NAME: &'static str = "Paper";
    const STACK_SIZE: u8 = 64;
}

/// Book
pub struct Book;

impl ItemDef for Book {
    const ID: i32 = 984;
    const STRING_ID: &'static str = "minecraft:book";
    const NAME: &'static str = "Book";
    const STACK_SIZE: u8 = 64;
}

impl VariantItem for Book {
    fn variants() -> &'static [ItemVariant] {
        &[ItemVariant {
            id: 1247,
            metadata: 0,
            name: "knowledge_book",
            display_name: "Knowledge Book",
            stack_size: 1,
        }]
    }
}

/// Slimeball
pub struct SlimeBall;

impl ItemDef for SlimeBall {
    const ID: i32 = 985;
    const STRING_ID: &'static str = "minecraft:slime_ball";
    const NAME: &'static str = "Slimeball";
    const STACK_SIZE: u8 = 64;
}

/// Egg
pub struct Egg;

impl ItemDef for Egg {
    const ID: i32 = 986;
    const STRING_ID: &'static str = "minecraft:egg";
    const NAME: &'static str = "Egg";
    const STACK_SIZE: u8 = 16;
}

/// Blue Egg
pub struct BlueEgg;

impl ItemDef for BlueEgg {
    const ID: i32 = 987;
    const STRING_ID: &'static str = "minecraft:blue_egg";
    const NAME: &'static str = "Blue Egg";
    const STACK_SIZE: u8 = 16;
}

/// Brown Egg
pub struct BrownEgg;

impl ItemDef for BrownEgg {
    const ID: i32 = 988;
    const STRING_ID: &'static str = "minecraft:brown_egg";
    const NAME: &'static str = "Brown Egg";
    const STACK_SIZE: u8 = 16;
}

/// Compass
pub struct Compass;

impl ItemDef for Compass {
    const ID: i32 = 989;
    const STRING_ID: &'static str = "minecraft:compass";
    const NAME: &'static str = "Compass";
    const STACK_SIZE: u8 = 64;
}

impl EnchantableItem for Compass {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[EnchantmentCategory::Vanishing]
    }
}

/// Recovery Compass
pub struct RecoveryCompass;

impl ItemDef for RecoveryCompass {
    const ID: i32 = 990;
    const STRING_ID: &'static str = "minecraft:recovery_compass";
    const NAME: &'static str = "Recovery Compass";
    const STACK_SIZE: u8 = 64;
}

/// Bundle
pub struct Bundle;

impl ItemDef for Bundle {
    const ID: i32 = 991;
    const STRING_ID: &'static str = "minecraft:bundle";
    const NAME: &'static str = "Bundle";
    const STACK_SIZE: u8 = 1;
}

/// White Bundle
pub struct WhiteBundle;

impl ItemDef for WhiteBundle {
    const ID: i32 = 992;
    const STRING_ID: &'static str = "minecraft:white_bundle";
    const NAME: &'static str = "White Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Orange Bundle
pub struct OrangeBundle;

impl ItemDef for OrangeBundle {
    const ID: i32 = 993;
    const STRING_ID: &'static str = "minecraft:orange_bundle";
    const NAME: &'static str = "Orange Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Magenta Bundle
pub struct MagentaBundle;

impl ItemDef for MagentaBundle {
    const ID: i32 = 994;
    const STRING_ID: &'static str = "minecraft:magenta_bundle";
    const NAME: &'static str = "Magenta Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Light Blue Bundle
pub struct LightBlueBundle;

impl ItemDef for LightBlueBundle {
    const ID: i32 = 995;
    const STRING_ID: &'static str = "minecraft:light_blue_bundle";
    const NAME: &'static str = "Light Blue Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Yellow Bundle
pub struct YellowBundle;

impl ItemDef for YellowBundle {
    const ID: i32 = 996;
    const STRING_ID: &'static str = "minecraft:yellow_bundle";
    const NAME: &'static str = "Yellow Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Lime Bundle
pub struct LimeBundle;

impl ItemDef for LimeBundle {
    const ID: i32 = 997;
    const STRING_ID: &'static str = "minecraft:lime_bundle";
    const NAME: &'static str = "Lime Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Pink Bundle
pub struct PinkBundle;

impl ItemDef for PinkBundle {
    const ID: i32 = 998;
    const STRING_ID: &'static str = "minecraft:pink_bundle";
    const NAME: &'static str = "Pink Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Gray Bundle
pub struct GrayBundle;

impl ItemDef for GrayBundle {
    const ID: i32 = 999;
    const STRING_ID: &'static str = "minecraft:gray_bundle";
    const NAME: &'static str = "Gray Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Light Gray Bundle
pub struct LightGrayBundle;

impl ItemDef for LightGrayBundle {
    const ID: i32 = 1000;
    const STRING_ID: &'static str = "minecraft:light_gray_bundle";
    const NAME: &'static str = "Light Gray Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Cyan Bundle
pub struct CyanBundle;

impl ItemDef for CyanBundle {
    const ID: i32 = 1001;
    const STRING_ID: &'static str = "minecraft:cyan_bundle";
    const NAME: &'static str = "Cyan Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Purple Bundle
pub struct PurpleBundle;

impl ItemDef for PurpleBundle {
    const ID: i32 = 1002;
    const STRING_ID: &'static str = "minecraft:purple_bundle";
    const NAME: &'static str = "Purple Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Blue Bundle
pub struct BlueBundle;

impl ItemDef for BlueBundle {
    const ID: i32 = 1003;
    const STRING_ID: &'static str = "minecraft:blue_bundle";
    const NAME: &'static str = "Blue Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Brown Bundle
pub struct BrownBundle;

impl ItemDef for BrownBundle {
    const ID: i32 = 1004;
    const STRING_ID: &'static str = "minecraft:brown_bundle";
    const NAME: &'static str = "Brown Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Green Bundle
pub struct GreenBundle;

impl ItemDef for GreenBundle {
    const ID: i32 = 1005;
    const STRING_ID: &'static str = "minecraft:green_bundle";
    const NAME: &'static str = "Green Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Red Bundle
pub struct RedBundle;

impl ItemDef for RedBundle {
    const ID: i32 = 1006;
    const STRING_ID: &'static str = "minecraft:red_bundle";
    const NAME: &'static str = "Red Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Black Bundle
pub struct BlackBundle;

impl ItemDef for BlackBundle {
    const ID: i32 = 1007;
    const STRING_ID: &'static str = "minecraft:black_bundle";
    const NAME: &'static str = "Black Bundle";
    const STACK_SIZE: u8 = 1;
}

/// Fishing Rod
pub struct FishingRod;

impl ItemDef for FishingRod {
    const ID: i32 = 1008;
    const STRING_ID: &'static str = "minecraft:fishing_rod";
    const NAME: &'static str = "Fishing Rod";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for FishingRod {
    const MAX_DURABILITY: u16 = 64;
}

impl EnchantableItem for FishingRod {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Clock
pub struct Clock;

impl ItemDef for Clock {
    const ID: i32 = 1009;
    const STRING_ID: &'static str = "minecraft:clock";
    const NAME: &'static str = "Clock";
    const STACK_SIZE: u8 = 64;
}

/// Spyglass
pub struct Spyglass;

impl ItemDef for Spyglass {
    const ID: i32 = 1010;
    const STRING_ID: &'static str = "minecraft:spyglass";
    const NAME: &'static str = "Spyglass";
    const STACK_SIZE: u8 = 1;
}

/// Glowstone Dust
pub struct GlowstoneDust;

impl ItemDef for GlowstoneDust {
    const ID: i32 = 1011;
    const STRING_ID: &'static str = "minecraft:glowstone_dust";
    const NAME: &'static str = "Glowstone Dust";
    const STACK_SIZE: u8 = 64;
}

/// Raw Cod
pub struct Cod;

impl ItemDef for Cod {
    const ID: i32 = 1012;
    const STRING_ID: &'static str = "minecraft:cod";
    const NAME: &'static str = "Raw Cod";
    const STACK_SIZE: u8 = 64;
}

/// Raw Salmon
pub struct Salmon;

impl ItemDef for Salmon {
    const ID: i32 = 1013;
    const STRING_ID: &'static str = "minecraft:salmon";
    const NAME: &'static str = "Raw Salmon";
    const STACK_SIZE: u8 = 64;
}

/// Tropical Fish
pub struct TropicalFish;

impl ItemDef for TropicalFish {
    const ID: i32 = 1014;
    const STRING_ID: &'static str = "minecraft:tropical_fish";
    const NAME: &'static str = "Tropical Fish";
    const STACK_SIZE: u8 = 64;
}

/// Pufferfish
pub struct Pufferfish;

impl ItemDef for Pufferfish {
    const ID: i32 = 1015;
    const STRING_ID: &'static str = "minecraft:pufferfish";
    const NAME: &'static str = "Pufferfish";
    const STACK_SIZE: u8 = 64;
}

/// Cooked Cod
pub struct CookedCod;

impl ItemDef for CookedCod {
    const ID: i32 = 1016;
    const STRING_ID: &'static str = "minecraft:cooked_cod";
    const NAME: &'static str = "Cooked Cod";
    const STACK_SIZE: u8 = 64;
}

/// Cooked Salmon
pub struct CookedSalmon;

impl ItemDef for CookedSalmon {
    const ID: i32 = 1017;
    const STRING_ID: &'static str = "minecraft:cooked_salmon";
    const NAME: &'static str = "Cooked Salmon";
    const STACK_SIZE: u8 = 64;
}

/// Ink Sac
pub struct InkSac;

impl ItemDef for InkSac {
    const ID: i32 = 1018;
    const STRING_ID: &'static str = "minecraft:ink_sac";
    const NAME: &'static str = "Ink Sac";
    const STACK_SIZE: u8 = 64;
}

/// Glow Ink Sac
pub struct GlowInkSac;

impl ItemDef for GlowInkSac {
    const ID: i32 = 1019;
    const STRING_ID: &'static str = "minecraft:glow_ink_sac";
    const NAME: &'static str = "Glow Ink Sac";
    const STACK_SIZE: u8 = 64;
}

/// Cocoa Beans
pub struct CocoaBeans;

impl ItemDef for CocoaBeans {
    const ID: i32 = 1020;
    const STRING_ID: &'static str = "minecraft:cocoa_beans";
    const NAME: &'static str = "Cocoa Beans";
    const STACK_SIZE: u8 = 64;
}

/// White Dye
pub struct WhiteDye;

impl ItemDef for WhiteDye {
    const ID: i32 = 1021;
    const STRING_ID: &'static str = "minecraft:white_dye";
    const NAME: &'static str = "White Dye";
    const STACK_SIZE: u8 = 64;
}

/// Orange Dye
pub struct OrangeDye;

impl ItemDef for OrangeDye {
    const ID: i32 = 1022;
    const STRING_ID: &'static str = "minecraft:orange_dye";
    const NAME: &'static str = "Orange Dye";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Dye
pub struct MagentaDye;

impl ItemDef for MagentaDye {
    const ID: i32 = 1023;
    const STRING_ID: &'static str = "minecraft:magenta_dye";
    const NAME: &'static str = "Magenta Dye";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Dye
pub struct LightBlueDye;

impl ItemDef for LightBlueDye {
    const ID: i32 = 1024;
    const STRING_ID: &'static str = "minecraft:light_blue_dye";
    const NAME: &'static str = "Light Blue Dye";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Dye
pub struct YellowDye;

impl ItemDef for YellowDye {
    const ID: i32 = 1025;
    const STRING_ID: &'static str = "minecraft:yellow_dye";
    const NAME: &'static str = "Yellow Dye";
    const STACK_SIZE: u8 = 64;
}

/// Lime Dye
pub struct LimeDye;

impl ItemDef for LimeDye {
    const ID: i32 = 1026;
    const STRING_ID: &'static str = "minecraft:lime_dye";
    const NAME: &'static str = "Lime Dye";
    const STACK_SIZE: u8 = 64;
}

/// Pink Dye
pub struct PinkDye;

impl ItemDef for PinkDye {
    const ID: i32 = 1027;
    const STRING_ID: &'static str = "minecraft:pink_dye";
    const NAME: &'static str = "Pink Dye";
    const STACK_SIZE: u8 = 64;
}

/// Gray Dye
pub struct GrayDye;

impl ItemDef for GrayDye {
    const ID: i32 = 1028;
    const STRING_ID: &'static str = "minecraft:gray_dye";
    const NAME: &'static str = "Gray Dye";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Dye
pub struct LightGrayDye;

impl ItemDef for LightGrayDye {
    const ID: i32 = 1029;
    const STRING_ID: &'static str = "minecraft:light_gray_dye";
    const NAME: &'static str = "Light Gray Dye";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Dye
pub struct CyanDye;

impl ItemDef for CyanDye {
    const ID: i32 = 1030;
    const STRING_ID: &'static str = "minecraft:cyan_dye";
    const NAME: &'static str = "Cyan Dye";
    const STACK_SIZE: u8 = 64;
}

/// Purple Dye
pub struct PurpleDye;

impl ItemDef for PurpleDye {
    const ID: i32 = 1031;
    const STRING_ID: &'static str = "minecraft:purple_dye";
    const NAME: &'static str = "Purple Dye";
    const STACK_SIZE: u8 = 64;
}

/// Blue Dye
pub struct BlueDye;

impl ItemDef for BlueDye {
    const ID: i32 = 1032;
    const STRING_ID: &'static str = "minecraft:blue_dye";
    const NAME: &'static str = "Blue Dye";
    const STACK_SIZE: u8 = 64;
}

/// Brown Dye
pub struct BrownDye;

impl ItemDef for BrownDye {
    const ID: i32 = 1033;
    const STRING_ID: &'static str = "minecraft:brown_dye";
    const NAME: &'static str = "Brown Dye";
    const STACK_SIZE: u8 = 64;
}

/// Green Dye
pub struct GreenDye;

impl ItemDef for GreenDye {
    const ID: i32 = 1034;
    const STRING_ID: &'static str = "minecraft:green_dye";
    const NAME: &'static str = "Green Dye";
    const STACK_SIZE: u8 = 64;
}

/// Red Dye
pub struct RedDye;

impl ItemDef for RedDye {
    const ID: i32 = 1035;
    const STRING_ID: &'static str = "minecraft:red_dye";
    const NAME: &'static str = "Red Dye";
    const STACK_SIZE: u8 = 64;
}

/// Black Dye
pub struct BlackDye;

impl ItemDef for BlackDye {
    const ID: i32 = 1036;
    const STRING_ID: &'static str = "minecraft:black_dye";
    const NAME: &'static str = "Black Dye";
    const STACK_SIZE: u8 = 64;
}

/// Bone Meal
pub struct BoneMeal;

impl ItemDef for BoneMeal {
    const ID: i32 = 1037;
    const STRING_ID: &'static str = "minecraft:bone_meal";
    const NAME: &'static str = "Bone Meal";
    const STACK_SIZE: u8 = 64;
}

/// Bone
pub struct Bone;

impl ItemDef for Bone {
    const ID: i32 = 1038;
    const STRING_ID: &'static str = "minecraft:bone";
    const NAME: &'static str = "Bone";
    const STACK_SIZE: u8 = 64;
}

/// Sugar
pub struct Sugar;

impl ItemDef for Sugar {
    const ID: i32 = 1039;
    const STRING_ID: &'static str = "minecraft:sugar";
    const NAME: &'static str = "Sugar";
    const STACK_SIZE: u8 = 64;
}

/// Cake
pub struct Cake;

impl ItemDef for Cake {
    const ID: i32 = 1040;
    const STRING_ID: &'static str = "minecraft:cake";
    const NAME: &'static str = "Cake";
    const STACK_SIZE: u8 = 1;
}

/// White Bed
pub struct Bed;

impl ItemDef for Bed {
    const ID: i32 = 1041;
    const STRING_ID: &'static str = "minecraft:bed";
    const NAME: &'static str = "White Bed";
    const STACK_SIZE: u8 = 1;
}

impl VariantItem for Bed {
    fn variants() -> &'static [ItemVariant] {
        &[
            ItemVariant {
                id: 1042,
                metadata: 1,
                name: "orange_bed",
                display_name: "Orange Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1043,
                metadata: 2,
                name: "magenta_bed",
                display_name: "Magenta Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1044,
                metadata: 3,
                name: "light_blue_bed",
                display_name: "Light Blue Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1045,
                metadata: 4,
                name: "yellow_bed",
                display_name: "Yellow Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1046,
                metadata: 5,
                name: "lime_bed",
                display_name: "Lime Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1047,
                metadata: 6,
                name: "pink_bed",
                display_name: "Pink Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1048,
                metadata: 7,
                name: "gray_bed",
                display_name: "Gray Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1049,
                metadata: 8,
                name: "light_gray_bed",
                display_name: "Light Gray Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1050,
                metadata: 9,
                name: "cyan_bed",
                display_name: "Cyan Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1051,
                metadata: 10,
                name: "purple_bed",
                display_name: "Purple Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1052,
                metadata: 11,
                name: "blue_bed",
                display_name: "Blue Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1053,
                metadata: 12,
                name: "brown_bed",
                display_name: "Brown Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1054,
                metadata: 13,
                name: "green_bed",
                display_name: "Green Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1055,
                metadata: 14,
                name: "red_bed",
                display_name: "Red Bed",
                stack_size: 1,
            },
            ItemVariant {
                id: 1056,
                metadata: 15,
                name: "black_bed",
                display_name: "Black Bed",
                stack_size: 1,
            },
        ]
    }
}

/// Cookie
pub struct Cookie;

impl ItemDef for Cookie {
    const ID: i32 = 1057;
    const STRING_ID: &'static str = "minecraft:cookie";
    const NAME: &'static str = "Cookie";
    const STACK_SIZE: u8 = 64;
}

/// Crafter
pub struct Crafter;

impl ItemDef for Crafter {
    const ID: i32 = 1058;
    const STRING_ID: &'static str = "minecraft:crafter";
    const NAME: &'static str = "Crafter";
    const STACK_SIZE: u8 = 64;
}

/// Map
pub struct FilledMap;

impl ItemDef for FilledMap {
    const ID: i32 = 1059;
    const STRING_ID: &'static str = "minecraft:filled_map";
    const NAME: &'static str = "Map";
    const STACK_SIZE: u8 = 64;
}

/// Shears
pub struct Shears;

impl ItemDef for Shears {
    const ID: i32 = 1060;
    const STRING_ID: &'static str = "minecraft:shears";
    const NAME: &'static str = "Shears";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for Shears {
    const MAX_DURABILITY: u16 = 238;
}

impl EnchantableItem for Shears {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Melon Slice
pub struct MelonSlice;

impl ItemDef for MelonSlice {
    const ID: i32 = 1061;
    const STRING_ID: &'static str = "minecraft:melon_slice";
    const NAME: &'static str = "Melon Slice";
    const STACK_SIZE: u8 = 64;
}

/// Dried Kelp
pub struct DriedKelp;

impl ItemDef for DriedKelp {
    const ID: i32 = 1062;
    const STRING_ID: &'static str = "minecraft:dried_kelp";
    const NAME: &'static str = "Dried Kelp";
    const STACK_SIZE: u8 = 64;
}

/// Pumpkin Seeds
pub struct PumpkinSeeds;

impl ItemDef for PumpkinSeeds {
    const ID: i32 = 1063;
    const STRING_ID: &'static str = "minecraft:pumpkin_seeds";
    const NAME: &'static str = "Pumpkin Seeds";
    const STACK_SIZE: u8 = 64;
}

/// Melon Seeds
pub struct MelonSeeds;

impl ItemDef for MelonSeeds {
    const ID: i32 = 1064;
    const STRING_ID: &'static str = "minecraft:melon_seeds";
    const NAME: &'static str = "Melon Seeds";
    const STACK_SIZE: u8 = 64;
}

/// Raw Beef
pub struct Beef;

impl ItemDef for Beef {
    const ID: i32 = 1065;
    const STRING_ID: &'static str = "minecraft:beef";
    const NAME: &'static str = "Raw Beef";
    const STACK_SIZE: u8 = 64;
}

/// Steak
pub struct CookedBeef;

impl ItemDef for CookedBeef {
    const ID: i32 = 1066;
    const STRING_ID: &'static str = "minecraft:cooked_beef";
    const NAME: &'static str = "Steak";
    const STACK_SIZE: u8 = 64;
}

/// Raw Chicken
pub struct Chicken;

impl ItemDef for Chicken {
    const ID: i32 = 1067;
    const STRING_ID: &'static str = "minecraft:chicken";
    const NAME: &'static str = "Raw Chicken";
    const STACK_SIZE: u8 = 64;
}

/// Cooked Chicken
pub struct CookedChicken;

impl ItemDef for CookedChicken {
    const ID: i32 = 1068;
    const STRING_ID: &'static str = "minecraft:cooked_chicken";
    const NAME: &'static str = "Cooked Chicken";
    const STACK_SIZE: u8 = 64;
}

/// Rotten Flesh
pub struct RottenFlesh;

impl ItemDef for RottenFlesh {
    const ID: i32 = 1069;
    const STRING_ID: &'static str = "minecraft:rotten_flesh";
    const NAME: &'static str = "Rotten Flesh";
    const STACK_SIZE: u8 = 64;
}

/// Ender Pearl
pub struct EnderPearl;

impl ItemDef for EnderPearl {
    const ID: i32 = 1070;
    const STRING_ID: &'static str = "minecraft:ender_pearl";
    const NAME: &'static str = "Ender Pearl";
    const STACK_SIZE: u8 = 16;
}

/// Blaze Rod
pub struct BlazeRod;

impl ItemDef for BlazeRod {
    const ID: i32 = 1071;
    const STRING_ID: &'static str = "minecraft:blaze_rod";
    const NAME: &'static str = "Blaze Rod";
    const STACK_SIZE: u8 = 64;
}

/// Ghast Tear
pub struct GhastTear;

impl ItemDef for GhastTear {
    const ID: i32 = 1072;
    const STRING_ID: &'static str = "minecraft:ghast_tear";
    const NAME: &'static str = "Ghast Tear";
    const STACK_SIZE: u8 = 64;
}

/// Gold Nugget
pub struct GoldNugget;

impl ItemDef for GoldNugget {
    const ID: i32 = 1073;
    const STRING_ID: &'static str = "minecraft:gold_nugget";
    const NAME: &'static str = "Gold Nugget";
    const STACK_SIZE: u8 = 64;
}

/// Nether Wart
pub struct NetherWart;

impl ItemDef for NetherWart {
    const ID: i32 = 1074;
    const STRING_ID: &'static str = "minecraft:nether_wart";
    const NAME: &'static str = "Nether Wart";
    const STACK_SIZE: u8 = 64;
}

/// Glass Bottle
pub struct GlassBottle;

impl ItemDef for GlassBottle {
    const ID: i32 = 1075;
    const STRING_ID: &'static str = "minecraft:glass_bottle";
    const NAME: &'static str = "Glass Bottle";
    const STACK_SIZE: u8 = 64;
}

/// Potion
pub struct Potion;

impl ItemDef for Potion {
    const ID: i32 = 1076;
    const STRING_ID: &'static str = "minecraft:potion";
    const NAME: &'static str = "Potion";
    const STACK_SIZE: u8 = 1;
}

/// Spider Eye
pub struct SpiderEye;

impl ItemDef for SpiderEye {
    const ID: i32 = 1077;
    const STRING_ID: &'static str = "minecraft:spider_eye";
    const NAME: &'static str = "Spider Eye";
    const STACK_SIZE: u8 = 64;
}

/// Fermented Spider Eye
pub struct FermentedSpiderEye;

impl ItemDef for FermentedSpiderEye {
    const ID: i32 = 1078;
    const STRING_ID: &'static str = "minecraft:fermented_spider_eye";
    const NAME: &'static str = "Fermented Spider Eye";
    const STACK_SIZE: u8 = 64;
}

/// Blaze Powder
pub struct BlazePowder;

impl ItemDef for BlazePowder {
    const ID: i32 = 1079;
    const STRING_ID: &'static str = "minecraft:blaze_powder";
    const NAME: &'static str = "Blaze Powder";
    const STACK_SIZE: u8 = 64;
}

/// Magma Cream
pub struct MagmaCream;

impl ItemDef for MagmaCream {
    const ID: i32 = 1080;
    const STRING_ID: &'static str = "minecraft:magma_cream";
    const NAME: &'static str = "Magma Cream";
    const STACK_SIZE: u8 = 64;
}

/// Brewing Stand
pub struct BrewingStand;

impl ItemDef for BrewingStand {
    const ID: i32 = 1081;
    const STRING_ID: &'static str = "minecraft:brewing_stand";
    const NAME: &'static str = "Brewing Stand";
    const STACK_SIZE: u8 = 64;
}

/// Cauldron
pub struct Cauldron;

impl ItemDef for Cauldron {
    const ID: i32 = 1082;
    const STRING_ID: &'static str = "minecraft:cauldron";
    const NAME: &'static str = "Cauldron";
    const STACK_SIZE: u8 = 64;
}

/// Eye of Ender
pub struct EnderEye;

impl ItemDef for EnderEye {
    const ID: i32 = 1083;
    const STRING_ID: &'static str = "minecraft:ender_eye";
    const NAME: &'static str = "Eye of Ender";
    const STACK_SIZE: u8 = 64;
}

/// Glistering Melon Slice
pub struct GlisteringMelonSlice;

impl ItemDef for GlisteringMelonSlice {
    const ID: i32 = 1084;
    const STRING_ID: &'static str = "minecraft:glistering_melon_slice";
    const NAME: &'static str = "Glistering Melon Slice";
    const STACK_SIZE: u8 = 64;
}

/// Armadillo Spawn Egg
pub struct ArmadilloSpawnEgg;

impl ItemDef for ArmadilloSpawnEgg {
    const ID: i32 = 1085;
    const STRING_ID: &'static str = "minecraft:armadillo_spawn_egg";
    const NAME: &'static str = "Armadillo Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Allay Spawn Egg
pub struct AllaySpawnEgg;

impl ItemDef for AllaySpawnEgg {
    const ID: i32 = 1086;
    const STRING_ID: &'static str = "minecraft:allay_spawn_egg";
    const NAME: &'static str = "Allay Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Axolotl Spawn Egg
pub struct AxolotlSpawnEgg;

impl ItemDef for AxolotlSpawnEgg {
    const ID: i32 = 1087;
    const STRING_ID: &'static str = "minecraft:axolotl_spawn_egg";
    const NAME: &'static str = "Axolotl Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Bat Spawn Egg
pub struct BatSpawnEgg;

impl ItemDef for BatSpawnEgg {
    const ID: i32 = 1088;
    const STRING_ID: &'static str = "minecraft:bat_spawn_egg";
    const NAME: &'static str = "Bat Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Bee Spawn Egg
pub struct BeeSpawnEgg;

impl ItemDef for BeeSpawnEgg {
    const ID: i32 = 1089;
    const STRING_ID: &'static str = "minecraft:bee_spawn_egg";
    const NAME: &'static str = "Bee Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Blaze Spawn Egg
pub struct BlazeSpawnEgg;

impl ItemDef for BlazeSpawnEgg {
    const ID: i32 = 1090;
    const STRING_ID: &'static str = "minecraft:blaze_spawn_egg";
    const NAME: &'static str = "Blaze Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Bogged Spawn Egg
pub struct BoggedSpawnEgg;

impl ItemDef for BoggedSpawnEgg {
    const ID: i32 = 1091;
    const STRING_ID: &'static str = "minecraft:bogged_spawn_egg";
    const NAME: &'static str = "Bogged Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Breeze Spawn Egg
pub struct BreezeSpawnEgg;

impl ItemDef for BreezeSpawnEgg {
    const ID: i32 = 1092;
    const STRING_ID: &'static str = "minecraft:breeze_spawn_egg";
    const NAME: &'static str = "Breeze Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Cat Spawn Egg
pub struct CatSpawnEgg;

impl ItemDef for CatSpawnEgg {
    const ID: i32 = 1093;
    const STRING_ID: &'static str = "minecraft:cat_spawn_egg";
    const NAME: &'static str = "Cat Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Camel Spawn Egg
pub struct CamelSpawnEgg;

impl ItemDef for CamelSpawnEgg {
    const ID: i32 = 1094;
    const STRING_ID: &'static str = "minecraft:camel_spawn_egg";
    const NAME: &'static str = "Camel Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Cave Spider Spawn Egg
pub struct CaveSpiderSpawnEgg;

impl ItemDef for CaveSpiderSpawnEgg {
    const ID: i32 = 1095;
    const STRING_ID: &'static str = "minecraft:cave_spider_spawn_egg";
    const NAME: &'static str = "Cave Spider Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Chicken Spawn Egg
pub struct ChickenSpawnEgg;

impl ItemDef for ChickenSpawnEgg {
    const ID: i32 = 1096;
    const STRING_ID: &'static str = "minecraft:chicken_spawn_egg";
    const NAME: &'static str = "Chicken Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Cod Spawn Egg
pub struct CodSpawnEgg;

impl ItemDef for CodSpawnEgg {
    const ID: i32 = 1097;
    const STRING_ID: &'static str = "minecraft:cod_spawn_egg";
    const NAME: &'static str = "Cod Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Cow Spawn Egg
pub struct CowSpawnEgg;

impl ItemDef for CowSpawnEgg {
    const ID: i32 = 1098;
    const STRING_ID: &'static str = "minecraft:cow_spawn_egg";
    const NAME: &'static str = "Cow Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Creeper Spawn Egg
pub struct CreeperSpawnEgg;

impl ItemDef for CreeperSpawnEgg {
    const ID: i32 = 1099;
    const STRING_ID: &'static str = "minecraft:creeper_spawn_egg";
    const NAME: &'static str = "Creeper Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Dolphin Spawn Egg
pub struct DolphinSpawnEgg;

impl ItemDef for DolphinSpawnEgg {
    const ID: i32 = 1100;
    const STRING_ID: &'static str = "minecraft:dolphin_spawn_egg";
    const NAME: &'static str = "Dolphin Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Donkey Spawn Egg
pub struct DonkeySpawnEgg;

impl ItemDef for DonkeySpawnEgg {
    const ID: i32 = 1101;
    const STRING_ID: &'static str = "minecraft:donkey_spawn_egg";
    const NAME: &'static str = "Donkey Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Drowned Spawn Egg
pub struct DrownedSpawnEgg;

impl ItemDef for DrownedSpawnEgg {
    const ID: i32 = 1102;
    const STRING_ID: &'static str = "minecraft:drowned_spawn_egg";
    const NAME: &'static str = "Drowned Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Elder Guardian Spawn Egg
pub struct ElderGuardianSpawnEgg;

impl ItemDef for ElderGuardianSpawnEgg {
    const ID: i32 = 1103;
    const STRING_ID: &'static str = "minecraft:elder_guardian_spawn_egg";
    const NAME: &'static str = "Elder Guardian Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Ender Dragon Spawn Egg
pub struct EnderDragonSpawnEgg;

impl ItemDef for EnderDragonSpawnEgg {
    const ID: i32 = 1104;
    const STRING_ID: &'static str = "minecraft:ender_dragon_spawn_egg";
    const NAME: &'static str = "Ender Dragon Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Enderman Spawn Egg
pub struct EndermanSpawnEgg;

impl ItemDef for EndermanSpawnEgg {
    const ID: i32 = 1105;
    const STRING_ID: &'static str = "minecraft:enderman_spawn_egg";
    const NAME: &'static str = "Enderman Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Endermite Spawn Egg
pub struct EndermiteSpawnEgg;

impl ItemDef for EndermiteSpawnEgg {
    const ID: i32 = 1106;
    const STRING_ID: &'static str = "minecraft:endermite_spawn_egg";
    const NAME: &'static str = "Endermite Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Evoker Spawn Egg
pub struct EvokerSpawnEgg;

impl ItemDef for EvokerSpawnEgg {
    const ID: i32 = 1107;
    const STRING_ID: &'static str = "minecraft:evoker_spawn_egg";
    const NAME: &'static str = "Evoker Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Fox Spawn Egg
pub struct FoxSpawnEgg;

impl ItemDef for FoxSpawnEgg {
    const ID: i32 = 1108;
    const STRING_ID: &'static str = "minecraft:fox_spawn_egg";
    const NAME: &'static str = "Fox Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Frog Spawn Egg
pub struct FrogSpawnEgg;

impl ItemDef for FrogSpawnEgg {
    const ID: i32 = 1109;
    const STRING_ID: &'static str = "minecraft:frog_spawn_egg";
    const NAME: &'static str = "Frog Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Ghast Spawn Egg
pub struct GhastSpawnEgg;

impl ItemDef for GhastSpawnEgg {
    const ID: i32 = 1110;
    const STRING_ID: &'static str = "minecraft:ghast_spawn_egg";
    const NAME: &'static str = "Ghast Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Happy Ghast Spawn Egg
pub struct HappyGhastSpawnEgg;

impl ItemDef for HappyGhastSpawnEgg {
    const ID: i32 = 1111;
    const STRING_ID: &'static str = "minecraft:happy_ghast_spawn_egg";
    const NAME: &'static str = "Happy Ghast Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Glow Squid Spawn Egg
pub struct GlowSquidSpawnEgg;

impl ItemDef for GlowSquidSpawnEgg {
    const ID: i32 = 1112;
    const STRING_ID: &'static str = "minecraft:glow_squid_spawn_egg";
    const NAME: &'static str = "Glow Squid Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Goat Spawn Egg
pub struct GoatSpawnEgg;

impl ItemDef for GoatSpawnEgg {
    const ID: i32 = 1113;
    const STRING_ID: &'static str = "minecraft:goat_spawn_egg";
    const NAME: &'static str = "Goat Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Guardian Spawn Egg
pub struct GuardianSpawnEgg;

impl ItemDef for GuardianSpawnEgg {
    const ID: i32 = 1114;
    const STRING_ID: &'static str = "minecraft:guardian_spawn_egg";
    const NAME: &'static str = "Guardian Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Hoglin Spawn Egg
pub struct HoglinSpawnEgg;

impl ItemDef for HoglinSpawnEgg {
    const ID: i32 = 1115;
    const STRING_ID: &'static str = "minecraft:hoglin_spawn_egg";
    const NAME: &'static str = "Hoglin Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Horse Spawn Egg
pub struct HorseSpawnEgg;

impl ItemDef for HorseSpawnEgg {
    const ID: i32 = 1116;
    const STRING_ID: &'static str = "minecraft:horse_spawn_egg";
    const NAME: &'static str = "Horse Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Husk Spawn Egg
pub struct HuskSpawnEgg;

impl ItemDef for HuskSpawnEgg {
    const ID: i32 = 1117;
    const STRING_ID: &'static str = "minecraft:husk_spawn_egg";
    const NAME: &'static str = "Husk Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Iron Golem Spawn Egg
pub struct IronGolemSpawnEgg;

impl ItemDef for IronGolemSpawnEgg {
    const ID: i32 = 1118;
    const STRING_ID: &'static str = "minecraft:iron_golem_spawn_egg";
    const NAME: &'static str = "Iron Golem Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Llama Spawn Egg
pub struct LlamaSpawnEgg;

impl ItemDef for LlamaSpawnEgg {
    const ID: i32 = 1119;
    const STRING_ID: &'static str = "minecraft:llama_spawn_egg";
    const NAME: &'static str = "Llama Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Magma Cube Spawn Egg
pub struct MagmaCubeSpawnEgg;

impl ItemDef for MagmaCubeSpawnEgg {
    const ID: i32 = 1120;
    const STRING_ID: &'static str = "minecraft:magma_cube_spawn_egg";
    const NAME: &'static str = "Magma Cube Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Mooshroom Spawn Egg
pub struct MooshroomSpawnEgg;

impl ItemDef for MooshroomSpawnEgg {
    const ID: i32 = 1121;
    const STRING_ID: &'static str = "minecraft:mooshroom_spawn_egg";
    const NAME: &'static str = "Mooshroom Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Mule Spawn Egg
pub struct MuleSpawnEgg;

impl ItemDef for MuleSpawnEgg {
    const ID: i32 = 1122;
    const STRING_ID: &'static str = "minecraft:mule_spawn_egg";
    const NAME: &'static str = "Mule Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Ocelot Spawn Egg
pub struct OcelotSpawnEgg;

impl ItemDef for OcelotSpawnEgg {
    const ID: i32 = 1123;
    const STRING_ID: &'static str = "minecraft:ocelot_spawn_egg";
    const NAME: &'static str = "Ocelot Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Panda Spawn Egg
pub struct PandaSpawnEgg;

impl ItemDef for PandaSpawnEgg {
    const ID: i32 = 1124;
    const STRING_ID: &'static str = "minecraft:panda_spawn_egg";
    const NAME: &'static str = "Panda Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Parrot Spawn Egg
pub struct ParrotSpawnEgg;

impl ItemDef for ParrotSpawnEgg {
    const ID: i32 = 1125;
    const STRING_ID: &'static str = "minecraft:parrot_spawn_egg";
    const NAME: &'static str = "Parrot Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Phantom Spawn Egg
pub struct PhantomSpawnEgg;

impl ItemDef for PhantomSpawnEgg {
    const ID: i32 = 1126;
    const STRING_ID: &'static str = "minecraft:phantom_spawn_egg";
    const NAME: &'static str = "Phantom Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Pig Spawn Egg
pub struct PigSpawnEgg;

impl ItemDef for PigSpawnEgg {
    const ID: i32 = 1127;
    const STRING_ID: &'static str = "minecraft:pig_spawn_egg";
    const NAME: &'static str = "Pig Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Piglin Spawn Egg
pub struct PiglinSpawnEgg;

impl ItemDef for PiglinSpawnEgg {
    const ID: i32 = 1128;
    const STRING_ID: &'static str = "minecraft:piglin_spawn_egg";
    const NAME: &'static str = "Piglin Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Piglin Brute Spawn Egg
pub struct PiglinBruteSpawnEgg;

impl ItemDef for PiglinBruteSpawnEgg {
    const ID: i32 = 1129;
    const STRING_ID: &'static str = "minecraft:piglin_brute_spawn_egg";
    const NAME: &'static str = "Piglin Brute Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Pillager Spawn Egg
pub struct PillagerSpawnEgg;

impl ItemDef for PillagerSpawnEgg {
    const ID: i32 = 1130;
    const STRING_ID: &'static str = "minecraft:pillager_spawn_egg";
    const NAME: &'static str = "Pillager Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Polar Bear Spawn Egg
pub struct PolarBearSpawnEgg;

impl ItemDef for PolarBearSpawnEgg {
    const ID: i32 = 1131;
    const STRING_ID: &'static str = "minecraft:polar_bear_spawn_egg";
    const NAME: &'static str = "Polar Bear Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Pufferfish Spawn Egg
pub struct PufferfishSpawnEgg;

impl ItemDef for PufferfishSpawnEgg {
    const ID: i32 = 1132;
    const STRING_ID: &'static str = "minecraft:pufferfish_spawn_egg";
    const NAME: &'static str = "Pufferfish Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Rabbit Spawn Egg
pub struct RabbitSpawnEgg;

impl ItemDef for RabbitSpawnEgg {
    const ID: i32 = 1133;
    const STRING_ID: &'static str = "minecraft:rabbit_spawn_egg";
    const NAME: &'static str = "Rabbit Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Ravager Spawn Egg
pub struct RavagerSpawnEgg;

impl ItemDef for RavagerSpawnEgg {
    const ID: i32 = 1134;
    const STRING_ID: &'static str = "minecraft:ravager_spawn_egg";
    const NAME: &'static str = "Ravager Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Salmon Spawn Egg
pub struct SalmonSpawnEgg;

impl ItemDef for SalmonSpawnEgg {
    const ID: i32 = 1135;
    const STRING_ID: &'static str = "minecraft:salmon_spawn_egg";
    const NAME: &'static str = "Salmon Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Sheep Spawn Egg
pub struct SheepSpawnEgg;

impl ItemDef for SheepSpawnEgg {
    const ID: i32 = 1136;
    const STRING_ID: &'static str = "minecraft:sheep_spawn_egg";
    const NAME: &'static str = "Sheep Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Shulker Spawn Egg
pub struct ShulkerSpawnEgg;

impl ItemDef for ShulkerSpawnEgg {
    const ID: i32 = 1137;
    const STRING_ID: &'static str = "minecraft:shulker_spawn_egg";
    const NAME: &'static str = "Shulker Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Silverfish Spawn Egg
pub struct SilverfishSpawnEgg;

impl ItemDef for SilverfishSpawnEgg {
    const ID: i32 = 1138;
    const STRING_ID: &'static str = "minecraft:silverfish_spawn_egg";
    const NAME: &'static str = "Silverfish Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Skeleton Spawn Egg
pub struct SkeletonSpawnEgg;

impl ItemDef for SkeletonSpawnEgg {
    const ID: i32 = 1139;
    const STRING_ID: &'static str = "minecraft:skeleton_spawn_egg";
    const NAME: &'static str = "Skeleton Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Skeleton Horse Spawn Egg
pub struct SkeletonHorseSpawnEgg;

impl ItemDef for SkeletonHorseSpawnEgg {
    const ID: i32 = 1140;
    const STRING_ID: &'static str = "minecraft:skeleton_horse_spawn_egg";
    const NAME: &'static str = "Skeleton Horse Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Slime Spawn Egg
pub struct SlimeSpawnEgg;

impl ItemDef for SlimeSpawnEgg {
    const ID: i32 = 1141;
    const STRING_ID: &'static str = "minecraft:slime_spawn_egg";
    const NAME: &'static str = "Slime Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Sniffer Spawn Egg
pub struct SnifferSpawnEgg;

impl ItemDef for SnifferSpawnEgg {
    const ID: i32 = 1142;
    const STRING_ID: &'static str = "minecraft:sniffer_spawn_egg";
    const NAME: &'static str = "Sniffer Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Snow Golem Spawn Egg
pub struct SnowGolemSpawnEgg;

impl ItemDef for SnowGolemSpawnEgg {
    const ID: i32 = 1143;
    const STRING_ID: &'static str = "minecraft:snow_golem_spawn_egg";
    const NAME: &'static str = "Snow Golem Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Spider Spawn Egg
pub struct SpiderSpawnEgg;

impl ItemDef for SpiderSpawnEgg {
    const ID: i32 = 1144;
    const STRING_ID: &'static str = "minecraft:spider_spawn_egg";
    const NAME: &'static str = "Spider Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Squid Spawn Egg
pub struct SquidSpawnEgg;

impl ItemDef for SquidSpawnEgg {
    const ID: i32 = 1145;
    const STRING_ID: &'static str = "minecraft:squid_spawn_egg";
    const NAME: &'static str = "Squid Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Stray Spawn Egg
pub struct StraySpawnEgg;

impl ItemDef for StraySpawnEgg {
    const ID: i32 = 1146;
    const STRING_ID: &'static str = "minecraft:stray_spawn_egg";
    const NAME: &'static str = "Stray Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Strider Spawn Egg
pub struct StriderSpawnEgg;

impl ItemDef for StriderSpawnEgg {
    const ID: i32 = 1147;
    const STRING_ID: &'static str = "minecraft:strider_spawn_egg";
    const NAME: &'static str = "Strider Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Tadpole Spawn Egg
pub struct TadpoleSpawnEgg;

impl ItemDef for TadpoleSpawnEgg {
    const ID: i32 = 1148;
    const STRING_ID: &'static str = "minecraft:tadpole_spawn_egg";
    const NAME: &'static str = "Tadpole Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Trader Llama Spawn Egg
pub struct TraderLlamaSpawnEgg;

impl ItemDef for TraderLlamaSpawnEgg {
    const ID: i32 = 1149;
    const STRING_ID: &'static str = "minecraft:trader_llama_spawn_egg";
    const NAME: &'static str = "Trader Llama Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Tropical Fish Spawn Egg
pub struct TropicalFishSpawnEgg;

impl ItemDef for TropicalFishSpawnEgg {
    const ID: i32 = 1150;
    const STRING_ID: &'static str = "minecraft:tropical_fish_spawn_egg";
    const NAME: &'static str = "Tropical Fish Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Turtle Spawn Egg
pub struct TurtleSpawnEgg;

impl ItemDef for TurtleSpawnEgg {
    const ID: i32 = 1151;
    const STRING_ID: &'static str = "minecraft:turtle_spawn_egg";
    const NAME: &'static str = "Turtle Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Vex Spawn Egg
pub struct VexSpawnEgg;

impl ItemDef for VexSpawnEgg {
    const ID: i32 = 1152;
    const STRING_ID: &'static str = "minecraft:vex_spawn_egg";
    const NAME: &'static str = "Vex Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Villager Spawn Egg
pub struct VillagerSpawnEgg;

impl ItemDef for VillagerSpawnEgg {
    const ID: i32 = 1153;
    const STRING_ID: &'static str = "minecraft:villager_spawn_egg";
    const NAME: &'static str = "Villager Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Vindicator Spawn Egg
pub struct VindicatorSpawnEgg;

impl ItemDef for VindicatorSpawnEgg {
    const ID: i32 = 1154;
    const STRING_ID: &'static str = "minecraft:vindicator_spawn_egg";
    const NAME: &'static str = "Vindicator Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Wandering Trader Spawn Egg
pub struct WanderingTraderSpawnEgg;

impl ItemDef for WanderingTraderSpawnEgg {
    const ID: i32 = 1155;
    const STRING_ID: &'static str = "minecraft:wandering_trader_spawn_egg";
    const NAME: &'static str = "Wandering Trader Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Warden Spawn Egg
pub struct WardenSpawnEgg;

impl ItemDef for WardenSpawnEgg {
    const ID: i32 = 1156;
    const STRING_ID: &'static str = "minecraft:warden_spawn_egg";
    const NAME: &'static str = "Warden Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Witch Spawn Egg
pub struct WitchSpawnEgg;

impl ItemDef for WitchSpawnEgg {
    const ID: i32 = 1157;
    const STRING_ID: &'static str = "minecraft:witch_spawn_egg";
    const NAME: &'static str = "Witch Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Wither Spawn Egg
pub struct WitherSpawnEgg;

impl ItemDef for WitherSpawnEgg {
    const ID: i32 = 1158;
    const STRING_ID: &'static str = "minecraft:wither_spawn_egg";
    const NAME: &'static str = "Wither Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Wither Skeleton Spawn Egg
pub struct WitherSkeletonSpawnEgg;

impl ItemDef for WitherSkeletonSpawnEgg {
    const ID: i32 = 1159;
    const STRING_ID: &'static str = "minecraft:wither_skeleton_spawn_egg";
    const NAME: &'static str = "Wither Skeleton Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Wolf Spawn Egg
pub struct WolfSpawnEgg;

impl ItemDef for WolfSpawnEgg {
    const ID: i32 = 1160;
    const STRING_ID: &'static str = "minecraft:wolf_spawn_egg";
    const NAME: &'static str = "Wolf Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Zoglin Spawn Egg
pub struct ZoglinSpawnEgg;

impl ItemDef for ZoglinSpawnEgg {
    const ID: i32 = 1161;
    const STRING_ID: &'static str = "minecraft:zoglin_spawn_egg";
    const NAME: &'static str = "Zoglin Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Creaking Spawn Egg
pub struct CreakingSpawnEgg;

impl ItemDef for CreakingSpawnEgg {
    const ID: i32 = 1162;
    const STRING_ID: &'static str = "minecraft:creaking_spawn_egg";
    const NAME: &'static str = "Creaking Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Zombie Spawn Egg
pub struct ZombieSpawnEgg;

impl ItemDef for ZombieSpawnEgg {
    const ID: i32 = 1163;
    const STRING_ID: &'static str = "minecraft:zombie_spawn_egg";
    const NAME: &'static str = "Zombie Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Zombie Horse Spawn Egg
pub struct ZombieHorseSpawnEgg;

impl ItemDef for ZombieHorseSpawnEgg {
    const ID: i32 = 1164;
    const STRING_ID: &'static str = "minecraft:zombie_horse_spawn_egg";
    const NAME: &'static str = "Zombie Horse Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Zombie Villager Spawn Egg
pub struct ZombieVillagerSpawnEgg;

impl ItemDef for ZombieVillagerSpawnEgg {
    const ID: i32 = 1165;
    const STRING_ID: &'static str = "minecraft:zombie_villager_spawn_egg";
    const NAME: &'static str = "Zombie Villager Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Zombified Piglin Spawn Egg
pub struct ZombiePigmanSpawnEgg;

impl ItemDef for ZombiePigmanSpawnEgg {
    const ID: i32 = 1166;
    const STRING_ID: &'static str = "minecraft:zombie_pigman_spawn_egg";
    const NAME: &'static str = "Zombified Piglin Spawn Egg";
    const STACK_SIZE: u8 = 64;
}

/// Bottle o' Enchanting
pub struct ExperienceBottle;

impl ItemDef for ExperienceBottle {
    const ID: i32 = 1167;
    const STRING_ID: &'static str = "minecraft:experience_bottle";
    const NAME: &'static str = "Bottle o' Enchanting";
    const STACK_SIZE: u8 = 64;
}

/// Fire Charge
pub struct FireCharge;

impl ItemDef for FireCharge {
    const ID: i32 = 1168;
    const STRING_ID: &'static str = "minecraft:fire_charge";
    const NAME: &'static str = "Fire Charge";
    const STACK_SIZE: u8 = 64;
}

/// Wind Charge
pub struct WindCharge;

impl ItemDef for WindCharge {
    const ID: i32 = 1169;
    const STRING_ID: &'static str = "minecraft:wind_charge";
    const NAME: &'static str = "Wind Charge";
    const STACK_SIZE: u8 = 64;
}

/// Book and Quill
pub struct WritableBook;

impl ItemDef for WritableBook {
    const ID: i32 = 1170;
    const STRING_ID: &'static str = "minecraft:writable_book";
    const NAME: &'static str = "Book and Quill";
    const STACK_SIZE: u8 = 1;
}

/// Written Book
pub struct WrittenBook;

impl ItemDef for WrittenBook {
    const ID: i32 = 1171;
    const STRING_ID: &'static str = "minecraft:written_book";
    const NAME: &'static str = "Written Book";
    const STACK_SIZE: u8 = 16;
}

/// Breeze Rod
pub struct BreezeRod;

impl ItemDef for BreezeRod {
    const ID: i32 = 1172;
    const STRING_ID: &'static str = "minecraft:breeze_rod";
    const NAME: &'static str = "Breeze Rod";
    const STACK_SIZE: u8 = 64;
}

/// Mace
pub struct Mace;

impl ItemDef for Mace {
    const ID: i32 = 1173;
    const STRING_ID: &'static str = "minecraft:mace";
    const NAME: &'static str = "Mace";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for Mace {
    const MAX_DURABILITY: u16 = 500;
}

impl RepairableItem for Mace {
    fn repair_items() -> &'static [i32] {
        &[1172]
    }
}

impl EnchantableItem for Mace {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Weapon,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Item Frame
pub struct Frame;

impl ItemDef for Frame {
    const ID: i32 = 1174;
    const STRING_ID: &'static str = "minecraft:frame";
    const NAME: &'static str = "Item Frame";
    const STACK_SIZE: u8 = 64;
}

/// Glow Item Frame
pub struct GlowFrame;

impl ItemDef for GlowFrame {
    const ID: i32 = 1175;
    const STRING_ID: &'static str = "minecraft:glow_frame";
    const NAME: &'static str = "Glow Item Frame";
    const STACK_SIZE: u8 = 64;
}

/// Flower Pot
pub struct FlowerPot;

impl ItemDef for FlowerPot {
    const ID: i32 = 1176;
    const STRING_ID: &'static str = "minecraft:flower_pot";
    const NAME: &'static str = "Flower Pot";
    const STACK_SIZE: u8 = 64;
}

/// Carrot
pub struct Carrot;

impl ItemDef for Carrot {
    const ID: i32 = 1177;
    const STRING_ID: &'static str = "minecraft:carrot";
    const NAME: &'static str = "Carrot";
    const STACK_SIZE: u8 = 64;
}

/// Potato
pub struct Potato;

impl ItemDef for Potato {
    const ID: i32 = 1178;
    const STRING_ID: &'static str = "minecraft:potato";
    const NAME: &'static str = "Potato";
    const STACK_SIZE: u8 = 64;
}

/// Baked Potato
pub struct BakedPotato;

impl ItemDef for BakedPotato {
    const ID: i32 = 1179;
    const STRING_ID: &'static str = "minecraft:baked_potato";
    const NAME: &'static str = "Baked Potato";
    const STACK_SIZE: u8 = 64;
}

/// Poisonous Potato
pub struct PoisonousPotato;

impl ItemDef for PoisonousPotato {
    const ID: i32 = 1180;
    const STRING_ID: &'static str = "minecraft:poisonous_potato";
    const NAME: &'static str = "Poisonous Potato";
    const STACK_SIZE: u8 = 64;
}

/// Empty Map
pub struct EmptyMap;

impl ItemDef for EmptyMap {
    const ID: i32 = 1181;
    const STRING_ID: &'static str = "minecraft:empty_map";
    const NAME: &'static str = "Empty Map";
    const STACK_SIZE: u8 = 64;
}

/// Golden Carrot
pub struct GoldenCarrot;

impl ItemDef for GoldenCarrot {
    const ID: i32 = 1182;
    const STRING_ID: &'static str = "minecraft:golden_carrot";
    const NAME: &'static str = "Golden Carrot";
    const STACK_SIZE: u8 = 64;
}

/// Skeleton Skull
pub struct SkeletonSkull;

impl ItemDef for SkeletonSkull {
    const ID: i32 = 1183;
    const STRING_ID: &'static str = "minecraft:skeleton_skull";
    const NAME: &'static str = "Skeleton Skull";
    const STACK_SIZE: u8 = 64;
}

impl EnchantableItem for SkeletonSkull {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Wither Skeleton Skull
pub struct WitherSkeletonSkull;

impl ItemDef for WitherSkeletonSkull {
    const ID: i32 = 1184;
    const STRING_ID: &'static str = "minecraft:wither_skeleton_skull";
    const NAME: &'static str = "Wither Skeleton Skull";
    const STACK_SIZE: u8 = 64;
}

impl EnchantableItem for WitherSkeletonSkull {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Player Head
pub struct PlayerHead;

impl ItemDef for PlayerHead {
    const ID: i32 = 1185;
    const STRING_ID: &'static str = "minecraft:player_head";
    const NAME: &'static str = "Player Head";
    const STACK_SIZE: u8 = 64;
}

impl EnchantableItem for PlayerHead {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Zombie Head
pub struct ZombieHead;

impl ItemDef for ZombieHead {
    const ID: i32 = 1186;
    const STRING_ID: &'static str = "minecraft:zombie_head";
    const NAME: &'static str = "Zombie Head";
    const STACK_SIZE: u8 = 64;
}

impl EnchantableItem for ZombieHead {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Creeper Head
pub struct CreeperHead;

impl ItemDef for CreeperHead {
    const ID: i32 = 1187;
    const STRING_ID: &'static str = "minecraft:creeper_head";
    const NAME: &'static str = "Creeper Head";
    const STACK_SIZE: u8 = 64;
}

impl EnchantableItem for CreeperHead {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Dragon Head
pub struct DragonHead;

impl ItemDef for DragonHead {
    const ID: i32 = 1188;
    const STRING_ID: &'static str = "minecraft:dragon_head";
    const NAME: &'static str = "Dragon Head";
    const STACK_SIZE: u8 = 64;
}

impl EnchantableItem for DragonHead {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Piglin Head
pub struct PiglinHead;

impl ItemDef for PiglinHead {
    const ID: i32 = 1189;
    const STRING_ID: &'static str = "minecraft:piglin_head";
    const NAME: &'static str = "Piglin Head";
    const STACK_SIZE: u8 = 64;
}

impl EnchantableItem for PiglinHead {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Equippable,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Nether Star
pub struct NetherStar;

impl ItemDef for NetherStar {
    const ID: i32 = 1190;
    const STRING_ID: &'static str = "minecraft:nether_star";
    const NAME: &'static str = "Nether Star";
    const STACK_SIZE: u8 = 64;
}

/// Pumpkin Pie
pub struct PumpkinPie;

impl ItemDef for PumpkinPie {
    const ID: i32 = 1191;
    const STRING_ID: &'static str = "minecraft:pumpkin_pie";
    const NAME: &'static str = "Pumpkin Pie";
    const STACK_SIZE: u8 = 64;
}

/// Firework Rocket
pub struct FireworkRocket;

impl ItemDef for FireworkRocket {
    const ID: i32 = 1192;
    const STRING_ID: &'static str = "minecraft:firework_rocket";
    const NAME: &'static str = "Firework Rocket";
    const STACK_SIZE: u8 = 64;
}

/// Firework Star
pub struct FireworkStar;

impl ItemDef for FireworkStar {
    const ID: i32 = 1193;
    const STRING_ID: &'static str = "minecraft:firework_star";
    const NAME: &'static str = "Firework Star";
    const STACK_SIZE: u8 = 64;
}

/// Enchanted Book
pub struct EnchantedBook;

impl ItemDef for EnchantedBook {
    const ID: i32 = 1194;
    const STRING_ID: &'static str = "minecraft:enchanted_book";
    const NAME: &'static str = "Enchanted Book";
    const STACK_SIZE: u8 = 1;
}

/// Nether Brick
pub struct Netherbrick;

impl ItemDef for Netherbrick {
    const ID: i32 = 1195;
    const STRING_ID: &'static str = "minecraft:netherbrick";
    const NAME: &'static str = "Nether Brick";
    const STACK_SIZE: u8 = 64;
}

/// Resin Brick
pub struct ResinBrick;

impl ItemDef for ResinBrick {
    const ID: i32 = 1196;
    const STRING_ID: &'static str = "minecraft:resin_brick";
    const NAME: &'static str = "Resin Brick";
    const STACK_SIZE: u8 = 64;
}

/// Prismarine Shard
pub struct PrismarineShard;

impl ItemDef for PrismarineShard {
    const ID: i32 = 1197;
    const STRING_ID: &'static str = "minecraft:prismarine_shard";
    const NAME: &'static str = "Prismarine Shard";
    const STACK_SIZE: u8 = 64;
}

/// Prismarine Crystals
pub struct PrismarineCrystals;

impl ItemDef for PrismarineCrystals {
    const ID: i32 = 1198;
    const STRING_ID: &'static str = "minecraft:prismarine_crystals";
    const NAME: &'static str = "Prismarine Crystals";
    const STACK_SIZE: u8 = 64;
}

/// Raw Rabbit
pub struct Rabbit;

impl ItemDef for Rabbit {
    const ID: i32 = 1199;
    const STRING_ID: &'static str = "minecraft:rabbit";
    const NAME: &'static str = "Raw Rabbit";
    const STACK_SIZE: u8 = 64;
}

/// Cooked Rabbit
pub struct CookedRabbit;

impl ItemDef for CookedRabbit {
    const ID: i32 = 1200;
    const STRING_ID: &'static str = "minecraft:cooked_rabbit";
    const NAME: &'static str = "Cooked Rabbit";
    const STACK_SIZE: u8 = 64;
}

/// Rabbit Stew
pub struct RabbitStew;

impl ItemDef for RabbitStew {
    const ID: i32 = 1201;
    const STRING_ID: &'static str = "minecraft:rabbit_stew";
    const NAME: &'static str = "Rabbit Stew";
    const STACK_SIZE: u8 = 1;
}

/// Rabbit's Foot
pub struct RabbitFoot;

impl ItemDef for RabbitFoot {
    const ID: i32 = 1202;
    const STRING_ID: &'static str = "minecraft:rabbit_foot";
    const NAME: &'static str = "Rabbit's Foot";
    const STACK_SIZE: u8 = 64;
}

/// Rabbit Hide
pub struct RabbitHide;

impl ItemDef for RabbitHide {
    const ID: i32 = 1203;
    const STRING_ID: &'static str = "minecraft:rabbit_hide";
    const NAME: &'static str = "Rabbit Hide";
    const STACK_SIZE: u8 = 64;
}

/// Armor Stand
pub struct ArmorStand;

impl ItemDef for ArmorStand {
    const ID: i32 = 1204;
    const STRING_ID: &'static str = "minecraft:armor_stand";
    const NAME: &'static str = "Armor Stand";
    const STACK_SIZE: u8 = 16;
}

/// Iron Horse Armor
pub struct IronHorseArmor;

impl ItemDef for IronHorseArmor {
    const ID: i32 = 1205;
    const STRING_ID: &'static str = "minecraft:iron_horse_armor";
    const NAME: &'static str = "Iron Horse Armor";
    const STACK_SIZE: u8 = 1;
}

/// Golden Horse Armor
pub struct GoldenHorseArmor;

impl ItemDef for GoldenHorseArmor {
    const ID: i32 = 1206;
    const STRING_ID: &'static str = "minecraft:golden_horse_armor";
    const NAME: &'static str = "Golden Horse Armor";
    const STACK_SIZE: u8 = 1;
}

/// Diamond Horse Armor
pub struct DiamondHorseArmor;

impl ItemDef for DiamondHorseArmor {
    const ID: i32 = 1207;
    const STRING_ID: &'static str = "minecraft:diamond_horse_armor";
    const NAME: &'static str = "Diamond Horse Armor";
    const STACK_SIZE: u8 = 1;
}

/// Leather Horse Armor
pub struct LeatherHorseArmor;

impl ItemDef for LeatherHorseArmor {
    const ID: i32 = 1208;
    const STRING_ID: &'static str = "minecraft:leather_horse_armor";
    const NAME: &'static str = "Leather Horse Armor";
    const STACK_SIZE: u8 = 1;
}

/// Lead
pub struct Lead;

impl ItemDef for Lead {
    const ID: i32 = 1209;
    const STRING_ID: &'static str = "minecraft:lead";
    const NAME: &'static str = "Lead";
    const STACK_SIZE: u8 = 64;
}

/// Name Tag
pub struct NameTag;

impl ItemDef for NameTag {
    const ID: i32 = 1210;
    const STRING_ID: &'static str = "minecraft:name_tag";
    const NAME: &'static str = "Name Tag";
    const STACK_SIZE: u8 = 64;
}

/// Minecart with Command Block
pub struct CommandBlockMinecart;

impl ItemDef for CommandBlockMinecart {
    const ID: i32 = 1211;
    const STRING_ID: &'static str = "minecraft:command_block_minecart";
    const NAME: &'static str = "Minecart with Command Block";
    const STACK_SIZE: u8 = 1;
}

/// Raw Mutton
pub struct Mutton;

impl ItemDef for Mutton {
    const ID: i32 = 1212;
    const STRING_ID: &'static str = "minecraft:mutton";
    const NAME: &'static str = "Raw Mutton";
    const STACK_SIZE: u8 = 64;
}

/// Cooked Mutton
pub struct CookedMutton;

impl ItemDef for CookedMutton {
    const ID: i32 = 1213;
    const STRING_ID: &'static str = "minecraft:cooked_mutton";
    const NAME: &'static str = "Cooked Mutton";
    const STACK_SIZE: u8 = 64;
}

/// Black Banner
pub struct Banner;

impl ItemDef for Banner {
    const ID: i32 = 1229;
    const STRING_ID: &'static str = "minecraft:banner";
    const NAME: &'static str = "Black Banner";
    const STACK_SIZE: u8 = 16;
}

impl VariantItem for Banner {
    fn variants() -> &'static [ItemVariant] {
        &[
            ItemVariant {
                id: 1228,
                metadata: 1,
                name: "red_banner",
                display_name: "Red Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1227,
                metadata: 2,
                name: "green_banner",
                display_name: "Green Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1226,
                metadata: 3,
                name: "brown_banner",
                display_name: "Brown Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1225,
                metadata: 4,
                name: "blue_banner",
                display_name: "Blue Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1224,
                metadata: 5,
                name: "purple_banner",
                display_name: "Purple Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1223,
                metadata: 6,
                name: "cyan_banner",
                display_name: "Cyan Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1222,
                metadata: 7,
                name: "light_gray_banner",
                display_name: "Light Gray Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1221,
                metadata: 8,
                name: "gray_banner",
                display_name: "Gray Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1220,
                metadata: 9,
                name: "pink_banner",
                display_name: "Pink Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1219,
                metadata: 10,
                name: "lime_banner",
                display_name: "Lime Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1218,
                metadata: 11,
                name: "yellow_banner",
                display_name: "Yellow Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1217,
                metadata: 12,
                name: "light_blue_banner",
                display_name: "Light Blue Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1216,
                metadata: 13,
                name: "magenta_banner",
                display_name: "Magenta Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1215,
                metadata: 14,
                name: "orange_banner",
                display_name: "Orange Banner",
                stack_size: 16,
            },
            ItemVariant {
                id: 1214,
                metadata: 15,
                name: "white_banner",
                display_name: "White Banner",
                stack_size: 16,
            },
        ]
    }
}

/// End Crystal
pub struct EndCrystal;

impl ItemDef for EndCrystal {
    const ID: i32 = 1230;
    const STRING_ID: &'static str = "minecraft:end_crystal";
    const NAME: &'static str = "End Crystal";
    const STACK_SIZE: u8 = 64;
}

/// Chorus Fruit
pub struct ChorusFruit;

impl ItemDef for ChorusFruit {
    const ID: i32 = 1231;
    const STRING_ID: &'static str = "minecraft:chorus_fruit";
    const NAME: &'static str = "Chorus Fruit";
    const STACK_SIZE: u8 = 64;
}

/// Popped Chorus Fruit
pub struct PoppedChorusFruit;

impl ItemDef for PoppedChorusFruit {
    const ID: i32 = 1232;
    const STRING_ID: &'static str = "minecraft:popped_chorus_fruit";
    const NAME: &'static str = "Popped Chorus Fruit";
    const STACK_SIZE: u8 = 64;
}

/// Torchflower Seeds
pub struct TorchflowerSeeds;

impl ItemDef for TorchflowerSeeds {
    const ID: i32 = 1233;
    const STRING_ID: &'static str = "minecraft:torchflower_seeds";
    const NAME: &'static str = "Torchflower Seeds";
    const STACK_SIZE: u8 = 64;
}

/// Pitcher Pod
pub struct PitcherPod;

impl ItemDef for PitcherPod {
    const ID: i32 = 1234;
    const STRING_ID: &'static str = "minecraft:pitcher_pod";
    const NAME: &'static str = "Pitcher Pod";
    const STACK_SIZE: u8 = 64;
}

/// Beetroot
pub struct Beetroot;

impl ItemDef for Beetroot {
    const ID: i32 = 1235;
    const STRING_ID: &'static str = "minecraft:beetroot";
    const NAME: &'static str = "Beetroot";
    const STACK_SIZE: u8 = 64;
}

/// Beetroot Seeds
pub struct BeetrootSeeds;

impl ItemDef for BeetrootSeeds {
    const ID: i32 = 1236;
    const STRING_ID: &'static str = "minecraft:beetroot_seeds";
    const NAME: &'static str = "Beetroot Seeds";
    const STACK_SIZE: u8 = 64;
}

/// Beetroot Soup
pub struct BeetrootSoup;

impl ItemDef for BeetrootSoup {
    const ID: i32 = 1237;
    const STRING_ID: &'static str = "minecraft:beetroot_soup";
    const NAME: &'static str = "Beetroot Soup";
    const STACK_SIZE: u8 = 1;
}

/// Dragon's Breath
pub struct DragonBreath;

impl ItemDef for DragonBreath {
    const ID: i32 = 1238;
    const STRING_ID: &'static str = "minecraft:dragon_breath";
    const NAME: &'static str = "Dragon's Breath";
    const STACK_SIZE: u8 = 64;
}

/// Splash Potion
pub struct SplashPotion;

impl ItemDef for SplashPotion {
    const ID: i32 = 1239;
    const STRING_ID: &'static str = "minecraft:splash_potion";
    const NAME: &'static str = "Splash Potion";
    const STACK_SIZE: u8 = 1;
}

/// Lingering Potion
pub struct LingeringPotion;

impl ItemDef for LingeringPotion {
    const ID: i32 = 1242;
    const STRING_ID: &'static str = "minecraft:lingering_potion";
    const NAME: &'static str = "Lingering Potion";
    const STACK_SIZE: u8 = 1;
}

/// Shield
pub struct Shield;

impl ItemDef for Shield {
    const ID: i32 = 1243;
    const STRING_ID: &'static str = "minecraft:shield";
    const NAME: &'static str = "Shield";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for Shield {
    const MAX_DURABILITY: u16 = 336;
}

impl RepairableItem for Shield {
    fn repair_items() -> &'static [i32] {
        &[36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47]
    }
}

impl EnchantableItem for Shield {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Totem of Undying
pub struct TotemOfUndying;

impl ItemDef for TotemOfUndying {
    const ID: i32 = 1244;
    const STRING_ID: &'static str = "minecraft:totem_of_undying";
    const NAME: &'static str = "Totem of Undying";
    const STACK_SIZE: u8 = 1;
}

/// Shulker Shell
pub struct ShulkerShell;

impl ItemDef for ShulkerShell {
    const ID: i32 = 1245;
    const STRING_ID: &'static str = "minecraft:shulker_shell";
    const NAME: &'static str = "Shulker Shell";
    const STACK_SIZE: u8 = 64;
}

/// Iron Nugget
pub struct IronNugget;

impl ItemDef for IronNugget {
    const ID: i32 = 1246;
    const STRING_ID: &'static str = "minecraft:iron_nugget";
    const NAME: &'static str = "Iron Nugget";
    const STACK_SIZE: u8 = 64;
}

/// Music Disc
pub struct MusicDisc13;

impl ItemDef for MusicDisc13 {
    const ID: i32 = 1249;
    const STRING_ID: &'static str = "minecraft:music_disc_13";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscCat;

impl ItemDef for MusicDiscCat {
    const ID: i32 = 1250;
    const STRING_ID: &'static str = "minecraft:music_disc_cat";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscBlocks;

impl ItemDef for MusicDiscBlocks {
    const ID: i32 = 1251;
    const STRING_ID: &'static str = "minecraft:music_disc_blocks";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscChirp;

impl ItemDef for MusicDiscChirp {
    const ID: i32 = 1252;
    const STRING_ID: &'static str = "minecraft:music_disc_chirp";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscCreator;

impl ItemDef for MusicDiscCreator {
    const ID: i32 = 1253;
    const STRING_ID: &'static str = "minecraft:music_disc_creator";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscCreatorMusicBox;

impl ItemDef for MusicDiscCreatorMusicBox {
    const ID: i32 = 1254;
    const STRING_ID: &'static str = "minecraft:music_disc_creator_music_box";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscFar;

impl ItemDef for MusicDiscFar {
    const ID: i32 = 1255;
    const STRING_ID: &'static str = "minecraft:music_disc_far";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscLavaChicken;

impl ItemDef for MusicDiscLavaChicken {
    const ID: i32 = 1256;
    const STRING_ID: &'static str = "minecraft:music_disc_lava_chicken";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscMall;

impl ItemDef for MusicDiscMall {
    const ID: i32 = 1257;
    const STRING_ID: &'static str = "minecraft:music_disc_mall";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscMellohi;

impl ItemDef for MusicDiscMellohi {
    const ID: i32 = 1258;
    const STRING_ID: &'static str = "minecraft:music_disc_mellohi";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscStal;

impl ItemDef for MusicDiscStal {
    const ID: i32 = 1259;
    const STRING_ID: &'static str = "minecraft:music_disc_stal";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscStrad;

impl ItemDef for MusicDiscStrad {
    const ID: i32 = 1260;
    const STRING_ID: &'static str = "minecraft:music_disc_strad";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscWard;

impl ItemDef for MusicDiscWard {
    const ID: i32 = 1261;
    const STRING_ID: &'static str = "minecraft:music_disc_ward";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDisc11;

impl ItemDef for MusicDisc11 {
    const ID: i32 = 1262;
    const STRING_ID: &'static str = "minecraft:music_disc_11";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscWait;

impl ItemDef for MusicDiscWait {
    const ID: i32 = 1263;
    const STRING_ID: &'static str = "minecraft:music_disc_wait";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscOtherside;

impl ItemDef for MusicDiscOtherside {
    const ID: i32 = 1264;
    const STRING_ID: &'static str = "minecraft:music_disc_otherside";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscRelic;

impl ItemDef for MusicDiscRelic {
    const ID: i32 = 1265;
    const STRING_ID: &'static str = "minecraft:music_disc_relic";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDisc5;

impl ItemDef for MusicDisc5 {
    const ID: i32 = 1266;
    const STRING_ID: &'static str = "minecraft:music_disc_5";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscPigstep;

impl ItemDef for MusicDiscPigstep {
    const ID: i32 = 1267;
    const STRING_ID: &'static str = "minecraft:music_disc_pigstep";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscPrecipice;

impl ItemDef for MusicDiscPrecipice {
    const ID: i32 = 1268;
    const STRING_ID: &'static str = "minecraft:music_disc_precipice";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Music Disc
pub struct MusicDiscTears;

impl ItemDef for MusicDiscTears {
    const ID: i32 = 1269;
    const STRING_ID: &'static str = "minecraft:music_disc_tears";
    const NAME: &'static str = "Music Disc";
    const STACK_SIZE: u8 = 1;
}

/// Disc Fragment
pub struct DiscFragment5;

impl ItemDef for DiscFragment5 {
    const ID: i32 = 1270;
    const STRING_ID: &'static str = "minecraft:disc_fragment_5";
    const NAME: &'static str = "Disc Fragment";
    const STACK_SIZE: u8 = 64;
}

/// Trident
pub struct Trident;

impl ItemDef for Trident {
    const ID: i32 = 1271;
    const STRING_ID: &'static str = "minecraft:trident";
    const NAME: &'static str = "Trident";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for Trident {
    const MAX_DURABILITY: u16 = 250;
}

impl EnchantableItem for Trident {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Trident,
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Nautilus Shell
pub struct NautilusShell;

impl ItemDef for NautilusShell {
    const ID: i32 = 1272;
    const STRING_ID: &'static str = "minecraft:nautilus_shell";
    const NAME: &'static str = "Nautilus Shell";
    const STACK_SIZE: u8 = 64;
}

/// Heart of the Sea
pub struct HeartOfTheSea;

impl ItemDef for HeartOfTheSea {
    const ID: i32 = 1273;
    const STRING_ID: &'static str = "minecraft:heart_of_the_sea";
    const NAME: &'static str = "Heart of the Sea";
    const STACK_SIZE: u8 = 64;
}

/// Crossbow
pub struct Crossbow;

impl ItemDef for Crossbow {
    const ID: i32 = 1274;
    const STRING_ID: &'static str = "minecraft:crossbow";
    const NAME: &'static str = "Crossbow";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for Crossbow {
    const MAX_DURABILITY: u16 = 465;
}

impl EnchantableItem for Crossbow {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Crossbow,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Suspicious Stew
pub struct SuspiciousStew;

impl ItemDef for SuspiciousStew {
    const ID: i32 = 1275;
    const STRING_ID: &'static str = "minecraft:suspicious_stew";
    const NAME: &'static str = "Suspicious Stew";
    const STACK_SIZE: u8 = 1;
}

/// Loom
pub struct Loom;

impl ItemDef for Loom {
    const ID: i32 = 1276;
    const STRING_ID: &'static str = "minecraft:loom";
    const NAME: &'static str = "Loom";
    const STACK_SIZE: u8 = 64;
}

/// Flower Charge Banner Pattern
pub struct FlowerBannerPattern;

impl ItemDef for FlowerBannerPattern {
    const ID: i32 = 1277;
    const STRING_ID: &'static str = "minecraft:flower_banner_pattern";
    const NAME: &'static str = "Flower Charge Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Creeper Charge Banner Pattern
pub struct CreeperBannerPattern;

impl ItemDef for CreeperBannerPattern {
    const ID: i32 = 1278;
    const STRING_ID: &'static str = "minecraft:creeper_banner_pattern";
    const NAME: &'static str = "Creeper Charge Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Skull Charge Banner Pattern
pub struct SkullBannerPattern;

impl ItemDef for SkullBannerPattern {
    const ID: i32 = 1279;
    const STRING_ID: &'static str = "minecraft:skull_banner_pattern";
    const NAME: &'static str = "Skull Charge Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Thing Banner Pattern
pub struct MojangBannerPattern;

impl ItemDef for MojangBannerPattern {
    const ID: i32 = 1280;
    const STRING_ID: &'static str = "minecraft:mojang_banner_pattern";
    const NAME: &'static str = "Thing Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Globe Banner Pattern
pub struct GlobeBannerPattern;

impl ItemDef for GlobeBannerPattern {
    const ID: i32 = 1281;
    const STRING_ID: &'static str = "minecraft:globe_banner_pattern";
    const NAME: &'static str = "Globe Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Snout Banner Pattern
pub struct PiglinBannerPattern;

impl ItemDef for PiglinBannerPattern {
    const ID: i32 = 1282;
    const STRING_ID: &'static str = "minecraft:piglin_banner_pattern";
    const NAME: &'static str = "Snout Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Flow Banner Pattern
pub struct FlowBannerPattern;

impl ItemDef for FlowBannerPattern {
    const ID: i32 = 1283;
    const STRING_ID: &'static str = "minecraft:flow_banner_pattern";
    const NAME: &'static str = "Flow Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Guster Banner Pattern
pub struct GusterBannerPattern;

impl ItemDef for GusterBannerPattern {
    const ID: i32 = 1284;
    const STRING_ID: &'static str = "minecraft:guster_banner_pattern";
    const NAME: &'static str = "Guster Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Field Masoned Banner Pattern
pub struct FieldMasonedBannerPattern;

impl ItemDef for FieldMasonedBannerPattern {
    const ID: i32 = 1285;
    const STRING_ID: &'static str = "minecraft:field_masoned_banner_pattern";
    const NAME: &'static str = "Field Masoned Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Bordure Indented Banner Pattern
pub struct BordureIndentedBannerPattern;

impl ItemDef for BordureIndentedBannerPattern {
    const ID: i32 = 1286;
    const STRING_ID: &'static str = "minecraft:bordure_indented_banner_pattern";
    const NAME: &'static str = "Bordure Indented Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Goat Horn
pub struct GoatHorn;

impl ItemDef for GoatHorn {
    const ID: i32 = 1287;
    const STRING_ID: &'static str = "minecraft:goat_horn";
    const NAME: &'static str = "Goat Horn";
    const STACK_SIZE: u8 = 1;
}

/// Composter
pub struct Composter;

impl ItemDef for Composter {
    const ID: i32 = 1288;
    const STRING_ID: &'static str = "minecraft:composter";
    const NAME: &'static str = "Composter";
    const STACK_SIZE: u8 = 64;
}

/// Barrel
pub struct Barrel;

impl ItemDef for Barrel {
    const ID: i32 = 1289;
    const STRING_ID: &'static str = "minecraft:barrel";
    const NAME: &'static str = "Barrel";
    const STACK_SIZE: u8 = 64;
}

/// Smoker
pub struct Smoker;

impl ItemDef for Smoker {
    const ID: i32 = 1290;
    const STRING_ID: &'static str = "minecraft:smoker";
    const NAME: &'static str = "Smoker";
    const STACK_SIZE: u8 = 64;
}

/// Blast Furnace
pub struct BlastFurnace;

impl ItemDef for BlastFurnace {
    const ID: i32 = 1291;
    const STRING_ID: &'static str = "minecraft:blast_furnace";
    const NAME: &'static str = "Blast Furnace";
    const STACK_SIZE: u8 = 64;
}

/// Cartography Table
pub struct CartographyTable;

impl ItemDef for CartographyTable {
    const ID: i32 = 1292;
    const STRING_ID: &'static str = "minecraft:cartography_table";
    const NAME: &'static str = "Cartography Table";
    const STACK_SIZE: u8 = 64;
}

/// Fletching Table
pub struct FletchingTable;

impl ItemDef for FletchingTable {
    const ID: i32 = 1293;
    const STRING_ID: &'static str = "minecraft:fletching_table";
    const NAME: &'static str = "Fletching Table";
    const STACK_SIZE: u8 = 64;
}

/// Grindstone
pub struct Grindstone;

impl ItemDef for Grindstone {
    const ID: i32 = 1294;
    const STRING_ID: &'static str = "minecraft:grindstone";
    const NAME: &'static str = "Grindstone";
    const STACK_SIZE: u8 = 64;
}

/// Smithing Table
pub struct SmithingTable;

impl ItemDef for SmithingTable {
    const ID: i32 = 1295;
    const STRING_ID: &'static str = "minecraft:smithing_table";
    const NAME: &'static str = "Smithing Table";
    const STACK_SIZE: u8 = 64;
}

/// Stonecutter
pub struct StonecutterBlock;

impl ItemDef for StonecutterBlock {
    const ID: i32 = 1296;
    const STRING_ID: &'static str = "minecraft:stonecutter_block";
    const NAME: &'static str = "Stonecutter";
    const STACK_SIZE: u8 = 64;
}

/// Bell
pub struct Bell;

impl ItemDef for Bell {
    const ID: i32 = 1297;
    const STRING_ID: &'static str = "minecraft:bell";
    const NAME: &'static str = "Bell";
    const STACK_SIZE: u8 = 64;
}

/// Lantern
pub struct Lantern;

impl ItemDef for Lantern {
    const ID: i32 = 1298;
    const STRING_ID: &'static str = "minecraft:lantern";
    const NAME: &'static str = "Lantern";
    const STACK_SIZE: u8 = 64;
}

/// Soul Lantern
pub struct SoulLantern;

impl ItemDef for SoulLantern {
    const ID: i32 = 1299;
    const STRING_ID: &'static str = "minecraft:soul_lantern";
    const NAME: &'static str = "Soul Lantern";
    const STACK_SIZE: u8 = 64;
}

/// Sweet Berries
pub struct SweetBerries;

impl ItemDef for SweetBerries {
    const ID: i32 = 1300;
    const STRING_ID: &'static str = "minecraft:sweet_berries";
    const NAME: &'static str = "Sweet Berries";
    const STACK_SIZE: u8 = 64;
}

/// Glow Berries
pub struct GlowBerries;

impl ItemDef for GlowBerries {
    const ID: i32 = 1301;
    const STRING_ID: &'static str = "minecraft:glow_berries";
    const NAME: &'static str = "Glow Berries";
    const STACK_SIZE: u8 = 64;
}

/// Campfire
pub struct Campfire;

impl ItemDef for Campfire {
    const ID: i32 = 1302;
    const STRING_ID: &'static str = "minecraft:campfire";
    const NAME: &'static str = "Campfire";
    const STACK_SIZE: u8 = 64;
}

/// Soul Campfire
pub struct SoulCampfire;

impl ItemDef for SoulCampfire {
    const ID: i32 = 1303;
    const STRING_ID: &'static str = "minecraft:soul_campfire";
    const NAME: &'static str = "Soul Campfire";
    const STACK_SIZE: u8 = 64;
}

/// Shroomlight
pub struct Shroomlight;

impl ItemDef for Shroomlight {
    const ID: i32 = 1304;
    const STRING_ID: &'static str = "minecraft:shroomlight";
    const NAME: &'static str = "Shroomlight";
    const STACK_SIZE: u8 = 64;
}

/// Honeycomb
pub struct Honeycomb;

impl ItemDef for Honeycomb {
    const ID: i32 = 1305;
    const STRING_ID: &'static str = "minecraft:honeycomb";
    const NAME: &'static str = "Honeycomb";
    const STACK_SIZE: u8 = 64;
}

/// Bee Nest
pub struct BeeNest;

impl ItemDef for BeeNest {
    const ID: i32 = 1306;
    const STRING_ID: &'static str = "minecraft:bee_nest";
    const NAME: &'static str = "Bee Nest";
    const STACK_SIZE: u8 = 64;
}

/// Beehive
pub struct Beehive;

impl ItemDef for Beehive {
    const ID: i32 = 1307;
    const STRING_ID: &'static str = "minecraft:beehive";
    const NAME: &'static str = "Beehive";
    const STACK_SIZE: u8 = 64;
}

/// Honey Bottle
pub struct HoneyBottle;

impl ItemDef for HoneyBottle {
    const ID: i32 = 1308;
    const STRING_ID: &'static str = "minecraft:honey_bottle";
    const NAME: &'static str = "Honey Bottle";
    const STACK_SIZE: u8 = 16;
}

/// Honeycomb Block
pub struct HoneycombBlock;

impl ItemDef for HoneycombBlock {
    const ID: i32 = 1309;
    const STRING_ID: &'static str = "minecraft:honeycomb_block";
    const NAME: &'static str = "Honeycomb Block";
    const STACK_SIZE: u8 = 64;
}

/// Lodestone
pub struct Lodestone;

impl ItemDef for Lodestone {
    const ID: i32 = 1310;
    const STRING_ID: &'static str = "minecraft:lodestone";
    const NAME: &'static str = "Lodestone";
    const STACK_SIZE: u8 = 64;
}

/// Crying Obsidian
pub struct CryingObsidian;

impl ItemDef for CryingObsidian {
    const ID: i32 = 1311;
    const STRING_ID: &'static str = "minecraft:crying_obsidian";
    const NAME: &'static str = "Crying Obsidian";
    const STACK_SIZE: u8 = 64;
}

/// Blackstone
pub struct Blackstone;

impl ItemDef for Blackstone {
    const ID: i32 = 1312;
    const STRING_ID: &'static str = "minecraft:blackstone";
    const NAME: &'static str = "Blackstone";
    const STACK_SIZE: u8 = 64;
}

/// Blackstone Slab
pub struct BlackstoneSlab;

impl ItemDef for BlackstoneSlab {
    const ID: i32 = 1313;
    const STRING_ID: &'static str = "minecraft:blackstone_slab";
    const NAME: &'static str = "Blackstone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Blackstone Stairs
pub struct BlackstoneStairs;

impl ItemDef for BlackstoneStairs {
    const ID: i32 = 1314;
    const STRING_ID: &'static str = "minecraft:blackstone_stairs";
    const NAME: &'static str = "Blackstone Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Gilded Blackstone
pub struct GildedBlackstone;

impl ItemDef for GildedBlackstone {
    const ID: i32 = 1315;
    const STRING_ID: &'static str = "minecraft:gilded_blackstone";
    const NAME: &'static str = "Gilded Blackstone";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone
pub struct PolishedBlackstone;

impl ItemDef for PolishedBlackstone {
    const ID: i32 = 1316;
    const STRING_ID: &'static str = "minecraft:polished_blackstone";
    const NAME: &'static str = "Polished Blackstone";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone Slab
pub struct PolishedBlackstoneSlab;

impl ItemDef for PolishedBlackstoneSlab {
    const ID: i32 = 1317;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_slab";
    const NAME: &'static str = "Polished Blackstone Slab";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone Stairs
pub struct PolishedBlackstoneStairs;

impl ItemDef for PolishedBlackstoneStairs {
    const ID: i32 = 1318;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_stairs";
    const NAME: &'static str = "Polished Blackstone Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Chiseled Polished Blackstone
pub struct ChiseledPolishedBlackstone;

impl ItemDef for ChiseledPolishedBlackstone {
    const ID: i32 = 1319;
    const STRING_ID: &'static str = "minecraft:chiseled_polished_blackstone";
    const NAME: &'static str = "Chiseled Polished Blackstone";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone Bricks
pub struct PolishedBlackstoneBricks;

impl ItemDef for PolishedBlackstoneBricks {
    const ID: i32 = 1320;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_bricks";
    const NAME: &'static str = "Polished Blackstone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone Brick Slab
pub struct PolishedBlackstoneBrickSlab;

impl ItemDef for PolishedBlackstoneBrickSlab {
    const ID: i32 = 1321;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_brick_slab";
    const NAME: &'static str = "Polished Blackstone Brick Slab";
    const STACK_SIZE: u8 = 64;
}

/// Polished Blackstone Brick Stairs
pub struct PolishedBlackstoneBrickStairs;

impl ItemDef for PolishedBlackstoneBrickStairs {
    const ID: i32 = 1322;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_brick_stairs";
    const NAME: &'static str = "Polished Blackstone Brick Stairs";
    const STACK_SIZE: u8 = 64;
}

/// Cracked Polished Blackstone Bricks
pub struct CrackedPolishedBlackstoneBricks;

impl ItemDef for CrackedPolishedBlackstoneBricks {
    const ID: i32 = 1323;
    const STRING_ID: &'static str = "minecraft:cracked_polished_blackstone_bricks";
    const NAME: &'static str = "Cracked Polished Blackstone Bricks";
    const STACK_SIZE: u8 = 64;
}

/// Respawn Anchor
pub struct RespawnAnchor;

impl ItemDef for RespawnAnchor {
    const ID: i32 = 1324;
    const STRING_ID: &'static str = "minecraft:respawn_anchor";
    const NAME: &'static str = "Respawn Anchor";
    const STACK_SIZE: u8 = 64;
}

/// Candle
pub struct Candle;

impl ItemDef for Candle {
    const ID: i32 = 1325;
    const STRING_ID: &'static str = "minecraft:candle";
    const NAME: &'static str = "Candle";
    const STACK_SIZE: u8 = 64;
}

/// White Candle
pub struct WhiteCandle;

impl ItemDef for WhiteCandle {
    const ID: i32 = 1326;
    const STRING_ID: &'static str = "minecraft:white_candle";
    const NAME: &'static str = "White Candle";
    const STACK_SIZE: u8 = 64;
}

/// Orange Candle
pub struct OrangeCandle;

impl ItemDef for OrangeCandle {
    const ID: i32 = 1327;
    const STRING_ID: &'static str = "minecraft:orange_candle";
    const NAME: &'static str = "Orange Candle";
    const STACK_SIZE: u8 = 64;
}

/// Magenta Candle
pub struct MagentaCandle;

impl ItemDef for MagentaCandle {
    const ID: i32 = 1328;
    const STRING_ID: &'static str = "minecraft:magenta_candle";
    const NAME: &'static str = "Magenta Candle";
    const STACK_SIZE: u8 = 64;
}

/// Light Blue Candle
pub struct LightBlueCandle;

impl ItemDef for LightBlueCandle {
    const ID: i32 = 1329;
    const STRING_ID: &'static str = "minecraft:light_blue_candle";
    const NAME: &'static str = "Light Blue Candle";
    const STACK_SIZE: u8 = 64;
}

/// Yellow Candle
pub struct YellowCandle;

impl ItemDef for YellowCandle {
    const ID: i32 = 1330;
    const STRING_ID: &'static str = "minecraft:yellow_candle";
    const NAME: &'static str = "Yellow Candle";
    const STACK_SIZE: u8 = 64;
}

/// Lime Candle
pub struct LimeCandle;

impl ItemDef for LimeCandle {
    const ID: i32 = 1331;
    const STRING_ID: &'static str = "minecraft:lime_candle";
    const NAME: &'static str = "Lime Candle";
    const STACK_SIZE: u8 = 64;
}

/// Pink Candle
pub struct PinkCandle;

impl ItemDef for PinkCandle {
    const ID: i32 = 1332;
    const STRING_ID: &'static str = "minecraft:pink_candle";
    const NAME: &'static str = "Pink Candle";
    const STACK_SIZE: u8 = 64;
}

/// Gray Candle
pub struct GrayCandle;

impl ItemDef for GrayCandle {
    const ID: i32 = 1333;
    const STRING_ID: &'static str = "minecraft:gray_candle";
    const NAME: &'static str = "Gray Candle";
    const STACK_SIZE: u8 = 64;
}

/// Light Gray Candle
pub struct LightGrayCandle;

impl ItemDef for LightGrayCandle {
    const ID: i32 = 1334;
    const STRING_ID: &'static str = "minecraft:light_gray_candle";
    const NAME: &'static str = "Light Gray Candle";
    const STACK_SIZE: u8 = 64;
}

/// Cyan Candle
pub struct CyanCandle;

impl ItemDef for CyanCandle {
    const ID: i32 = 1335;
    const STRING_ID: &'static str = "minecraft:cyan_candle";
    const NAME: &'static str = "Cyan Candle";
    const STACK_SIZE: u8 = 64;
}

/// Purple Candle
pub struct PurpleCandle;

impl ItemDef for PurpleCandle {
    const ID: i32 = 1336;
    const STRING_ID: &'static str = "minecraft:purple_candle";
    const NAME: &'static str = "Purple Candle";
    const STACK_SIZE: u8 = 64;
}

/// Blue Candle
pub struct BlueCandle;

impl ItemDef for BlueCandle {
    const ID: i32 = 1337;
    const STRING_ID: &'static str = "minecraft:blue_candle";
    const NAME: &'static str = "Blue Candle";
    const STACK_SIZE: u8 = 64;
}

/// Brown Candle
pub struct BrownCandle;

impl ItemDef for BrownCandle {
    const ID: i32 = 1338;
    const STRING_ID: &'static str = "minecraft:brown_candle";
    const NAME: &'static str = "Brown Candle";
    const STACK_SIZE: u8 = 64;
}

/// Green Candle
pub struct GreenCandle;

impl ItemDef for GreenCandle {
    const ID: i32 = 1339;
    const STRING_ID: &'static str = "minecraft:green_candle";
    const NAME: &'static str = "Green Candle";
    const STACK_SIZE: u8 = 64;
}

/// Red Candle
pub struct RedCandle;

impl ItemDef for RedCandle {
    const ID: i32 = 1340;
    const STRING_ID: &'static str = "minecraft:red_candle";
    const NAME: &'static str = "Red Candle";
    const STACK_SIZE: u8 = 64;
}

/// Black Candle
pub struct BlackCandle;

impl ItemDef for BlackCandle {
    const ID: i32 = 1341;
    const STRING_ID: &'static str = "minecraft:black_candle";
    const NAME: &'static str = "Black Candle";
    const STACK_SIZE: u8 = 64;
}

/// Small Amethyst Bud
pub struct SmallAmethystBud;

impl ItemDef for SmallAmethystBud {
    const ID: i32 = 1342;
    const STRING_ID: &'static str = "minecraft:small_amethyst_bud";
    const NAME: &'static str = "Small Amethyst Bud";
    const STACK_SIZE: u8 = 64;
}

/// Medium Amethyst Bud
pub struct MediumAmethystBud;

impl ItemDef for MediumAmethystBud {
    const ID: i32 = 1343;
    const STRING_ID: &'static str = "minecraft:medium_amethyst_bud";
    const NAME: &'static str = "Medium Amethyst Bud";
    const STACK_SIZE: u8 = 64;
}

/// Large Amethyst Bud
pub struct LargeAmethystBud;

impl ItemDef for LargeAmethystBud {
    const ID: i32 = 1344;
    const STRING_ID: &'static str = "minecraft:large_amethyst_bud";
    const NAME: &'static str = "Large Amethyst Bud";
    const STACK_SIZE: u8 = 64;
}

/// Amethyst Cluster
pub struct AmethystCluster;

impl ItemDef for AmethystCluster {
    const ID: i32 = 1345;
    const STRING_ID: &'static str = "minecraft:amethyst_cluster";
    const NAME: &'static str = "Amethyst Cluster";
    const STACK_SIZE: u8 = 64;
}

/// Pointed Dripstone
pub struct PointedDripstone;

impl ItemDef for PointedDripstone {
    const ID: i32 = 1346;
    const STRING_ID: &'static str = "minecraft:pointed_dripstone";
    const NAME: &'static str = "Pointed Dripstone";
    const STACK_SIZE: u8 = 64;
}

/// Ochre Froglight
pub struct OchreFroglight;

impl ItemDef for OchreFroglight {
    const ID: i32 = 1347;
    const STRING_ID: &'static str = "minecraft:ochre_froglight";
    const NAME: &'static str = "Ochre Froglight";
    const STACK_SIZE: u8 = 64;
}

/// Verdant Froglight
pub struct VerdantFroglight;

impl ItemDef for VerdantFroglight {
    const ID: i32 = 1348;
    const STRING_ID: &'static str = "minecraft:verdant_froglight";
    const NAME: &'static str = "Verdant Froglight";
    const STACK_SIZE: u8 = 64;
}

/// Pearlescent Froglight
pub struct PearlescentFroglight;

impl ItemDef for PearlescentFroglight {
    const ID: i32 = 1349;
    const STRING_ID: &'static str = "minecraft:pearlescent_froglight";
    const NAME: &'static str = "Pearlescent Froglight";
    const STACK_SIZE: u8 = 64;
}

/// Frogspawn
pub struct FrogSpawn;

impl ItemDef for FrogSpawn {
    const ID: i32 = 1350;
    const STRING_ID: &'static str = "minecraft:frog_spawn";
    const NAME: &'static str = "Frogspawn";
    const STACK_SIZE: u8 = 64;
}

/// Echo Shard
pub struct EchoShard;

impl ItemDef for EchoShard {
    const ID: i32 = 1351;
    const STRING_ID: &'static str = "minecraft:echo_shard";
    const NAME: &'static str = "Echo Shard";
    const STACK_SIZE: u8 = 64;
}

/// Brush
pub struct Brush;

impl ItemDef for Brush {
    const ID: i32 = 1352;
    const STRING_ID: &'static str = "minecraft:brush";
    const NAME: &'static str = "Brush";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for Brush {
    const MAX_DURABILITY: u16 = 64;
}

impl EnchantableItem for Brush {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[
            EnchantmentCategory::Durability,
            EnchantmentCategory::Vanishing,
        ]
    }
}

/// Netherite Upgrade
pub struct NetheriteUpgradeSmithingTemplate;

impl ItemDef for NetheriteUpgradeSmithingTemplate {
    const ID: i32 = 1353;
    const STRING_ID: &'static str = "minecraft:netherite_upgrade_smithing_template";
    const NAME: &'static str = "Netherite Upgrade";
    const STACK_SIZE: u8 = 64;
}

/// Sentry Armor Trim
pub struct SentryArmorTrimSmithingTemplate;

impl ItemDef for SentryArmorTrimSmithingTemplate {
    const ID: i32 = 1354;
    const STRING_ID: &'static str = "minecraft:sentry_armor_trim_smithing_template";
    const NAME: &'static str = "Sentry Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Dune Armor Trim
pub struct DuneArmorTrimSmithingTemplate;

impl ItemDef for DuneArmorTrimSmithingTemplate {
    const ID: i32 = 1355;
    const STRING_ID: &'static str = "minecraft:dune_armor_trim_smithing_template";
    const NAME: &'static str = "Dune Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Coast Armor Trim
pub struct CoastArmorTrimSmithingTemplate;

impl ItemDef for CoastArmorTrimSmithingTemplate {
    const ID: i32 = 1356;
    const STRING_ID: &'static str = "minecraft:coast_armor_trim_smithing_template";
    const NAME: &'static str = "Coast Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Wild Armor Trim
pub struct WildArmorTrimSmithingTemplate;

impl ItemDef for WildArmorTrimSmithingTemplate {
    const ID: i32 = 1357;
    const STRING_ID: &'static str = "minecraft:wild_armor_trim_smithing_template";
    const NAME: &'static str = "Wild Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Ward Armor Trim
pub struct WardArmorTrimSmithingTemplate;

impl ItemDef for WardArmorTrimSmithingTemplate {
    const ID: i32 = 1358;
    const STRING_ID: &'static str = "minecraft:ward_armor_trim_smithing_template";
    const NAME: &'static str = "Ward Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Eye Armor Trim
pub struct EyeArmorTrimSmithingTemplate;

impl ItemDef for EyeArmorTrimSmithingTemplate {
    const ID: i32 = 1359;
    const STRING_ID: &'static str = "minecraft:eye_armor_trim_smithing_template";
    const NAME: &'static str = "Eye Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Vex Armor Trim
pub struct VexArmorTrimSmithingTemplate;

impl ItemDef for VexArmorTrimSmithingTemplate {
    const ID: i32 = 1360;
    const STRING_ID: &'static str = "minecraft:vex_armor_trim_smithing_template";
    const NAME: &'static str = "Vex Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Tide Armor Trim
pub struct TideArmorTrimSmithingTemplate;

impl ItemDef for TideArmorTrimSmithingTemplate {
    const ID: i32 = 1361;
    const STRING_ID: &'static str = "minecraft:tide_armor_trim_smithing_template";
    const NAME: &'static str = "Tide Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Snout Armor Trim
pub struct SnoutArmorTrimSmithingTemplate;

impl ItemDef for SnoutArmorTrimSmithingTemplate {
    const ID: i32 = 1362;
    const STRING_ID: &'static str = "minecraft:snout_armor_trim_smithing_template";
    const NAME: &'static str = "Snout Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Rib Armor Trim
pub struct RibArmorTrimSmithingTemplate;

impl ItemDef for RibArmorTrimSmithingTemplate {
    const ID: i32 = 1363;
    const STRING_ID: &'static str = "minecraft:rib_armor_trim_smithing_template";
    const NAME: &'static str = "Rib Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Spire Armor Trim
pub struct SpireArmorTrimSmithingTemplate;

impl ItemDef for SpireArmorTrimSmithingTemplate {
    const ID: i32 = 1364;
    const STRING_ID: &'static str = "minecraft:spire_armor_trim_smithing_template";
    const NAME: &'static str = "Spire Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Wayfinder Armor Trim
pub struct WayfinderArmorTrimSmithingTemplate;

impl ItemDef for WayfinderArmorTrimSmithingTemplate {
    const ID: i32 = 1365;
    const STRING_ID: &'static str = "minecraft:wayfinder_armor_trim_smithing_template";
    const NAME: &'static str = "Wayfinder Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Shaper Armor Trim
pub struct ShaperArmorTrimSmithingTemplate;

impl ItemDef for ShaperArmorTrimSmithingTemplate {
    const ID: i32 = 1366;
    const STRING_ID: &'static str = "minecraft:shaper_armor_trim_smithing_template";
    const NAME: &'static str = "Shaper Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Silence Armor Trim
pub struct SilenceArmorTrimSmithingTemplate;

impl ItemDef for SilenceArmorTrimSmithingTemplate {
    const ID: i32 = 1367;
    const STRING_ID: &'static str = "minecraft:silence_armor_trim_smithing_template";
    const NAME: &'static str = "Silence Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Raiser Armor Trim
pub struct RaiserArmorTrimSmithingTemplate;

impl ItemDef for RaiserArmorTrimSmithingTemplate {
    const ID: i32 = 1368;
    const STRING_ID: &'static str = "minecraft:raiser_armor_trim_smithing_template";
    const NAME: &'static str = "Raiser Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Host Armor Trim
pub struct HostArmorTrimSmithingTemplate;

impl ItemDef for HostArmorTrimSmithingTemplate {
    const ID: i32 = 1369;
    const STRING_ID: &'static str = "minecraft:host_armor_trim_smithing_template";
    const NAME: &'static str = "Host Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Flow Armor Trim
pub struct FlowArmorTrimSmithingTemplate;

impl ItemDef for FlowArmorTrimSmithingTemplate {
    const ID: i32 = 1370;
    const STRING_ID: &'static str = "minecraft:flow_armor_trim_smithing_template";
    const NAME: &'static str = "Flow Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Bolt Armor Trim
pub struct BoltArmorTrimSmithingTemplate;

impl ItemDef for BoltArmorTrimSmithingTemplate {
    const ID: i32 = 1371;
    const STRING_ID: &'static str = "minecraft:bolt_armor_trim_smithing_template";
    const NAME: &'static str = "Bolt Armor Trim";
    const STACK_SIZE: u8 = 64;
}

/// Angler Pottery Sherd
pub struct AnglerPotterySherd;

impl ItemDef for AnglerPotterySherd {
    const ID: i32 = 1372;
    const STRING_ID: &'static str = "minecraft:angler_pottery_sherd";
    const NAME: &'static str = "Angler Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Archer Pottery Sherd
pub struct ArcherPotterySherd;

impl ItemDef for ArcherPotterySherd {
    const ID: i32 = 1373;
    const STRING_ID: &'static str = "minecraft:archer_pottery_sherd";
    const NAME: &'static str = "Archer Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Arms Up Pottery Sherd
pub struct ArmsUpPotterySherd;

impl ItemDef for ArmsUpPotterySherd {
    const ID: i32 = 1374;
    const STRING_ID: &'static str = "minecraft:arms_up_pottery_sherd";
    const NAME: &'static str = "Arms Up Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Blade Pottery Sherd
pub struct BladePotterySherd;

impl ItemDef for BladePotterySherd {
    const ID: i32 = 1375;
    const STRING_ID: &'static str = "minecraft:blade_pottery_sherd";
    const NAME: &'static str = "Blade Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Brewer Pottery Sherd
pub struct BrewerPotterySherd;

impl ItemDef for BrewerPotterySherd {
    const ID: i32 = 1376;
    const STRING_ID: &'static str = "minecraft:brewer_pottery_sherd";
    const NAME: &'static str = "Brewer Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Burn Pottery Sherd
pub struct BurnPotterySherd;

impl ItemDef for BurnPotterySherd {
    const ID: i32 = 1377;
    const STRING_ID: &'static str = "minecraft:burn_pottery_sherd";
    const NAME: &'static str = "Burn Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Danger Pottery Sherd
pub struct DangerPotterySherd;

impl ItemDef for DangerPotterySherd {
    const ID: i32 = 1378;
    const STRING_ID: &'static str = "minecraft:danger_pottery_sherd";
    const NAME: &'static str = "Danger Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Explorer Pottery Sherd
pub struct ExplorerPotterySherd;

impl ItemDef for ExplorerPotterySherd {
    const ID: i32 = 1379;
    const STRING_ID: &'static str = "minecraft:explorer_pottery_sherd";
    const NAME: &'static str = "Explorer Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Flow Pottery Sherd
pub struct FlowPotterySherd;

impl ItemDef for FlowPotterySherd {
    const ID: i32 = 1380;
    const STRING_ID: &'static str = "minecraft:flow_pottery_sherd";
    const NAME: &'static str = "Flow Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Friend Pottery Sherd
pub struct FriendPotterySherd;

impl ItemDef for FriendPotterySherd {
    const ID: i32 = 1381;
    const STRING_ID: &'static str = "minecraft:friend_pottery_sherd";
    const NAME: &'static str = "Friend Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Guster Pottery Sherd
pub struct GusterPotterySherd;

impl ItemDef for GusterPotterySherd {
    const ID: i32 = 1382;
    const STRING_ID: &'static str = "minecraft:guster_pottery_sherd";
    const NAME: &'static str = "Guster Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Heart Pottery Sherd
pub struct HeartPotterySherd;

impl ItemDef for HeartPotterySherd {
    const ID: i32 = 1383;
    const STRING_ID: &'static str = "minecraft:heart_pottery_sherd";
    const NAME: &'static str = "Heart Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Heartbreak Pottery Sherd
pub struct HeartbreakPotterySherd;

impl ItemDef for HeartbreakPotterySherd {
    const ID: i32 = 1384;
    const STRING_ID: &'static str = "minecraft:heartbreak_pottery_sherd";
    const NAME: &'static str = "Heartbreak Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Howl Pottery Sherd
pub struct HowlPotterySherd;

impl ItemDef for HowlPotterySherd {
    const ID: i32 = 1385;
    const STRING_ID: &'static str = "minecraft:howl_pottery_sherd";
    const NAME: &'static str = "Howl Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Miner Pottery Sherd
pub struct MinerPotterySherd;

impl ItemDef for MinerPotterySherd {
    const ID: i32 = 1386;
    const STRING_ID: &'static str = "minecraft:miner_pottery_sherd";
    const NAME: &'static str = "Miner Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Mourner Pottery Sherd
pub struct MournerPotterySherd;

impl ItemDef for MournerPotterySherd {
    const ID: i32 = 1387;
    const STRING_ID: &'static str = "minecraft:mourner_pottery_sherd";
    const NAME: &'static str = "Mourner Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Plenty Pottery Sherd
pub struct PlentyPotterySherd;

impl ItemDef for PlentyPotterySherd {
    const ID: i32 = 1388;
    const STRING_ID: &'static str = "minecraft:plenty_pottery_sherd";
    const NAME: &'static str = "Plenty Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Prize Pottery Sherd
pub struct PrizePotterySherd;

impl ItemDef for PrizePotterySherd {
    const ID: i32 = 1389;
    const STRING_ID: &'static str = "minecraft:prize_pottery_sherd";
    const NAME: &'static str = "Prize Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Scrape Pottery Sherd
pub struct ScrapePotterySherd;

impl ItemDef for ScrapePotterySherd {
    const ID: i32 = 1390;
    const STRING_ID: &'static str = "minecraft:scrape_pottery_sherd";
    const NAME: &'static str = "Scrape Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Sheaf Pottery Sherd
pub struct SheafPotterySherd;

impl ItemDef for SheafPotterySherd {
    const ID: i32 = 1391;
    const STRING_ID: &'static str = "minecraft:sheaf_pottery_sherd";
    const NAME: &'static str = "Sheaf Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Shelter Pottery Sherd
pub struct ShelterPotterySherd;

impl ItemDef for ShelterPotterySherd {
    const ID: i32 = 1392;
    const STRING_ID: &'static str = "minecraft:shelter_pottery_sherd";
    const NAME: &'static str = "Shelter Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Skull Pottery Sherd
pub struct SkullPotterySherd;

impl ItemDef for SkullPotterySherd {
    const ID: i32 = 1393;
    const STRING_ID: &'static str = "minecraft:skull_pottery_sherd";
    const NAME: &'static str = "Skull Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Snort Pottery Sherd
pub struct SnortPotterySherd;

impl ItemDef for SnortPotterySherd {
    const ID: i32 = 1394;
    const STRING_ID: &'static str = "minecraft:snort_pottery_sherd";
    const NAME: &'static str = "Snort Pottery Sherd";
    const STACK_SIZE: u8 = 64;
}

/// Copper Grate
pub struct CopperGrate;

impl ItemDef for CopperGrate {
    const ID: i32 = 1395;
    const STRING_ID: &'static str = "minecraft:copper_grate";
    const NAME: &'static str = "Copper Grate";
    const STACK_SIZE: u8 = 64;
}

/// Exposed Copper Grate
pub struct ExposedCopperGrate;

impl ItemDef for ExposedCopperGrate {
    const ID: i32 = 1396;
    const STRING_ID: &'static str = "minecraft:exposed_copper_grate";
    const NAME: &'static str = "Exposed Copper Grate";
    const STACK_SIZE: u8 = 64;
}

/// Weathered Copper Grate
pub struct WeatheredCopperGrate;

impl ItemDef for WeatheredCopperGrate {
    const ID: i32 = 1397;
    const STRING_ID: &'static str = "minecraft:weathered_copper_grate";
    const NAME: &'static str = "Weathered Copper Grate";
    const STACK_SIZE: u8 = 64;
}

/// Oxidized Copper Grate
pub struct OxidizedCopperGrate;

impl ItemDef for OxidizedCopperGrate {
    const ID: i32 = 1398;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_grate";
    const NAME: &'static str = "Oxidized Copper Grate";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Copper Grate
pub struct WaxedCopperGrate;

impl ItemDef for WaxedCopperGrate {
    const ID: i32 = 1399;
    const STRING_ID: &'static str = "minecraft:waxed_copper_grate";
    const NAME: &'static str = "Waxed Copper Grate";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Exposed Copper Grate
pub struct WaxedExposedCopperGrate;

impl ItemDef for WaxedExposedCopperGrate {
    const ID: i32 = 1400;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_grate";
    const NAME: &'static str = "Waxed Exposed Copper Grate";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Weathered Copper Grate
pub struct WaxedWeatheredCopperGrate;

impl ItemDef for WaxedWeatheredCopperGrate {
    const ID: i32 = 1401;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_grate";
    const NAME: &'static str = "Waxed Weathered Copper Grate";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Oxidized Copper Grate
pub struct WaxedOxidizedCopperGrate;

impl ItemDef for WaxedOxidizedCopperGrate {
    const ID: i32 = 1402;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_grate";
    const NAME: &'static str = "Waxed Oxidized Copper Grate";
    const STACK_SIZE: u8 = 64;
}

/// Copper Bulb
pub struct CopperBulb;

impl ItemDef for CopperBulb {
    const ID: i32 = 1403;
    const STRING_ID: &'static str = "minecraft:copper_bulb";
    const NAME: &'static str = "Copper Bulb";
    const STACK_SIZE: u8 = 64;
}

/// Exposed Copper Bulb
pub struct ExposedCopperBulb;

impl ItemDef for ExposedCopperBulb {
    const ID: i32 = 1404;
    const STRING_ID: &'static str = "minecraft:exposed_copper_bulb";
    const NAME: &'static str = "Exposed Copper Bulb";
    const STACK_SIZE: u8 = 64;
}

/// Weathered Copper Bulb
pub struct WeatheredCopperBulb;

impl ItemDef for WeatheredCopperBulb {
    const ID: i32 = 1405;
    const STRING_ID: &'static str = "minecraft:weathered_copper_bulb";
    const NAME: &'static str = "Weathered Copper Bulb";
    const STACK_SIZE: u8 = 64;
}

/// Oxidized Copper Bulb
pub struct OxidizedCopperBulb;

impl ItemDef for OxidizedCopperBulb {
    const ID: i32 = 1406;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_bulb";
    const NAME: &'static str = "Oxidized Copper Bulb";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Copper Bulb
pub struct WaxedCopperBulb;

impl ItemDef for WaxedCopperBulb {
    const ID: i32 = 1407;
    const STRING_ID: &'static str = "minecraft:waxed_copper_bulb";
    const NAME: &'static str = "Waxed Copper Bulb";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Exposed Copper Bulb
pub struct WaxedExposedCopperBulb;

impl ItemDef for WaxedExposedCopperBulb {
    const ID: i32 = 1408;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_bulb";
    const NAME: &'static str = "Waxed Exposed Copper Bulb";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Weathered Copper Bulb
pub struct WaxedWeatheredCopperBulb;

impl ItemDef for WaxedWeatheredCopperBulb {
    const ID: i32 = 1409;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_bulb";
    const NAME: &'static str = "Waxed Weathered Copper Bulb";
    const STACK_SIZE: u8 = 64;
}

/// Waxed Oxidized Copper Bulb
pub struct WaxedOxidizedCopperBulb;

impl ItemDef for WaxedOxidizedCopperBulb {
    const ID: i32 = 1410;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_bulb";
    const NAME: &'static str = "Waxed Oxidized Copper Bulb";
    const STACK_SIZE: u8 = 64;
}

/// Trial Spawner
pub struct TrialSpawner;

impl ItemDef for TrialSpawner {
    const ID: i32 = 1411;
    const STRING_ID: &'static str = "minecraft:trial_spawner";
    const NAME: &'static str = "Trial Spawner";
    const STACK_SIZE: u8 = 64;
}

/// Trial Key
pub struct TrialKey;

impl ItemDef for TrialKey {
    const ID: i32 = 1412;
    const STRING_ID: &'static str = "minecraft:trial_key";
    const NAME: &'static str = "Trial Key";
    const STACK_SIZE: u8 = 64;
}

/// Ominous Trial Key
pub struct OminousTrialKey;

impl ItemDef for OminousTrialKey {
    const ID: i32 = 1413;
    const STRING_ID: &'static str = "minecraft:ominous_trial_key";
    const NAME: &'static str = "Ominous Trial Key";
    const STACK_SIZE: u8 = 64;
}

/// Vault
pub struct Vault;

impl ItemDef for Vault {
    const ID: i32 = 1414;
    const STRING_ID: &'static str = "minecraft:vault";
    const NAME: &'static str = "Vault";
    const STACK_SIZE: u8 = 64;
}

/// Ominous Bottle
pub struct OminousBottle;

impl ItemDef for OminousBottle {
    const ID: i32 = 1415;
    const STRING_ID: &'static str = "minecraft:ominous_bottle";
    const NAME: &'static str = "Ominous Bottle";
    const STACK_SIZE: u8 = 64;
}

/// Mangrove Door
pub struct MangroveDoorItem;

impl ItemDef for MangroveDoorItem {
    const ID: i32 = 9017;
    const STRING_ID: &'static str = "minecraft:item.mangrove_door";
    const NAME: &'static str = "Mangrove Door";
    const STACK_SIZE: u8 = 1;
}

/// Rapid Fertilizer
pub struct RapidFertilizer;

impl ItemDef for RapidFertilizer {
    const ID: i32 = 9022;
    const STRING_ID: &'static str = "minecraft:rapid_fertilizer";
    const NAME: &'static str = "Rapid Fertilizer";
    const STACK_SIZE: u8 = 1;
}

/// Sparkler
pub struct Sparkler;

impl ItemDef for Sparkler {
    const ID: i32 = 9039;
    const STRING_ID: &'static str = "minecraft:sparkler";
    const NAME: &'static str = "Sparkler";
    const STACK_SIZE: u8 = 1;
}

/// Underwater Tnt
pub struct UnderwaterTnt;

impl ItemDef for UnderwaterTnt {
    const ID: i32 = 9044;
    const STRING_ID: &'static str = "minecraft:underwater_tnt";
    const NAME: &'static str = "Underwater Tnt";
    const STACK_SIZE: u8 = 1;
}

/// Frame
pub struct FrameItem;

impl ItemDef for FrameItem {
    const ID: i32 = 9046;
    const STRING_ID: &'static str = "minecraft:item.frame";
    const NAME: &'static str = "Frame";
    const STACK_SIZE: u8 = 1;
}

/// Element 15
pub struct Element15;

impl ItemDef for Element15 {
    const ID: i32 = 9056;
    const STRING_ID: &'static str = "minecraft:element_15";
    const NAME: &'static str = "Element 15";
    const STACK_SIZE: u8 = 1;
}

/// Polished Tuff Double Slab
pub struct PolishedTuffDoubleSlab;

impl ItemDef for PolishedTuffDoubleSlab {
    const ID: i32 = 9060;
    const STRING_ID: &'static str = "minecraft:polished_tuff_double_slab";
    const NAME: &'static str = "Polished Tuff Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Balloon
pub struct Balloon;

impl ItemDef for Balloon {
    const ID: i32 = 9073;
    const STRING_ID: &'static str = "minecraft:balloon";
    const NAME: &'static str = "Balloon";
    const STACK_SIZE: u8 = 1;
}

/// Smooth Sandstone Double Slab
pub struct SmoothSandstoneDoubleSlab;

impl ItemDef for SmoothSandstoneDoubleSlab {
    const ID: i32 = 9074;
    const STRING_ID: &'static str = "minecraft:smooth_sandstone_double_slab";
    const NAME: &'static str = "Smooth Sandstone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Element 104
pub struct Element104;

impl ItemDef for Element104 {
    const ID: i32 = 9078;
    const STRING_ID: &'static str = "minecraft:element_104";
    const NAME: &'static str = "Element 104";
    const STACK_SIZE: u8 = 1;
}

/// White Candle Cake
pub struct WhiteCandleCake;

impl ItemDef for WhiteCandleCake {
    const ID: i32 = 9080;
    const STRING_ID: &'static str = "minecraft:white_candle_cake";
    const NAME: &'static str = "White Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Dead Tube Coral Wall Fan
pub struct DeadTubeCoralWallFan;

impl ItemDef for DeadTubeCoralWallFan {
    const ID: i32 = 9083;
    const STRING_ID: &'static str = "minecraft:dead_tube_coral_wall_fan";
    const NAME: &'static str = "Dead Tube Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// Element 13
pub struct Element13;

impl ItemDef for Element13 {
    const ID: i32 = 9096;
    const STRING_ID: &'static str = "minecraft:element_13";
    const NAME: &'static str = "Element 13";
    const STACK_SIZE: u8 = 1;
}

/// Element 43
pub struct Element43;

impl ItemDef for Element43 {
    const ID: i32 = 9118;
    const STRING_ID: &'static str = "minecraft:element_43";
    const NAME: &'static str = "Element 43";
    const STACK_SIZE: u8 = 1;
}

/// Copper Sword
pub struct CopperSword;

impl ItemDef for CopperSword {
    const ID: i32 = 9125;
    const STRING_ID: &'static str = "minecraft:copper_sword";
    const NAME: &'static str = "Copper Sword";
    const STACK_SIZE: u8 = 1;
}

/// Element 68
pub struct Element68;

impl ItemDef for Element68 {
    const ID: i32 = 9126;
    const STRING_ID: &'static str = "minecraft:element_68";
    const NAME: &'static str = "Element 68";
    const STACK_SIZE: u8 = 1;
}

/// Element 50
pub struct Element50;

impl ItemDef for Element50 {
    const ID: i32 = 9132;
    const STRING_ID: &'static str = "minecraft:element_50";
    const NAME: &'static str = "Element 50";
    const STACK_SIZE: u8 = 1;
}

/// Board
pub struct Board;

impl ItemDef for Board {
    const ID: i32 = 9133;
    const STRING_ID: &'static str = "minecraft:board";
    const NAME: &'static str = "Board";
    const STACK_SIZE: u8 = 1;
}

/// Hard Blue Stained Glass
pub struct HardBlueStainedGlass;

impl ItemDef for HardBlueStainedGlass {
    const ID: i32 = 9134;
    const STRING_ID: &'static str = "minecraft:hard_blue_stained_glass";
    const NAME: &'static str = "Hard Blue Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Iron Door
pub struct IronDoorItem;

impl ItemDef for IronDoorItem {
    const ID: i32 = 9148;
    const STRING_ID: &'static str = "minecraft:item.iron_door";
    const NAME: &'static str = "Iron Door";
    const STACK_SIZE: u8 = 1;
}

/// Element 27
pub struct Element27;

impl ItemDef for Element27 {
    const ID: i32 = 9151;
    const STRING_ID: &'static str = "minecraft:element_27";
    const NAME: &'static str = "Element 27";
    const STACK_SIZE: u8 = 1;
}

/// Wooden Slab
pub struct WoodenSlab;

impl ItemDef for WoodenSlab {
    const ID: i32 = 9158;
    const STRING_ID: &'static str = "minecraft:wooden_slab";
    const NAME: &'static str = "Wooden Slab";
    const STACK_SIZE: u8 = 1;
}

/// Magenta Candle Cake
pub struct MagentaCandleCake;

impl ItemDef for MagentaCandleCake {
    const ID: i32 = 9171;
    const STRING_ID: &'static str = "minecraft:magenta_candle_cake";
    const NAME: &'static str = "Magenta Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Campfire
pub struct CampfireItem;

impl ItemDef for CampfireItem {
    const ID: i32 = 9172;
    const STRING_ID: &'static str = "minecraft:item.campfire";
    const NAME: &'static str = "Campfire";
    const STACK_SIZE: u8 = 1;
}

/// Element 62
pub struct Element62;

impl ItemDef for Element62 {
    const ID: i32 = 9180;
    const STRING_ID: &'static str = "minecraft:element_62";
    const NAME: &'static str = "Element 62";
    const STACK_SIZE: u8 = 1;
}

/// Hard Pink Stained Glass
pub struct HardPinkStainedGlass;

impl ItemDef for HardPinkStainedGlass {
    const ID: i32 = 9200;
    const STRING_ID: &'static str = "minecraft:hard_pink_stained_glass";
    const NAME: &'static str = "Hard Pink Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Element 2
pub struct Element2;

impl ItemDef for Element2 {
    const ID: i32 = 9204;
    const STRING_ID: &'static str = "minecraft:element_2";
    const NAME: &'static str = "Element 2";
    const STACK_SIZE: u8 = 1;
}

/// Acacia Shelf
pub struct AcaciaShelf;

impl ItemDef for AcaciaShelf {
    const ID: i32 = 9207;
    const STRING_ID: &'static str = "minecraft:acacia_shelf";
    const NAME: &'static str = "Acacia Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Powder Snow
pub struct PowderSnow;

impl ItemDef for PowderSnow {
    const ID: i32 = 9210;
    const STRING_ID: &'static str = "minecraft:powder_snow";
    const NAME: &'static str = "Powder Snow";
    const STACK_SIZE: u8 = 1;
}

/// Element 80
pub struct Element80;

impl ItemDef for Element80 {
    const ID: i32 = 9213;
    const STRING_ID: &'static str = "minecraft:element_80";
    const NAME: &'static str = "Element 80";
    const STACK_SIZE: u8 = 1;
}

/// Hard Brown Stained Glass
pub struct HardBrownStainedGlass;

impl ItemDef for HardBrownStainedGlass {
    const ID: i32 = 9217;
    const STRING_ID: &'static str = "minecraft:hard_brown_stained_glass";
    const NAME: &'static str = "Hard Brown Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Copper Golem Statue
pub struct CopperGolemStatue;

impl ItemDef for CopperGolemStatue {
    const ID: i32 = 9225;
    const STRING_ID: &'static str = "minecraft:copper_golem_statue";
    const NAME: &'static str = "Copper Golem Statue";
    const STACK_SIZE: u8 = 1;
}

/// Portal
pub struct Portal;

impl ItemDef for Portal {
    const ID: i32 = 9235;
    const STRING_ID: &'static str = "minecraft:portal";
    const NAME: &'static str = "Portal";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Exposed Copper Lantern
pub struct WaxedExposedCopperLantern;

impl ItemDef for WaxedExposedCopperLantern {
    const ID: i32 = 9248;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_lantern";
    const NAME: &'static str = "Waxed Exposed Copper Lantern";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 15
pub struct LightBlock15;

impl ItemDef for LightBlock15 {
    const ID: i32 = 9253;
    const STRING_ID: &'static str = "minecraft:light_block_15";
    const NAME: &'static str = "Light Block 15";
    const STACK_SIZE: u8 = 1;
}

/// Planks
pub struct Planks;

impl ItemDef for Planks {
    const ID: i32 = 9258;
    const STRING_ID: &'static str = "minecraft:planks";
    const NAME: &'static str = "Planks";
    const STACK_SIZE: u8 = 1;
}

/// Stained Glass Pane
pub struct StainedGlassPane;

impl ItemDef for StainedGlassPane {
    const ID: i32 = 9261;
    const STRING_ID: &'static str = "minecraft:stained_glass_pane";
    const NAME: &'static str = "Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Colored Torch Purple
pub struct ColoredTorchPurple;

impl ItemDef for ColoredTorchPurple {
    const ID: i32 = 9266;
    const STRING_ID: &'static str = "minecraft:colored_torch_purple";
    const NAME: &'static str = "Colored Torch Purple";
    const STACK_SIZE: u8 = 1;
}

/// Hard Glass
pub struct HardGlass;

impl ItemDef for HardGlass {
    const ID: i32 = 9275;
    const STRING_ID: &'static str = "minecraft:hard_glass";
    const NAME: &'static str = "Hard Glass";
    const STACK_SIZE: u8 = 1;
}

/// Flowing Water
pub struct FlowingWater;

impl ItemDef for FlowingWater {
    const ID: i32 = 9281;
    const STRING_ID: &'static str = "minecraft:flowing_water";
    const NAME: &'static str = "Flowing Water";
    const STACK_SIZE: u8 = 1;
}

/// Lit Deepslate Redstone Ore
pub struct LitDeepslateRedstoneOre;

impl ItemDef for LitDeepslateRedstoneOre {
    const ID: i32 = 9291;
    const STRING_ID: &'static str = "minecraft:lit_deepslate_redstone_ore";
    const NAME: &'static str = "Lit Deepslate Redstone Ore";
    const STACK_SIZE: u8 = 1;
}

/// Lit Redstone Lamp
pub struct LitRedstoneLamp;

impl ItemDef for LitRedstoneLamp {
    const ID: i32 = 9295;
    const STRING_ID: &'static str = "minecraft:lit_redstone_lamp";
    const NAME: &'static str = "Lit Redstone Lamp";
    const STACK_SIZE: u8 = 1;
}

/// Element 52
pub struct Element52;

impl ItemDef for Element52 {
    const ID: i32 = 9297;
    const STRING_ID: &'static str = "minecraft:element_52";
    const NAME: &'static str = "Element 52";
    const STACK_SIZE: u8 = 1;
}

/// Element 86
pub struct Element86;

impl ItemDef for Element86 {
    const ID: i32 = 9315;
    const STRING_ID: &'static str = "minecraft:element_86";
    const NAME: &'static str = "Element 86";
    const STACK_SIZE: u8 = 1;
}

/// Petrified Oak Double Slab
pub struct PetrifiedOakDoubleSlab;

impl ItemDef for PetrifiedOakDoubleSlab {
    const ID: i32 = 9323;
    const STRING_ID: &'static str = "minecraft:petrified_oak_double_slab";
    const NAME: &'static str = "Petrified Oak Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// End Gateway
pub struct EndGateway;

impl ItemDef for EndGateway {
    const ID: i32 = 9329;
    const STRING_ID: &'static str = "minecraft:end_gateway";
    const NAME: &'static str = "End Gateway";
    const STACK_SIZE: u8 = 1;
}

/// Beetroot
pub struct BeetrootItem;

impl ItemDef for BeetrootItem {
    const ID: i32 = 9331;
    const STRING_ID: &'static str = "minecraft:item.beetroot";
    const NAME: &'static str = "Beetroot";
    const STACK_SIZE: u8 = 1;
}

/// Dark Oak Double Slab
pub struct DarkOakDoubleSlab;

impl ItemDef for DarkOakDoubleSlab {
    const ID: i32 = 9332;
    const STRING_ID: &'static str = "minecraft:dark_oak_double_slab";
    const NAME: &'static str = "Dark Oak Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Hard Cyan Stained Glass Pane
pub struct HardCyanStainedGlassPane;

impl ItemDef for HardCyanStainedGlassPane {
    const ID: i32 = 9341;
    const STRING_ID: &'static str = "minecraft:hard_cyan_stained_glass_pane";
    const NAME: &'static str = "Hard Cyan Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Element 51
pub struct Element51;

impl ItemDef for Element51 {
    const ID: i32 = 9352;
    const STRING_ID: &'static str = "minecraft:element_51";
    const NAME: &'static str = "Element 51";
    const STACK_SIZE: u8 = 1;
}

/// Hard Cyan Stained Glass
pub struct HardCyanStainedGlass;

impl ItemDef for HardCyanStainedGlass {
    const ID: i32 = 9360;
    const STRING_ID: &'static str = "minecraft:hard_cyan_stained_glass";
    const NAME: &'static str = "Hard Cyan Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Agent Spawn Egg
pub struct AgentSpawnEgg;

impl ItemDef for AgentSpawnEgg {
    const ID: i32 = 9362;
    const STRING_ID: &'static str = "minecraft:agent_spawn_egg";
    const NAME: &'static str = "Agent Spawn Egg";
    const STACK_SIZE: u8 = 1;
}

/// Carpet
pub struct Carpet;

impl ItemDef for Carpet {
    const ID: i32 = 9363;
    const STRING_ID: &'static str = "minecraft:carpet";
    const NAME: &'static str = "Carpet";
    const STACK_SIZE: u8 = 1;
}

/// Colored Torch Blue
pub struct ColoredTorchBlue;

impl ItemDef for ColoredTorchBlue {
    const ID: i32 = 9367;
    const STRING_ID: &'static str = "minecraft:colored_torch_blue";
    const NAME: &'static str = "Colored Torch Blue";
    const STACK_SIZE: u8 = 1;
}

/// Cherry Wall Sign
pub struct CherryWallSign;

impl ItemDef for CherryWallSign {
    const ID: i32 = 9376;
    const STRING_ID: &'static str = "minecraft:cherry_wall_sign";
    const NAME: &'static str = "Cherry Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Element 74
pub struct Element74;

impl ItemDef for Element74 {
    const ID: i32 = 9377;
    const STRING_ID: &'static str = "minecraft:element_74";
    const NAME: &'static str = "Element 74";
    const STACK_SIZE: u8 = 1;
}

/// Dead Brain Coral Wall Fan
pub struct DeadBrainCoralWallFan;

impl ItemDef for DeadBrainCoralWallFan {
    const ID: i32 = 9381;
    const STRING_ID: &'static str = "minecraft:dead_brain_coral_wall_fan";
    const NAME: &'static str = "Dead Brain Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// Oxidized Copper Chain
pub struct OxidizedCopperChain;

impl ItemDef for OxidizedCopperChain {
    const ID: i32 = 9389;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_chain";
    const NAME: &'static str = "Oxidized Copper Chain";
    const STACK_SIZE: u8 = 1;
}

/// Nether Wart
pub struct NetherWartItem;

impl ItemDef for NetherWartItem {
    const ID: i32 = 9397;
    const STRING_ID: &'static str = "minecraft:item.nether_wart";
    const NAME: &'static str = "Nether Wart";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Weathered Copper Bars
pub struct WaxedWeatheredCopperBars;

impl ItemDef for WaxedWeatheredCopperBars {
    const ID: i32 = 9416;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_bars";
    const NAME: &'static str = "Waxed Weathered Copper Bars";
    const STACK_SIZE: u8 = 1;
}

/// Dead Fire Coral Wall Fan
pub struct DeadFireCoralWallFan;

impl ItemDef for DeadFireCoralWallFan {
    const ID: i32 = 9417;
    const STRING_ID: &'static str = "minecraft:dead_fire_coral_wall_fan";
    const NAME: &'static str = "Dead Fire Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// Element 97
pub struct Element97;

impl ItemDef for Element97 {
    const ID: i32 = 9421;
    const STRING_ID: &'static str = "minecraft:element_97";
    const NAME: &'static str = "Element 97";
    const STACK_SIZE: u8 = 1;
}

/// Chemistry Table
pub struct ChemistryTable;

impl ItemDef for ChemistryTable {
    const ID: i32 = 9426;
    const STRING_ID: &'static str = "minecraft:chemistry_table";
    const NAME: &'static str = "Chemistry Table";
    const STACK_SIZE: u8 = 1;
}

/// Hard Brown Stained Glass Pane
pub struct HardBrownStainedGlassPane;

impl ItemDef for HardBrownStainedGlassPane {
    const ID: i32 = 9441;
    const STRING_ID: &'static str = "minecraft:hard_brown_stained_glass_pane";
    const NAME: &'static str = "Hard Brown Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Lime Stained Glass Pane
pub struct HardLimeStainedGlassPane;

impl ItemDef for HardLimeStainedGlassPane {
    const ID: i32 = 9447;
    const STRING_ID: &'static str = "minecraft:hard_lime_stained_glass_pane";
    const NAME: &'static str = "Hard Lime Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Element 23
pub struct Element23;

impl ItemDef for Element23 {
    const ID: i32 = 9452;
    const STRING_ID: &'static str = "minecraft:element_23";
    const NAME: &'static str = "Element 23";
    const STACK_SIZE: u8 = 1;
}

/// Coral
pub struct Coral;

impl ItemDef for Coral {
    const ID: i32 = 9456;
    const STRING_ID: &'static str = "minecraft:coral";
    const NAME: &'static str = "Coral";
    const STACK_SIZE: u8 = 1;
}

/// Reserved6
pub struct Reserved6;

impl ItemDef for Reserved6 {
    const ID: i32 = 9470;
    const STRING_ID: &'static str = "minecraft:reserved6";
    const NAME: &'static str = "Reserved6";
    const STACK_SIZE: u8 = 1;
}

/// Shulker Box
pub struct ShulkerBox;

impl ItemDef for ShulkerBox {
    const ID: i32 = 9476;
    const STRING_ID: &'static str = "minecraft:shulker_box";
    const NAME: &'static str = "Shulker Box";
    const STACK_SIZE: u8 = 1;
}

/// Red Candle Cake
pub struct RedCandleCake;

impl ItemDef for RedCandleCake {
    const ID: i32 = 9477;
    const STRING_ID: &'static str = "minecraft:red_candle_cake";
    const NAME: &'static str = "Red Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Deny
pub struct Deny;

impl ItemDef for Deny {
    const ID: i32 = 9479;
    const STRING_ID: &'static str = "minecraft:deny";
    const NAME: &'static str = "Deny";
    const STACK_SIZE: u8 = 1;
}

/// Pale Oak Wall Sign
pub struct PaleOakWallSign;

impl ItemDef for PaleOakWallSign {
    const ID: i32 = 9485;
    const STRING_ID: &'static str = "minecraft:pale_oak_wall_sign";
    const NAME: &'static str = "Pale Oak Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Cake
pub struct CakeItem;

impl ItemDef for CakeItem {
    const ID: i32 = 9490;
    const STRING_ID: &'static str = "minecraft:item.cake";
    const NAME: &'static str = "Cake";
    const STACK_SIZE: u8 = 1;
}

/// Exposed Copper Bars
pub struct ExposedCopperBars;

impl ItemDef for ExposedCopperBars {
    const ID: i32 = 9491;
    const STRING_ID: &'static str = "minecraft:exposed_copper_bars";
    const NAME: &'static str = "Exposed Copper Bars";
    const STACK_SIZE: u8 = 1;
}

/// Andesite Double Slab
pub struct AndesiteDoubleSlab;

impl ItemDef for AndesiteDoubleSlab {
    const ID: i32 = 9498;
    const STRING_ID: &'static str = "minecraft:andesite_double_slab";
    const NAME: &'static str = "Andesite Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Sticky Piston Arm Collision
pub struct StickyPistonArmCollision;

impl ItemDef for StickyPistonArmCollision {
    const ID: i32 = 9501;
    const STRING_ID: &'static str = "minecraft:sticky_piston_arm_collision";
    const NAME: &'static str = "Sticky Piston Arm Collision";
    const STACK_SIZE: u8 = 1;
}

/// Quartz Double Slab
pub struct QuartzDoubleSlab;

impl ItemDef for QuartzDoubleSlab {
    const ID: i32 = 9505;
    const STRING_ID: &'static str = "minecraft:quartz_double_slab";
    const NAME: &'static str = "Quartz Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Element 41
pub struct Element41;

impl ItemDef for Element41 {
    const ID: i32 = 9506;
    const STRING_ID: &'static str = "minecraft:element_41";
    const NAME: &'static str = "Element 41";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 0
pub struct LightBlock0;

impl ItemDef for LightBlock0 {
    const ID: i32 = 9511;
    const STRING_ID: &'static str = "minecraft:light_block_0";
    const NAME: &'static str = "Light Block 0";
    const STACK_SIZE: u8 = 1;
}

/// Stained Glass
pub struct StainedGlass;

impl ItemDef for StainedGlass {
    const ID: i32 = 9519;
    const STRING_ID: &'static str = "minecraft:stained_glass";
    const NAME: &'static str = "Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Flower Pot
pub struct FlowerPotItem;

impl ItemDef for FlowerPotItem {
    const ID: i32 = 9529;
    const STRING_ID: &'static str = "minecraft:item.flower_pot";
    const NAME: &'static str = "Flower Pot";
    const STACK_SIZE: u8 = 1;
}

/// Compound Creator
pub struct CompoundCreator;

impl ItemDef for CompoundCreator {
    const ID: i32 = 9534;
    const STRING_ID: &'static str = "minecraft:compound_creator";
    const NAME: &'static str = "Compound Creator";
    const STACK_SIZE: u8 = 1;
}

/// Camera
pub struct Camera;

impl ItemDef for Camera {
    const ID: i32 = 9536;
    const STRING_ID: &'static str = "minecraft:camera";
    const NAME: &'static str = "Camera";
    const STACK_SIZE: u8 = 1;
}

/// Nether Brick Double Slab
pub struct NetherBrickDoubleSlab;

impl ItemDef for NetherBrickDoubleSlab {
    const ID: i32 = 9555;
    const STRING_ID: &'static str = "minecraft:nether_brick_double_slab";
    const NAME: &'static str = "Nether Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Hard Red Stained Glass
pub struct HardRedStainedGlass;

impl ItemDef for HardRedStainedGlass {
    const ID: i32 = 9559;
    const STRING_ID: &'static str = "minecraft:hard_red_stained_glass";
    const NAME: &'static str = "Hard Red Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Smooth Stone Double Slab
pub struct SmoothStoneDoubleSlab;

impl ItemDef for SmoothStoneDoubleSlab {
    const ID: i32 = 9561;
    const STRING_ID: &'static str = "minecraft:smooth_stone_double_slab";
    const NAME: &'static str = "Smooth Stone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Element 44
pub struct Element44;

impl ItemDef for Element44 {
    const ID: i32 = 9564;
    const STRING_ID: &'static str = "minecraft:element_44";
    const NAME: &'static str = "Element 44";
    const STACK_SIZE: u8 = 1;
}

/// Colored Torch Rg
pub struct ColoredTorchRg;

impl ItemDef for ColoredTorchRg {
    const ID: i32 = 9584;
    const STRING_ID: &'static str = "minecraft:colored_torch_rg";
    const NAME: &'static str = "Colored Torch Rg";
    const STACK_SIZE: u8 = 1;
}

/// Bleach
pub struct Bleach;

impl ItemDef for Bleach {
    const ID: i32 = 9585;
    const STRING_ID: &'static str = "minecraft:bleach";
    const NAME: &'static str = "Bleach";
    const STACK_SIZE: u8 = 1;
}

/// Hard Red Stained Glass Pane
pub struct HardRedStainedGlassPane;

impl ItemDef for HardRedStainedGlassPane {
    const ID: i32 = 9587;
    const STRING_ID: &'static str = "minecraft:hard_red_stained_glass_pane";
    const NAME: &'static str = "Hard Red Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Chalkboard
pub struct Chalkboard;

impl ItemDef for Chalkboard {
    const ID: i32 = 9606;
    const STRING_ID: &'static str = "minecraft:chalkboard";
    const NAME: &'static str = "Chalkboard";
    const STACK_SIZE: u8 = 1;
}

/// Pink Candle Cake
pub struct PinkCandleCake;

impl ItemDef for PinkCandleCake {
    const ID: i32 = 9611;
    const STRING_ID: &'static str = "minecraft:pink_candle_cake";
    const NAME: &'static str = "Pink Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Double Plant
pub struct DoublePlant;

impl ItemDef for DoublePlant {
    const ID: i32 = 9618;
    const STRING_ID: &'static str = "minecraft:double_plant";
    const NAME: &'static str = "Double Plant";
    const STACK_SIZE: u8 = 1;
}

/// Element 109
pub struct Element109;

impl ItemDef for Element109 {
    const ID: i32 = 9621;
    const STRING_ID: &'static str = "minecraft:element_109";
    const NAME: &'static str = "Element 109";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 14
pub struct LightBlock14;

impl ItemDef for LightBlock14 {
    const ID: i32 = 9624;
    const STRING_ID: &'static str = "minecraft:light_block_14";
    const NAME: &'static str = "Light Block 14";
    const STACK_SIZE: u8 = 1;
}

/// Npc Spawn Egg
pub struct NpcSpawnEgg;

impl ItemDef for NpcSpawnEgg {
    const ID: i32 = 9631;
    const STRING_ID: &'static str = "minecraft:npc_spawn_egg";
    const NAME: &'static str = "Npc Spawn Egg";
    const STACK_SIZE: u8 = 1;
}

/// Spruce Wall Sign
pub struct SpruceWallSign;

impl ItemDef for SpruceWallSign {
    const ID: i32 = 9636;
    const STRING_ID: &'static str = "minecraft:spruce_wall_sign";
    const NAME: &'static str = "Spruce Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Daylight Detector Inverted
pub struct DaylightDetectorInverted;

impl ItemDef for DaylightDetectorInverted {
    const ID: i32 = 9641;
    const STRING_ID: &'static str = "minecraft:daylight_detector_inverted";
    const NAME: &'static str = "Daylight Detector Inverted";
    const STACK_SIZE: u8 = 1;
}

/// Diorite Double Slab
pub struct DioriteDoubleSlab;

impl ItemDef for DioriteDoubleSlab {
    const ID: i32 = 9645;
    const STRING_ID: &'static str = "minecraft:diorite_double_slab";
    const NAME: &'static str = "Diorite Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Standing Sign
pub struct StandingSign;

impl ItemDef for StandingSign {
    const ID: i32 = 9649;
    const STRING_ID: &'static str = "minecraft:standing_sign";
    const NAME: &'static str = "Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Normal Stone Double Slab
pub struct NormalStoneDoubleSlab;

impl ItemDef for NormalStoneDoubleSlab {
    const ID: i32 = 9653;
    const STRING_ID: &'static str = "minecraft:normal_stone_double_slab";
    const NAME: &'static str = "Normal Stone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Double Cut Copper Slab
pub struct DoubleCutCopperSlab;

impl ItemDef for DoubleCutCopperSlab {
    const ID: i32 = 9657;
    const STRING_ID: &'static str = "minecraft:double_cut_copper_slab";
    const NAME: &'static str = "Double Cut Copper Slab";
    const STACK_SIZE: u8 = 1;
}

/// Element Constructor
pub struct ElementConstructor;

impl ItemDef for ElementConstructor {
    const ID: i32 = 9659;
    const STRING_ID: &'static str = "minecraft:element_constructor";
    const NAME: &'static str = "Element Constructor";
    const STACK_SIZE: u8 = 1;
}

/// Acacia Door
pub struct AcaciaDoorItem;

impl ItemDef for AcaciaDoorItem {
    const ID: i32 = 9660;
    const STRING_ID: &'static str = "minecraft:item.acacia_door";
    const NAME: &'static str = "Acacia Door";
    const STACK_SIZE: u8 = 1;
}

/// Purpur Double Slab
pub struct PurpurDoubleSlab;

impl ItemDef for PurpurDoubleSlab {
    const ID: i32 = 9665;
    const STRING_ID: &'static str = "minecraft:purpur_double_slab";
    const NAME: &'static str = "Purpur Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Orange Candle Cake
pub struct OrangeCandleCake;

impl ItemDef for OrangeCandleCake {
    const ID: i32 = 9691;
    const STRING_ID: &'static str = "minecraft:orange_candle_cake";
    const NAME: &'static str = "Orange Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Double Stone Block Slab3
pub struct DoubleStoneBlockSlab3;

impl ItemDef for DoubleStoneBlockSlab3 {
    const ID: i32 = 9699;
    const STRING_ID: &'static str = "minecraft:double_stone_block_slab3";
    const NAME: &'static str = "Double Stone Block Slab3";
    const STACK_SIZE: u8 = 1;
}

/// Element 69
pub struct Element69;

impl ItemDef for Element69 {
    const ID: i32 = 9715;
    const STRING_ID: &'static str = "minecraft:element_69";
    const NAME: &'static str = "Element 69";
    const STACK_SIZE: u8 = 1;
}

/// Copper Torch
pub struct CopperTorch;

impl ItemDef for CopperTorch {
    const ID: i32 = 9718;
    const STRING_ID: &'static str = "minecraft:copper_torch";
    const NAME: &'static str = "Copper Torch";
    const STACK_SIZE: u8 = 1;
}

/// Deepslate Brick Double Slab
pub struct DeepslateBrickDoubleSlab;

impl ItemDef for DeepslateBrickDoubleSlab {
    const ID: i32 = 9719;
    const STRING_ID: &'static str = "minecraft:deepslate_brick_double_slab";
    const NAME: &'static str = "Deepslate Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Dark Prismarine Double Slab
pub struct DarkPrismarineDoubleSlab;

impl ItemDef for DarkPrismarineDoubleSlab {
    const ID: i32 = 9721;
    const STRING_ID: &'static str = "minecraft:dark_prismarine_double_slab";
    const NAME: &'static str = "Dark Prismarine Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Copper Chain
pub struct CopperChain;

impl ItemDef for CopperChain {
    const ID: i32 = 9722;
    const STRING_ID: &'static str = "minecraft:copper_chain";
    const NAME: &'static str = "Copper Chain";
    const STACK_SIZE: u8 = 1;
}

/// Element 102
pub struct Element102;

impl ItemDef for Element102 {
    const ID: i32 = 9729;
    const STRING_ID: &'static str = "minecraft:element_102";
    const NAME: &'static str = "Element 102";
    const STACK_SIZE: u8 = 1;
}

/// Colored Torch Bp
pub struct ColoredTorchBp;

impl ItemDef for ColoredTorchBp {
    const ID: i32 = 9730;
    const STRING_ID: &'static str = "minecraft:colored_torch_bp";
    const NAME: &'static str = "Colored Torch Bp";
    const STACK_SIZE: u8 = 1;
}

/// Dead Bubble Coral Wall Fan
pub struct DeadBubbleCoralWallFan;

impl ItemDef for DeadBubbleCoralWallFan {
    const ID: i32 = 9735;
    const STRING_ID: &'static str = "minecraft:dead_bubble_coral_wall_fan";
    const NAME: &'static str = "Dead Bubble Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// Info Update2
pub struct InfoUpdate2;

impl ItemDef for InfoUpdate2 {
    const ID: i32 = 9751;
    const STRING_ID: &'static str = "minecraft:info_update2";
    const NAME: &'static str = "Info Update2";
    const STACK_SIZE: u8 = 1;
}

/// Element 32
pub struct Element32;

impl ItemDef for Element32 {
    const ID: i32 = 9755;
    const STRING_ID: &'static str = "minecraft:element_32";
    const NAME: &'static str = "Element 32";
    const STACK_SIZE: u8 = 1;
}

/// Element 42
pub struct Element42;

impl ItemDef for Element42 {
    const ID: i32 = 9764;
    const STRING_ID: &'static str = "minecraft:element_42";
    const NAME: &'static str = "Element 42";
    const STACK_SIZE: u8 = 1;
}

/// Coral Fan Dead
pub struct CoralFanDead;

impl ItemDef for CoralFanDead {
    const ID: i32 = 9783;
    const STRING_ID: &'static str = "minecraft:coral_fan_dead";
    const NAME: &'static str = "Coral Fan Dead";
    const STACK_SIZE: u8 = 1;
}

/// Cherry Shelf
pub struct CherryShelf;

impl ItemDef for CherryShelf {
    const ID: i32 = 9784;
    const STRING_ID: &'static str = "minecraft:cherry_shelf";
    const NAME: &'static str = "Cherry Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Copper Axe
pub struct CopperAxe;

impl ItemDef for CopperAxe {
    const ID: i32 = 9785;
    const STRING_ID: &'static str = "minecraft:copper_axe";
    const NAME: &'static str = "Copper Axe";
    const STACK_SIZE: u8 = 1;
}

/// Monster Egg
pub struct MonsterEgg;

impl ItemDef for MonsterEgg {
    const ID: i32 = 9792;
    const STRING_ID: &'static str = "minecraft:monster_egg";
    const NAME: &'static str = "Monster Egg";
    const STACK_SIZE: u8 = 1;
}

/// Purple Candle Cake
pub struct PurpleCandleCake;

impl ItemDef for PurpleCandleCake {
    const ID: i32 = 9796;
    const STRING_ID: &'static str = "minecraft:purple_candle_cake";
    const NAME: &'static str = "Purple Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Potatoes
pub struct Potatoes;

impl ItemDef for Potatoes {
    const ID: i32 = 9799;
    const STRING_ID: &'static str = "minecraft:potatoes";
    const NAME: &'static str = "Potatoes";
    const STACK_SIZE: u8 = 1;
}

/// Boat
pub struct Boat;

impl ItemDef for Boat {
    const ID: i32 = 9802;
    const STRING_ID: &'static str = "minecraft:boat";
    const NAME: &'static str = "Boat";
    const STACK_SIZE: u8 = 1;
}

/// Birch Shelf
pub struct BirchShelf;

impl ItemDef for BirchShelf {
    const ID: i32 = 9807;
    const STRING_ID: &'static str = "minecraft:birch_shelf";
    const NAME: &'static str = "Birch Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Blue Candle Cake
pub struct BlueCandleCake;

impl ItemDef for BlueCandleCake {
    const ID: i32 = 9809;
    const STRING_ID: &'static str = "minecraft:blue_candle_cake";
    const NAME: &'static str = "Blue Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Element 22
pub struct Element22;

impl ItemDef for Element22 {
    const ID: i32 = 9810;
    const STRING_ID: &'static str = "minecraft:element_22";
    const NAME: &'static str = "Element 22";
    const STACK_SIZE: u8 = 1;
}

/// Compound
pub struct Compound;

impl ItemDef for Compound {
    const ID: i32 = 9813;
    const STRING_ID: &'static str = "minecraft:compound";
    const NAME: &'static str = "Compound";
    const STACK_SIZE: u8 = 1;
}

/// Ice Bomb
pub struct IceBomb;

impl ItemDef for IceBomb {
    const ID: i32 = 9815;
    const STRING_ID: &'static str = "minecraft:ice_bomb";
    const NAME: &'static str = "Ice Bomb";
    const STACK_SIZE: u8 = 1;
}

/// Medicine
pub struct Medicine;

impl ItemDef for Medicine {
    const ID: i32 = 9816;
    const STRING_ID: &'static str = "minecraft:medicine";
    const NAME: &'static str = "Medicine";
    const STACK_SIZE: u8 = 1;
}

/// Glow Stick
pub struct GlowStick;

impl ItemDef for GlowStick {
    const ID: i32 = 9817;
    const STRING_ID: &'static str = "minecraft:glow_stick";
    const NAME: &'static str = "Glow Stick";
    const STACK_SIZE: u8 = 1;
}

/// Element 83
pub struct Element83;

impl ItemDef for Element83 {
    const ID: i32 = 9818;
    const STRING_ID: &'static str = "minecraft:element_83";
    const NAME: &'static str = "Element 83";
    const STACK_SIZE: u8 = 1;
}

/// Lodestone Compass
pub struct LodestoneCompass;

impl ItemDef for LodestoneCompass {
    const ID: i32 = 9819;
    const STRING_ID: &'static str = "minecraft:lodestone_compass";
    const NAME: &'static str = "Lodestone Compass";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Copper Golem Statue
pub struct WaxedCopperGolemStatue;

impl ItemDef for WaxedCopperGolemStatue {
    const ID: i32 = 9822;
    const STRING_ID: &'static str = "minecraft:waxed_copper_golem_statue";
    const NAME: &'static str = "Waxed Copper Golem Statue";
    const STACK_SIZE: u8 = 1;
}

/// Polished Deepslate Double Slab
pub struct PolishedDeepslateDoubleSlab;

impl ItemDef for PolishedDeepslateDoubleSlab {
    const ID: i32 = 9827;
    const STRING_ID: &'static str = "minecraft:polished_deepslate_double_slab";
    const NAME: &'static str = "Polished Deepslate Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Mossy Cobblestone Double Slab
pub struct MossyCobblestoneDoubleSlab;

impl ItemDef for MossyCobblestoneDoubleSlab {
    const ID: i32 = 9830;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone_double_slab";
    const NAME: &'static str = "Mossy Cobblestone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Concrete
pub struct Concrete;

impl ItemDef for Concrete {
    const ID: i32 = 9837;
    const STRING_ID: &'static str = "minecraft:concrete";
    const NAME: &'static str = "Concrete";
    const STACK_SIZE: u8 = 1;
}

/// Element 33
pub struct Element33;

impl ItemDef for Element33 {
    const ID: i32 = 9852;
    const STRING_ID: &'static str = "minecraft:element_33";
    const NAME: &'static str = "Element 33";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Weathered Double Cut Copper Slab
pub struct WaxedWeatheredDoubleCutCopperSlab;

impl ItemDef for WaxedWeatheredDoubleCutCopperSlab {
    const ID: i32 = 9859;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_double_cut_copper_slab";
    const NAME: &'static str = "Waxed Weathered Double Cut Copper Slab";
    const STACK_SIZE: u8 = 1;
}

/// Jungle Standing Sign
pub struct JungleStandingSign;

impl ItemDef for JungleStandingSign {
    const ID: i32 = 9867;
    const STRING_ID: &'static str = "minecraft:jungle_standing_sign";
    const NAME: &'static str = "Jungle Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Candle Cake
pub struct CandleCake;

impl ItemDef for CandleCake {
    const ID: i32 = 9869;
    const STRING_ID: &'static str = "minecraft:candle_cake";
    const NAME: &'static str = "Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Info Update
pub struct InfoUpdate;

impl ItemDef for InfoUpdate {
    const ID: i32 = 9880;
    const STRING_ID: &'static str = "minecraft:info_update";
    const NAME: &'static str = "Info Update";
    const STACK_SIZE: u8 = 1;
}

/// Chest Boat
pub struct ChestBoat;

impl ItemDef for ChestBoat {
    const ID: i32 = 9886;
    const STRING_ID: &'static str = "minecraft:chest_boat";
    const NAME: &'static str = "Chest Boat";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Exposed Copper Chest
pub struct WaxedExposedCopperChest;

impl ItemDef for WaxedExposedCopperChest {
    const ID: i32 = 9892;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_chest";
    const NAME: &'static str = "Waxed Exposed Copper Chest";
    const STACK_SIZE: u8 = 1;
}

/// Lit Furnace
pub struct LitFurnace;

impl ItemDef for LitFurnace {
    const ID: i32 = 9894;
    const STRING_ID: &'static str = "minecraft:lit_furnace";
    const NAME: &'static str = "Lit Furnace";
    const STACK_SIZE: u8 = 1;
}

/// Element 89
pub struct Element89;

impl ItemDef for Element89 {
    const ID: i32 = 9902;
    const STRING_ID: &'static str = "minecraft:element_89";
    const NAME: &'static str = "Element 89";
    const STACK_SIZE: u8 = 1;
}

/// Crimson Shelf
pub struct CrimsonShelf;

impl ItemDef for CrimsonShelf {
    const ID: i32 = 9904;
    const STRING_ID: &'static str = "minecraft:crimson_shelf";
    const NAME: &'static str = "Crimson Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Double Stone Block Slab
pub struct DoubleStoneBlockSlab;

impl ItemDef for DoubleStoneBlockSlab {
    const ID: i32 = 9915;
    const STRING_ID: &'static str = "minecraft:double_stone_block_slab";
    const NAME: &'static str = "Double Stone Block Slab";
    const STACK_SIZE: u8 = 1;
}

/// Stone Brick Double Slab
pub struct StoneBrickDoubleSlab;

impl ItemDef for StoneBrickDoubleSlab {
    const ID: i32 = 9917;
    const STRING_ID: &'static str = "minecraft:stone_brick_double_slab";
    const NAME: &'static str = "Stone Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Brick Double Slab
pub struct BrickDoubleSlab;

impl ItemDef for BrickDoubleSlab {
    const ID: i32 = 9927;
    const STRING_ID: &'static str = "minecraft:brick_double_slab";
    const NAME: &'static str = "Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Unlit Redstone Torch
pub struct UnlitRedstoneTorch;

impl ItemDef for UnlitRedstoneTorch {
    const ID: i32 = 9929;
    const STRING_ID: &'static str = "minecraft:unlit_redstone_torch";
    const NAME: &'static str = "Unlit Redstone Torch";
    const STACK_SIZE: u8 = 1;
}

/// Element 118
pub struct Element118;

impl ItemDef for Element118 {
    const ID: i32 = 9940;
    const STRING_ID: &'static str = "minecraft:element_118";
    const NAME: &'static str = "Element 118";
    const STACK_SIZE: u8 = 1;
}

/// Element 4
pub struct Element4;

impl ItemDef for Element4 {
    const ID: i32 = 9942;
    const STRING_ID: &'static str = "minecraft:element_4";
    const NAME: &'static str = "Element 4";
    const STACK_SIZE: u8 = 1;
}

/// Weathered Copper Golem Statue
pub struct WeatheredCopperGolemStatue;

impl ItemDef for WeatheredCopperGolemStatue {
    const ID: i32 = 9946;
    const STRING_ID: &'static str = "minecraft:weathered_copper_golem_statue";
    const NAME: &'static str = "Weathered Copper Golem Statue";
    const STACK_SIZE: u8 = 1;
}

/// Wool
pub struct Wool;

impl ItemDef for Wool {
    const ID: i32 = 9947;
    const STRING_ID: &'static str = "minecraft:wool";
    const NAME: &'static str = "Wool";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 10
pub struct LightBlock10;

impl ItemDef for LightBlock10 {
    const ID: i32 = 9952;
    const STRING_ID: &'static str = "minecraft:light_block_10";
    const NAME: &'static str = "Light Block 10";
    const STACK_SIZE: u8 = 1;
}

/// Element 11
pub struct Element11;

impl ItemDef for Element11 {
    const ID: i32 = 9954;
    const STRING_ID: &'static str = "minecraft:element_11";
    const NAME: &'static str = "Element 11";
    const STACK_SIZE: u8 = 1;
}

/// Cobblestone Double Slab
pub struct CobblestoneDoubleSlab;

impl ItemDef for CobblestoneDoubleSlab {
    const ID: i32 = 9956;
    const STRING_ID: &'static str = "minecraft:cobblestone_double_slab";
    const NAME: &'static str = "Cobblestone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Skull
pub struct Skull;

impl ItemDef for Skull {
    const ID: i32 = 9965;
    const STRING_ID: &'static str = "minecraft:skull";
    const NAME: &'static str = "Skull";
    const STACK_SIZE: u8 = 1;
}

/// Copper Nugget
pub struct CopperNugget;

impl ItemDef for CopperNugget {
    const ID: i32 = 9970;
    const STRING_ID: &'static str = "minecraft:copper_nugget";
    const NAME: &'static str = "Copper Nugget";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Oxidized Copper Lantern
pub struct WaxedOxidizedCopperLantern;

impl ItemDef for WaxedOxidizedCopperLantern {
    const ID: i32 = 9973;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_lantern";
    const NAME: &'static str = "Waxed Oxidized Copper Lantern";
    const STACK_SIZE: u8 = 1;
}

/// Copper Horse Armor
pub struct CopperHorseArmor;

impl ItemDef for CopperHorseArmor {
    const ID: i32 = 9975;
    const STRING_ID: &'static str = "minecraft:copper_horse_armor";
    const NAME: &'static str = "Copper Horse Armor";
    const STACK_SIZE: u8 = 1;
}

/// Smooth Red Sandstone Double Slab
pub struct SmoothRedSandstoneDoubleSlab;

impl ItemDef for SmoothRedSandstoneDoubleSlab {
    const ID: i32 = 9977;
    const STRING_ID: &'static str = "minecraft:smooth_red_sandstone_double_slab";
    const NAME: &'static str = "Smooth Red Sandstone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Stained Hardened Clay
pub struct StainedHardenedClay;

impl ItemDef for StainedHardenedClay {
    const ID: i32 = 9985;
    const STRING_ID: &'static str = "minecraft:stained_hardened_clay";
    const NAME: &'static str = "Stained Hardened Clay";
    const STACK_SIZE: u8 = 1;
}

/// Element 9
pub struct Element9;

impl ItemDef for Element9 {
    const ID: i32 = 9990;
    const STRING_ID: &'static str = "minecraft:element_9";
    const NAME: &'static str = "Element 9";
    const STACK_SIZE: u8 = 1;
}

/// Stone Block Slab4
pub struct StoneBlockSlab4;

impl ItemDef for StoneBlockSlab4 {
    const ID: i32 = 10003;
    const STRING_ID: &'static str = "minecraft:stone_block_slab4";
    const NAME: &'static str = "Stone Block Slab4";
    const STACK_SIZE: u8 = 1;
}

/// Double Stone Block Slab2
pub struct DoubleStoneBlockSlab2;

impl ItemDef for DoubleStoneBlockSlab2 {
    const ID: i32 = 10015;
    const STRING_ID: &'static str = "minecraft:double_stone_block_slab2";
    const NAME: &'static str = "Double Stone Block Slab2";
    const STACK_SIZE: u8 = 1;
}

/// Copper Golem Spawn Egg
pub struct CopperGolemSpawnEgg;

impl ItemDef for CopperGolemSpawnEgg {
    const ID: i32 = 10041;
    const STRING_ID: &'static str = "minecraft:copper_golem_spawn_egg";
    const NAME: &'static str = "Copper Golem Spawn Egg";
    const STACK_SIZE: u8 = 1;
}

/// Copper Shovel
pub struct CopperShovel;

impl ItemDef for CopperShovel {
    const ID: i32 = 10042;
    const STRING_ID: &'static str = "minecraft:copper_shovel";
    const NAME: &'static str = "Copper Shovel";
    const STACK_SIZE: u8 = 1;
}

/// Hard Gray Stained Glass
pub struct HardGrayStainedGlass;

impl ItemDef for HardGrayStainedGlass {
    const ID: i32 = 10043;
    const STRING_ID: &'static str = "minecraft:hard_gray_stained_glass";
    const NAME: &'static str = "Hard Gray Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Trip Wire
pub struct TripWire;

impl ItemDef for TripWire {
    const ID: i32 = 10044;
    const STRING_ID: &'static str = "minecraft:trip_wire";
    const NAME: &'static str = "Trip Wire";
    const STACK_SIZE: u8 = 1;
}

/// Copper Pickaxe
pub struct CopperPickaxe;

impl ItemDef for CopperPickaxe {
    const ID: i32 = 10045;
    const STRING_ID: &'static str = "minecraft:copper_pickaxe";
    const NAME: &'static str = "Copper Pickaxe";
    const STACK_SIZE: u8 = 1;
}

/// Cave Vines Body With Berries
pub struct CaveVinesBodyWithBerries;

impl ItemDef for CaveVinesBodyWithBerries {
    const ID: i32 = 10048;
    const STRING_ID: &'static str = "minecraft:cave_vines_body_with_berries";
    const NAME: &'static str = "Cave Vines Body With Berries";
    const STACK_SIZE: u8 = 1;
}

/// Copper Hoe
pub struct CopperHoe;

impl ItemDef for CopperHoe {
    const ID: i32 = 10050;
    const STRING_ID: &'static str = "minecraft:copper_hoe";
    const NAME: &'static str = "Copper Hoe";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 2
pub struct LightBlock2;

impl ItemDef for LightBlock2 {
    const ID: i32 = 10051;
    const STRING_ID: &'static str = "minecraft:light_block_2";
    const NAME: &'static str = "Light Block 2";
    const STACK_SIZE: u8 = 1;
}

/// Copper Helmet
pub struct CopperHelmet;

impl ItemDef for CopperHelmet {
    const ID: i32 = 10052;
    const STRING_ID: &'static str = "minecraft:copper_helmet";
    const NAME: &'static str = "Copper Helmet";
    const STACK_SIZE: u8 = 1;
}

/// Spruce Standing Sign
pub struct SpruceStandingSign;

impl ItemDef for SpruceStandingSign {
    const ID: i32 = 10053;
    const STRING_ID: &'static str = "minecraft:spruce_standing_sign";
    const NAME: &'static str = "Spruce Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Copper Chestplate
pub struct CopperChestplate;

impl ItemDef for CopperChestplate {
    const ID: i32 = 10054;
    const STRING_ID: &'static str = "minecraft:copper_chestplate";
    const NAME: &'static str = "Copper Chestplate";
    const STACK_SIZE: u8 = 1;
}

/// Copper Leggings
pub struct CopperLeggings;

impl ItemDef for CopperLeggings {
    const ID: i32 = 10059;
    const STRING_ID: &'static str = "minecraft:copper_leggings";
    const NAME: &'static str = "Copper Leggings";
    const STACK_SIZE: u8 = 1;
}

/// Double Stone Block Slab4
pub struct DoubleStoneBlockSlab4;

impl ItemDef for DoubleStoneBlockSlab4 {
    const ID: i32 = 10061;
    const STRING_ID: &'static str = "minecraft:double_stone_block_slab4";
    const NAME: &'static str = "Double Stone Block Slab4";
    const STACK_SIZE: u8 = 1;
}

/// Copper Boots
pub struct CopperBoots;

impl ItemDef for CopperBoots {
    const ID: i32 = 10062;
    const STRING_ID: &'static str = "minecraft:copper_boots";
    const NAME: &'static str = "Copper Boots";
    const STACK_SIZE: u8 = 1;
}

/// Prismarine Brick Double Slab
pub struct PrismarineBrickDoubleSlab;

impl ItemDef for PrismarineBrickDoubleSlab {
    const ID: i32 = 10063;
    const STRING_ID: &'static str = "minecraft:prismarine_brick_double_slab";
    const NAME: &'static str = "Prismarine Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Element 18
pub struct Element18;

impl ItemDef for Element18 {
    const ID: i32 = 10074;
    const STRING_ID: &'static str = "minecraft:element_18";
    const NAME: &'static str = "Element 18";
    const STACK_SIZE: u8 = 1;
}

/// Cherry Double Slab
pub struct CherryDoubleSlab;

impl ItemDef for CherryDoubleSlab {
    const ID: i32 = 10076;
    const STRING_ID: &'static str = "minecraft:cherry_double_slab";
    const NAME: &'static str = "Cherry Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Element 29
pub struct Element29;

impl ItemDef for Element29 {
    const ID: i32 = 10078;
    const STRING_ID: &'static str = "minecraft:element_29";
    const NAME: &'static str = "Element 29";
    const STACK_SIZE: u8 = 1;
}

/// Hard Black Stained Glass
pub struct HardBlackStainedGlass;

impl ItemDef for HardBlackStainedGlass {
    const ID: i32 = 10085;
    const STRING_ID: &'static str = "minecraft:hard_black_stained_glass";
    const NAME: &'static str = "Hard Black Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Log
pub struct Log;

impl ItemDef for Log {
    const ID: i32 = 10092;
    const STRING_ID: &'static str = "minecraft:log";
    const NAME: &'static str = "Log";
    const STACK_SIZE: u8 = 1;
}

/// Element 53
pub struct Element53;

impl ItemDef for Element53 {
    const ID: i32 = 10097;
    const STRING_ID: &'static str = "minecraft:element_53";
    const NAME: &'static str = "Element 53";
    const STACK_SIZE: u8 = 1;
}

/// Fence
pub struct Fence;

impl ItemDef for Fence {
    const ID: i32 = 10098;
    const STRING_ID: &'static str = "minecraft:fence";
    const NAME: &'static str = "Fence";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Oxidized Double Cut Copper Slab
pub struct WaxedOxidizedDoubleCutCopperSlab;

impl ItemDef for WaxedOxidizedDoubleCutCopperSlab {
    const ID: i32 = 10099;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_double_cut_copper_slab";
    const NAME: &'static str = "Waxed Oxidized Double Cut Copper Slab";
    const STACK_SIZE: u8 = 1;
}

/// Oxidized Copper Golem Statue
pub struct OxidizedCopperGolemStatue;

impl ItemDef for OxidizedCopperGolemStatue {
    const ID: i32 = 10100;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_golem_statue";
    const NAME: &'static str = "Oxidized Copper Golem Statue";
    const STACK_SIZE: u8 = 1;
}

/// Stonebrick
pub struct Stonebrick;

impl ItemDef for Stonebrick {
    const ID: i32 = 10106;
    const STRING_ID: &'static str = "minecraft:stonebrick";
    const NAME: &'static str = "Stonebrick";
    const STACK_SIZE: u8 = 1;
}

/// Oxidized Copper Lantern
pub struct OxidizedCopperLantern;

impl ItemDef for OxidizedCopperLantern {
    const ID: i32 = 10111;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_lantern";
    const NAME: &'static str = "Oxidized Copper Lantern";
    const STACK_SIZE: u8 = 1;
}

/// Lit Blast Furnace
pub struct LitBlastFurnace;

impl ItemDef for LitBlastFurnace {
    const ID: i32 = 10115;
    const STRING_ID: &'static str = "minecraft:lit_blast_furnace";
    const NAME: &'static str = "Lit Blast Furnace";
    const STACK_SIZE: u8 = 1;
}

/// Coral Block
pub struct CoralBlock;

impl ItemDef for CoralBlock {
    const ID: i32 = 10117;
    const STRING_ID: &'static str = "minecraft:coral_block";
    const NAME: &'static str = "Coral Block";
    const STACK_SIZE: u8 = 1;
}

/// Stone Block Slab
pub struct StoneBlockSlab;

impl ItemDef for StoneBlockSlab {
    const ID: i32 = 10121;
    const STRING_ID: &'static str = "minecraft:stone_block_slab";
    const NAME: &'static str = "Stone Block Slab";
    const STACK_SIZE: u8 = 1;
}

/// Leaves
pub struct Leaves;

impl ItemDef for Leaves {
    const ID: i32 = 10122;
    const STRING_ID: &'static str = "minecraft:leaves";
    const NAME: &'static str = "Leaves";
    const STACK_SIZE: u8 = 1;
}

/// Stone Block Slab2
pub struct StoneBlockSlab2;

impl ItemDef for StoneBlockSlab2 {
    const ID: i32 = 10127;
    const STRING_ID: &'static str = "minecraft:stone_block_slab2";
    const NAME: &'static str = "Stone Block Slab2";
    const STACK_SIZE: u8 = 1;
}

/// Leaves2
pub struct Leaves2;

impl ItemDef for Leaves2 {
    const ID: i32 = 10128;
    const STRING_ID: &'static str = "minecraft:leaves2";
    const NAME: &'static str = "Leaves2";
    const STACK_SIZE: u8 = 1;
}

/// Birch Standing Sign
pub struct BirchStandingSign;

impl ItemDef for BirchStandingSign {
    const ID: i32 = 10139;
    const STRING_ID: &'static str = "minecraft:birch_standing_sign";
    const NAME: &'static str = "Birch Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Stone Block Slab3
pub struct StoneBlockSlab3;

impl ItemDef for StoneBlockSlab3 {
    const ID: i32 = 10142;
    const STRING_ID: &'static str = "minecraft:stone_block_slab3";
    const NAME: &'static str = "Stone Block Slab3";
    const STACK_SIZE: u8 = 1;
}

/// Element 16
pub struct Element16;

impl ItemDef for Element16 {
    const ID: i32 = 10145;
    const STRING_ID: &'static str = "minecraft:element_16";
    const NAME: &'static str = "Element 16";
    const STACK_SIZE: u8 = 1;
}

/// Sandstone Double Slab
pub struct SandstoneDoubleSlab;

impl ItemDef for SandstoneDoubleSlab {
    const ID: i32 = 10147;
    const STRING_ID: &'static str = "minecraft:sandstone_double_slab";
    const NAME: &'static str = "Sandstone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Red Sandstone Double Slab
pub struct RedSandstoneDoubleSlab;

impl ItemDef for RedSandstoneDoubleSlab {
    const ID: i32 = 10148;
    const STRING_ID: &'static str = "minecraft:red_sandstone_double_slab";
    const NAME: &'static str = "Red Sandstone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Prismarine Double Slab
pub struct PrismarineDoubleSlab;

impl ItemDef for PrismarineDoubleSlab {
    const ID: i32 = 10150;
    const STRING_ID: &'static str = "minecraft:prismarine_double_slab";
    const NAME: &'static str = "Prismarine Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Red Nether Brick Double Slab
pub struct RedNetherBrickDoubleSlab;

impl ItemDef for RedNetherBrickDoubleSlab {
    const ID: i32 = 10152;
    const STRING_ID: &'static str = "minecraft:red_nether_brick_double_slab";
    const NAME: &'static str = "Red Nether Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// End Stone Brick Double Slab
pub struct EndStoneBrickDoubleSlab;

impl ItemDef for EndStoneBrickDoubleSlab {
    const ID: i32 = 10154;
    const STRING_ID: &'static str = "minecraft:end_stone_brick_double_slab";
    const NAME: &'static str = "End Stone Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Polished Andesite Double Slab
pub struct PolishedAndesiteDoubleSlab;

impl ItemDef for PolishedAndesiteDoubleSlab {
    const ID: i32 = 10155;
    const STRING_ID: &'static str = "minecraft:polished_andesite_double_slab";
    const NAME: &'static str = "Polished Andesite Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Border Block
pub struct BorderBlock;

impl ItemDef for BorderBlock {
    const ID: i32 = 10156;
    const STRING_ID: &'static str = "minecraft:border_block";
    const NAME: &'static str = "Border Block";
    const STACK_SIZE: u8 = 1;
}

/// Polished Diorite Double Slab
pub struct PolishedDioriteDoubleSlab;

impl ItemDef for PolishedDioriteDoubleSlab {
    const ID: i32 = 10157;
    const STRING_ID: &'static str = "minecraft:polished_diorite_double_slab";
    const NAME: &'static str = "Polished Diorite Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Granite Double Slab
pub struct GraniteDoubleSlab;

impl ItemDef for GraniteDoubleSlab {
    const ID: i32 = 10158;
    const STRING_ID: &'static str = "minecraft:granite_double_slab";
    const NAME: &'static str = "Granite Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Element 10
pub struct Element10;

impl ItemDef for Element10 {
    const ID: i32 = 10159;
    const STRING_ID: &'static str = "minecraft:element_10";
    const NAME: &'static str = "Element 10";
    const STACK_SIZE: u8 = 1;
}

/// Polished Granite Double Slab
pub struct PolishedGraniteDoubleSlab;

impl ItemDef for PolishedGraniteDoubleSlab {
    const ID: i32 = 10160;
    const STRING_ID: &'static str = "minecraft:polished_granite_double_slab";
    const NAME: &'static str = "Polished Granite Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Mossy Stone Brick Double Slab
pub struct MossyStoneBrickDoubleSlab;

impl ItemDef for MossyStoneBrickDoubleSlab {
    const ID: i32 = 10161;
    const STRING_ID: &'static str = "minecraft:mossy_stone_brick_double_slab";
    const NAME: &'static str = "Mossy Stone Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Smooth Quartz Double Slab
pub struct SmoothQuartzDoubleSlab;

impl ItemDef for SmoothQuartzDoubleSlab {
    const ID: i32 = 10164;
    const STRING_ID: &'static str = "minecraft:smooth_quartz_double_slab";
    const NAME: &'static str = "Smooth Quartz Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Cut Sandstone Double Slab
pub struct CutSandstoneDoubleSlab;

impl ItemDef for CutSandstoneDoubleSlab {
    const ID: i32 = 10165;
    const STRING_ID: &'static str = "minecraft:cut_sandstone_double_slab";
    const NAME: &'static str = "Cut Sandstone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Cut Red Sandstone Double Slab
pub struct CutRedSandstoneDoubleSlab;

impl ItemDef for CutRedSandstoneDoubleSlab {
    const ID: i32 = 10167;
    const STRING_ID: &'static str = "minecraft:cut_red_sandstone_double_slab";
    const NAME: &'static str = "Cut Red Sandstone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Sweet Berry Bush
pub struct SweetBerryBush;

impl ItemDef for SweetBerryBush {
    const ID: i32 = 10168;
    const STRING_ID: &'static str = "minecraft:sweet_berry_bush";
    const NAME: &'static str = "Sweet Berry Bush";
    const STACK_SIZE: u8 = 1;
}

/// Coral Fan
pub struct CoralFan;

impl ItemDef for CoralFan {
    const ID: i32 = 10175;
    const STRING_ID: &'static str = "minecraft:coral_fan";
    const NAME: &'static str = "Coral Fan";
    const STACK_SIZE: u8 = 1;
}

/// Sapling
pub struct Sapling;

impl ItemDef for Sapling {
    const ID: i32 = 10183;
    const STRING_ID: &'static str = "minecraft:sapling";
    const NAME: &'static str = "Sapling";
    const STACK_SIZE: u8 = 1;
}

/// Soul Fire
pub struct SoulFire;

impl ItemDef for SoulFire {
    const ID: i32 = 10188;
    const STRING_ID: &'static str = "minecraft:soul_fire";
    const NAME: &'static str = "Soul Fire";
    const STACK_SIZE: u8 = 1;
}

/// Red Flower
pub struct RedFlower;

impl ItemDef for RedFlower {
    const ID: i32 = 10208;
    const STRING_ID: &'static str = "minecraft:red_flower";
    const NAME: &'static str = "Red Flower";
    const STACK_SIZE: u8 = 1;
}

/// Deprecated Purpur Block 1
pub struct DeprecatedPurpurBlock1;

impl ItemDef for DeprecatedPurpurBlock1 {
    const ID: i32 = 10223;
    const STRING_ID: &'static str = "minecraft:deprecated_purpur_block_1";
    const NAME: &'static str = "Deprecated Purpur Block 1";
    const STACK_SIZE: u8 = 1;
}

/// Element 77
pub struct Element77;

impl ItemDef for Element77 {
    const ID: i32 = 10224;
    const STRING_ID: &'static str = "minecraft:element_77";
    const NAME: &'static str = "Element 77";
    const STACK_SIZE: u8 = 1;
}

/// Deprecated Purpur Block 2
pub struct DeprecatedPurpurBlock2;

impl ItemDef for DeprecatedPurpurBlock2 {
    const ID: i32 = 10226;
    const STRING_ID: &'static str = "minecraft:deprecated_purpur_block_2";
    const NAME: &'static str = "Deprecated Purpur Block 2";
    const STACK_SIZE: u8 = 1;
}

/// Tallgrass
pub struct Tallgrass;

impl ItemDef for Tallgrass {
    const ID: i32 = 10237;
    const STRING_ID: &'static str = "minecraft:tallgrass";
    const NAME: &'static str = "Tallgrass";
    const STACK_SIZE: u8 = 1;
}

/// Element 103
pub struct Element103;

impl ItemDef for Element103 {
    const ID: i32 = 10238;
    const STRING_ID: &'static str = "minecraft:element_103";
    const NAME: &'static str = "Element 103";
    const STACK_SIZE: u8 = 1;
}

/// Log2
pub struct Log2;

impl ItemDef for Log2 {
    const ID: i32 = 10242;
    const STRING_ID: &'static str = "minecraft:log2";
    const NAME: &'static str = "Log2";
    const STACK_SIZE: u8 = 1;
}

/// Deprecated Anvil
pub struct DeprecatedAnvil;

impl ItemDef for DeprecatedAnvil {
    const ID: i32 = 10248;
    const STRING_ID: &'static str = "minecraft:deprecated_anvil";
    const NAME: &'static str = "Deprecated Anvil";
    const STACK_SIZE: u8 = 1;
}

/// Element 56
pub struct Element56;

impl ItemDef for Element56 {
    const ID: i32 = 10257;
    const STRING_ID: &'static str = "minecraft:element_56";
    const NAME: &'static str = "Element 56";
    const STACK_SIZE: u8 = 1;
}

/// Concrete Powder
pub struct ConcretePowder;

impl ItemDef for ConcretePowder {
    const ID: i32 = 10275;
    const STRING_ID: &'static str = "minecraft:concrete_powder";
    const NAME: &'static str = "Concrete Powder";
    const STACK_SIZE: u8 = 1;
}

/// Element 75
pub struct Element75;

impl ItemDef for Element75 {
    const ID: i32 = 10276;
    const STRING_ID: &'static str = "minecraft:element_75";
    const NAME: &'static str = "Element 75";
    const STACK_SIZE: u8 = 1;
}

/// Element 64
pub struct Element64;

impl ItemDef for Element64 {
    const ID: i32 = 10278;
    const STRING_ID: &'static str = "minecraft:element_64";
    const NAME: &'static str = "Element 64";
    const STACK_SIZE: u8 = 1;
}

/// Hopper
pub struct HopperItem;

impl ItemDef for HopperItem {
    const ID: i32 = 10282;
    const STRING_ID: &'static str = "minecraft:item.hopper";
    const NAME: &'static str = "Hopper";
    const STACK_SIZE: u8 = 1;
}

/// Wood
pub struct Wood;

impl ItemDef for Wood {
    const ID: i32 = 10283;
    const STRING_ID: &'static str = "minecraft:wood";
    const NAME: &'static str = "Wood";
    const STACK_SIZE: u8 = 1;
}

/// Hard Magenta Stained Glass
pub struct HardMagentaStainedGlass;

impl ItemDef for HardMagentaStainedGlass {
    const ID: i32 = 10284;
    const STRING_ID: &'static str = "minecraft:hard_magenta_stained_glass";
    const NAME: &'static str = "Hard Magenta Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Mud Brick Double Slab
pub struct MudBrickDoubleSlab;

impl ItemDef for MudBrickDoubleSlab {
    const ID: i32 = 10285;
    const STRING_ID: &'static str = "minecraft:mud_brick_double_slab";
    const NAME: &'static str = "Mud Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Crimson Double Slab
pub struct CrimsonDoubleSlab;

impl ItemDef for CrimsonDoubleSlab {
    const ID: i32 = 10287;
    const STRING_ID: &'static str = "minecraft:crimson_double_slab";
    const NAME: &'static str = "Crimson Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Hard Purple Stained Glass
pub struct HardPurpleStainedGlass;

impl ItemDef for HardPurpleStainedGlass {
    const ID: i32 = 10288;
    const STRING_ID: &'static str = "minecraft:hard_purple_stained_glass";
    const NAME: &'static str = "Hard Purple Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Dark Oak Door
pub struct DarkOakDoorItem;

impl ItemDef for DarkOakDoorItem {
    const ID: i32 = 10295;
    const STRING_ID: &'static str = "minecraft:item.dark_oak_door";
    const NAME: &'static str = "Dark Oak Door";
    const STACK_SIZE: u8 = 1;
}

/// Hard Green Stained Glass Pane
pub struct HardGreenStainedGlassPane;

impl ItemDef for HardGreenStainedGlassPane {
    const ID: i32 = 10303;
    const STRING_ID: &'static str = "minecraft:hard_green_stained_glass_pane";
    const NAME: &'static str = "Hard Green Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Torchflower Crop
pub struct TorchflowerCrop;

impl ItemDef for TorchflowerCrop {
    const ID: i32 = 10316;
    const STRING_ID: &'static str = "minecraft:torchflower_crop";
    const NAME: &'static str = "Torchflower Crop";
    const STACK_SIZE: u8 = 1;
}

/// Material Reducer
pub struct MaterialReducer;

impl ItemDef for MaterialReducer {
    const ID: i32 = 10331;
    const STRING_ID: &'static str = "minecraft:material_reducer";
    const NAME: &'static str = "Material Reducer";
    const STACK_SIZE: u8 = 1;
}

/// Lab Table
pub struct LabTable;

impl ItemDef for LabTable {
    const ID: i32 = 10332;
    const STRING_ID: &'static str = "minecraft:lab_table";
    const NAME: &'static str = "Lab Table";
    const STACK_SIZE: u8 = 1;
}

/// Hard White Stained Glass
pub struct HardWhiteStainedGlass;

impl ItemDef for HardWhiteStainedGlass {
    const ID: i32 = 10333;
    const STRING_ID: &'static str = "minecraft:hard_white_stained_glass";
    const NAME: &'static str = "Hard White Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Hard Orange Stained Glass
pub struct HardOrangeStainedGlass;

impl ItemDef for HardOrangeStainedGlass {
    const ID: i32 = 10335;
    const STRING_ID: &'static str = "minecraft:hard_orange_stained_glass";
    const NAME: &'static str = "Hard Orange Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Hard Light Blue Stained Glass
pub struct HardLightBlueStainedGlass;

impl ItemDef for HardLightBlueStainedGlass {
    const ID: i32 = 10337;
    const STRING_ID: &'static str = "minecraft:hard_light_blue_stained_glass";
    const NAME: &'static str = "Hard Light Blue Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Hard Yellow Stained Glass
pub struct HardYellowStainedGlass;

impl ItemDef for HardYellowStainedGlass {
    const ID: i32 = 10338;
    const STRING_ID: &'static str = "minecraft:hard_yellow_stained_glass";
    const NAME: &'static str = "Hard Yellow Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Hard Lime Stained Glass
pub struct HardLimeStainedGlass;

impl ItemDef for HardLimeStainedGlass {
    const ID: i32 = 10339;
    const STRING_ID: &'static str = "minecraft:hard_lime_stained_glass";
    const NAME: &'static str = "Hard Lime Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Hard Light Gray Stained Glass
pub struct HardLightGrayStainedGlass;

impl ItemDef for HardLightGrayStainedGlass {
    const ID: i32 = 10340;
    const STRING_ID: &'static str = "minecraft:hard_light_gray_stained_glass";
    const NAME: &'static str = "Hard Light Gray Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Hard Green Stained Glass
pub struct HardGreenStainedGlass;

impl ItemDef for HardGreenStainedGlass {
    const ID: i32 = 10341;
    const STRING_ID: &'static str = "minecraft:hard_green_stained_glass";
    const NAME: &'static str = "Hard Green Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Element 84
pub struct Element84;

impl ItemDef for Element84 {
    const ID: i32 = 10342;
    const STRING_ID: &'static str = "minecraft:element_84";
    const NAME: &'static str = "Element 84";
    const STACK_SIZE: u8 = 1;
}

/// Hard Stained Glass
pub struct HardStainedGlass;

impl ItemDef for HardStainedGlass {
    const ID: i32 = 10343;
    const STRING_ID: &'static str = "minecraft:hard_stained_glass";
    const NAME: &'static str = "Hard Stained Glass";
    const STACK_SIZE: u8 = 1;
}

/// Hard White Stained Glass Pane
pub struct HardWhiteStainedGlassPane;

impl ItemDef for HardWhiteStainedGlassPane {
    const ID: i32 = 10344;
    const STRING_ID: &'static str = "minecraft:hard_white_stained_glass_pane";
    const NAME: &'static str = "Hard White Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Orange Stained Glass Pane
pub struct HardOrangeStainedGlassPane;

impl ItemDef for HardOrangeStainedGlassPane {
    const ID: i32 = 10345;
    const STRING_ID: &'static str = "minecraft:hard_orange_stained_glass_pane";
    const NAME: &'static str = "Hard Orange Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Magenta Stained Glass Pane
pub struct HardMagentaStainedGlassPane;

impl ItemDef for HardMagentaStainedGlassPane {
    const ID: i32 = 10346;
    const STRING_ID: &'static str = "minecraft:hard_magenta_stained_glass_pane";
    const NAME: &'static str = "Hard Magenta Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Light Blue Stained Glass Pane
pub struct HardLightBlueStainedGlassPane;

impl ItemDef for HardLightBlueStainedGlassPane {
    const ID: i32 = 10347;
    const STRING_ID: &'static str = "minecraft:hard_light_blue_stained_glass_pane";
    const NAME: &'static str = "Hard Light Blue Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Yellow Stained Glass Pane
pub struct HardYellowStainedGlassPane;

impl ItemDef for HardYellowStainedGlassPane {
    const ID: i32 = 10348;
    const STRING_ID: &'static str = "minecraft:hard_yellow_stained_glass_pane";
    const NAME: &'static str = "Hard Yellow Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Pink Stained Glass Pane
pub struct HardPinkStainedGlassPane;

impl ItemDef for HardPinkStainedGlassPane {
    const ID: i32 = 10349;
    const STRING_ID: &'static str = "minecraft:hard_pink_stained_glass_pane";
    const NAME: &'static str = "Hard Pink Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Carrots
pub struct Carrots;

impl ItemDef for Carrots {
    const ID: i32 = 10350;
    const STRING_ID: &'static str = "minecraft:carrots";
    const NAME: &'static str = "Carrots";
    const STACK_SIZE: u8 = 1;
}

/// Hard Gray Stained Glass Pane
pub struct HardGrayStainedGlassPane;

impl ItemDef for HardGrayStainedGlassPane {
    const ID: i32 = 10351;
    const STRING_ID: &'static str = "minecraft:hard_gray_stained_glass_pane";
    const NAME: &'static str = "Hard Gray Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Light Gray Stained Glass Pane
pub struct HardLightGrayStainedGlassPane;

impl ItemDef for HardLightGrayStainedGlassPane {
    const ID: i32 = 10352;
    const STRING_ID: &'static str = "minecraft:hard_light_gray_stained_glass_pane";
    const NAME: &'static str = "Hard Light Gray Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Purple Stained Glass Pane
pub struct HardPurpleStainedGlassPane;

impl ItemDef for HardPurpleStainedGlassPane {
    const ID: i32 = 10353;
    const STRING_ID: &'static str = "minecraft:hard_purple_stained_glass_pane";
    const NAME: &'static str = "Hard Purple Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Blue Stained Glass Pane
pub struct HardBlueStainedGlassPane;

impl ItemDef for HardBlueStainedGlassPane {
    const ID: i32 = 10354;
    const STRING_ID: &'static str = "minecraft:hard_blue_stained_glass_pane";
    const NAME: &'static str = "Hard Blue Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Black Stained Glass Pane
pub struct HardBlackStainedGlassPane;

impl ItemDef for HardBlackStainedGlassPane {
    const ID: i32 = 10355;
    const STRING_ID: &'static str = "minecraft:hard_black_stained_glass_pane";
    const NAME: &'static str = "Hard Black Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Hard Stained Glass Pane
pub struct HardStainedGlassPane;

impl ItemDef for HardStainedGlassPane {
    const ID: i32 = 10356;
    const STRING_ID: &'static str = "minecraft:hard_stained_glass_pane";
    const NAME: &'static str = "Hard Stained Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Colored Torch Red
pub struct ColoredTorchRed;

impl ItemDef for ColoredTorchRed {
    const ID: i32 = 10358;
    const STRING_ID: &'static str = "minecraft:colored_torch_red";
    const NAME: &'static str = "Colored Torch Red";
    const STACK_SIZE: u8 = 1;
}

/// Colored Torch Green
pub struct ColoredTorchGreen;

impl ItemDef for ColoredTorchGreen {
    const ID: i32 = 10360;
    const STRING_ID: &'static str = "minecraft:colored_torch_green";
    const NAME: &'static str = "Colored Torch Green";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 1
pub struct LightBlock1;

impl ItemDef for LightBlock1 {
    const ID: i32 = 10361;
    const STRING_ID: &'static str = "minecraft:light_block_1";
    const NAME: &'static str = "Light Block 1";
    const STACK_SIZE: u8 = 1;
}

/// Mangrove Shelf
pub struct MangroveShelf;

impl ItemDef for MangroveShelf {
    const ID: i32 = 10362;
    const STRING_ID: &'static str = "minecraft:mangrove_shelf";
    const NAME: &'static str = "Mangrove Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 3
pub struct LightBlock3;

impl ItemDef for LightBlock3 {
    const ID: i32 = 10363;
    const STRING_ID: &'static str = "minecraft:light_block_3";
    const NAME: &'static str = "Light Block 3";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 4
pub struct LightBlock4;

impl ItemDef for LightBlock4 {
    const ID: i32 = 10364;
    const STRING_ID: &'static str = "minecraft:light_block_4";
    const NAME: &'static str = "Light Block 4";
    const STACK_SIZE: u8 = 1;
}

/// Gray Candle Cake
pub struct GrayCandleCake;

impl ItemDef for GrayCandleCake {
    const ID: i32 = 10365;
    const STRING_ID: &'static str = "minecraft:gray_candle_cake";
    const NAME: &'static str = "Gray Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 5
pub struct LightBlock5;

impl ItemDef for LightBlock5 {
    const ID: i32 = 10366;
    const STRING_ID: &'static str = "minecraft:light_block_5";
    const NAME: &'static str = "Light Block 5";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 6
pub struct LightBlock6;

impl ItemDef for LightBlock6 {
    const ID: i32 = 10367;
    const STRING_ID: &'static str = "minecraft:light_block_6";
    const NAME: &'static str = "Light Block 6";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 7
pub struct LightBlock7;

impl ItemDef for LightBlock7 {
    const ID: i32 = 10370;
    const STRING_ID: &'static str = "minecraft:light_block_7";
    const NAME: &'static str = "Light Block 7";
    const STACK_SIZE: u8 = 1;
}

/// Deepslate Tile Double Slab
pub struct DeepslateTileDoubleSlab;

impl ItemDef for DeepslateTileDoubleSlab {
    const ID: i32 = 10371;
    const STRING_ID: &'static str = "minecraft:deepslate_tile_double_slab";
    const NAME: &'static str = "Deepslate Tile Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 8
pub struct LightBlock8;

impl ItemDef for LightBlock8 {
    const ID: i32 = 10372;
    const STRING_ID: &'static str = "minecraft:light_block_8";
    const NAME: &'static str = "Light Block 8";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 9
pub struct LightBlock9;

impl ItemDef for LightBlock9 {
    const ID: i32 = 10373;
    const STRING_ID: &'static str = "minecraft:light_block_9";
    const NAME: &'static str = "Light Block 9";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Oxidized Copper Bars
pub struct WaxedOxidizedCopperBars;

impl ItemDef for WaxedOxidizedCopperBars {
    const ID: i32 = 10374;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_bars";
    const NAME: &'static str = "Waxed Oxidized Copper Bars";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 11
pub struct LightBlock11;

impl ItemDef for LightBlock11 {
    const ID: i32 = 10375;
    const STRING_ID: &'static str = "minecraft:light_block_11";
    const NAME: &'static str = "Light Block 11";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 12
pub struct LightBlock12;

impl ItemDef for LightBlock12 {
    const ID: i32 = 10377;
    const STRING_ID: &'static str = "minecraft:light_block_12";
    const NAME: &'static str = "Light Block 12";
    const STACK_SIZE: u8 = 1;
}

/// Light Block 13
pub struct LightBlock13;

impl ItemDef for LightBlock13 {
    const ID: i32 = 10378;
    const STRING_ID: &'static str = "minecraft:light_block_13";
    const NAME: &'static str = "Light Block 13";
    const STACK_SIZE: u8 = 1;
}

/// Fire
pub struct Fire;

impl ItemDef for Fire {
    const ID: i32 = 10384;
    const STRING_ID: &'static str = "minecraft:fire";
    const NAME: &'static str = "Fire";
    const STACK_SIZE: u8 = 1;
}

/// Black Candle Cake
pub struct BlackCandleCake;

impl ItemDef for BlackCandleCake {
    const ID: i32 = 10387;
    const STRING_ID: &'static str = "minecraft:black_candle_cake";
    const NAME: &'static str = "Black Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Element 0
pub struct Element0;

impl ItemDef for Element0 {
    const ID: i32 = 10394;
    const STRING_ID: &'static str = "minecraft:element_0";
    const NAME: &'static str = "Element 0";
    const STACK_SIZE: u8 = 1;
}

/// Element 1
pub struct Element1;

impl ItemDef for Element1 {
    const ID: i32 = 10395;
    const STRING_ID: &'static str = "minecraft:element_1";
    const NAME: &'static str = "Element 1";
    const STACK_SIZE: u8 = 1;
}

/// Element 3
pub struct Element3;

impl ItemDef for Element3 {
    const ID: i32 = 10396;
    const STRING_ID: &'static str = "minecraft:element_3";
    const NAME: &'static str = "Element 3";
    const STACK_SIZE: u8 = 1;
}

/// Element 5
pub struct Element5;

impl ItemDef for Element5 {
    const ID: i32 = 10397;
    const STRING_ID: &'static str = "minecraft:element_5";
    const NAME: &'static str = "Element 5";
    const STACK_SIZE: u8 = 1;
}

/// Element 6
pub struct Element6;

impl ItemDef for Element6 {
    const ID: i32 = 10398;
    const STRING_ID: &'static str = "minecraft:element_6";
    const NAME: &'static str = "Element 6";
    const STACK_SIZE: u8 = 1;
}

/// Element 7
pub struct Element7;

impl ItemDef for Element7 {
    const ID: i32 = 10400;
    const STRING_ID: &'static str = "minecraft:element_7";
    const NAME: &'static str = "Element 7";
    const STACK_SIZE: u8 = 1;
}

/// Element 8
pub struct Element8;

impl ItemDef for Element8 {
    const ID: i32 = 10401;
    const STRING_ID: &'static str = "minecraft:element_8";
    const NAME: &'static str = "Element 8";
    const STACK_SIZE: u8 = 1;
}

/// Element 12
pub struct Element12;

impl ItemDef for Element12 {
    const ID: i32 = 10402;
    const STRING_ID: &'static str = "minecraft:element_12";
    const NAME: &'static str = "Element 12";
    const STACK_SIZE: u8 = 1;
}

/// Element 14
pub struct Element14;

impl ItemDef for Element14 {
    const ID: i32 = 10403;
    const STRING_ID: &'static str = "minecraft:element_14";
    const NAME: &'static str = "Element 14";
    const STACK_SIZE: u8 = 1;
}

/// Pale Oak Standing Sign
pub struct PaleOakStandingSign;

impl ItemDef for PaleOakStandingSign {
    const ID: i32 = 10404;
    const STRING_ID: &'static str = "minecraft:pale_oak_standing_sign";
    const NAME: &'static str = "Pale Oak Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Client Request Placeholder Block
pub struct ClientRequestPlaceholderBlock;

impl ItemDef for ClientRequestPlaceholderBlock {
    const ID: i32 = 10405;
    const STRING_ID: &'static str = "minecraft:client_request_placeholder_block";
    const NAME: &'static str = "Client Request Placeholder Block";
    const STACK_SIZE: u8 = 1;
}

/// Element 17
pub struct Element17;

impl ItemDef for Element17 {
    const ID: i32 = 10406;
    const STRING_ID: &'static str = "minecraft:element_17";
    const NAME: &'static str = "Element 17";
    const STACK_SIZE: u8 = 1;
}

/// Element 19
pub struct Element19;

impl ItemDef for Element19 {
    const ID: i32 = 10407;
    const STRING_ID: &'static str = "minecraft:element_19";
    const NAME: &'static str = "Element 19";
    const STACK_SIZE: u8 = 1;
}

/// Element 20
pub struct Element20;

impl ItemDef for Element20 {
    const ID: i32 = 10409;
    const STRING_ID: &'static str = "minecraft:element_20";
    const NAME: &'static str = "Element 20";
    const STACK_SIZE: u8 = 1;
}

/// Element 21
pub struct Element21;

impl ItemDef for Element21 {
    const ID: i32 = 10410;
    const STRING_ID: &'static str = "minecraft:element_21";
    const NAME: &'static str = "Element 21";
    const STACK_SIZE: u8 = 1;
}

/// Element 24
pub struct Element24;

impl ItemDef for Element24 {
    const ID: i32 = 10411;
    const STRING_ID: &'static str = "minecraft:element_24";
    const NAME: &'static str = "Element 24";
    const STACK_SIZE: u8 = 1;
}

/// Element 25
pub struct Element25;

impl ItemDef for Element25 {
    const ID: i32 = 10412;
    const STRING_ID: &'static str = "minecraft:element_25";
    const NAME: &'static str = "Element 25";
    const STACK_SIZE: u8 = 1;
}

/// Element 26
pub struct Element26;

impl ItemDef for Element26 {
    const ID: i32 = 10413;
    const STRING_ID: &'static str = "minecraft:element_26";
    const NAME: &'static str = "Element 26";
    const STACK_SIZE: u8 = 1;
}

/// Element 28
pub struct Element28;

impl ItemDef for Element28 {
    const ID: i32 = 10414;
    const STRING_ID: &'static str = "minecraft:element_28";
    const NAME: &'static str = "Element 28";
    const STACK_SIZE: u8 = 1;
}

/// Element 30
pub struct Element30;

impl ItemDef for Element30 {
    const ID: i32 = 10415;
    const STRING_ID: &'static str = "minecraft:element_30";
    const NAME: &'static str = "Element 30";
    const STACK_SIZE: u8 = 1;
}

/// Element 31
pub struct Element31;

impl ItemDef for Element31 {
    const ID: i32 = 10416;
    const STRING_ID: &'static str = "minecraft:element_31";
    const NAME: &'static str = "Element 31";
    const STACK_SIZE: u8 = 1;
}

/// Element 34
pub struct Element34;

impl ItemDef for Element34 {
    const ID: i32 = 10417;
    const STRING_ID: &'static str = "minecraft:element_34";
    const NAME: &'static str = "Element 34";
    const STACK_SIZE: u8 = 1;
}

/// Bamboo Double Slab
pub struct BambooDoubleSlab;

impl ItemDef for BambooDoubleSlab {
    const ID: i32 = 10418;
    const STRING_ID: &'static str = "minecraft:bamboo_double_slab";
    const NAME: &'static str = "Bamboo Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Element 35
pub struct Element35;

impl ItemDef for Element35 {
    const ID: i32 = 10419;
    const STRING_ID: &'static str = "minecraft:element_35";
    const NAME: &'static str = "Element 35";
    const STACK_SIZE: u8 = 1;
}

/// Element 36
pub struct Element36;

impl ItemDef for Element36 {
    const ID: i32 = 10420;
    const STRING_ID: &'static str = "minecraft:element_36";
    const NAME: &'static str = "Element 36";
    const STACK_SIZE: u8 = 1;
}

/// Element 37
pub struct Element37;

impl ItemDef for Element37 {
    const ID: i32 = 10421;
    const STRING_ID: &'static str = "minecraft:element_37";
    const NAME: &'static str = "Element 37";
    const STACK_SIZE: u8 = 1;
}

/// Element 38
pub struct Element38;

impl ItemDef for Element38 {
    const ID: i32 = 10422;
    const STRING_ID: &'static str = "minecraft:element_38";
    const NAME: &'static str = "Element 38";
    const STACK_SIZE: u8 = 1;
}

/// Element 39
pub struct Element39;

impl ItemDef for Element39 {
    const ID: i32 = 10423;
    const STRING_ID: &'static str = "minecraft:element_39";
    const NAME: &'static str = "Element 39";
    const STACK_SIZE: u8 = 1;
}

/// Element 40
pub struct Element40;

impl ItemDef for Element40 {
    const ID: i32 = 10424;
    const STRING_ID: &'static str = "minecraft:element_40";
    const NAME: &'static str = "Element 40";
    const STACK_SIZE: u8 = 1;
}

/// Element 45
pub struct Element45;

impl ItemDef for Element45 {
    const ID: i32 = 10425;
    const STRING_ID: &'static str = "minecraft:element_45";
    const NAME: &'static str = "Element 45";
    const STACK_SIZE: u8 = 1;
}

/// Element 46
pub struct Element46;

impl ItemDef for Element46 {
    const ID: i32 = 10426;
    const STRING_ID: &'static str = "minecraft:element_46";
    const NAME: &'static str = "Element 46";
    const STACK_SIZE: u8 = 1;
}

/// Element 47
pub struct Element47;

impl ItemDef for Element47 {
    const ID: i32 = 10427;
    const STRING_ID: &'static str = "minecraft:element_47";
    const NAME: &'static str = "Element 47";
    const STACK_SIZE: u8 = 1;
}

/// Element 48
pub struct Element48;

impl ItemDef for Element48 {
    const ID: i32 = 10428;
    const STRING_ID: &'static str = "minecraft:element_48";
    const NAME: &'static str = "Element 48";
    const STACK_SIZE: u8 = 1;
}

/// Element 49
pub struct Element49;

impl ItemDef for Element49 {
    const ID: i32 = 10430;
    const STRING_ID: &'static str = "minecraft:element_49";
    const NAME: &'static str = "Element 49";
    const STACK_SIZE: u8 = 1;
}

/// Element 54
pub struct Element54;

impl ItemDef for Element54 {
    const ID: i32 = 10431;
    const STRING_ID: &'static str = "minecraft:element_54";
    const NAME: &'static str = "Element 54";
    const STACK_SIZE: u8 = 1;
}

/// Element 55
pub struct Element55;

impl ItemDef for Element55 {
    const ID: i32 = 10432;
    const STRING_ID: &'static str = "minecraft:element_55";
    const NAME: &'static str = "Element 55";
    const STACK_SIZE: u8 = 1;
}

/// Element 57
pub struct Element57;

impl ItemDef for Element57 {
    const ID: i32 = 10434;
    const STRING_ID: &'static str = "minecraft:element_57";
    const NAME: &'static str = "Element 57";
    const STACK_SIZE: u8 = 1;
}

/// Element 58
pub struct Element58;

impl ItemDef for Element58 {
    const ID: i32 = 10435;
    const STRING_ID: &'static str = "minecraft:element_58";
    const NAME: &'static str = "Element 58";
    const STACK_SIZE: u8 = 1;
}

/// Element 59
pub struct Element59;

impl ItemDef for Element59 {
    const ID: i32 = 10436;
    const STRING_ID: &'static str = "minecraft:element_59";
    const NAME: &'static str = "Element 59";
    const STACK_SIZE: u8 = 1;
}

/// Element 60
pub struct Element60;

impl ItemDef for Element60 {
    const ID: i32 = 10437;
    const STRING_ID: &'static str = "minecraft:element_60";
    const NAME: &'static str = "Element 60";
    const STACK_SIZE: u8 = 1;
}

/// Element 61
pub struct Element61;

impl ItemDef for Element61 {
    const ID: i32 = 10438;
    const STRING_ID: &'static str = "minecraft:element_61";
    const NAME: &'static str = "Element 61";
    const STACK_SIZE: u8 = 1;
}

/// Element 63
pub struct Element63;

impl ItemDef for Element63 {
    const ID: i32 = 10439;
    const STRING_ID: &'static str = "minecraft:element_63";
    const NAME: &'static str = "Element 63";
    const STACK_SIZE: u8 = 1;
}

/// Element 65
pub struct Element65;

impl ItemDef for Element65 {
    const ID: i32 = 10440;
    const STRING_ID: &'static str = "minecraft:element_65";
    const NAME: &'static str = "Element 65";
    const STACK_SIZE: u8 = 1;
}

/// Element 66
pub struct Element66;

impl ItemDef for Element66 {
    const ID: i32 = 10441;
    const STRING_ID: &'static str = "minecraft:element_66";
    const NAME: &'static str = "Element 66";
    const STACK_SIZE: u8 = 1;
}

/// Element 67
pub struct Element67;

impl ItemDef for Element67 {
    const ID: i32 = 10442;
    const STRING_ID: &'static str = "minecraft:element_67";
    const NAME: &'static str = "Element 67";
    const STACK_SIZE: u8 = 1;
}

/// Element 70
pub struct Element70;

impl ItemDef for Element70 {
    const ID: i32 = 10443;
    const STRING_ID: &'static str = "minecraft:element_70";
    const NAME: &'static str = "Element 70";
    const STACK_SIZE: u8 = 1;
}

/// Element 71
pub struct Element71;

impl ItemDef for Element71 {
    const ID: i32 = 10444;
    const STRING_ID: &'static str = "minecraft:element_71";
    const NAME: &'static str = "Element 71";
    const STACK_SIZE: u8 = 1;
}

/// Element 72
pub struct Element72;

impl ItemDef for Element72 {
    const ID: i32 = 10445;
    const STRING_ID: &'static str = "minecraft:element_72";
    const NAME: &'static str = "Element 72";
    const STACK_SIZE: u8 = 1;
}

/// Element 73
pub struct Element73;

impl ItemDef for Element73 {
    const ID: i32 = 10446;
    const STRING_ID: &'static str = "minecraft:element_73";
    const NAME: &'static str = "Element 73";
    const STACK_SIZE: u8 = 1;
}

/// Element 76
pub struct Element76;

impl ItemDef for Element76 {
    const ID: i32 = 10448;
    const STRING_ID: &'static str = "minecraft:element_76";
    const NAME: &'static str = "Element 76";
    const STACK_SIZE: u8 = 1;
}

/// Element 78
pub struct Element78;

impl ItemDef for Element78 {
    const ID: i32 = 10449;
    const STRING_ID: &'static str = "minecraft:element_78";
    const NAME: &'static str = "Element 78";
    const STACK_SIZE: u8 = 1;
}

/// Element 79
pub struct Element79;

impl ItemDef for Element79 {
    const ID: i32 = 10452;
    const STRING_ID: &'static str = "minecraft:element_79";
    const NAME: &'static str = "Element 79";
    const STACK_SIZE: u8 = 1;
}

/// Weathered Copper Bars
pub struct WeatheredCopperBars;

impl ItemDef for WeatheredCopperBars {
    const ID: i32 = 10453;
    const STRING_ID: &'static str = "minecraft:weathered_copper_bars";
    const NAME: &'static str = "Weathered Copper Bars";
    const STACK_SIZE: u8 = 1;
}

/// Element 81
pub struct Element81;

impl ItemDef for Element81 {
    const ID: i32 = 10454;
    const STRING_ID: &'static str = "minecraft:element_81";
    const NAME: &'static str = "Element 81";
    const STACK_SIZE: u8 = 1;
}

/// Exposed Lightning Rod
pub struct ExposedLightningRod;

impl ItemDef for ExposedLightningRod {
    const ID: i32 = 10455;
    const STRING_ID: &'static str = "minecraft:exposed_lightning_rod";
    const NAME: &'static str = "Exposed Lightning Rod";
    const STACK_SIZE: u8 = 1;
}

/// Element 82
pub struct Element82;

impl ItemDef for Element82 {
    const ID: i32 = 10456;
    const STRING_ID: &'static str = "minecraft:element_82";
    const NAME: &'static str = "Element 82";
    const STACK_SIZE: u8 = 1;
}

/// Element 85
pub struct Element85;

impl ItemDef for Element85 {
    const ID: i32 = 10458;
    const STRING_ID: &'static str = "minecraft:element_85";
    const NAME: &'static str = "Element 85";
    const STACK_SIZE: u8 = 1;
}

/// Element 87
pub struct Element87;

impl ItemDef for Element87 {
    const ID: i32 = 10459;
    const STRING_ID: &'static str = "minecraft:element_87";
    const NAME: &'static str = "Element 87";
    const STACK_SIZE: u8 = 1;
}

/// Element 88
pub struct Element88;

impl ItemDef for Element88 {
    const ID: i32 = 10460;
    const STRING_ID: &'static str = "minecraft:element_88";
    const NAME: &'static str = "Element 88";
    const STACK_SIZE: u8 = 1;
}

/// Element 90
pub struct Element90;

impl ItemDef for Element90 {
    const ID: i32 = 10461;
    const STRING_ID: &'static str = "minecraft:element_90";
    const NAME: &'static str = "Element 90";
    const STACK_SIZE: u8 = 1;
}

/// Element 91
pub struct Element91;

impl ItemDef for Element91 {
    const ID: i32 = 10462;
    const STRING_ID: &'static str = "minecraft:element_91";
    const NAME: &'static str = "Element 91";
    const STACK_SIZE: u8 = 1;
}

/// Element 92
pub struct Element92;

impl ItemDef for Element92 {
    const ID: i32 = 10463;
    const STRING_ID: &'static str = "minecraft:element_92";
    const NAME: &'static str = "Element 92";
    const STACK_SIZE: u8 = 1;
}

/// Element 93
pub struct Element93;

impl ItemDef for Element93 {
    const ID: i32 = 10464;
    const STRING_ID: &'static str = "minecraft:element_93";
    const NAME: &'static str = "Element 93";
    const STACK_SIZE: u8 = 1;
}

/// Element 94
pub struct Element94;

impl ItemDef for Element94 {
    const ID: i32 = 10465;
    const STRING_ID: &'static str = "minecraft:element_94";
    const NAME: &'static str = "Element 94";
    const STACK_SIZE: u8 = 1;
}

/// Element 95
pub struct Element95;

impl ItemDef for Element95 {
    const ID: i32 = 10466;
    const STRING_ID: &'static str = "minecraft:element_95";
    const NAME: &'static str = "Element 95";
    const STACK_SIZE: u8 = 1;
}

/// Element 96
pub struct Element96;

impl ItemDef for Element96 {
    const ID: i32 = 10468;
    const STRING_ID: &'static str = "minecraft:element_96";
    const NAME: &'static str = "Element 96";
    const STACK_SIZE: u8 = 1;
}

/// Element 98
pub struct Element98;

impl ItemDef for Element98 {
    const ID: i32 = 10469;
    const STRING_ID: &'static str = "minecraft:element_98";
    const NAME: &'static str = "Element 98";
    const STACK_SIZE: u8 = 1;
}

/// Element 99
pub struct Element99;

impl ItemDef for Element99 {
    const ID: i32 = 10471;
    const STRING_ID: &'static str = "minecraft:element_99";
    const NAME: &'static str = "Element 99";
    const STACK_SIZE: u8 = 1;
}

/// Element 100
pub struct Element100;

impl ItemDef for Element100 {
    const ID: i32 = 10473;
    const STRING_ID: &'static str = "minecraft:element_100";
    const NAME: &'static str = "Element 100";
    const STACK_SIZE: u8 = 1;
}

/// Element 101
pub struct Element101;

impl ItemDef for Element101 {
    const ID: i32 = 10474;
    const STRING_ID: &'static str = "minecraft:element_101";
    const NAME: &'static str = "Element 101";
    const STACK_SIZE: u8 = 1;
}

/// Element 105
pub struct Element105;

impl ItemDef for Element105 {
    const ID: i32 = 10475;
    const STRING_ID: &'static str = "minecraft:element_105";
    const NAME: &'static str = "Element 105";
    const STACK_SIZE: u8 = 1;
}

/// Element 106
pub struct Element106;

impl ItemDef for Element106 {
    const ID: i32 = 10476;
    const STRING_ID: &'static str = "minecraft:element_106";
    const NAME: &'static str = "Element 106";
    const STACK_SIZE: u8 = 1;
}

/// Element 107
pub struct Element107;

impl ItemDef for Element107 {
    const ID: i32 = 10477;
    const STRING_ID: &'static str = "minecraft:element_107";
    const NAME: &'static str = "Element 107";
    const STACK_SIZE: u8 = 1;
}

/// Element 108
pub struct Element108;

impl ItemDef for Element108 {
    const ID: i32 = 10478;
    const STRING_ID: &'static str = "minecraft:element_108";
    const NAME: &'static str = "Element 108";
    const STACK_SIZE: u8 = 1;
}

/// Element 110
pub struct Element110;

impl ItemDef for Element110 {
    const ID: i32 = 10479;
    const STRING_ID: &'static str = "minecraft:element_110";
    const NAME: &'static str = "Element 110";
    const STACK_SIZE: u8 = 1;
}

/// Element 111
pub struct Element111;

impl ItemDef for Element111 {
    const ID: i32 = 10480;
    const STRING_ID: &'static str = "minecraft:element_111";
    const NAME: &'static str = "Element 111";
    const STACK_SIZE: u8 = 1;
}

/// Element 112
pub struct Element112;

impl ItemDef for Element112 {
    const ID: i32 = 10481;
    const STRING_ID: &'static str = "minecraft:element_112";
    const NAME: &'static str = "Element 112";
    const STACK_SIZE: u8 = 1;
}

/// Element 113
pub struct Element113;

impl ItemDef for Element113 {
    const ID: i32 = 10483;
    const STRING_ID: &'static str = "minecraft:element_113";
    const NAME: &'static str = "Element 113";
    const STACK_SIZE: u8 = 1;
}

/// Element 114
pub struct Element114;

impl ItemDef for Element114 {
    const ID: i32 = 10485;
    const STRING_ID: &'static str = "minecraft:element_114";
    const NAME: &'static str = "Element 114";
    const STACK_SIZE: u8 = 1;
}

/// Element 115
pub struct Element115;

impl ItemDef for Element115 {
    const ID: i32 = 10486;
    const STRING_ID: &'static str = "minecraft:element_115";
    const NAME: &'static str = "Element 115";
    const STACK_SIZE: u8 = 1;
}

/// Element 116
pub struct Element116;

impl ItemDef for Element116 {
    const ID: i32 = 10488;
    const STRING_ID: &'static str = "minecraft:element_116";
    const NAME: &'static str = "Element 116";
    const STACK_SIZE: u8 = 1;
}

/// Element 117
pub struct Element117;

impl ItemDef for Element117 {
    const ID: i32 = 10489;
    const STRING_ID: &'static str = "minecraft:element_117";
    const NAME: &'static str = "Element 117";
    const STACK_SIZE: u8 = 1;
}

/// Dye
pub struct Dye;

impl ItemDef for Dye {
    const ID: i32 = 10492;
    const STRING_ID: &'static str = "minecraft:dye";
    const NAME: &'static str = "Dye";
    const STACK_SIZE: u8 = 1;
}

/// Banner Pattern
pub struct BannerPattern;

impl ItemDef for BannerPattern {
    const ID: i32 = 10493;
    const STRING_ID: &'static str = "minecraft:banner_pattern";
    const NAME: &'static str = "Banner Pattern";
    const STACK_SIZE: u8 = 1;
}

/// Spawn Egg
pub struct SpawnEgg;

impl ItemDef for SpawnEgg {
    const ID: i32 = 10494;
    const STRING_ID: &'static str = "minecraft:spawn_egg";
    const NAME: &'static str = "Spawn Egg";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Weathered Copper Chain
pub struct WaxedWeatheredCopperChain;

impl ItemDef for WaxedWeatheredCopperChain {
    const ID: i32 = 10498;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_chain";
    const NAME: &'static str = "Waxed Weathered Copper Chain";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Weathered Copper Chest
pub struct WaxedWeatheredCopperChest;

impl ItemDef for WaxedWeatheredCopperChest {
    const ID: i32 = 10499;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_chest";
    const NAME: &'static str = "Waxed Weathered Copper Chest";
    const STACK_SIZE: u8 = 1;
}

/// Warped Door
pub struct WarpedDoorItem;

impl ItemDef for WarpedDoorItem {
    const ID: i32 = 10500;
    const STRING_ID: &'static str = "minecraft:item.warped_door";
    const NAME: &'static str = "Warped Door";
    const STACK_SIZE: u8 = 1;
}

/// Piston Arm Collision
pub struct PistonArmCollision;

impl ItemDef for PistonArmCollision {
    const ID: i32 = 10502;
    const STRING_ID: &'static str = "minecraft:piston_arm_collision";
    const NAME: &'static str = "Piston Arm Collision";
    const STACK_SIZE: u8 = 1;
}

/// Blackstone Double Slab
pub struct BlackstoneDoubleSlab;

impl ItemDef for BlackstoneDoubleSlab {
    const ID: i32 = 10507;
    const STRING_ID: &'static str = "minecraft:blackstone_double_slab";
    const NAME: &'static str = "Blackstone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Crimson Wall Sign
pub struct CrimsonWallSign;

impl ItemDef for CrimsonWallSign {
    const ID: i32 = 10511;
    const STRING_ID: &'static str = "minecraft:crimson_wall_sign";
    const NAME: &'static str = "Crimson Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Glow Frame
pub struct GlowFrameItem;

impl ItemDef for GlowFrameItem {
    const ID: i32 = 10513;
    const STRING_ID: &'static str = "minecraft:item.glow_frame";
    const NAME: &'static str = "Glow Frame";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Exposed Copper Chain
pub struct WaxedExposedCopperChain;

impl ItemDef for WaxedExposedCopperChain {
    const ID: i32 = 10515;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_chain";
    const NAME: &'static str = "Waxed Exposed Copper Chain";
    const STACK_SIZE: u8 = 1;
}

/// Warped Standing Sign
pub struct WarpedStandingSign;

impl ItemDef for WarpedStandingSign {
    const ID: i32 = 10525;
    const STRING_ID: &'static str = "minecraft:warped_standing_sign";
    const NAME: &'static str = "Warped Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Pitcher Crop
pub struct PitcherCrop;

impl ItemDef for PitcherCrop {
    const ID: i32 = 10527;
    const STRING_ID: &'static str = "minecraft:pitcher_crop";
    const NAME: &'static str = "Pitcher Crop";
    const STACK_SIZE: u8 = 1;
}

/// Light Blue Candle Cake
pub struct LightBlueCandleCake;

impl ItemDef for LightBlueCandleCake {
    const ID: i32 = 10533;
    const STRING_ID: &'static str = "minecraft:light_blue_candle_cake";
    const NAME: &'static str = "Light Blue Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Oxidized Lightning Rod
pub struct OxidizedLightningRod;

impl ItemDef for OxidizedLightningRod {
    const ID: i32 = 10535;
    const STRING_ID: &'static str = "minecraft:oxidized_lightning_rod";
    const NAME: &'static str = "Oxidized Lightning Rod";
    const STACK_SIZE: u8 = 1;
}

/// Powered Comparator
pub struct PoweredComparator;

impl ItemDef for PoweredComparator {
    const ID: i32 = 10545;
    const STRING_ID: &'static str = "minecraft:powered_comparator";
    const NAME: &'static str = "Powered Comparator";
    const STACK_SIZE: u8 = 1;
}

/// Warped Wall Sign
pub struct WarpedWallSign;

impl ItemDef for WarpedWallSign {
    const ID: i32 = 10546;
    const STRING_ID: &'static str = "minecraft:warped_wall_sign";
    const NAME: &'static str = "Warped Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Mangrove Double Slab
pub struct MangroveDoubleSlab;

impl ItemDef for MangroveDoubleSlab {
    const ID: i32 = 10549;
    const STRING_ID: &'static str = "minecraft:mangrove_double_slab";
    const NAME: &'static str = "Mangrove Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Oxidized Double Cut Copper Slab
pub struct OxidizedDoubleCutCopperSlab;

impl ItemDef for OxidizedDoubleCutCopperSlab {
    const ID: i32 = 10551;
    const STRING_ID: &'static str = "minecraft:oxidized_double_cut_copper_slab";
    const NAME: &'static str = "Oxidized Double Cut Copper Slab";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Copper Bars
pub struct WaxedCopperBars;

impl ItemDef for WaxedCopperBars {
    const ID: i32 = 10552;
    const STRING_ID: &'static str = "minecraft:waxed_copper_bars";
    const NAME: &'static str = "Waxed Copper Bars";
    const STACK_SIZE: u8 = 1;
}

/// Jungle Shelf
pub struct JungleShelf;

impl ItemDef for JungleShelf {
    const ID: i32 = 10554;
    const STRING_ID: &'static str = "minecraft:jungle_shelf";
    const NAME: &'static str = "Jungle Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Exposed Double Cut Copper Slab
pub struct ExposedDoubleCutCopperSlab;

impl ItemDef for ExposedDoubleCutCopperSlab {
    const ID: i32 = 10555;
    const STRING_ID: &'static str = "minecraft:exposed_double_cut_copper_slab";
    const NAME: &'static str = "Exposed Double Cut Copper Slab";
    const STACK_SIZE: u8 = 1;
}

/// Polished Blackstone Double Slab
pub struct PolishedBlackstoneDoubleSlab;

impl ItemDef for PolishedBlackstoneDoubleSlab {
    const ID: i32 = 10560;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_double_slab";
    const NAME: &'static str = "Polished Blackstone Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Hard Glass Pane
pub struct HardGlassPane;

impl ItemDef for HardGlassPane {
    const ID: i32 = 10561;
    const STRING_ID: &'static str = "minecraft:hard_glass_pane";
    const NAME: &'static str = "Hard Glass Pane";
    const STACK_SIZE: u8 = 1;
}

/// Polished Blackstone Brick Double Slab
pub struct PolishedBlackstoneBrickDoubleSlab;

impl ItemDef for PolishedBlackstoneBrickDoubleSlab {
    const ID: i32 = 10563;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_brick_double_slab";
    const NAME: &'static str = "Polished Blackstone Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Resin Brick Double Slab
pub struct ResinBrickDoubleSlab;

impl ItemDef for ResinBrickDoubleSlab {
    const ID: i32 = 10566;
    const STRING_ID: &'static str = "minecraft:resin_brick_double_slab";
    const NAME: &'static str = "Resin Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Cyan Candle Cake
pub struct CyanCandleCake;

impl ItemDef for CyanCandleCake {
    const ID: i32 = 10567;
    const STRING_ID: &'static str = "minecraft:cyan_candle_cake";
    const NAME: &'static str = "Cyan Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Stonecutter
pub struct Stonecutter;

impl ItemDef for Stonecutter {
    const ID: i32 = 10574;
    const STRING_ID: &'static str = "minecraft:stonecutter";
    const NAME: &'static str = "Stonecutter";
    const STACK_SIZE: u8 = 1;
}

/// Invisible Bedrock
pub struct InvisibleBedrock;

impl ItemDef for InvisibleBedrock {
    const ID: i32 = 10578;
    const STRING_ID: &'static str = "minecraft:invisible_bedrock";
    const NAME: &'static str = "Invisible Bedrock";
    const STACK_SIZE: u8 = 1;
}

/// Oxidized Copper Bars
pub struct OxidizedCopperBars;

impl ItemDef for OxidizedCopperBars {
    const ID: i32 = 10581;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_bars";
    const NAME: &'static str = "Oxidized Copper Bars";
    const STACK_SIZE: u8 = 1;
}

/// Underwater Torch
pub struct UnderwaterTorch;

impl ItemDef for UnderwaterTorch {
    const ID: i32 = 10585;
    const STRING_ID: &'static str = "minecraft:underwater_torch";
    const NAME: &'static str = "Underwater Torch";
    const STACK_SIZE: u8 = 1;
}

/// Wall Banner
pub struct WallBanner;

impl ItemDef for WallBanner {
    const ID: i32 = 10586;
    const STRING_ID: &'static str = "minecraft:wall_banner";
    const NAME: &'static str = "Wall Banner";
    const STACK_SIZE: u8 = 1;
}

/// Spruce Double Slab
pub struct SpruceDoubleSlab;

impl ItemDef for SpruceDoubleSlab {
    const ID: i32 = 10587;
    const STRING_ID: &'static str = "minecraft:spruce_double_slab";
    const NAME: &'static str = "Spruce Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Glowingobsidian
pub struct Glowingobsidian;

impl ItemDef for Glowingobsidian {
    const ID: i32 = 10591;
    const STRING_ID: &'static str = "minecraft:glowingobsidian";
    const NAME: &'static str = "Glowingobsidian";
    const STACK_SIZE: u8 = 1;
}

/// Exposed Copper Lantern
pub struct ExposedCopperLantern;

impl ItemDef for ExposedCopperLantern {
    const ID: i32 = 10594;
    const STRING_ID: &'static str = "minecraft:exposed_copper_lantern";
    const NAME: &'static str = "Exposed Copper Lantern";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Exposed Copper Bars
pub struct WaxedExposedCopperBars;

impl ItemDef for WaxedExposedCopperBars {
    const ID: i32 = 10597;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_bars";
    const NAME: &'static str = "Waxed Exposed Copper Bars";
    const STACK_SIZE: u8 = 1;
}

/// Spruce Shelf
pub struct SpruceShelf;

impl ItemDef for SpruceShelf {
    const ID: i32 = 10598;
    const STRING_ID: &'static str = "minecraft:spruce_shelf";
    const NAME: &'static str = "Spruce Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Moving Block
pub struct MovingBlock;

impl ItemDef for MovingBlock {
    const ID: i32 = 10599;
    const STRING_ID: &'static str = "minecraft:moving_block";
    const NAME: &'static str = "Moving Block";
    const STACK_SIZE: u8 = 1;
}

/// Green Candle Cake
pub struct GreenCandleCake;

impl ItemDef for GreenCandleCake {
    const ID: i32 = 10605;
    const STRING_ID: &'static str = "minecraft:green_candle_cake";
    const NAME: &'static str = "Green Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Lightning Rod
pub struct WaxedLightningRod;

impl ItemDef for WaxedLightningRod {
    const ID: i32 = 10608;
    const STRING_ID: &'static str = "minecraft:waxed_lightning_rod";
    const NAME: &'static str = "Waxed Lightning Rod";
    const STACK_SIZE: u8 = 1;
}

/// Warped Shelf
pub struct WarpedShelf;

impl ItemDef for WarpedShelf {
    const ID: i32 = 10609;
    const STRING_ID: &'static str = "minecraft:warped_shelf";
    const NAME: &'static str = "Warped Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Copper Bars
pub struct CopperBars;

impl ItemDef for CopperBars {
    const ID: i32 = 10610;
    const STRING_ID: &'static str = "minecraft:copper_bars";
    const NAME: &'static str = "Copper Bars";
    const STACK_SIZE: u8 = 1;
}

/// Oak Double Slab
pub struct OakDoubleSlab;

impl ItemDef for OakDoubleSlab {
    const ID: i32 = 10611;
    const STRING_ID: &'static str = "minecraft:oak_double_slab";
    const NAME: &'static str = "Oak Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Brown Candle Cake
pub struct BrownCandleCake;

impl ItemDef for BrownCandleCake {
    const ID: i32 = 10612;
    const STRING_ID: &'static str = "minecraft:brown_candle_cake";
    const NAME: &'static str = "Brown Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Acacia Wall Sign
pub struct AcaciaWallSign;

impl ItemDef for AcaciaWallSign {
    const ID: i32 = 10614;
    const STRING_ID: &'static str = "minecraft:acacia_wall_sign";
    const NAME: &'static str = "Acacia Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Copper Chest
pub struct CopperChest;

impl ItemDef for CopperChest {
    const ID: i32 = 10622;
    const STRING_ID: &'static str = "minecraft:copper_chest";
    const NAME: &'static str = "Copper Chest";
    const STACK_SIZE: u8 = 1;
}

/// Wooden Door
pub struct WoodenDoorItem;

impl ItemDef for WoodenDoorItem {
    const ID: i32 = 10623;
    const STRING_ID: &'static str = "minecraft:item.wooden_door";
    const NAME: &'static str = "Wooden Door";
    const STACK_SIZE: u8 = 1;
}

/// Redstone Wire
pub struct RedstoneWire;

impl ItemDef for RedstoneWire {
    const ID: i32 = 10628;
    const STRING_ID: &'static str = "minecraft:redstone_wire";
    const NAME: &'static str = "Redstone Wire";
    const STACK_SIZE: u8 = 1;
}

/// Lava
pub struct Lava;

impl ItemDef for Lava {
    const ID: i32 = 10630;
    const STRING_ID: &'static str = "minecraft:lava";
    const NAME: &'static str = "Lava";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Weathered Copper Lantern
pub struct WaxedWeatheredCopperLantern;

impl ItemDef for WaxedWeatheredCopperLantern {
    const ID: i32 = 10631;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_lantern";
    const NAME: &'static str = "Waxed Weathered Copper Lantern";
    const STACK_SIZE: u8 = 1;
}

/// Crimson Door
pub struct CrimsonDoorItem;

impl ItemDef for CrimsonDoorItem {
    const ID: i32 = 10632;
    const STRING_ID: &'static str = "minecraft:item.crimson_door";
    const NAME: &'static str = "Crimson Door";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Exposed Lightning Rod
pub struct WaxedExposedLightningRod;

impl ItemDef for WaxedExposedLightningRod {
    const ID: i32 = 10635;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_lightning_rod";
    const NAME: &'static str = "Waxed Exposed Lightning Rod";
    const STACK_SIZE: u8 = 1;
}

/// Brain Coral Wall Fan
pub struct BrainCoralWallFan;

impl ItemDef for BrainCoralWallFan {
    const ID: i32 = 10641;
    const STRING_ID: &'static str = "minecraft:brain_coral_wall_fan";
    const NAME: &'static str = "Brain Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// Darkoak Standing Sign
pub struct DarkoakStandingSign;

impl ItemDef for DarkoakStandingSign {
    const ID: i32 = 10642;
    const STRING_ID: &'static str = "minecraft:darkoak_standing_sign";
    const NAME: &'static str = "Darkoak Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Exposed Copper Golem Statue
pub struct WaxedExposedCopperGolemStatue;

impl ItemDef for WaxedExposedCopperGolemStatue {
    const ID: i32 = 10643;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_golem_statue";
    const NAME: &'static str = "Waxed Exposed Copper Golem Statue";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Oxidized Copper Chest
pub struct WaxedOxidizedCopperChest;

impl ItemDef for WaxedOxidizedCopperChest {
    const ID: i32 = 10645;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_chest";
    const NAME: &'static str = "Waxed Oxidized Copper Chest";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Oxidized Copper Chain
pub struct WaxedOxidizedCopperChain;

impl ItemDef for WaxedOxidizedCopperChain {
    const ID: i32 = 10646;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_chain";
    const NAME: &'static str = "Waxed Oxidized Copper Chain";
    const STACK_SIZE: u8 = 1;
}

/// Bamboo Mosaic Double Slab
pub struct BambooMosaicDoubleSlab;

impl ItemDef for BambooMosaicDoubleSlab {
    const ID: i32 = 10647;
    const STRING_ID: &'static str = "minecraft:bamboo_mosaic_double_slab";
    const NAME: &'static str = "Bamboo Mosaic Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Mangrove Standing Sign
pub struct MangroveStandingSign;

impl ItemDef for MangroveStandingSign {
    const ID: i32 = 10651;
    const STRING_ID: &'static str = "minecraft:mangrove_standing_sign";
    const NAME: &'static str = "Mangrove Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Dark Oak Shelf
pub struct DarkOakShelf;

impl ItemDef for DarkOakShelf {
    const ID: i32 = 10653;
    const STRING_ID: &'static str = "minecraft:dark_oak_shelf";
    const NAME: &'static str = "Dark Oak Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Lit Redstone Ore
pub struct LitRedstoneOre;

impl ItemDef for LitRedstoneOre {
    const ID: i32 = 10654;
    const STRING_ID: &'static str = "minecraft:lit_redstone_ore";
    const NAME: &'static str = "Lit Redstone Ore";
    const STACK_SIZE: u8 = 1;
}

/// Bamboo Shelf
pub struct BambooShelf;

impl ItemDef for BambooShelf {
    const ID: i32 = 10659;
    const STRING_ID: &'static str = "minecraft:bamboo_shelf";
    const NAME: &'static str = "Bamboo Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Copper Chest
pub struct WaxedCopperChest;

impl ItemDef for WaxedCopperChest {
    const ID: i32 = 10661;
    const STRING_ID: &'static str = "minecraft:waxed_copper_chest";
    const NAME: &'static str = "Waxed Copper Chest";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Copper Chain
pub struct WaxedCopperChain;

impl ItemDef for WaxedCopperChain {
    const ID: i32 = 10662;
    const STRING_ID: &'static str = "minecraft:waxed_copper_chain";
    const NAME: &'static str = "Waxed Copper Chain";
    const STACK_SIZE: u8 = 1;
}

/// Warped Double Slab
pub struct WarpedDoubleSlab;

impl ItemDef for WarpedDoubleSlab {
    const ID: i32 = 10670;
    const STRING_ID: &'static str = "minecraft:warped_double_slab";
    const NAME: &'static str = "Warped Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Jungle Wall Sign
pub struct JungleWallSign;

impl ItemDef for JungleWallSign {
    const ID: i32 = 10671;
    const STRING_ID: &'static str = "minecraft:jungle_wall_sign";
    const NAME: &'static str = "Jungle Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Oak Shelf
pub struct OakShelf;

impl ItemDef for OakShelf {
    const ID: i32 = 10673;
    const STRING_ID: &'static str = "minecraft:oak_shelf";
    const NAME: &'static str = "Oak Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Powered Repeater
pub struct PoweredRepeater;

impl ItemDef for PoweredRepeater {
    const ID: i32 = 10681;
    const STRING_ID: &'static str = "minecraft:powered_repeater";
    const NAME: &'static str = "Powered Repeater";
    const STACK_SIZE: u8 = 1;
}

/// Yellow Candle Cake
pub struct YellowCandleCake;

impl ItemDef for YellowCandleCake {
    const ID: i32 = 10687;
    const STRING_ID: &'static str = "minecraft:yellow_candle_cake";
    const NAME: &'static str = "Yellow Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Weathered Copper Golem Statue
pub struct WaxedWeatheredCopperGolemStatue;

impl ItemDef for WaxedWeatheredCopperGolemStatue {
    const ID: i32 = 10696;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_golem_statue";
    const NAME: &'static str = "Waxed Weathered Copper Golem Statue";
    const STACK_SIZE: u8 = 1;
}

/// Wheat
pub struct WheatItem;

impl ItemDef for WheatItem {
    const ID: i32 = 10697;
    const STRING_ID: &'static str = "minecraft:item.wheat";
    const NAME: &'static str = "Wheat";
    const STACK_SIZE: u8 = 1;
}

/// Spruce Door
pub struct SpruceDoorItem;

impl ItemDef for SpruceDoorItem {
    const ID: i32 = 10698;
    const STRING_ID: &'static str = "minecraft:item.spruce_door";
    const NAME: &'static str = "Spruce Door";
    const STACK_SIZE: u8 = 1;
}

/// Frosted Ice
pub struct FrostedIce;

impl ItemDef for FrostedIce {
    const ID: i32 = 10700;
    const STRING_ID: &'static str = "minecraft:frosted_ice";
    const NAME: &'static str = "Frosted Ice";
    const STACK_SIZE: u8 = 1;
}

/// Cave Vines
pub struct CaveVines;

impl ItemDef for CaveVines {
    const ID: i32 = 10705;
    const STRING_ID: &'static str = "minecraft:cave_vines";
    const NAME: &'static str = "Cave Vines";
    const STACK_SIZE: u8 = 1;
}

/// Melon Stem
pub struct MelonStem;

impl ItemDef for MelonStem {
    const ID: i32 = 10706;
    const STRING_ID: &'static str = "minecraft:melon_stem";
    const NAME: &'static str = "Melon Stem";
    const STACK_SIZE: u8 = 1;
}

/// Horn Coral Wall Fan
pub struct HornCoralWallFan;

impl ItemDef for HornCoralWallFan {
    const ID: i32 = 10709;
    const STRING_ID: &'static str = "minecraft:horn_coral_wall_fan";
    const NAME: &'static str = "Horn Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// Wall Sign
pub struct WallSign;

impl ItemDef for WallSign {
    const ID: i32 = 10712;
    const STRING_ID: &'static str = "minecraft:wall_sign";
    const NAME: &'static str = "Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Oxidized Copper Golem Statue
pub struct WaxedOxidizedCopperGolemStatue;

impl ItemDef for WaxedOxidizedCopperGolemStatue {
    const ID: i32 = 10713;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_golem_statue";
    const NAME: &'static str = "Waxed Oxidized Copper Golem Statue";
    const STACK_SIZE: u8 = 1;
}

/// Birch Double Slab
pub struct BirchDoubleSlab;

impl ItemDef for BirchDoubleSlab {
    const ID: i32 = 10715;
    const STRING_ID: &'static str = "minecraft:birch_double_slab";
    const NAME: &'static str = "Birch Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Weathered Lightning Rod
pub struct WeatheredLightningRod;

impl ItemDef for WeatheredLightningRod {
    const ID: i32 = 10718;
    const STRING_ID: &'static str = "minecraft:weathered_lightning_rod";
    const NAME: &'static str = "Weathered Lightning Rod";
    const STACK_SIZE: u8 = 1;
}

/// Mangrove Wall Sign
pub struct MangroveWallSign;

impl ItemDef for MangroveWallSign {
    const ID: i32 = 10720;
    const STRING_ID: &'static str = "minecraft:mangrove_wall_sign";
    const NAME: &'static str = "Mangrove Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Light Gray Candle Cake
pub struct LightGrayCandleCake;

impl ItemDef for LightGrayCandleCake {
    const ID: i32 = 10721;
    const STRING_ID: &'static str = "minecraft:light_gray_candle_cake";
    const NAME: &'static str = "Light Gray Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Darkoak Wall Sign
pub struct DarkoakWallSign;

impl ItemDef for DarkoakWallSign {
    const ID: i32 = 10723;
    const STRING_ID: &'static str = "minecraft:darkoak_wall_sign";
    const NAME: &'static str = "Darkoak Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Fire Coral Wall Fan
pub struct FireCoralWallFan;

impl ItemDef for FireCoralWallFan {
    const ID: i32 = 10727;
    const STRING_ID: &'static str = "minecraft:fire_coral_wall_fan";
    const NAME: &'static str = "Fire Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// Flowing Lava
pub struct FlowingLava;

impl ItemDef for FlowingLava {
    const ID: i32 = 10728;
    const STRING_ID: &'static str = "minecraft:flowing_lava";
    const NAME: &'static str = "Flowing Lava";
    const STACK_SIZE: u8 = 1;
}

/// Exposed Copper Chest
pub struct ExposedCopperChest;

impl ItemDef for ExposedCopperChest {
    const ID: i32 = 10733;
    const STRING_ID: &'static str = "minecraft:exposed_copper_chest";
    const NAME: &'static str = "Exposed Copper Chest";
    const STACK_SIZE: u8 = 1;
}

/// Exposed Copper Chain
pub struct ExposedCopperChain;

impl ItemDef for ExposedCopperChain {
    const ID: i32 = 10734;
    const STRING_ID: &'static str = "minecraft:exposed_copper_chain";
    const NAME: &'static str = "Exposed Copper Chain";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Double Cut Copper Slab
pub struct WaxedDoubleCutCopperSlab;

impl ItemDef for WaxedDoubleCutCopperSlab {
    const ID: i32 = 10735;
    const STRING_ID: &'static str = "minecraft:waxed_double_cut_copper_slab";
    const NAME: &'static str = "Waxed Double Cut Copper Slab";
    const STACK_SIZE: u8 = 1;
}

/// Kelp
pub struct KelpItem;

impl ItemDef for KelpItem {
    const ID: i32 = 10736;
    const STRING_ID: &'static str = "minecraft:item.kelp";
    const NAME: &'static str = "Kelp";
    const STACK_SIZE: u8 = 1;
}

/// Water
pub struct Water;

impl ItemDef for Water {
    const ID: i32 = 10738;
    const STRING_ID: &'static str = "minecraft:water";
    const NAME: &'static str = "Water";
    const STACK_SIZE: u8 = 1;
}

/// Chemical Heat
pub struct ChemicalHeat;

impl ItemDef for ChemicalHeat {
    const ID: i32 = 10739;
    const STRING_ID: &'static str = "minecraft:chemical_heat";
    const NAME: &'static str = "Chemical Heat";
    const STACK_SIZE: u8 = 1;
}

/// Unpowered Repeater
pub struct UnpoweredRepeater;

impl ItemDef for UnpoweredRepeater {
    const ID: i32 = 10741;
    const STRING_ID: &'static str = "minecraft:unpowered_repeater";
    const NAME: &'static str = "Unpowered Repeater";
    const STACK_SIZE: u8 = 1;
}

/// Weathered Copper Chest
pub struct WeatheredCopperChest;

impl ItemDef for WeatheredCopperChest {
    const ID: i32 = 10744;
    const STRING_ID: &'static str = "minecraft:weathered_copper_chest";
    const NAME: &'static str = "Weathered Copper Chest";
    const STACK_SIZE: u8 = 1;
}

/// Weathered Copper Chain
pub struct WeatheredCopperChain;

impl ItemDef for WeatheredCopperChain {
    const ID: i32 = 10745;
    const STRING_ID: &'static str = "minecraft:weathered_copper_chain";
    const NAME: &'static str = "Weathered Copper Chain";
    const STACK_SIZE: u8 = 1;
}

/// Acacia Double Slab
pub struct AcaciaDoubleSlab;

impl ItemDef for AcaciaDoubleSlab {
    const ID: i32 = 10747;
    const STRING_ID: &'static str = "minecraft:acacia_double_slab";
    const NAME: &'static str = "Acacia Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Bubble Column
pub struct BubbleColumn;

impl ItemDef for BubbleColumn {
    const ID: i32 = 10751;
    const STRING_ID: &'static str = "minecraft:bubble_column";
    const NAME: &'static str = "Bubble Column";
    const STACK_SIZE: u8 = 1;
}

/// Cobbled Deepslate Double Slab
pub struct CobbledDeepslateDoubleSlab;

impl ItemDef for CobbledDeepslateDoubleSlab {
    const ID: i32 = 10754;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate_double_slab";
    const NAME: &'static str = "Cobbled Deepslate Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Cherry Standing Sign
pub struct CherryStandingSign;

impl ItemDef for CherryStandingSign {
    const ID: i32 = 10756;
    const STRING_ID: &'static str = "minecraft:cherry_standing_sign";
    const NAME: &'static str = "Cherry Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Pale Oak Shelf
pub struct PaleOakShelf;

impl ItemDef for PaleOakShelf {
    const ID: i32 = 10757;
    const STRING_ID: &'static str = "minecraft:pale_oak_shelf";
    const NAME: &'static str = "Pale Oak Shelf";
    const STACK_SIZE: u8 = 1;
}

/// Tuff Brick Double Slab
pub struct TuffBrickDoubleSlab;

impl ItemDef for TuffBrickDoubleSlab {
    const ID: i32 = 10763;
    const STRING_ID: &'static str = "minecraft:tuff_brick_double_slab";
    const NAME: &'static str = "Tuff Brick Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Reeds
pub struct Reeds;

impl ItemDef for Reeds {
    const ID: i32 = 10768;
    const STRING_ID: &'static str = "minecraft:item.reeds";
    const NAME: &'static str = "Reeds";
    const STACK_SIZE: u8 = 1;
}

/// Camera
pub struct CameraItem;

impl ItemDef for CameraItem {
    const ID: i32 = 10771;
    const STRING_ID: &'static str = "minecraft:item.camera";
    const NAME: &'static str = "Camera";
    const STACK_SIZE: u8 = 1;
}

/// Jungle Door
pub struct JungleDoorItem;

impl ItemDef for JungleDoorItem {
    const ID: i32 = 10774;
    const STRING_ID: &'static str = "minecraft:item.jungle_door";
    const NAME: &'static str = "Jungle Door";
    const STACK_SIZE: u8 = 1;
}

/// Acacia Standing Sign
pub struct AcaciaStandingSign;

impl ItemDef for AcaciaStandingSign {
    const ID: i32 = 10780;
    const STRING_ID: &'static str = "minecraft:acacia_standing_sign";
    const NAME: &'static str = "Acacia Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Pumpkin Stem
pub struct PumpkinStem;

impl ItemDef for PumpkinStem {
    const ID: i32 = 10786;
    const STRING_ID: &'static str = "minecraft:pumpkin_stem";
    const NAME: &'static str = "Pumpkin Stem";
    const STACK_SIZE: u8 = 1;
}

/// Unpowered Comparator
pub struct UnpoweredComparator;

impl ItemDef for UnpoweredComparator {
    const ID: i32 = 10789;
    const STRING_ID: &'static str = "minecraft:unpowered_comparator";
    const NAME: &'static str = "Unpowered Comparator";
    const STACK_SIZE: u8 = 1;
}

/// Weathered Copper Lantern
pub struct WeatheredCopperLantern;

impl ItemDef for WeatheredCopperLantern {
    const ID: i32 = 10791;
    const STRING_ID: &'static str = "minecraft:weathered_copper_lantern";
    const NAME: &'static str = "Weathered Copper Lantern";
    const STACK_SIZE: u8 = 1;
}

/// Nether Sprouts
pub struct NetherSproutsItem;

impl ItemDef for NetherSproutsItem {
    const ID: i32 = 10792;
    const STRING_ID: &'static str = "minecraft:item.nether_sprouts";
    const NAME: &'static str = "Nether Sprouts";
    const STACK_SIZE: u8 = 1;
}

/// Cocoa
pub struct Cocoa;

impl ItemDef for Cocoa {
    const ID: i32 = 10797;
    const STRING_ID: &'static str = "minecraft:cocoa";
    const NAME: &'static str = "Cocoa";
    const STACK_SIZE: u8 = 1;
}

/// Bed
pub struct BedItem;

impl ItemDef for BedItem {
    const ID: i32 = 10803;
    const STRING_ID: &'static str = "minecraft:item.bed";
    const NAME: &'static str = "Bed";
    const STACK_SIZE: u8 = 1;
}

/// Oxidized Copper Chest
pub struct OxidizedCopperChest;

impl ItemDef for OxidizedCopperChest {
    const ID: i32 = 10805;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_chest";
    const NAME: &'static str = "Oxidized Copper Chest";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Exposed Double Cut Copper Slab
pub struct WaxedExposedDoubleCutCopperSlab;

impl ItemDef for WaxedExposedDoubleCutCopperSlab {
    const ID: i32 = 10807;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_double_cut_copper_slab";
    const NAME: &'static str = "Waxed Exposed Double Cut Copper Slab";
    const STACK_SIZE: u8 = 1;
}

/// Bubble Coral Wall Fan
pub struct BubbleCoralWallFan;

impl ItemDef for BubbleCoralWallFan {
    const ID: i32 = 10808;
    const STRING_ID: &'static str = "minecraft:bubble_coral_wall_fan";
    const NAME: &'static str = "Bubble Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// Birch Wall Sign
pub struct BirchWallSign;

impl ItemDef for BirchWallSign {
    const ID: i32 = 10811;
    const STRING_ID: &'static str = "minecraft:birch_wall_sign";
    const NAME: &'static str = "Birch Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Bamboo Wall Sign
pub struct BambooWallSign;

impl ItemDef for BambooWallSign {
    const ID: i32 = 10812;
    const STRING_ID: &'static str = "minecraft:bamboo_wall_sign";
    const NAME: &'static str = "Bamboo Wall Sign";
    const STACK_SIZE: u8 = 1;
}

/// Bamboo Sapling
pub struct BambooSapling;

impl ItemDef for BambooSapling {
    const ID: i32 = 10814;
    const STRING_ID: &'static str = "minecraft:bamboo_sapling";
    const NAME: &'static str = "Bamboo Sapling";
    const STACK_SIZE: u8 = 1;
}

/// Standing Banner
pub struct StandingBanner;

impl ItemDef for StandingBanner {
    const ID: i32 = 10815;
    const STRING_ID: &'static str = "minecraft:standing_banner";
    const NAME: &'static str = "Standing Banner";
    const STACK_SIZE: u8 = 1;
}

/// Jungle Double Slab
pub struct JungleDoubleSlab;

impl ItemDef for JungleDoubleSlab {
    const ID: i32 = 10817;
    const STRING_ID: &'static str = "minecraft:jungle_double_slab";
    const NAME: &'static str = "Jungle Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Dead Horn Coral Wall Fan
pub struct DeadHornCoralWallFan;

impl ItemDef for DeadHornCoralWallFan {
    const ID: i32 = 10824;
    const STRING_ID: &'static str = "minecraft:dead_horn_coral_wall_fan";
    const NAME: &'static str = "Dead Horn Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// Weathered Double Cut Copper Slab
pub struct WeatheredDoubleCutCopperSlab;

impl ItemDef for WeatheredDoubleCutCopperSlab {
    const ID: i32 = 10825;
    const STRING_ID: &'static str = "minecraft:weathered_double_cut_copper_slab";
    const NAME: &'static str = "Weathered Double Cut Copper Slab";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Oxidized Lightning Rod
pub struct WaxedOxidizedLightningRod;

impl ItemDef for WaxedOxidizedLightningRod {
    const ID: i32 = 10828;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_lightning_rod";
    const NAME: &'static str = "Waxed Oxidized Lightning Rod";
    const STACK_SIZE: u8 = 1;
}

/// Allow
pub struct Allow;

impl ItemDef for Allow {
    const ID: i32 = 10829;
    const STRING_ID: &'static str = "minecraft:allow";
    const NAME: &'static str = "Allow";
    const STACK_SIZE: u8 = 1;
}

/// Birch Door
pub struct BirchDoorItem;

impl ItemDef for BirchDoorItem {
    const ID: i32 = 10830;
    const STRING_ID: &'static str = "minecraft:item.birch_door";
    const NAME: &'static str = "Birch Door";
    const STACK_SIZE: u8 = 1;
}

/// Bamboo Standing Sign
pub struct BambooStandingSign;

impl ItemDef for BambooStandingSign {
    const ID: i32 = 10832;
    const STRING_ID: &'static str = "minecraft:bamboo_standing_sign";
    const NAME: &'static str = "Bamboo Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Crimson Standing Sign
pub struct CrimsonStandingSign;

impl ItemDef for CrimsonStandingSign {
    const ID: i32 = 10841;
    const STRING_ID: &'static str = "minecraft:crimson_standing_sign";
    const NAME: &'static str = "Crimson Standing Sign";
    const STACK_SIZE: u8 = 1;
}

/// Netherreactor
pub struct Netherreactor;

impl ItemDef for Netherreactor {
    const ID: i32 = 10844;
    const STRING_ID: &'static str = "minecraft:netherreactor";
    const NAME: &'static str = "Netherreactor";
    const STACK_SIZE: u8 = 1;
}

/// Exposed Copper Golem Statue
pub struct ExposedCopperGolemStatue;

impl ItemDef for ExposedCopperGolemStatue {
    const ID: i32 = 10845;
    const STRING_ID: &'static str = "minecraft:exposed_copper_golem_statue";
    const NAME: &'static str = "Exposed Copper Golem Statue";
    const STACK_SIZE: u8 = 1;
}

/// Cauldron
pub struct CauldronItem;

impl ItemDef for CauldronItem {
    const ID: i32 = 10846;
    const STRING_ID: &'static str = "minecraft:item.cauldron";
    const NAME: &'static str = "Cauldron";
    const STACK_SIZE: u8 = 1;
}

/// Cave Vines Head With Berries
pub struct CaveVinesHeadWithBerries;

impl ItemDef for CaveVinesHeadWithBerries {
    const ID: i32 = 10847;
    const STRING_ID: &'static str = "minecraft:cave_vines_head_with_berries";
    const NAME: &'static str = "Cave Vines Head With Berries";
    const STACK_SIZE: u8 = 1;
}

/// Brewing Stand
pub struct BrewingStandItem;

impl ItemDef for BrewingStandItem {
    const ID: i32 = 10849;
    const STRING_ID: &'static str = "minecraft:item.brewing_stand";
    const NAME: &'static str = "Brewing Stand";
    const STACK_SIZE: u8 = 1;
}

/// End Portal
pub struct EndPortal;

impl ItemDef for EndPortal {
    const ID: i32 = 10851;
    const STRING_ID: &'static str = "minecraft:end_portal";
    const NAME: &'static str = "End Portal";
    const STACK_SIZE: u8 = 1;
}

/// Lit Smoker
pub struct LitSmoker;

impl ItemDef for LitSmoker {
    const ID: i32 = 10853;
    const STRING_ID: &'static str = "minecraft:lit_smoker";
    const NAME: &'static str = "Lit Smoker";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Copper Lantern
pub struct WaxedCopperLantern;

impl ItemDef for WaxedCopperLantern {
    const ID: i32 = 10855;
    const STRING_ID: &'static str = "minecraft:waxed_copper_lantern";
    const NAME: &'static str = "Waxed Copper Lantern";
    const STACK_SIZE: u8 = 1;
}

/// Tuff Double Slab
pub struct TuffDoubleSlab;

impl ItemDef for TuffDoubleSlab {
    const ID: i32 = 10859;
    const STRING_ID: &'static str = "minecraft:tuff_double_slab";
    const NAME: &'static str = "Tuff Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Soul Campfire
pub struct SoulCampfireItem;

impl ItemDef for SoulCampfireItem {
    const ID: i32 = 10865;
    const STRING_ID: &'static str = "minecraft:item.soul_campfire";
    const NAME: &'static str = "Soul Campfire";
    const STACK_SIZE: u8 = 1;
}

/// Lime Candle Cake
pub struct LimeCandleCake;

impl ItemDef for LimeCandleCake {
    const ID: i32 = 10867;
    const STRING_ID: &'static str = "minecraft:lime_candle_cake";
    const NAME: &'static str = "Lime Candle Cake";
    const STACK_SIZE: u8 = 1;
}

/// Waxed Weathered Lightning Rod
pub struct WaxedWeatheredLightningRod;

impl ItemDef for WaxedWeatheredLightningRod {
    const ID: i32 = 10870;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_lightning_rod";
    const NAME: &'static str = "Waxed Weathered Lightning Rod";
    const STACK_SIZE: u8 = 1;
}

/// Pale Oak Double Slab
pub struct PaleOakDoubleSlab;

impl ItemDef for PaleOakDoubleSlab {
    const ID: i32 = 10872;
    const STRING_ID: &'static str = "minecraft:pale_oak_double_slab";
    const NAME: &'static str = "Pale Oak Double Slab";
    const STACK_SIZE: u8 = 1;
}

/// Copper Lantern
pub struct CopperLantern;

impl ItemDef for CopperLantern {
    const ID: i32 = 10879;
    const STRING_ID: &'static str = "minecraft:copper_lantern";
    const NAME: &'static str = "Copper Lantern";
    const STACK_SIZE: u8 = 1;
}

/// Tube Coral Wall Fan
pub struct TubeCoralWallFan;

impl ItemDef for TubeCoralWallFan {
    const ID: i32 = 10880;
    const STRING_ID: &'static str = "minecraft:tube_coral_wall_fan";
    const NAME: &'static str = "Tube Coral Wall Fan";
    const STACK_SIZE: u8 = 1;
}

/// All vanilla items as dynamic references.
pub static ITEMS: &[&'static dyn ItemDefDyn] = &[
    &Air,
    &Stone,
    &Granite,
    &PolishedGranite,
    &Diorite,
    &PolishedDiorite,
    &Andesite,
    &PolishedAndesite,
    &Deepslate,
    &CobbledDeepslate,
    &PolishedDeepslate,
    &Calcite,
    &Tuff,
    &TuffSlab,
    &TuffStairs,
    &TuffWall,
    &ChiseledTuff,
    &PolishedTuff,
    &PolishedTuffSlab,
    &PolishedTuffStairs,
    &PolishedTuffWall,
    &TuffBricks,
    &TuffBrickSlab,
    &TuffBrickStairs,
    &TuffBrickWall,
    &ChiseledTuffBricks,
    &DripstoneBlock,
    &GrassBlock,
    &Dirt,
    &CoarseDirt,
    &Podzol,
    &DirtWithRoots,
    &Mud,
    &CrimsonNylium,
    &WarpedNylium,
    &Cobblestone,
    &OakPlanks,
    &SprucePlanks,
    &BirchPlanks,
    &JunglePlanks,
    &AcaciaPlanks,
    &CherryPlanks,
    &DarkOakPlanks,
    &PaleOakPlanks,
    &MangrovePlanks,
    &BambooPlanks,
    &CrimsonPlanks,
    &WarpedPlanks,
    &BambooMosaic,
    &OakSapling,
    &SpruceSapling,
    &BirchSapling,
    &JungleSapling,
    &AcaciaSapling,
    &CherrySapling,
    &DarkOakSapling,
    &PaleOakSapling,
    &MangrovePropagule,
    &Bedrock,
    &Sand,
    &SuspiciousSand,
    &SuspiciousGravel,
    &RedSand,
    &Gravel,
    &CoalOre,
    &DeepslateCoalOre,
    &IronOre,
    &DeepslateIronOre,
    &CopperOre,
    &DeepslateCopperOre,
    &GoldOre,
    &DeepslateGoldOre,
    &RedstoneOre,
    &DeepslateRedstoneOre,
    &EmeraldOre,
    &DeepslateEmeraldOre,
    &LapisOre,
    &DeepslateLapisOre,
    &DiamondOre,
    &DeepslateDiamondOre,
    &NetherGoldOre,
    &QuartzOre,
    &AncientDebris,
    &CoalBlock,
    &RawIronBlock,
    &RawCopperBlock,
    &RawGoldBlock,
    &HeavyCore,
    &AmethystBlock,
    &BuddingAmethyst,
    &IronBlock,
    &CopperBlock,
    &GoldBlock,
    &DiamondBlock,
    &NetheriteBlock,
    &ExposedCopper,
    &WeatheredCopper,
    &OxidizedCopper,
    &ChiseledCopper,
    &ExposedChiseledCopper,
    &WeatheredChiseledCopper,
    &OxidizedChiseledCopper,
    &CutCopper,
    &ExposedCutCopper,
    &WeatheredCutCopper,
    &OxidizedCutCopper,
    &CutCopperStairs,
    &ExposedCutCopperStairs,
    &WeatheredCutCopperStairs,
    &OxidizedCutCopperStairs,
    &CutCopperSlab,
    &ExposedCutCopperSlab,
    &WeatheredCutCopperSlab,
    &OxidizedCutCopperSlab,
    &WaxedCopper,
    &WaxedExposedCopper,
    &WaxedWeatheredCopper,
    &WaxedOxidizedCopper,
    &WaxedChiseledCopper,
    &WaxedExposedChiseledCopper,
    &WaxedWeatheredChiseledCopper,
    &WaxedOxidizedChiseledCopper,
    &WaxedCutCopper,
    &WaxedExposedCutCopper,
    &WaxedWeatheredCutCopper,
    &WaxedOxidizedCutCopper,
    &WaxedCutCopperStairs,
    &WaxedExposedCutCopperStairs,
    &WaxedWeatheredCutCopperStairs,
    &WaxedOxidizedCutCopperStairs,
    &WaxedCutCopperSlab,
    &WaxedExposedCutCopperSlab,
    &WaxedWeatheredCutCopperSlab,
    &WaxedOxidizedCutCopperSlab,
    &OakLog,
    &SpruceLog,
    &BirchLog,
    &JungleLog,
    &AcaciaLog,
    &CherryLog,
    &PaleOakLog,
    &DarkOakLog,
    &MangroveLog,
    &MangroveRoots,
    &MuddyMangroveRoots,
    &CrimsonStem,
    &WarpedStem,
    &BambooBlock,
    &StrippedOakLog,
    &StrippedSpruceLog,
    &StrippedBirchLog,
    &StrippedJungleLog,
    &StrippedAcaciaLog,
    &StrippedCherryLog,
    &StrippedDarkOakLog,
    &StrippedPaleOakLog,
    &StrippedMangroveLog,
    &StrippedCrimsonStem,
    &StrippedWarpedStem,
    &StrippedOakWood,
    &StrippedSpruceWood,
    &StrippedBirchWood,
    &StrippedJungleWood,
    &StrippedAcaciaWood,
    &StrippedCherryWood,
    &StrippedDarkOakWood,
    &StrippedPaleOakWood,
    &StrippedMangroveWood,
    &StrippedCrimsonHyphae,
    &StrippedWarpedHyphae,
    &StrippedBambooBlock,
    &OakWood,
    &SpruceWood,
    &BirchWood,
    &JungleWood,
    &AcaciaWood,
    &CherryWood,
    &PaleOakWood,
    &DarkOakWood,
    &MangroveWood,
    &CrimsonHyphae,
    &WarpedHyphae,
    &OakLeaves,
    &SpruceLeaves,
    &BirchLeaves,
    &JungleLeaves,
    &AcaciaLeaves,
    &CherryLeaves,
    &DarkOakLeaves,
    &PaleOakLeaves,
    &MangroveLeaves,
    &AzaleaLeaves,
    &AzaleaLeavesFlowered,
    &Sponge,
    &WetSponge,
    &Glass,
    &TintedGlass,
    &LapisBlock,
    &Sandstone,
    &ChiseledSandstone,
    &CutSandstone,
    &Web,
    &ShortGrass,
    &Fern,
    &Bush,
    &Azalea,
    &FloweringAzalea,
    &Deadbush,
    &FireflyBush,
    &ShortDryGrass,
    &TallDryGrass,
    &Seagrass,
    &SeaPickle,
    &WhiteWool,
    &OrangeWool,
    &MagentaWool,
    &LightBlueWool,
    &YellowWool,
    &LimeWool,
    &PinkWool,
    &GrayWool,
    &LightGrayWool,
    &CyanWool,
    &PurpleWool,
    &BlueWool,
    &BrownWool,
    &GreenWool,
    &RedWool,
    &BlackWool,
    &Dandelion,
    &OpenEyeblossom,
    &ClosedEyeblossom,
    &Poppy,
    &BlueOrchid,
    &Allium,
    &AzureBluet,
    &RedTulip,
    &OrangeTulip,
    &WhiteTulip,
    &PinkTulip,
    &OxeyeDaisy,
    &Cornflower,
    &LilyOfTheValley,
    &WitherRose,
    &Torchflower,
    &PitcherPlant,
    &SporeBlossom,
    &BrownMushroom,
    &RedMushroom,
    &CrimsonFungus,
    &WarpedFungus,
    &CrimsonRoots,
    &WarpedRoots,
    &NetherSprouts,
    &WeepingVines,
    &TwistingVines,
    &SugarCane,
    &Kelp,
    &PinkPetals,
    &Wildflowers,
    &LeafLitter,
    &MossCarpet,
    &MossBlock,
    &PaleMossCarpet,
    &PaleHangingMoss,
    &PaleMossBlock,
    &HangingRoots,
    &BigDripleaf,
    &SmallDripleafBlock,
    &Bamboo,
    &OakSlab,
    &SpruceSlab,
    &BirchSlab,
    &JungleSlab,
    &AcaciaSlab,
    &CherrySlab,
    &DarkOakSlab,
    &PaleOakSlab,
    &MangroveSlab,
    &BambooSlab,
    &BambooMosaicSlab,
    &CrimsonSlab,
    &WarpedSlab,
    &NormalStoneSlab,
    &SmoothStoneSlab,
    &SandstoneSlab,
    &CutSandstoneSlab,
    &PetrifiedOakSlab,
    &CobblestoneSlab,
    &BrickSlab,
    &StoneBrickSlab,
    &MudBrickSlab,
    &NetherBrickSlab,
    &QuartzSlab,
    &RedSandstoneSlab,
    &CutRedSandstoneSlab,
    &PurpurSlab,
    &PrismarineSlab,
    &PrismarineBrickSlab,
    &DarkPrismarineSlab,
    &SmoothQuartz,
    &SmoothRedSandstone,
    &SmoothSandstone,
    &SmoothStone,
    &BrickBlock,
    &Bookshelf,
    &ChiseledBookshelf,
    &DecoratedPot,
    &MossyCobblestone,
    &Obsidian,
    &Torch,
    &EndRod,
    &ChorusPlant,
    &ChorusFlower,
    &PurpurBlock,
    &PurpurPillar,
    &PurpurStairs,
    &MobSpawner,
    &CreakingHeart,
    &Chest,
    &CraftingTable,
    &Farmland,
    &Furnace,
    &Ladder,
    &StoneStairs,
    &SnowLayer,
    &Ice,
    &Snow,
    &Cactus,
    &CactusFlower,
    &Clay,
    &Jukebox,
    &OakFence,
    &SpruceFence,
    &BirchFence,
    &JungleFence,
    &AcaciaFence,
    &CherryFence,
    &DarkOakFence,
    &PaleOakFence,
    &MangroveFence,
    &BambooFence,
    &CrimsonFence,
    &WarpedFence,
    &Pumpkin,
    &CarvedPumpkin,
    &LitPumpkin,
    &Netherrack,
    &SoulSand,
    &SoulSoil,
    &Basalt,
    &PolishedBasalt,
    &SmoothBasalt,
    &SoulTorch,
    &Glowstone,
    &InfestedStone,
    &InfestedCobblestone,
    &InfestedStoneBricks,
    &InfestedMossyStoneBricks,
    &InfestedCrackedStoneBricks,
    &InfestedChiseledStoneBricks,
    &InfestedDeepslate,
    &StoneBricks,
    &MossyStoneBricks,
    &CrackedStoneBricks,
    &ChiseledStoneBricks,
    &PackedMud,
    &MudBricks,
    &DeepslateBricks,
    &CrackedDeepslateBricks,
    &DeepslateTiles,
    &CrackedDeepslateTiles,
    &ChiseledDeepslate,
    &ReinforcedDeepslate,
    &BrownMushroomBlock,
    &RedMushroomBlock,
    &MushroomStem,
    &IronBars,
    &IronChain,
    &GlassPane,
    &MelonBlock,
    &Vine,
    &GlowLichen,
    &ResinClump,
    &ResinBlock,
    &ResinBricks,
    &ResinBrickStairs,
    &ResinBrickSlab,
    &ResinBrickWall,
    &ChiseledResinBricks,
    &BrickStairs,
    &StoneBrickStairs,
    &MudBrickStairs,
    &Mycelium,
    &Waterlily,
    &NetherBrick,
    &CrackedNetherBricks,
    &ChiseledNetherBricks,
    &NetherBrickFence,
    &NetherBrickStairs,
    &Sculk,
    &SculkVein,
    &SculkCatalyst,
    &SculkShrieker,
    &EnchantingTable,
    &EndPortalFrame,
    &EndStone,
    &EndBricks,
    &DragonEgg,
    &SandstoneStairs,
    &EnderChest,
    &EmeraldBlock,
    &OakStairs,
    &SpruceStairs,
    &BirchStairs,
    &JungleStairs,
    &AcaciaStairs,
    &CherryStairs,
    &DarkOakStairs,
    &PaleOakStairs,
    &MangroveStairs,
    &BambooStairs,
    &BambooMosaicStairs,
    &CrimsonStairs,
    &WarpedStairs,
    &CommandBlock,
    &Beacon,
    &CobblestoneWall,
    &MossyCobblestoneWall,
    &BrickWall,
    &PrismarineWall,
    &RedSandstoneWall,
    &MossyStoneBrickWall,
    &GraniteWall,
    &StoneBrickWall,
    &MudBrickWall,
    &NetherBrickWall,
    &AndesiteWall,
    &RedNetherBrickWall,
    &SandstoneWall,
    &EndStoneBrickWall,
    &DioriteWall,
    &BlackstoneWall,
    &PolishedBlackstoneWall,
    &PolishedBlackstoneBrickWall,
    &CobbledDeepslateWall,
    &PolishedDeepslateWall,
    &DeepslateBrickWall,
    &DeepslateTileWall,
    &Anvil,
    &ChippedAnvil,
    &DamagedAnvil,
    &ChiseledQuartzBlock,
    &QuartzBlock,
    &QuartzBricks,
    &QuartzPillar,
    &QuartzStairs,
    &WhiteTerracotta,
    &OrangeTerracotta,
    &MagentaTerracotta,
    &LightBlueTerracotta,
    &YellowTerracotta,
    &LimeTerracotta,
    &PinkTerracotta,
    &GrayTerracotta,
    &LightGrayTerracotta,
    &CyanTerracotta,
    &PurpleTerracotta,
    &BlueTerracotta,
    &BrownTerracotta,
    &GreenTerracotta,
    &RedTerracotta,
    &BlackTerracotta,
    &Barrier,
    &LightBlock,
    &HayBlock,
    &WhiteCarpet,
    &OrangeCarpet,
    &MagentaCarpet,
    &LightBlueCarpet,
    &YellowCarpet,
    &LimeCarpet,
    &PinkCarpet,
    &GrayCarpet,
    &LightGrayCarpet,
    &CyanCarpet,
    &PurpleCarpet,
    &BlueCarpet,
    &BrownCarpet,
    &GreenCarpet,
    &RedCarpet,
    &BlackCarpet,
    &HardenedClay,
    &PackedIce,
    &GrassPath,
    &Sunflower,
    &Lilac,
    &RoseBush,
    &Peony,
    &TallGrass,
    &LargeFern,
    &WhiteStainedGlass,
    &OrangeStainedGlass,
    &MagentaStainedGlass,
    &LightBlueStainedGlass,
    &YellowStainedGlass,
    &LimeStainedGlass,
    &PinkStainedGlass,
    &GrayStainedGlass,
    &LightGrayStainedGlass,
    &CyanStainedGlass,
    &PurpleStainedGlass,
    &BlueStainedGlass,
    &BrownStainedGlass,
    &GreenStainedGlass,
    &RedStainedGlass,
    &BlackStainedGlass,
    &WhiteStainedGlassPane,
    &OrangeStainedGlassPane,
    &MagentaStainedGlassPane,
    &LightBlueStainedGlassPane,
    &YellowStainedGlassPane,
    &LimeStainedGlassPane,
    &PinkStainedGlassPane,
    &GrayStainedGlassPane,
    &LightGrayStainedGlassPane,
    &CyanStainedGlassPane,
    &PurpleStainedGlassPane,
    &BlueStainedGlassPane,
    &BrownStainedGlassPane,
    &GreenStainedGlassPane,
    &RedStainedGlassPane,
    &BlackStainedGlassPane,
    &Prismarine,
    &PrismarineBricks,
    &DarkPrismarine,
    &PrismarineStairs,
    &PrismarineBricksStairs,
    &DarkPrismarineStairs,
    &SeaLantern,
    &RedSandstone,
    &ChiseledRedSandstone,
    &CutRedSandstone,
    &RedSandstoneStairs,
    &RepeatingCommandBlock,
    &ChainCommandBlock,
    &Magma,
    &NetherWartBlock,
    &WarpedWartBlock,
    &RedNetherBrick,
    &BoneBlock,
    &StructureVoid,
    &UndyedShulkerBox,
    &WhiteShulkerBox,
    &OrangeShulkerBox,
    &MagentaShulkerBox,
    &LightBlueShulkerBox,
    &YellowShulkerBox,
    &LimeShulkerBox,
    &PinkShulkerBox,
    &GrayShulkerBox,
    &LightGrayShulkerBox,
    &CyanShulkerBox,
    &PurpleShulkerBox,
    &BlueShulkerBox,
    &BrownShulkerBox,
    &GreenShulkerBox,
    &RedShulkerBox,
    &BlackShulkerBox,
    &WhiteGlazedTerracotta,
    &OrangeGlazedTerracotta,
    &MagentaGlazedTerracotta,
    &LightBlueGlazedTerracotta,
    &YellowGlazedTerracotta,
    &LimeGlazedTerracotta,
    &PinkGlazedTerracotta,
    &GrayGlazedTerracotta,
    &SilverGlazedTerracotta,
    &CyanGlazedTerracotta,
    &PurpleGlazedTerracotta,
    &BlueGlazedTerracotta,
    &BrownGlazedTerracotta,
    &GreenGlazedTerracotta,
    &RedGlazedTerracotta,
    &BlackGlazedTerracotta,
    &WhiteConcrete,
    &OrangeConcrete,
    &MagentaConcrete,
    &LightBlueConcrete,
    &YellowConcrete,
    &LimeConcrete,
    &PinkConcrete,
    &GrayConcrete,
    &LightGrayConcrete,
    &CyanConcrete,
    &PurpleConcrete,
    &BlueConcrete,
    &BrownConcrete,
    &GreenConcrete,
    &RedConcrete,
    &BlackConcrete,
    &WhiteConcretePowder,
    &OrangeConcretePowder,
    &MagentaConcretePowder,
    &LightBlueConcretePowder,
    &YellowConcretePowder,
    &LimeConcretePowder,
    &PinkConcretePowder,
    &GrayConcretePowder,
    &LightGrayConcretePowder,
    &CyanConcretePowder,
    &PurpleConcretePowder,
    &BlueConcretePowder,
    &BrownConcretePowder,
    &GreenConcretePowder,
    &RedConcretePowder,
    &BlackConcretePowder,
    &TurtleEgg,
    &SnifferEgg,
    &DriedGhast,
    &DeadTubeCoralBlock,
    &DeadBrainCoralBlock,
    &DeadBubbleCoralBlock,
    &DeadFireCoralBlock,
    &DeadHornCoralBlock,
    &TubeCoralBlock,
    &BrainCoralBlock,
    &BubbleCoralBlock,
    &FireCoralBlock,
    &HornCoralBlock,
    &TubeCoral,
    &BrainCoral,
    &BubbleCoral,
    &FireCoral,
    &HornCoral,
    &DeadBrainCoral,
    &DeadBubbleCoral,
    &DeadFireCoral,
    &DeadHornCoral,
    &DeadTubeCoral,
    &TubeCoralFan,
    &BrainCoralFan,
    &BubbleCoralFan,
    &FireCoralFan,
    &HornCoralFan,
    &DeadTubeCoralFan,
    &DeadBrainCoralFan,
    &DeadBubbleCoralFan,
    &DeadFireCoralFan,
    &DeadHornCoralFan,
    &BlueIce,
    &Conduit,
    &PolishedGraniteStairs,
    &SmoothRedSandstoneStairs,
    &MossyStoneBrickStairs,
    &PolishedDioriteStairs,
    &MossyCobblestoneStairs,
    &EndBrickStairs,
    &NormalStoneStairs,
    &SmoothSandstoneStairs,
    &SmoothQuartzStairs,
    &GraniteStairs,
    &AndesiteStairs,
    &RedNetherBrickStairs,
    &PolishedAndesiteStairs,
    &DioriteStairs,
    &CobbledDeepslateStairs,
    &PolishedDeepslateStairs,
    &DeepslateBrickStairs,
    &DeepslateTileStairs,
    &PolishedGraniteSlab,
    &SmoothRedSandstoneSlab,
    &MossyStoneBrickSlab,
    &PolishedDioriteSlab,
    &MossyCobblestoneSlab,
    &EndStoneBrickSlab,
    &SmoothSandstoneSlab,
    &SmoothQuartzSlab,
    &GraniteSlab,
    &AndesiteSlab,
    &RedNetherBrickSlab,
    &PolishedAndesiteSlab,
    &DioriteSlab,
    &CobbledDeepslateSlab,
    &PolishedDeepslateSlab,
    &DeepslateBrickSlab,
    &DeepslateTileSlab,
    &Scaffolding,
    &Redstone,
    &RedstoneTorch,
    &RedstoneBlock,
    &Repeater,
    &Comparator,
    &Piston,
    &StickyPiston,
    &Slime,
    &HoneyBlock,
    &Observer,
    &Hopper,
    &Dispenser,
    &Dropper,
    &Lectern,
    &Target,
    &Lever,
    &LightningRod,
    &DaylightDetector,
    &SculkSensor,
    &CalibratedSculkSensor,
    &TripwireHook,
    &TrappedChest,
    &Tnt,
    &RedstoneLamp,
    &Noteblock,
    &StoneButton,
    &PolishedBlackstoneButton,
    &WoodenButton,
    &SpruceButton,
    &BirchButton,
    &JungleButton,
    &AcaciaButton,
    &CherryButton,
    &DarkOakButton,
    &PaleOakButton,
    &MangroveButton,
    &BambooButton,
    &CrimsonButton,
    &WarpedButton,
    &StonePressurePlate,
    &PolishedBlackstonePressurePlate,
    &LightWeightedPressurePlate,
    &HeavyWeightedPressurePlate,
    &WoodenPressurePlate,
    &SprucePressurePlate,
    &BirchPressurePlate,
    &JunglePressurePlate,
    &AcaciaPressurePlate,
    &CherryPressurePlate,
    &DarkOakPressurePlate,
    &PaleOakPressurePlate,
    &MangrovePressurePlate,
    &BambooPressurePlate,
    &CrimsonPressurePlate,
    &WarpedPressurePlate,
    &IronDoor,
    &WoodenDoor,
    &SpruceDoor,
    &BirchDoor,
    &JungleDoor,
    &AcaciaDoor,
    &CherryDoor,
    &DarkOakDoor,
    &PaleOakDoor,
    &MangroveDoor,
    &BambooDoor,
    &CrimsonDoor,
    &WarpedDoor,
    &CopperDoor,
    &ExposedCopperDoor,
    &WeatheredCopperDoor,
    &OxidizedCopperDoor,
    &WaxedCopperDoor,
    &WaxedExposedCopperDoor,
    &WaxedWeatheredCopperDoor,
    &WaxedOxidizedCopperDoor,
    &IronTrapdoor,
    &Trapdoor,
    &SpruceTrapdoor,
    &BirchTrapdoor,
    &JungleTrapdoor,
    &AcaciaTrapdoor,
    &CherryTrapdoor,
    &DarkOakTrapdoor,
    &PaleOakTrapdoor,
    &MangroveTrapdoor,
    &BambooTrapdoor,
    &CrimsonTrapdoor,
    &WarpedTrapdoor,
    &CopperTrapdoor,
    &ExposedCopperTrapdoor,
    &WeatheredCopperTrapdoor,
    &OxidizedCopperTrapdoor,
    &WaxedCopperTrapdoor,
    &WaxedExposedCopperTrapdoor,
    &WaxedWeatheredCopperTrapdoor,
    &WaxedOxidizedCopperTrapdoor,
    &FenceGate,
    &SpruceFenceGate,
    &BirchFenceGate,
    &JungleFenceGate,
    &AcaciaFenceGate,
    &CherryFenceGate,
    &DarkOakFenceGate,
    &PaleOakFenceGate,
    &MangroveFenceGate,
    &BambooFenceGate,
    &CrimsonFenceGate,
    &WarpedFenceGate,
    &GoldenRail,
    &DetectorRail,
    &Rail,
    &ActivatorRail,
    &Saddle,
    &WhiteHarness,
    &OrangeHarness,
    &MagentaHarness,
    &LightBlueHarness,
    &YellowHarness,
    &LimeHarness,
    &PinkHarness,
    &GrayHarness,
    &LightGrayHarness,
    &CyanHarness,
    &PurpleHarness,
    &BlueHarness,
    &BrownHarness,
    &GreenHarness,
    &RedHarness,
    &BlackHarness,
    &Minecart,
    &ChestMinecart,
    &HopperMinecart,
    &TntMinecart,
    &CarrotOnAStick,
    &WarpedFungusOnAStick,
    &PhantomMembrane,
    &Elytra,
    &OakBoat,
    &OakChestBoat,
    &SpruceBoat,
    &SpruceChestBoat,
    &BirchBoat,
    &BirchChestBoat,
    &JungleBoat,
    &JungleChestBoat,
    &AcaciaBoat,
    &AcaciaChestBoat,
    &CherryBoat,
    &CherryChestBoat,
    &DarkOakBoat,
    &DarkOakChestBoat,
    &PaleOakBoat,
    &PaleOakChestBoat,
    &MangroveBoat,
    &MangroveChestBoat,
    &BambooRaft,
    &BambooChestRaft,
    &StructureBlock,
    &Jigsaw,
    &Unknown,
    &TurtleHelmet,
    &TurtleScute,
    &ArmadilloScute,
    &WolfArmor,
    &FlintAndSteel,
    &Bowl,
    &Apple,
    &Bow,
    &Arrow,
    &Coal,
    &Charcoal,
    &Diamond,
    &Emerald,
    &LapisLazuli,
    &Quartz,
    &AmethystShard,
    &RawIron,
    &IronIngot,
    &RawCopper,
    &CopperIngot,
    &RawGold,
    &GoldIngot,
    &NetheriteIngot,
    &NetheriteScrap,
    &WoodenSword,
    &WoodenShovel,
    &WoodenPickaxe,
    &WoodenAxe,
    &WoodenHoe,
    &StoneSword,
    &StoneShovel,
    &StonePickaxe,
    &StoneAxe,
    &StoneHoe,
    &GoldenSword,
    &GoldenShovel,
    &GoldenPickaxe,
    &GoldenAxe,
    &GoldenHoe,
    &IronSword,
    &IronShovel,
    &IronPickaxe,
    &IronAxe,
    &IronHoe,
    &DiamondSword,
    &DiamondShovel,
    &DiamondPickaxe,
    &DiamondAxe,
    &DiamondHoe,
    &NetheriteSword,
    &NetheriteShovel,
    &NetheritePickaxe,
    &NetheriteAxe,
    &NetheriteHoe,
    &Stick,
    &MushroomStew,
    &String,
    &Feather,
    &Gunpowder,
    &WheatSeeds,
    &Wheat,
    &Bread,
    &LeatherHelmet,
    &LeatherChestplate,
    &LeatherLeggings,
    &LeatherBoots,
    &ChainmailHelmet,
    &ChainmailChestplate,
    &ChainmailLeggings,
    &ChainmailBoots,
    &IronHelmet,
    &IronChestplate,
    &IronLeggings,
    &IronBoots,
    &DiamondHelmet,
    &DiamondChestplate,
    &DiamondLeggings,
    &DiamondBoots,
    &GoldenHelmet,
    &GoldenChestplate,
    &GoldenLeggings,
    &GoldenBoots,
    &NetheriteHelmet,
    &NetheriteChestplate,
    &NetheriteLeggings,
    &NetheriteBoots,
    &Flint,
    &Porkchop,
    &CookedPorkchop,
    &Painting,
    &GoldenApple,
    &EnchantedGoldenApple,
    &OakSign,
    &SpruceSign,
    &BirchSign,
    &JungleSign,
    &AcaciaSign,
    &CherrySign,
    &DarkOakSign,
    &PaleOakSign,
    &MangroveSign,
    &BambooSign,
    &CrimsonSign,
    &WarpedSign,
    &OakHangingSign,
    &SpruceHangingSign,
    &BirchHangingSign,
    &JungleHangingSign,
    &AcaciaHangingSign,
    &CherryHangingSign,
    &DarkOakHangingSign,
    &PaleOakHangingSign,
    &MangroveHangingSign,
    &BambooHangingSign,
    &CrimsonHangingSign,
    &WarpedHangingSign,
    &Bucket,
    &WaterBucket,
    &LavaBucket,
    &PowderSnowBucket,
    &Snowball,
    &Leather,
    &MilkBucket,
    &PufferfishBucket,
    &SalmonBucket,
    &CodBucket,
    &TropicalFishBucket,
    &AxolotlBucket,
    &TadpoleBucket,
    &Brick,
    &ClayBall,
    &DriedKelpBlock,
    &Paper,
    &Book,
    &SlimeBall,
    &Egg,
    &BlueEgg,
    &BrownEgg,
    &Compass,
    &RecoveryCompass,
    &Bundle,
    &WhiteBundle,
    &OrangeBundle,
    &MagentaBundle,
    &LightBlueBundle,
    &YellowBundle,
    &LimeBundle,
    &PinkBundle,
    &GrayBundle,
    &LightGrayBundle,
    &CyanBundle,
    &PurpleBundle,
    &BlueBundle,
    &BrownBundle,
    &GreenBundle,
    &RedBundle,
    &BlackBundle,
    &FishingRod,
    &Clock,
    &Spyglass,
    &GlowstoneDust,
    &Cod,
    &Salmon,
    &TropicalFish,
    &Pufferfish,
    &CookedCod,
    &CookedSalmon,
    &InkSac,
    &GlowInkSac,
    &CocoaBeans,
    &WhiteDye,
    &OrangeDye,
    &MagentaDye,
    &LightBlueDye,
    &YellowDye,
    &LimeDye,
    &PinkDye,
    &GrayDye,
    &LightGrayDye,
    &CyanDye,
    &PurpleDye,
    &BlueDye,
    &BrownDye,
    &GreenDye,
    &RedDye,
    &BlackDye,
    &BoneMeal,
    &Bone,
    &Sugar,
    &Cake,
    &Bed,
    &Cookie,
    &Crafter,
    &FilledMap,
    &Shears,
    &MelonSlice,
    &DriedKelp,
    &PumpkinSeeds,
    &MelonSeeds,
    &Beef,
    &CookedBeef,
    &Chicken,
    &CookedChicken,
    &RottenFlesh,
    &EnderPearl,
    &BlazeRod,
    &GhastTear,
    &GoldNugget,
    &NetherWart,
    &GlassBottle,
    &Potion,
    &SpiderEye,
    &FermentedSpiderEye,
    &BlazePowder,
    &MagmaCream,
    &BrewingStand,
    &Cauldron,
    &EnderEye,
    &GlisteringMelonSlice,
    &ArmadilloSpawnEgg,
    &AllaySpawnEgg,
    &AxolotlSpawnEgg,
    &BatSpawnEgg,
    &BeeSpawnEgg,
    &BlazeSpawnEgg,
    &BoggedSpawnEgg,
    &BreezeSpawnEgg,
    &CatSpawnEgg,
    &CamelSpawnEgg,
    &CaveSpiderSpawnEgg,
    &ChickenSpawnEgg,
    &CodSpawnEgg,
    &CowSpawnEgg,
    &CreeperSpawnEgg,
    &DolphinSpawnEgg,
    &DonkeySpawnEgg,
    &DrownedSpawnEgg,
    &ElderGuardianSpawnEgg,
    &EnderDragonSpawnEgg,
    &EndermanSpawnEgg,
    &EndermiteSpawnEgg,
    &EvokerSpawnEgg,
    &FoxSpawnEgg,
    &FrogSpawnEgg,
    &GhastSpawnEgg,
    &HappyGhastSpawnEgg,
    &GlowSquidSpawnEgg,
    &GoatSpawnEgg,
    &GuardianSpawnEgg,
    &HoglinSpawnEgg,
    &HorseSpawnEgg,
    &HuskSpawnEgg,
    &IronGolemSpawnEgg,
    &LlamaSpawnEgg,
    &MagmaCubeSpawnEgg,
    &MooshroomSpawnEgg,
    &MuleSpawnEgg,
    &OcelotSpawnEgg,
    &PandaSpawnEgg,
    &ParrotSpawnEgg,
    &PhantomSpawnEgg,
    &PigSpawnEgg,
    &PiglinSpawnEgg,
    &PiglinBruteSpawnEgg,
    &PillagerSpawnEgg,
    &PolarBearSpawnEgg,
    &PufferfishSpawnEgg,
    &RabbitSpawnEgg,
    &RavagerSpawnEgg,
    &SalmonSpawnEgg,
    &SheepSpawnEgg,
    &ShulkerSpawnEgg,
    &SilverfishSpawnEgg,
    &SkeletonSpawnEgg,
    &SkeletonHorseSpawnEgg,
    &SlimeSpawnEgg,
    &SnifferSpawnEgg,
    &SnowGolemSpawnEgg,
    &SpiderSpawnEgg,
    &SquidSpawnEgg,
    &StraySpawnEgg,
    &StriderSpawnEgg,
    &TadpoleSpawnEgg,
    &TraderLlamaSpawnEgg,
    &TropicalFishSpawnEgg,
    &TurtleSpawnEgg,
    &VexSpawnEgg,
    &VillagerSpawnEgg,
    &VindicatorSpawnEgg,
    &WanderingTraderSpawnEgg,
    &WardenSpawnEgg,
    &WitchSpawnEgg,
    &WitherSpawnEgg,
    &WitherSkeletonSpawnEgg,
    &WolfSpawnEgg,
    &ZoglinSpawnEgg,
    &CreakingSpawnEgg,
    &ZombieSpawnEgg,
    &ZombieHorseSpawnEgg,
    &ZombieVillagerSpawnEgg,
    &ZombiePigmanSpawnEgg,
    &ExperienceBottle,
    &FireCharge,
    &WindCharge,
    &WritableBook,
    &WrittenBook,
    &BreezeRod,
    &Mace,
    &Frame,
    &GlowFrame,
    &FlowerPot,
    &Carrot,
    &Potato,
    &BakedPotato,
    &PoisonousPotato,
    &EmptyMap,
    &GoldenCarrot,
    &SkeletonSkull,
    &WitherSkeletonSkull,
    &PlayerHead,
    &ZombieHead,
    &CreeperHead,
    &DragonHead,
    &PiglinHead,
    &NetherStar,
    &PumpkinPie,
    &FireworkRocket,
    &FireworkStar,
    &EnchantedBook,
    &Netherbrick,
    &ResinBrick,
    &PrismarineShard,
    &PrismarineCrystals,
    &Rabbit,
    &CookedRabbit,
    &RabbitStew,
    &RabbitFoot,
    &RabbitHide,
    &ArmorStand,
    &IronHorseArmor,
    &GoldenHorseArmor,
    &DiamondHorseArmor,
    &LeatherHorseArmor,
    &Lead,
    &NameTag,
    &CommandBlockMinecart,
    &Mutton,
    &CookedMutton,
    &Banner,
    &EndCrystal,
    &ChorusFruit,
    &PoppedChorusFruit,
    &TorchflowerSeeds,
    &PitcherPod,
    &Beetroot,
    &BeetrootSeeds,
    &BeetrootSoup,
    &DragonBreath,
    &SplashPotion,
    &LingeringPotion,
    &Shield,
    &TotemOfUndying,
    &ShulkerShell,
    &IronNugget,
    &MusicDisc13,
    &MusicDiscCat,
    &MusicDiscBlocks,
    &MusicDiscChirp,
    &MusicDiscCreator,
    &MusicDiscCreatorMusicBox,
    &MusicDiscFar,
    &MusicDiscLavaChicken,
    &MusicDiscMall,
    &MusicDiscMellohi,
    &MusicDiscStal,
    &MusicDiscStrad,
    &MusicDiscWard,
    &MusicDisc11,
    &MusicDiscWait,
    &MusicDiscOtherside,
    &MusicDiscRelic,
    &MusicDisc5,
    &MusicDiscPigstep,
    &MusicDiscPrecipice,
    &MusicDiscTears,
    &DiscFragment5,
    &Trident,
    &NautilusShell,
    &HeartOfTheSea,
    &Crossbow,
    &SuspiciousStew,
    &Loom,
    &FlowerBannerPattern,
    &CreeperBannerPattern,
    &SkullBannerPattern,
    &MojangBannerPattern,
    &GlobeBannerPattern,
    &PiglinBannerPattern,
    &FlowBannerPattern,
    &GusterBannerPattern,
    &FieldMasonedBannerPattern,
    &BordureIndentedBannerPattern,
    &GoatHorn,
    &Composter,
    &Barrel,
    &Smoker,
    &BlastFurnace,
    &CartographyTable,
    &FletchingTable,
    &Grindstone,
    &SmithingTable,
    &StonecutterBlock,
    &Bell,
    &Lantern,
    &SoulLantern,
    &SweetBerries,
    &GlowBerries,
    &Campfire,
    &SoulCampfire,
    &Shroomlight,
    &Honeycomb,
    &BeeNest,
    &Beehive,
    &HoneyBottle,
    &HoneycombBlock,
    &Lodestone,
    &CryingObsidian,
    &Blackstone,
    &BlackstoneSlab,
    &BlackstoneStairs,
    &GildedBlackstone,
    &PolishedBlackstone,
    &PolishedBlackstoneSlab,
    &PolishedBlackstoneStairs,
    &ChiseledPolishedBlackstone,
    &PolishedBlackstoneBricks,
    &PolishedBlackstoneBrickSlab,
    &PolishedBlackstoneBrickStairs,
    &CrackedPolishedBlackstoneBricks,
    &RespawnAnchor,
    &Candle,
    &WhiteCandle,
    &OrangeCandle,
    &MagentaCandle,
    &LightBlueCandle,
    &YellowCandle,
    &LimeCandle,
    &PinkCandle,
    &GrayCandle,
    &LightGrayCandle,
    &CyanCandle,
    &PurpleCandle,
    &BlueCandle,
    &BrownCandle,
    &GreenCandle,
    &RedCandle,
    &BlackCandle,
    &SmallAmethystBud,
    &MediumAmethystBud,
    &LargeAmethystBud,
    &AmethystCluster,
    &PointedDripstone,
    &OchreFroglight,
    &VerdantFroglight,
    &PearlescentFroglight,
    &FrogSpawn,
    &EchoShard,
    &Brush,
    &NetheriteUpgradeSmithingTemplate,
    &SentryArmorTrimSmithingTemplate,
    &DuneArmorTrimSmithingTemplate,
    &CoastArmorTrimSmithingTemplate,
    &WildArmorTrimSmithingTemplate,
    &WardArmorTrimSmithingTemplate,
    &EyeArmorTrimSmithingTemplate,
    &VexArmorTrimSmithingTemplate,
    &TideArmorTrimSmithingTemplate,
    &SnoutArmorTrimSmithingTemplate,
    &RibArmorTrimSmithingTemplate,
    &SpireArmorTrimSmithingTemplate,
    &WayfinderArmorTrimSmithingTemplate,
    &ShaperArmorTrimSmithingTemplate,
    &SilenceArmorTrimSmithingTemplate,
    &RaiserArmorTrimSmithingTemplate,
    &HostArmorTrimSmithingTemplate,
    &FlowArmorTrimSmithingTemplate,
    &BoltArmorTrimSmithingTemplate,
    &AnglerPotterySherd,
    &ArcherPotterySherd,
    &ArmsUpPotterySherd,
    &BladePotterySherd,
    &BrewerPotterySherd,
    &BurnPotterySherd,
    &DangerPotterySherd,
    &ExplorerPotterySherd,
    &FlowPotterySherd,
    &FriendPotterySherd,
    &GusterPotterySherd,
    &HeartPotterySherd,
    &HeartbreakPotterySherd,
    &HowlPotterySherd,
    &MinerPotterySherd,
    &MournerPotterySherd,
    &PlentyPotterySherd,
    &PrizePotterySherd,
    &ScrapePotterySherd,
    &SheafPotterySherd,
    &ShelterPotterySherd,
    &SkullPotterySherd,
    &SnortPotterySherd,
    &CopperGrate,
    &ExposedCopperGrate,
    &WeatheredCopperGrate,
    &OxidizedCopperGrate,
    &WaxedCopperGrate,
    &WaxedExposedCopperGrate,
    &WaxedWeatheredCopperGrate,
    &WaxedOxidizedCopperGrate,
    &CopperBulb,
    &ExposedCopperBulb,
    &WeatheredCopperBulb,
    &OxidizedCopperBulb,
    &WaxedCopperBulb,
    &WaxedExposedCopperBulb,
    &WaxedWeatheredCopperBulb,
    &WaxedOxidizedCopperBulb,
    &TrialSpawner,
    &TrialKey,
    &OminousTrialKey,
    &Vault,
    &OminousBottle,
    &MangroveDoorItem,
    &RapidFertilizer,
    &Sparkler,
    &UnderwaterTnt,
    &FrameItem,
    &Element15,
    &PolishedTuffDoubleSlab,
    &Balloon,
    &SmoothSandstoneDoubleSlab,
    &Element104,
    &WhiteCandleCake,
    &DeadTubeCoralWallFan,
    &Element13,
    &Element43,
    &CopperSword,
    &Element68,
    &Element50,
    &Board,
    &HardBlueStainedGlass,
    &IronDoorItem,
    &Element27,
    &WoodenSlab,
    &MagentaCandleCake,
    &CampfireItem,
    &Element62,
    &HardPinkStainedGlass,
    &Element2,
    &AcaciaShelf,
    &PowderSnow,
    &Element80,
    &HardBrownStainedGlass,
    &CopperGolemStatue,
    &Portal,
    &WaxedExposedCopperLantern,
    &LightBlock15,
    &Planks,
    &StainedGlassPane,
    &ColoredTorchPurple,
    &HardGlass,
    &FlowingWater,
    &LitDeepslateRedstoneOre,
    &LitRedstoneLamp,
    &Element52,
    &Element86,
    &PetrifiedOakDoubleSlab,
    &EndGateway,
    &BeetrootItem,
    &DarkOakDoubleSlab,
    &HardCyanStainedGlassPane,
    &Element51,
    &HardCyanStainedGlass,
    &AgentSpawnEgg,
    &Carpet,
    &ColoredTorchBlue,
    &CherryWallSign,
    &Element74,
    &DeadBrainCoralWallFan,
    &OxidizedCopperChain,
    &NetherWartItem,
    &WaxedWeatheredCopperBars,
    &DeadFireCoralWallFan,
    &Element97,
    &ChemistryTable,
    &HardBrownStainedGlassPane,
    &HardLimeStainedGlassPane,
    &Element23,
    &Coral,
    &Reserved6,
    &ShulkerBox,
    &RedCandleCake,
    &Deny,
    &PaleOakWallSign,
    &CakeItem,
    &ExposedCopperBars,
    &AndesiteDoubleSlab,
    &StickyPistonArmCollision,
    &QuartzDoubleSlab,
    &Element41,
    &LightBlock0,
    &StainedGlass,
    &FlowerPotItem,
    &CompoundCreator,
    &Camera,
    &NetherBrickDoubleSlab,
    &HardRedStainedGlass,
    &SmoothStoneDoubleSlab,
    &Element44,
    &ColoredTorchRg,
    &Bleach,
    &HardRedStainedGlassPane,
    &Chalkboard,
    &PinkCandleCake,
    &DoublePlant,
    &Element109,
    &LightBlock14,
    &NpcSpawnEgg,
    &SpruceWallSign,
    &DaylightDetectorInverted,
    &DioriteDoubleSlab,
    &StandingSign,
    &NormalStoneDoubleSlab,
    &DoubleCutCopperSlab,
    &ElementConstructor,
    &AcaciaDoorItem,
    &PurpurDoubleSlab,
    &OrangeCandleCake,
    &DoubleStoneBlockSlab3,
    &Element69,
    &CopperTorch,
    &DeepslateBrickDoubleSlab,
    &DarkPrismarineDoubleSlab,
    &CopperChain,
    &Element102,
    &ColoredTorchBp,
    &DeadBubbleCoralWallFan,
    &InfoUpdate2,
    &Element32,
    &Element42,
    &CoralFanDead,
    &CherryShelf,
    &CopperAxe,
    &MonsterEgg,
    &PurpleCandleCake,
    &Potatoes,
    &Boat,
    &BirchShelf,
    &BlueCandleCake,
    &Element22,
    &Compound,
    &IceBomb,
    &Medicine,
    &GlowStick,
    &Element83,
    &LodestoneCompass,
    &WaxedCopperGolemStatue,
    &PolishedDeepslateDoubleSlab,
    &MossyCobblestoneDoubleSlab,
    &Concrete,
    &Element33,
    &WaxedWeatheredDoubleCutCopperSlab,
    &JungleStandingSign,
    &CandleCake,
    &InfoUpdate,
    &ChestBoat,
    &WaxedExposedCopperChest,
    &LitFurnace,
    &Element89,
    &CrimsonShelf,
    &DoubleStoneBlockSlab,
    &StoneBrickDoubleSlab,
    &BrickDoubleSlab,
    &UnlitRedstoneTorch,
    &Element118,
    &Element4,
    &WeatheredCopperGolemStatue,
    &Wool,
    &LightBlock10,
    &Element11,
    &CobblestoneDoubleSlab,
    &Skull,
    &CopperNugget,
    &WaxedOxidizedCopperLantern,
    &CopperHorseArmor,
    &SmoothRedSandstoneDoubleSlab,
    &StainedHardenedClay,
    &Element9,
    &StoneBlockSlab4,
    &DoubleStoneBlockSlab2,
    &CopperGolemSpawnEgg,
    &CopperShovel,
    &HardGrayStainedGlass,
    &TripWire,
    &CopperPickaxe,
    &CaveVinesBodyWithBerries,
    &CopperHoe,
    &LightBlock2,
    &CopperHelmet,
    &SpruceStandingSign,
    &CopperChestplate,
    &CopperLeggings,
    &DoubleStoneBlockSlab4,
    &CopperBoots,
    &PrismarineBrickDoubleSlab,
    &Element18,
    &CherryDoubleSlab,
    &Element29,
    &HardBlackStainedGlass,
    &Log,
    &Element53,
    &Fence,
    &WaxedOxidizedDoubleCutCopperSlab,
    &OxidizedCopperGolemStatue,
    &Stonebrick,
    &OxidizedCopperLantern,
    &LitBlastFurnace,
    &CoralBlock,
    &StoneBlockSlab,
    &Leaves,
    &StoneBlockSlab2,
    &Leaves2,
    &BirchStandingSign,
    &StoneBlockSlab3,
    &Element16,
    &SandstoneDoubleSlab,
    &RedSandstoneDoubleSlab,
    &PrismarineDoubleSlab,
    &RedNetherBrickDoubleSlab,
    &EndStoneBrickDoubleSlab,
    &PolishedAndesiteDoubleSlab,
    &BorderBlock,
    &PolishedDioriteDoubleSlab,
    &GraniteDoubleSlab,
    &Element10,
    &PolishedGraniteDoubleSlab,
    &MossyStoneBrickDoubleSlab,
    &SmoothQuartzDoubleSlab,
    &CutSandstoneDoubleSlab,
    &CutRedSandstoneDoubleSlab,
    &SweetBerryBush,
    &CoralFan,
    &Sapling,
    &SoulFire,
    &RedFlower,
    &DeprecatedPurpurBlock1,
    &Element77,
    &DeprecatedPurpurBlock2,
    &Tallgrass,
    &Element103,
    &Log2,
    &DeprecatedAnvil,
    &Element56,
    &ConcretePowder,
    &Element75,
    &Element64,
    &HopperItem,
    &Wood,
    &HardMagentaStainedGlass,
    &MudBrickDoubleSlab,
    &CrimsonDoubleSlab,
    &HardPurpleStainedGlass,
    &DarkOakDoorItem,
    &HardGreenStainedGlassPane,
    &TorchflowerCrop,
    &MaterialReducer,
    &LabTable,
    &HardWhiteStainedGlass,
    &HardOrangeStainedGlass,
    &HardLightBlueStainedGlass,
    &HardYellowStainedGlass,
    &HardLimeStainedGlass,
    &HardLightGrayStainedGlass,
    &HardGreenStainedGlass,
    &Element84,
    &HardStainedGlass,
    &HardWhiteStainedGlassPane,
    &HardOrangeStainedGlassPane,
    &HardMagentaStainedGlassPane,
    &HardLightBlueStainedGlassPane,
    &HardYellowStainedGlassPane,
    &HardPinkStainedGlassPane,
    &Carrots,
    &HardGrayStainedGlassPane,
    &HardLightGrayStainedGlassPane,
    &HardPurpleStainedGlassPane,
    &HardBlueStainedGlassPane,
    &HardBlackStainedGlassPane,
    &HardStainedGlassPane,
    &ColoredTorchRed,
    &ColoredTorchGreen,
    &LightBlock1,
    &MangroveShelf,
    &LightBlock3,
    &LightBlock4,
    &GrayCandleCake,
    &LightBlock5,
    &LightBlock6,
    &LightBlock7,
    &DeepslateTileDoubleSlab,
    &LightBlock8,
    &LightBlock9,
    &WaxedOxidizedCopperBars,
    &LightBlock11,
    &LightBlock12,
    &LightBlock13,
    &Fire,
    &BlackCandleCake,
    &Element0,
    &Element1,
    &Element3,
    &Element5,
    &Element6,
    &Element7,
    &Element8,
    &Element12,
    &Element14,
    &PaleOakStandingSign,
    &ClientRequestPlaceholderBlock,
    &Element17,
    &Element19,
    &Element20,
    &Element21,
    &Element24,
    &Element25,
    &Element26,
    &Element28,
    &Element30,
    &Element31,
    &Element34,
    &BambooDoubleSlab,
    &Element35,
    &Element36,
    &Element37,
    &Element38,
    &Element39,
    &Element40,
    &Element45,
    &Element46,
    &Element47,
    &Element48,
    &Element49,
    &Element54,
    &Element55,
    &Element57,
    &Element58,
    &Element59,
    &Element60,
    &Element61,
    &Element63,
    &Element65,
    &Element66,
    &Element67,
    &Element70,
    &Element71,
    &Element72,
    &Element73,
    &Element76,
    &Element78,
    &Element79,
    &WeatheredCopperBars,
    &Element81,
    &ExposedLightningRod,
    &Element82,
    &Element85,
    &Element87,
    &Element88,
    &Element90,
    &Element91,
    &Element92,
    &Element93,
    &Element94,
    &Element95,
    &Element96,
    &Element98,
    &Element99,
    &Element100,
    &Element101,
    &Element105,
    &Element106,
    &Element107,
    &Element108,
    &Element110,
    &Element111,
    &Element112,
    &Element113,
    &Element114,
    &Element115,
    &Element116,
    &Element117,
    &Dye,
    &BannerPattern,
    &SpawnEgg,
    &WaxedWeatheredCopperChain,
    &WaxedWeatheredCopperChest,
    &WarpedDoorItem,
    &PistonArmCollision,
    &BlackstoneDoubleSlab,
    &CrimsonWallSign,
    &GlowFrameItem,
    &WaxedExposedCopperChain,
    &WarpedStandingSign,
    &PitcherCrop,
    &LightBlueCandleCake,
    &OxidizedLightningRod,
    &PoweredComparator,
    &WarpedWallSign,
    &MangroveDoubleSlab,
    &OxidizedDoubleCutCopperSlab,
    &WaxedCopperBars,
    &JungleShelf,
    &ExposedDoubleCutCopperSlab,
    &PolishedBlackstoneDoubleSlab,
    &HardGlassPane,
    &PolishedBlackstoneBrickDoubleSlab,
    &ResinBrickDoubleSlab,
    &CyanCandleCake,
    &Stonecutter,
    &InvisibleBedrock,
    &OxidizedCopperBars,
    &UnderwaterTorch,
    &WallBanner,
    &SpruceDoubleSlab,
    &Glowingobsidian,
    &ExposedCopperLantern,
    &WaxedExposedCopperBars,
    &SpruceShelf,
    &MovingBlock,
    &GreenCandleCake,
    &WaxedLightningRod,
    &WarpedShelf,
    &CopperBars,
    &OakDoubleSlab,
    &BrownCandleCake,
    &AcaciaWallSign,
    &CopperChest,
    &WoodenDoorItem,
    &RedstoneWire,
    &Lava,
    &WaxedWeatheredCopperLantern,
    &CrimsonDoorItem,
    &WaxedExposedLightningRod,
    &BrainCoralWallFan,
    &DarkoakStandingSign,
    &WaxedExposedCopperGolemStatue,
    &WaxedOxidizedCopperChest,
    &WaxedOxidizedCopperChain,
    &BambooMosaicDoubleSlab,
    &MangroveStandingSign,
    &DarkOakShelf,
    &LitRedstoneOre,
    &BambooShelf,
    &WaxedCopperChest,
    &WaxedCopperChain,
    &WarpedDoubleSlab,
    &JungleWallSign,
    &OakShelf,
    &PoweredRepeater,
    &YellowCandleCake,
    &WaxedWeatheredCopperGolemStatue,
    &WheatItem,
    &SpruceDoorItem,
    &FrostedIce,
    &CaveVines,
    &MelonStem,
    &HornCoralWallFan,
    &WallSign,
    &WaxedOxidizedCopperGolemStatue,
    &BirchDoubleSlab,
    &WeatheredLightningRod,
    &MangroveWallSign,
    &LightGrayCandleCake,
    &DarkoakWallSign,
    &FireCoralWallFan,
    &FlowingLava,
    &ExposedCopperChest,
    &ExposedCopperChain,
    &WaxedDoubleCutCopperSlab,
    &KelpItem,
    &Water,
    &ChemicalHeat,
    &UnpoweredRepeater,
    &WeatheredCopperChest,
    &WeatheredCopperChain,
    &AcaciaDoubleSlab,
    &BubbleColumn,
    &CobbledDeepslateDoubleSlab,
    &CherryStandingSign,
    &PaleOakShelf,
    &TuffBrickDoubleSlab,
    &Reeds,
    &CameraItem,
    &JungleDoorItem,
    &AcaciaStandingSign,
    &PumpkinStem,
    &UnpoweredComparator,
    &WeatheredCopperLantern,
    &NetherSproutsItem,
    &Cocoa,
    &BedItem,
    &OxidizedCopperChest,
    &WaxedExposedDoubleCutCopperSlab,
    &BubbleCoralWallFan,
    &BirchWallSign,
    &BambooWallSign,
    &BambooSapling,
    &StandingBanner,
    &JungleDoubleSlab,
    &DeadHornCoralWallFan,
    &WeatheredDoubleCutCopperSlab,
    &WaxedOxidizedLightningRod,
    &Allow,
    &BirchDoorItem,
    &BambooStandingSign,
    &CrimsonStandingSign,
    &Netherreactor,
    &ExposedCopperGolemStatue,
    &CauldronItem,
    &CaveVinesHeadWithBerries,
    &BrewingStandItem,
    &EndPortal,
    &LitSmoker,
    &WaxedCopperLantern,
    &TuffDoubleSlab,
    &SoulCampfireItem,
    &LimeCandleCake,
    &WaxedWeatheredLightningRod,
    &PaleOakDoubleSlab,
    &CopperLantern,
    &TubeCoralWallFan,
];
