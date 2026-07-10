//! Generated vanilla block definitions.
//! Do not edit: regenerate with valentine_gen.

use valentine_bedrock_core::block::{BlockDef, BlockDefDyn};

/// Cyan Terracotta
pub struct CyanTerracotta;

impl BlockDef for CyanTerracotta {
    const ID: u32 = 493;
    const STRING_ID: &'static str = "minecraft:cyan_terracotta";
    const NAME: &'static str = "Cyan Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 0;
    const MAX_STATE_ID: u32 = 0;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Pink Stained Glass
pub struct HardPinkStainedGlass;

impl BlockDef for HardPinkStainedGlass {
    const ID: u32 = 8099;
    const STRING_ID: &'static str = "minecraft:hard_pink_stained_glass";
    const NAME: &'static str = "Hard Pink Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1;
    const MAX_STATE_ID: u32 = 1;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Candle
pub struct BlueCandle;

impl BlockDef for BlueCandle {
    const ID: u32 = 956;
    const STRING_ID: &'static str = "minecraft:blue_candle";
    const NAME: &'static str = "Blue Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2;
    const MAX_STATE_ID: u32 = 9;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Wood
pub struct DarkOakWood;

impl BlockDef for DarkOakWood {
    const ID: u32 = 77;
    const STRING_ID: &'static str = "minecraft:dark_oak_wood";
    const NAME: &'static str = "Dark Oak Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10;
    const MAX_STATE_ID: u32 = 12;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Sign
pub struct BirchStandingSign;

impl BlockDef for BirchStandingSign {
    const ID: u32 = 212;
    const STRING_ID: &'static str = "minecraft:birch_standing_sign";
    const NAME: &'static str = "Birch Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13;
    const MAX_STATE_ID: u32 = 28;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Basalt
pub struct PolishedBasalt;

impl BlockDef for PolishedBasalt {
    const ID: u32 = 289;
    const STRING_ID: &'static str = "minecraft:polished_basalt";
    const NAME: &'static str = "Polished Basalt";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 29;
    const MAX_STATE_ID: u32 = 31;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Gold Ore
pub struct NetherGoldOre;

impl BlockDef for NetherGoldOre {
    const ID: u32 = 48;
    const STRING_ID: &'static str = "minecraft:nether_gold_ore";
    const NAME: &'static str = "Nether Gold Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 32;
    const MAX_STATE_ID: u32 = 32;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Zombie Head
pub struct ZombieHead;

impl BlockDef for ZombieHead {
    const ID: u32 = 457;
    const STRING_ID: &'static str = "minecraft:zombie_head";
    const NAME: &'static str = "Zombie Head";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 33;
    const MAX_STATE_ID: u32 = 38;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper Chain
pub struct WaxedWeatheredCopperChain;

impl BlockDef for WaxedWeatheredCopperChain {
    const ID: u32 = 357;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_chain";
    const NAME: &'static str = "Waxed Weathered Copper Chain";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 39;
    const MAX_STATE_ID: u32 = 41;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper Chest
pub struct WaxedWeatheredCopperChest;

impl BlockDef for WaxedWeatheredCopperChest {
    const ID: u32 = 1114;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_chest";
    const NAME: &'static str = "Waxed Weathered Copper Chest";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 42;
    const MAX_STATE_ID: u32 = 45;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Leaf Litter
pub struct LeafLitter;

impl BlockDef for LeafLitter {
    const ID: u32 = 1143;
    const STRING_ID: &'static str = "minecraft:leaf_litter";
    const NAME: &'static str = "Leaf Litter";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 46;
    const MAX_STATE_ID: u32 = 77;
    type State = super::states::PetalsState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Door
pub struct WarpedDoor;

impl BlockDef for WarpedDoor {
    const ID: u32 = 900;
    const STRING_ID: &'static str = "minecraft:warped_door";
    const NAME: &'static str = "Warped Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 78;
    const MAX_STATE_ID: u32 = 109;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Concrete Powder
pub struct LightBlueConcretePowder;

impl BlockDef for LightBlueConcretePowder {
    const ID: u32 = 729;
    const STRING_ID: &'static str = "minecraft:light_blue_concrete_powder";
    const NAME: &'static str = "Light Blue Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 110;
    const MAX_STATE_ID: u32 = 110;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Bamboo
pub struct BambooBlock;

impl BlockDef for BambooBlock {
    const ID: u32 = 60;
    const STRING_ID: &'static str = "minecraft:bamboo_block";
    const NAME: &'static str = "Block of Bamboo";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 111;
    const MAX_STATE_ID: u32 = 113;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Piston Head
pub struct PistonArmCollision;

impl BlockDef for PistonArmCollision {
    const ID: u32 = 139;
    const STRING_ID: &'static str = "minecraft:piston_arm_collision";
    const NAME: &'static str = "Piston Head";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 114;
    const MAX_STATE_ID: u32 = 119;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Chiseled Copper
pub struct WaxedOxidizedChiseledCopper;

impl BlockDef for WaxedOxidizedChiseledCopper {
    const ID: u32 = 1059;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_chiseled_copper";
    const NAME: &'static str = "Waxed Oxidized Chiseled Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 120;
    const MAX_STATE_ID: u32 = 120;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Wet Sponge
pub struct WetSponge;

impl BlockDef for WetSponge {
    const ID: u32 = 100;
    const STRING_ID: &'static str = "minecraft:wet_sponge";
    const NAME: &'static str = "Wet Sponge";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 121;
    const MAX_STATE_ID: u32 = 121;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Stone Brick Wall
pub struct EndStoneBrickWall;

impl BlockDef for EndStoneBrickWall {
    const ID: u32 = 835;
    const STRING_ID: &'static str = "minecraft:end_stone_brick_wall";
    const NAME: &'static str = "End Stone Brick Wall";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 9.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 122;
    const MAX_STATE_ID: u32 = 283;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Granite
pub struct Granite;

impl BlockDef for Granite {
    const ID: u32 = 2;
    const STRING_ID: &'static str = "minecraft:granite";
    const NAME: &'static str = "Granite";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 284;
    const MAX_STATE_ID: u32 = 284;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Stained Glass Pane
pub struct BlueStainedGlassPane;

impl BlockDef for BlueStainedGlassPane {
    const ID: u32 = 511;
    const STRING_ID: &'static str = "minecraft:blue_stained_glass_pane";
    const NAME: &'static str = "Blue Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 285;
    const MAX_STATE_ID: u32 = 285;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Fence Gate
pub struct FenceGate;

impl BlockDef for FenceGate {
    const ID: u32 = 107;
    const STRING_ID: &'static str = "minecraft:fence_gate";
    const NAME: &'static str = "Oak Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 286;
    const MAX_STATE_ID: u32 = 301;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Shelf
pub struct BirchShelf;

impl BlockDef for BirchShelf {
    const ID: u32 = 182;
    const STRING_ID: &'static str = "minecraft:birch_shelf";
    const NAME: &'static str = "Birch Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 302;
    const MAX_STATE_ID: u32 = 333;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Powder Snow
pub struct PowderSnow;

impl BlockDef for PowderSnow {
    const ID: u32 = 1027;
    const STRING_ID: &'static str = "minecraft:powder_snow";
    const NAME: &'static str = "Powder Snow";
    const HARDNESS: f32 = 0.25_f32;
    const RESISTANCE: f32 = 0.25_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 334;
    const MAX_STATE_ID: u32 = 334;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Button
pub struct DarkOakButton;

impl BlockDef for DarkOakButton {
    const ID: u32 = 449;
    const STRING_ID: &'static str = "minecraft:dark_oak_button";
    const NAME: &'static str = "Dark Oak Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 335;
    const MAX_STATE_ID: u32 = 346;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Copper Ore
pub struct DeepslateCopperOre;

impl BlockDef for DeepslateCopperOre {
    const ID: u32 = 1043;
    const STRING_ID: &'static str = "minecraft:deepslate_copper_ore";
    const NAME: &'static str = "Deepslate Copper Ore";
    const HARDNESS: f32 = 4.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 347;
    const MAX_STATE_ID: u32 = 347;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Stone Bricks
pub struct ChiseledStoneBricks;

impl BlockDef for ChiseledStoneBricks {
    const ID: u32 = 329;
    const STRING_ID: &'static str = "minecraft:chiseled_stone_bricks";
    const NAME: &'static str = "Chiseled Stone Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 348;
    const MAX_STATE_ID: u32 = 348;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Brick Stairs
pub struct NetherBrickStairs;

impl BlockDef for NetherBrickStairs {
    const ID: u32 = 114;
    const STRING_ID: &'static str = "minecraft:nether_brick_stairs";
    const NAME: &'static str = "Nether Brick Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 349;
    const MAX_STATE_ID: u32 = 356;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Shulker Box
pub struct YellowShulkerBox;

impl BlockDef for YellowShulkerBox {
    const ID: u32 = 682;
    const STRING_ID: &'static str = "minecraft:yellow_shulker_box";
    const NAME: &'static str = "Yellow Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 357;
    const MAX_STATE_ID: u32 = 357;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blackstone Slab
pub struct BlackstoneDoubleSlab;

impl BlockDef for BlackstoneDoubleSlab {
    const ID: u32 = 927;
    const STRING_ID: &'static str = "minecraft:blackstone_double_slab";
    const NAME: &'static str = "Blackstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 358;
    const MAX_STATE_ID: u32 = 359;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Stained Glass
pub struct LimeStainedGlass;

impl BlockDef for LimeStainedGlass {
    const ID: u32 = 305;
    const STRING_ID: &'static str = "minecraft:lime_stained_glass";
    const NAME: &'static str = "Lime Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 360;
    const MAX_STATE_ID: u32 = 360;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Wool
pub struct RedWool;

impl BlockDef for RedWool {
    const ID: u32 = 154;
    const STRING_ID: &'static str = "minecraft:red_wool";
    const NAME: &'static str = "Red Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 361;
    const MAX_STATE_ID: u32 = 361;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Button
pub struct JungleButton;

impl BlockDef for JungleButton {
    const ID: u32 = 446;
    const STRING_ID: &'static str = "minecraft:jungle_button";
    const NAME: &'static str = "Jungle Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 362;
    const MAX_STATE_ID: u32 = 373;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Stairs
pub struct SpruceStairs;

impl BlockDef for SpruceStairs {
    const ID: u32 = 134;
    const STRING_ID: &'static str = "minecraft:spruce_stairs";
    const NAME: &'static str = "Spruce Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 374;
    const MAX_STATE_ID: u32 = 381;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Green Stained Glass Pane
pub struct HardGreenStainedGlassPane;

impl BlockDef for HardGreenStainedGlassPane {
    const ID: u32 = 8098;
    const STRING_ID: &'static str = "minecraft:hard_green_stained_glass_pane";
    const NAME: &'static str = "Hard Green Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 382;
    const MAX_STATE_ID: u32 = 382;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Shelf
pub struct AcaciaShelf;

impl BlockDef for AcaciaShelf {
    const ID: u32 = 180;
    const STRING_ID: &'static str = "minecraft:acacia_shelf";
    const NAME: &'static str = "Acacia Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 383;
    const MAX_STATE_ID: u32 = 414;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Diorite
pub struct Diorite;

impl BlockDef for Diorite {
    const ID: u32 = 4;
    const STRING_ID: &'static str = "minecraft:diorite";
    const NAME: &'static str = "Diorite";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 415;
    const MAX_STATE_ID: u32 = 415;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Fence Gate
pub struct PaleOakFenceGate;

impl BlockDef for PaleOakFenceGate {
    const ID: u32 = 634;
    const STRING_ID: &'static str = "minecraft:pale_oak_fence_gate";
    const NAME: &'static str = "Pale Oak Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 416;
    const MAX_STATE_ID: u32 = 431;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Gray Candle
pub struct GrayCandleCake;

impl BlockDef for GrayCandleCake {
    const ID: u32 = 969;
    const STRING_ID: &'static str = "minecraft:gray_candle_cake";
    const NAME: &'static str = "Cake with Gray Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 432;
    const MAX_STATE_ID: u32 = 433;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Tuff Slab
pub struct PolishedTuffSlab;

impl BlockDef for PolishedTuffSlab {
    const ID: u32 = 989;
    const STRING_ID: &'static str = "minecraft:polished_tuff_slab";
    const NAME: &'static str = "Polished Tuff Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 434;
    const MAX_STATE_ID: u32 = 435;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Pressure Plate
pub struct CherryPressurePlate;

impl BlockDef for CherryPressurePlate {
    const ID: u32 = 266;
    const STRING_ID: &'static str = "minecraft:cherry_pressure_plate";
    const NAME: &'static str = "Cherry Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 436;
    const MAX_STATE_ID: u32 = 451;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Hanging Sign
pub struct CherryHangingSign;

impl BlockDef for CherryHangingSign {
    const ID: u32 = 238;
    const STRING_ID: &'static str = "minecraft:cherry_hanging_sign";
    const NAME: &'static str = "Cherry Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 452;
    const MAX_STATE_ID: u32 = 835;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Wool
pub struct YellowWool;

impl BlockDef for YellowWool {
    const ID: u32 = 144;
    const STRING_ID: &'static str = "minecraft:yellow_wool";
    const NAME: &'static str = "Yellow Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 836;
    const MAX_STATE_ID: u32 = 836;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Sign
pub struct CrimsonWallSign;

impl BlockDef for CrimsonWallSign {
    const ID: u32 = 903;
    const STRING_ID: &'static str = "minecraft:crimson_wall_sign";
    const NAME: &'static str = "Crimson Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 837;
    const MAX_STATE_ID: u32 = 842;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Stained Glass Pane
pub struct YellowStainedGlassPane;

impl BlockDef for YellowStainedGlassPane {
    const ID: u32 = 504;
    const STRING_ID: &'static str = "minecraft:yellow_stained_glass_pane";
    const NAME: &'static str = "Yellow Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 843;
    const MAX_STATE_ID: u32 = 843;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Gateway
pub struct EndGateway;

impl BlockDef for EndGateway {
    const ID: u32 = 209;
    const STRING_ID: &'static str = "minecraft:end_gateway";
    const NAME: &'static str = "End Gateway";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 844;
    const MAX_STATE_ID: u32 = 844;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Azure Bluet
pub struct AzureBluet;

impl BlockDef for AzureBluet {
    const ID: u32 = 163;
    const STRING_ID: &'static str = "minecraft:azure_bluet";
    const NAME: &'static str = "Azure Bluet";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 845;
    const MAX_STATE_ID: u32 = 845;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Beacon
pub struct Beacon;

impl BlockDef for Beacon {
    const ID: u32 = 138;
    const STRING_ID: &'static str = "minecraft:beacon";
    const NAME: &'static str = "Beacon";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 846;
    const MAX_STATE_ID: u32 = 846;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Nether Bricks
pub struct RedNetherBrick;

impl BlockDef for RedNetherBrick {
    const ID: u32 = 215;
    const STRING_ID: &'static str = "minecraft:red_nether_brick";
    const NAME: &'static str = "Red Nether Bricks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 847;
    const MAX_STATE_ID: u32 = 847;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brick Wall
pub struct BrickWall;

impl BlockDef for BrickWall {
    const ID: u32 = 824;
    const STRING_ID: &'static str = "minecraft:brick_wall";
    const NAME: &'static str = "Brick Wall";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 848;
    const MAX_STATE_ID: u32 = 1009;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobbled Deepslate Stairs
pub struct CobbledDeepslateStairs;

impl BlockDef for CobbledDeepslateStairs {
    const ID: u32 = 1153;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate_stairs";
    const NAME: &'static str = "Cobbled Deepslate Stairs";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1010;
    const MAX_STATE_ID: u32 = 1017;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Sandstone
pub struct SmoothSandstone;

impl BlockDef for SmoothSandstone {
    const ID: u32 = 625;
    const STRING_ID: &'static str = "minecraft:smooth_sandstone";
    const NAME: &'static str = "Smooth Sandstone";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1018;
    const MAX_STATE_ID: u32 = 1018;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Snow
pub struct SnowLayer;

impl BlockDef for SnowLayer {
    const ID: u32 = 78;
    const STRING_ID: &'static str = "minecraft:snow_layer";
    const NAME: &'static str = "Snow";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1019;
    const MAX_STATE_ID: u32 = 1034;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brick Slab
pub struct BrickDoubleSlab;

impl BlockDef for BrickDoubleSlab {
    const ID: u32 = 616;
    const STRING_ID: &'static str = "minecraft:brick_double_slab";
    const NAME: &'static str = "Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1035;
    const MAX_STATE_ID: u32 = 1036;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Candle
pub struct BlackCandle;

impl BlockDef for BlackCandle {
    const ID: u32 = 960;
    const STRING_ID: &'static str = "minecraft:black_candle";
    const NAME: &'static str = "Black Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1037;
    const MAX_STATE_ID: u32 = 1044;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Carpet
pub struct BlueCarpet;

impl BlockDef for BlueCarpet {
    const ID: u32 = 549;
    const STRING_ID: &'static str = "minecraft:blue_carpet";
    const NAME: &'static str = "Blue Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1045;
    const MAX_STATE_ID: u32 = 1045;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Glow Frame
pub struct GlowFrame;

impl BlockDef for GlowFrame {
    const ID: u32 = 8097;
    const STRING_ID: &'static str = "minecraft:glow_frame";
    const NAME: &'static str = "Glow Frame";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1046;
    const MAX_STATE_ID: u32 = 1069;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mud Brick Slab
pub struct MudBrickDoubleSlab;

impl BlockDef for MudBrickDoubleSlab {
    const ID: u32 = 618;
    const STRING_ID: &'static str = "minecraft:mud_brick_double_slab";
    const NAME: &'static str = "Mud Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1070;
    const MAX_STATE_ID: u32 = 1071;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hanging Roots
pub struct HangingRoots;

impl BlockDef for HangingRoots {
    const ID: u32 = 1148;
    const STRING_ID: &'static str = "minecraft:hanging_roots";
    const NAME: &'static str = "Hanging Roots";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1072;
    const MAX_STATE_ID: u32 = 1072;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Sandstone Wall
pub struct RedSandstoneWall;

impl BlockDef for RedSandstoneWall {
    const ID: u32 = 826;
    const STRING_ID: &'static str = "minecraft:red_sandstone_wall";
    const NAME: &'static str = "Red Sandstone Wall";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1073;
    const MAX_STATE_ID: u32 = 1234;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Prismarine Brick Stairs
pub struct PrismarineBricksStairs;

impl BlockDef for PrismarineBricksStairs {
    const ID: u32 = 531;
    const STRING_ID: &'static str = "minecraft:prismarine_bricks_stairs";
    const NAME: &'static str = "Prismarine Brick Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1235;
    const MAX_STATE_ID: u32 = 1242;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Cut Copper
pub struct WaxedOxidizedCutCopper;

impl BlockDef for WaxedOxidizedCutCopper {
    const ID: u32 = 1051;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_cut_copper";
    const NAME: &'static str = "Waxed Oxidized Cut Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1243;
    const MAX_STATE_ID: u32 = 1243;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper Chain
pub struct WaxedExposedCopperChain;

impl BlockDef for WaxedExposedCopperChain {
    const ID: u32 = 356;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_chain";
    const NAME: &'static str = "Waxed Exposed Copper Chain";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1244;
    const MAX_STATE_ID: u32 = 1246;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper Chest
pub struct WaxedExposedCopperChest;

impl BlockDef for WaxedExposedCopperChest {
    const ID: u32 = 1113;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_chest";
    const NAME: &'static str = "Waxed Exposed Copper Chest";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1247;
    const MAX_STATE_ID: u32 = 1250;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Calcite
pub struct Calcite;

impl BlockDef for Calcite {
    const ID: u32 = 1025;
    const STRING_ID: &'static str = "minecraft:calcite";
    const NAME: &'static str = "Calcite";
    const HARDNESS: f32 = 0.75_f32;
    const RESISTANCE: f32 = 0.75_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1251;
    const MAX_STATE_ID: u32 = 1251;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Diorite Slab
pub struct DioriteSlab;

impl BlockDef for DioriteSlab {
    const ID: u32 = 823;
    const STRING_ID: &'static str = "minecraft:diorite_slab";
    const NAME: &'static str = "Diorite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1252;
    const MAX_STATE_ID: u32 = 1253;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Dark Oak Log
pub struct StrippedDarkOakLog;

impl BlockDef for StrippedDarkOakLog {
    const ID: u32 = 66;
    const STRING_ID: &'static str = "minecraft:stripped_dark_oak_log";
    const NAME: &'static str = "Stripped Dark Oak Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1254;
    const MAX_STATE_ID: u32 = 1256;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Orange Stained Glass Pane
pub struct HardOrangeStainedGlassPane;

impl BlockDef for HardOrangeStainedGlassPane {
    const ID: u32 = 1258;
    const STRING_ID: &'static str = "minecraft:hard_orange_stained_glass_pane";
    const NAME: &'static str = "Hard Orange Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1257;
    const MAX_STATE_ID: u32 = 1257;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Bubble Coral Fan
pub struct DeadBubbleCoralFan;

impl BlockDef for DeadBubbleCoralFan {
    const ID: u32 = 770;
    const STRING_ID: &'static str = "minecraft:dead_bubble_coral_fan";
    const NAME: &'static str = "Dead Bubble Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1258;
    const MAX_STATE_ID: u32 = 1259;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Log
pub struct JungleLog;

impl BlockDef for JungleLog {
    const ID: u32 = 52;
    const STRING_ID: &'static str = "minecraft:jungle_log";
    const NAME: &'static str = "Jungle Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1260;
    const MAX_STATE_ID: u32 = 1262;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bubble Coral Fan
pub struct BubbleCoralFan;

impl BlockDef for BubbleCoralFan {
    const ID: u32 = 775;
    const STRING_ID: &'static str = "minecraft:bubble_coral_fan";
    const NAME: &'static str = "Bubble Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1263;
    const MAX_STATE_ID: u32 = 1264;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Brown Stained Glass
pub struct HardBrownStainedGlass;

impl BlockDef for HardBrownStainedGlass {
    const ID: u32 = 1266;
    const STRING_ID: &'static str = "minecraft:hard_brown_stained_glass";
    const NAME: &'static str = "Hard Brown Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1265;
    const MAX_STATE_ID: u32 = 1265;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sculk Shrieker
pub struct SculkShrieker;

impl BlockDef for SculkShrieker {
    const ID: u32 = 1033;
    const STRING_ID: &'static str = "minecraft:sculk_shrieker";
    const NAME: &'static str = "Sculk Shrieker";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1266;
    const MAX_STATE_ID: u32 = 1269;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Wool
pub struct GrayWool;

impl BlockDef for GrayWool {
    const ID: u32 = 147;
    const STRING_ID: &'static str = "minecraft:gray_wool";
    const NAME: &'static str = "Gray Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1270;
    const MAX_STATE_ID: u32 = 1270;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Stained Glass Pane
pub struct OrangeStainedGlassPane;

impl BlockDef for OrangeStainedGlassPane {
    const ID: u32 = 501;
    const STRING_ID: &'static str = "minecraft:orange_stained_glass_pane";
    const NAME: &'static str = "Orange Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1271;
    const MAX_STATE_ID: u32 = 1271;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Black Stained Glass Pane
pub struct HardBlackStainedGlassPane;

impl BlockDef for HardBlackStainedGlassPane {
    const ID: u32 = 1273;
    const STRING_ID: &'static str = "minecraft:hard_black_stained_glass_pane";
    const NAME: &'static str = "Hard Black Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1272;
    const MAX_STATE_ID: u32 = 1272;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Carpet
pub struct GrayCarpet;

impl BlockDef for GrayCarpet {
    const ID: u32 = 545;
    const STRING_ID: &'static str = "minecraft:gray_carpet";
    const NAME: &'static str = "Gray Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1273;
    const MAX_STATE_ID: u32 = 1273;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lily of the Valley
pub struct LilyOfTheValley;

impl BlockDef for LilyOfTheValley {
    const ID: u32 = 171;
    const STRING_ID: &'static str = "minecraft:lily_of_the_valley";
    const NAME: &'static str = "Lily of the Valley";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1274;
    const MAX_STATE_ID: u32 = 1274;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Glazed Terracotta
pub struct LimeGlazedTerracotta;

impl BlockDef for LimeGlazedTerracotta {
    const ID: u32 = 225;
    const STRING_ID: &'static str = "minecraft:lime_glazed_terracotta";
    const NAME: &'static str = "Lime Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1275;
    const MAX_STATE_ID: u32 = 1280;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Trapdoor
pub struct Trapdoor;

impl BlockDef for Trapdoor {
    const ID: u32 = 96;
    const STRING_ID: &'static str = "minecraft:trapdoor";
    const NAME: &'static str = "Oak Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1281;
    const MAX_STATE_ID: u32 = 1296;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cactus Flower
pub struct CactusFlower;

impl BlockDef for CactusFlower {
    const ID: u32 = 280;
    const STRING_ID: &'static str = "minecraft:cactus_flower";
    const NAME: &'static str = "Cactus Flower";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1297;
    const MAX_STATE_ID: u32 = 1297;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Brain Coral Fan
pub struct DeadBrainCoralFan;

impl BlockDef for DeadBrainCoralFan {
    const ID: u32 = 769;
    const STRING_ID: &'static str = "minecraft:dead_brain_coral_fan";
    const NAME: &'static str = "Dead Brain Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1298;
    const MAX_STATE_ID: u32 = 1299;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Info Update
pub struct InfoUpdate;

impl BlockDef for InfoUpdate {
    const ID: u32 = 248;
    const STRING_ID: &'static str = "minecraft:info_update";
    const NAME: &'static str = "Info Update";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1300;
    const MAX_STATE_ID: u32 = 1300;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Seagrass
pub struct Seagrass;

impl BlockDef for Seagrass {
    const ID: u32 = 136;
    const STRING_ID: &'static str = "minecraft:seagrass";
    const NAME: &'static str = "Seagrass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1301;
    const MAX_STATE_ID: u32 = 1303;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tube Coral Fan
pub struct TubeCoralFan;

impl BlockDef for TubeCoralFan {
    const ID: u32 = 773;
    const STRING_ID: &'static str = "minecraft:tube_coral_fan";
    const NAME: &'static str = "Tube Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1304;
    const MAX_STATE_ID: u32 = 1305;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Cut Copper Slab
pub struct WaxedExposedCutCopperSlab;

impl BlockDef for WaxedExposedCutCopperSlab {
    const ID: u32 = 8091;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_cut_copper_slab";
    const NAME: &'static str = "Waxed Exposed Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1306;
    const MAX_STATE_ID: u32 = 1307;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Lamp
pub struct RedstoneLamp;

impl BlockDef for RedstoneLamp {
    const ID: u32 = 123;
    const STRING_ID: &'static str = "minecraft:redstone_lamp";
    const NAME: &'static str = "Redstone Lamp";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1308;
    const MAX_STATE_ID: u32 = 1308;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Cobblestone
pub struct MossyCobblestone;

impl BlockDef for MossyCobblestone {
    const ID: u32 = 48;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone";
    const NAME: &'static str = "Mossy Cobblestone";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1309;
    const MAX_STATE_ID: u32 = 1309;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate
pub struct Deepslate;

impl BlockDef for Deepslate {
    const ID: u32 = 1151;
    const STRING_ID: &'static str = "minecraft:deepslate";
    const NAME: &'static str = "Deepslate";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1310;
    const MAX_STATE_ID: u32 = 1312;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Carpet
pub struct MagentaCarpet;

impl BlockDef for MagentaCarpet {
    const ID: u32 = 540;
    const STRING_ID: &'static str = "minecraft:magenta_carpet";
    const NAME: &'static str = "Magenta Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1313;
    const MAX_STATE_ID: u32 = 1313;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pitcher Crop
pub struct PitcherCrop;

impl BlockDef for PitcherCrop {
    const ID: u32 = 663;
    const STRING_ID: &'static str = "minecraft:pitcher_crop";
    const NAME: &'static str = "Pitcher Crop";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1314;
    const MAX_STATE_ID: u32 = 1329;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Wool
pub struct BrownWool;

impl BlockDef for BrownWool {
    const ID: u32 = 152;
    const STRING_ID: &'static str = "minecraft:brown_wool";
    const NAME: &'static str = "Brown Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1330;
    const MAX_STATE_ID: u32 = 1330;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Chiseled Copper
pub struct WaxedExposedChiseledCopper;

impl BlockDef for WaxedExposedChiseledCopper {
    const ID: u32 = 1057;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_chiseled_copper";
    const NAME: &'static str = "Waxed Exposed Chiseled Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1331;
    const MAX_STATE_ID: u32 = 1331;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff Slab
pub struct TuffSlab;

impl BlockDef for TuffSlab {
    const ID: u32 = 985;
    const STRING_ID: &'static str = "minecraft:tuff_slab";
    const NAME: &'static str = "Tuff Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1332;
    const MAX_STATE_ID: u32 = 1333;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Pressure Plate
pub struct WarpedPressurePlate;

impl BlockDef for WarpedPressurePlate {
    const ID: u32 = 888;
    const STRING_ID: &'static str = "minecraft:warped_pressure_plate";
    const NAME: &'static str = "Warped Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1334;
    const MAX_STATE_ID: u32 = 1349;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Acacia Wood
pub struct StrippedAcaciaWood;

impl BlockDef for StrippedAcaciaWood {
    const ID: u32 = 83;
    const STRING_ID: &'static str = "minecraft:stripped_acacia_wood";
    const NAME: &'static str = "Stripped Acacia Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1350;
    const MAX_STATE_ID: u32 = 1352;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Firefly Bush
pub struct FireflyBush;

impl BlockDef for FireflyBush {
    const ID: u32 = 1195;
    const STRING_ID: &'static str = "minecraft:firefly_bush";
    const NAME: &'static str = "Firefly Bush";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 2;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1353;
    const MAX_STATE_ID: u32 = 1353;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Diamond
pub struct DiamondBlock;

impl BlockDef for DiamondBlock {
    const ID: u32 = 57;
    const STRING_ID: &'static str = "minecraft:diamond_block";
    const NAME: &'static str = "Block of Diamond";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1354;
    const MAX_STATE_ID: u32 = 1354;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Prismarine Slab
pub struct DarkPrismarineDoubleSlab;

impl BlockDef for DarkPrismarineDoubleSlab {
    const ID: u32 = 535;
    const STRING_ID: &'static str = "minecraft:dark_prismarine_double_slab";
    const NAME: &'static str = "Dark Prismarine Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1355;
    const MAX_STATE_ID: u32 = 1356;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Stairs
pub struct OakStairs;

impl BlockDef for OakStairs {
    const ID: u32 = 53;
    const STRING_ID: &'static str = "minecraft:oak_stairs";
    const NAME: &'static str = "Oak Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1357;
    const MAX_STATE_ID: u32 = 1364;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Gray Stained Glass
pub struct HardGrayStainedGlass;

impl BlockDef for HardGrayStainedGlass {
    const ID: u32 = 1528;
    const STRING_ID: &'static str = "minecraft:hard_gray_stained_glass";
    const NAME: &'static str = "Hard Gray Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1365;
    const MAX_STATE_ID: u32 = 1365;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Log
pub struct OakLog;

impl BlockDef for OakLog {
    const ID: u32 = 49;
    const STRING_ID: &'static str = "minecraft:oak_log";
    const NAME: &'static str = "Oak Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1366;
    const MAX_STATE_ID: u32 = 1368;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Stained Glass Pane
pub struct BrownStainedGlassPane;

impl BlockDef for BrownStainedGlassPane {
    const ID: u32 = 512;
    const STRING_ID: &'static str = "minecraft:brown_stained_glass_pane";
    const NAME: &'static str = "Brown Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1369;
    const MAX_STATE_ID: u32 = 1369;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Stone Bricks
pub struct EndBricks;

impl BlockDef for EndBricks {
    const ID: u32 = 206;
    const STRING_ID: &'static str = "minecraft:end_bricks";
    const NAME: &'static str = "End Stone Bricks";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 9.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1370;
    const MAX_STATE_ID: u32 = 1370;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Shulker Box
pub struct MagentaShulkerBox;

impl BlockDef for MagentaShulkerBox {
    const ID: u32 = 680;
    const STRING_ID: &'static str = "minecraft:magenta_shulker_box";
    const NAME: &'static str = "Magenta Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1371;
    const MAX_STATE_ID: u32 = 1371;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Packed Ice
pub struct PackedIce;

impl BlockDef for PackedIce {
    const ID: u32 = 174;
    const STRING_ID: &'static str = "minecraft:packed_ice";
    const NAME: &'static str = "Packed Ice";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1372;
    const MAX_STATE_ID: u32 = 1372;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Packed Mud
pub struct PackedMud;

impl BlockDef for PackedMud {
    const ID: u32 = 330;
    const STRING_ID: &'static str = "minecraft:packed_mud";
    const NAME: &'static str = "Packed Mud";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1373;
    const MAX_STATE_ID: u32 = 1373;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Light Blue Candle
pub struct LightBlueCandleCake;

impl BlockDef for LightBlueCandleCake {
    const ID: u32 = 965;
    const STRING_ID: &'static str = "minecraft:light_blue_candle_cake";
    const NAME: &'static str = "Cake with Light Blue Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1374;
    const MAX_STATE_ID: u32 = 1375;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Moss Carpet
pub struct MossCarpet;

impl BlockDef for MossCarpet {
    const ID: u32 = 1140;
    const STRING_ID: &'static str = "minecraft:moss_carpet";
    const NAME: &'static str = "Moss Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1376;
    const MAX_STATE_ID: u32 = 1376;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Fungus
pub struct WarpedFungus;

impl BlockDef for WarpedFungus {
    const ID: u32 = 867;
    const STRING_ID: &'static str = "minecraft:warped_fungus";
    const NAME: &'static str = "Warped Fungus";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1377;
    const MAX_STATE_ID: u32 = 1377;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Lightning Rod
pub struct OxidizedLightningRod;

impl BlockDef for OxidizedLightningRod {
    const ID: u32 = 1127;
    const STRING_ID: &'static str = "minecraft:oxidized_lightning_rod";
    const NAME: &'static str = "Oxidized Lightning Rod";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1378;
    const MAX_STATE_ID: u32 = 1389;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Deepslate Slab
pub struct PolishedDeepslateSlab;

impl BlockDef for PolishedDeepslateSlab {
    const ID: u32 = 8096;
    const STRING_ID: &'static str = "minecraft:polished_deepslate_slab";
    const NAME: &'static str = "Polished Deepslate Slab";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1390;
    const MAX_STATE_ID: u32 = 1391;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Door
pub struct BambooDoor;

impl BlockDef for BambooDoor {
    const ID: u32 = 654;
    const STRING_ID: &'static str = "minecraft:bamboo_door";
    const NAME: &'static str = "Bamboo Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1392;
    const MAX_STATE_ID: u32 = 1423;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Amethyst
pub struct AmethystBlock;

impl BlockDef for AmethystBlock {
    const ID: u32 = 978;
    const STRING_ID: &'static str = "minecraft:amethyst_block";
    const NAME: &'static str = "Block of Amethyst";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1424;
    const MAX_STATE_ID: u32 = 1424;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Bubble Coral Wall Fan
pub struct DeadBubbleCoralWallFan;

impl BlockDef for DeadBubbleCoralWallFan {
    const ID: u32 = 780;
    const STRING_ID: &'static str = "minecraft:dead_bubble_coral_wall_fan";
    const NAME: &'static str = "Dead Bubble Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1425;
    const MAX_STATE_ID: u32 = 1428;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Gold
pub struct GoldBlock;

impl BlockDef for GoldBlock {
    const ID: u32 = 41;
    const STRING_ID: &'static str = "minecraft:gold_block";
    const NAME: &'static str = "Block of Gold";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1429;
    const MAX_STATE_ID: u32 = 1429;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Potted Closed Eyeblossom
pub struct FlowerPot;

impl BlockDef for FlowerPot {
    const ID: u32 = 140;
    const STRING_ID: &'static str = "minecraft:flower_pot";
    const NAME: &'static str = "Potted Closed Eyeblossom";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1430;
    const MAX_STATE_ID: u32 = 1431;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Bookshelf
pub struct ChiseledBookshelf;

impl BlockDef for ChiseledBookshelf {
    const ID: u32 = 179;
    const STRING_ID: &'static str = "minecraft:chiseled_bookshelf";
    const NAME: &'static str = "Chiseled Bookshelf";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1432;
    const MAX_STATE_ID: u32 = 1687;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Deepslate Stairs
pub struct PolishedDeepslateStairs;

impl BlockDef for PolishedDeepslateStairs {
    const ID: u32 = 1157;
    const STRING_ID: &'static str = "minecraft:polished_deepslate_stairs";
    const NAME: &'static str = "Polished Deepslate Stairs";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1688;
    const MAX_STATE_ID: u32 = 1695;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Shulker Box
pub struct LimeShulkerBox;

impl BlockDef for LimeShulkerBox {
    const ID: u32 = 683;
    const STRING_ID: &'static str = "minecraft:lime_shulker_box";
    const NAME: &'static str = "Lime Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1696;
    const MAX_STATE_ID: u32 = 1696;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Chiseled Copper
pub struct WeatheredChiseledCopper;

impl BlockDef for WeatheredChiseledCopper {
    const ID: u32 = 1054;
    const STRING_ID: &'static str = "minecraft:weathered_chiseled_copper";
    const NAME: &'static str = "Weathered Chiseled Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1697;
    const MAX_STATE_ID: u32 = 1697;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Small Amethyst Bud
pub struct SmallAmethystBud;

impl BlockDef for SmallAmethystBud {
    const ID: u32 = 983;
    const STRING_ID: &'static str = "minecraft:small_amethyst_bud";
    const NAME: &'static str = "Small Amethyst Bud";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 1;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1698;
    const MAX_STATE_ID: u32 = 1703;
    type State = super::states::BlockFaceState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Activator Rail
pub struct ActivatorRail;

impl BlockDef for ActivatorRail {
    const ID: u32 = 126;
    const STRING_ID: &'static str = "minecraft:activator_rail";
    const NAME: &'static str = "Activator Rail";
    const HARDNESS: f32 = 0.7_f32;
    const RESISTANCE: f32 = 0.7_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1704;
    const MAX_STATE_ID: u32 = 1715;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Iron Trapdoor
pub struct IronTrapdoor;

impl BlockDef for IronTrapdoor {
    const ID: u32 = 167;
    const STRING_ID: &'static str = "minecraft:iron_trapdoor";
    const NAME: &'static str = "Iron Trapdoor";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 5.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1716;
    const MAX_STATE_ID: u32 = 1731;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Potatoes
pub struct Potatoes;

impl BlockDef for Potatoes {
    const ID: u32 = 142;
    const STRING_ID: &'static str = "minecraft:potatoes";
    const NAME: &'static str = "Potatoes";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1732;
    const MAX_STATE_ID: u32 = 1739;
    type State = super::states::CropState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Muddy Mangrove Roots
pub struct MuddyMangroveRoots;

impl BlockDef for MuddyMangroveRoots {
    const ID: u32 = 59;
    const STRING_ID: &'static str = "minecraft:muddy_mangrove_roots";
    const NAME: &'static str = "Muddy Mangrove Roots";
    const HARDNESS: f32 = 0.7_f32;
    const RESISTANCE: f32 = 0.7_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1740;
    const MAX_STATE_ID: u32 = 1742;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Pressure Plate
pub struct PaleOakPressurePlate;

impl BlockDef for PaleOakPressurePlate {
    const ID: u32 = 268;
    const STRING_ID: &'static str = "minecraft:pale_oak_pressure_plate";
    const NAME: &'static str = "Pale Oak Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1743;
    const MAX_STATE_ID: u32 = 1758;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Jungle Wood
pub struct StrippedJungleWood;

impl BlockDef for StrippedJungleWood {
    const ID: u32 = 82;
    const STRING_ID: &'static str = "minecraft:stripped_jungle_wood";
    const NAME: &'static str = "Stripped Jungle Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1759;
    const MAX_STATE_ID: u32 = 1761;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Note Block
pub struct Noteblock;

impl BlockDef for Noteblock {
    const ID: u32 = 25;
    const STRING_ID: &'static str = "minecraft:noteblock";
    const NAME: &'static str = "Note Block";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1762;
    const MAX_STATE_ID: u32 = 1762;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff
pub struct Tuff;

impl BlockDef for Tuff {
    const ID: u32 = 984;
    const STRING_ID: &'static str = "minecraft:tuff";
    const NAME: &'static str = "Tuff";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1763;
    const MAX_STATE_ID: u32 = 1763;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Log
pub struct MangroveLog;

impl BlockDef for MangroveLog {
    const ID: u32 = 57;
    const STRING_ID: &'static str = "minecraft:mangrove_log";
    const NAME: &'static str = "Mangrove Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1764;
    const MAX_STATE_ID: u32 = 1766;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Cut Copper Stairs
pub struct OxidizedCutCopperStairs;

impl BlockDef for OxidizedCutCopperStairs {
    const ID: u32 = 1063;
    const STRING_ID: &'static str = "minecraft:oxidized_cut_copper_stairs";
    const NAME: &'static str = "Oxidized Cut Copper Stairs";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1767;
    const MAX_STATE_ID: u32 = 1774;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Fence
pub struct PaleOakFence;

impl BlockDef for PaleOakFence {
    const ID: u32 = 643;
    const STRING_ID: &'static str = "minecraft:pale_oak_fence";
    const NAME: &'static str = "Pale Oak Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1775;
    const MAX_STATE_ID: u32 = 1775;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Leaves
pub struct PaleOakLeaves;

impl BlockDef for PaleOakLeaves {
    const ID: u32 = 95;
    const STRING_ID: &'static str = "minecraft:pale_oak_leaves";
    const NAME: &'static str = "Pale Oak Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1776;
    const MAX_STATE_ID: u32 = 1779;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Tile Slab
pub struct DeepslateTileDoubleSlab;

impl BlockDef for DeepslateTileDoubleSlab {
    const ID: u32 = 8092;
    const STRING_ID: &'static str = "minecraft:deepslate_tile_double_slab";
    const NAME: &'static str = "Deepslate Tile Slab";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1780;
    const MAX_STATE_ID: u32 = 1781;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sandstone Slab
pub struct SandstoneSlab;

impl BlockDef for SandstoneSlab {
    const ID: u32 = 612;
    const STRING_ID: &'static str = "minecraft:sandstone_slab";
    const NAME: &'static str = "Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1782;
    const MAX_STATE_ID: u32 = 1783;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Stone Brick Slab
pub struct MossyStoneBrickSlab;

impl BlockDef for MossyStoneBrickSlab {
    const ID: u32 = 813;
    const STRING_ID: &'static str = "minecraft:mossy_stone_brick_slab";
    const NAME: &'static str = "Mossy Stone Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1784;
    const MAX_STATE_ID: u32 = 1785;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Raw Gold
pub struct RawGoldBlock;

impl BlockDef for RawGoldBlock {
    const ID: u32 = 1175;
    const STRING_ID: &'static str = "minecraft:raw_gold_block";
    const NAME: &'static str = "Block of Raw Gold";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1786;
    const MAX_STATE_ID: u32 = 1786;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Allium
pub struct Allium;

impl BlockDef for Allium {
    const ID: u32 = 162;
    const STRING_ID: &'static str = "minecraft:allium";
    const NAME: &'static str = "Allium";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1787;
    const MAX_STATE_ID: u32 = 1787;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Shulker Box
pub struct WhiteShulkerBox;

impl BlockDef for WhiteShulkerBox {
    const ID: u32 = 678;
    const STRING_ID: &'static str = "minecraft:white_shulker_box";
    const NAME: &'static str = "White Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1788;
    const MAX_STATE_ID: u32 = 1788;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Grate
pub struct CopperGrate;

impl BlockDef for CopperGrate {
    const ID: u32 = 1092;
    const STRING_ID: &'static str = "minecraft:copper_grate";
    const NAME: &'static str = "Copper Grate";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1789;
    const MAX_STATE_ID: u32 = 1789;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Wool
pub struct BlackWool;

impl BlockDef for BlackWool {
    const ID: u32 = 155;
    const STRING_ID: &'static str = "minecraft:black_wool";
    const NAME: &'static str = "Black Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1790;
    const MAX_STATE_ID: u32 = 1790;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Candle
pub struct OrangeCandle;

impl BlockDef for OrangeCandle {
    const ID: u32 = 946;
    const STRING_ID: &'static str = "minecraft:orange_candle";
    const NAME: &'static str = "Orange Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1791;
    const MAX_STATE_ID: u32 = 1798;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Comparator
pub struct PoweredComparator;

impl BlockDef for PoweredComparator {
    const ID: u32 = 150;
    const STRING_ID: &'static str = "minecraft:powered_comparator";
    const NAME: &'static str = "Redstone Comparator";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1799;
    const MAX_STATE_ID: u32 = 1814;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Fence
pub struct JungleFence;

impl BlockDef for JungleFence {
    const ID: u32 = 639;
    const STRING_ID: &'static str = "minecraft:jungle_fence";
    const NAME: &'static str = "Jungle Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1815;
    const MAX_STATE_ID: u32 = 1815;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Sandstone Slab
pub struct CutSandstoneDoubleSlab;

impl BlockDef for CutSandstoneDoubleSlab {
    const ID: u32 = 613;
    const STRING_ID: &'static str = "minecraft:cut_sandstone_double_slab";
    const NAME: &'static str = "Cut Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1816;
    const MAX_STATE_ID: u32 = 1817;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Sign
pub struct WarpedWallSign;

impl BlockDef for WarpedWallSign {
    const ID: u32 = 904;
    const STRING_ID: &'static str = "minecraft:warped_wall_sign";
    const NAME: &'static str = "Warped Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1818;
    const MAX_STATE_ID: u32 = 1823;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Fence
pub struct SpruceFence;

impl BlockDef for SpruceFence {
    const ID: u32 = 637;
    const STRING_ID: &'static str = "minecraft:spruce_fence";
    const NAME: &'static str = "Spruce Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1824;
    const MAX_STATE_ID: u32 = 1824;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Sapling
pub struct DarkOakSapling;

impl BlockDef for DarkOakSapling {
    const ID: u32 = 31;
    const STRING_ID: &'static str = "minecraft:dark_oak_sapling";
    const NAME: &'static str = "Dark Oak Sapling";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1825;
    const MAX_STATE_ID: u32 = 1826;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Melon
pub struct MelonBlock;

impl BlockDef for MelonBlock {
    const ID: u32 = 103;
    const STRING_ID: &'static str = "minecraft:melon_block";
    const NAME: &'static str = "Melon";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1827;
    const MAX_STATE_ID: u32 = 1827;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Concrete Powder
pub struct BlackConcretePowder;

impl BlockDef for BlackConcretePowder {
    const ID: u32 = 741;
    const STRING_ID: &'static str = "minecraft:black_concrete_powder";
    const NAME: &'static str = "Black Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1828;
    const MAX_STATE_ID: u32 = 1828;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sandstone Slab
pub struct SandstoneDoubleSlab;

impl BlockDef for SandstoneDoubleSlab {
    const ID: u32 = 8043;
    const STRING_ID: &'static str = "minecraft:sandstone_double_slab";
    const NAME: &'static str = "Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1829;
    const MAX_STATE_ID: u32 = 1830;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Cut Copper Stairs
pub struct WaxedCutCopperStairs;

impl BlockDef for WaxedCutCopperStairs {
    const ID: u32 = 1064;
    const STRING_ID: &'static str = "minecraft:waxed_cut_copper_stairs";
    const NAME: &'static str = "Waxed Cut Copper Stairs";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1831;
    const MAX_STATE_ID: u32 = 1838;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Open Eyeblossom
pub struct OpenEyeblossom;

impl BlockDef for OpenEyeblossom {
    const ID: u32 = 1191;
    const STRING_ID: &'static str = "minecraft:open_eyeblossom";
    const NAME: &'static str = "Open Eyeblossom";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1839;
    const MAX_STATE_ID: u32 = 1839;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Monster Spawner
pub struct MobSpawner;

impl BlockDef for MobSpawner {
    const ID: u32 = 52;
    const STRING_ID: &'static str = "minecraft:mob_spawner";
    const NAME: &'static str = "Monster Spawner";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 5.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 1840;
    const MAX_STATE_ID: u32 = 1840;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Sapling
pub struct PaleOakSapling;

impl BlockDef for PaleOakSapling {
    const ID: u32 = 32;
    const STRING_ID: &'static str = "minecraft:pale_oak_sapling";
    const NAME: &'static str = "Pale Oak Sapling";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1841;
    const MAX_STATE_ID: u32 = 1842;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Granite
pub struct PolishedGranite;

impl BlockDef for PolishedGranite {
    const ID: u32 = 3;
    const STRING_ID: &'static str = "minecraft:polished_granite";
    const NAME: &'static str = "Polished Granite";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1843;
    const MAX_STATE_ID: u32 = 1843;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Sign
pub struct PaleOakWallSign;

impl BlockDef for PaleOakWallSign {
    const ID: u32 = 231;
    const STRING_ID: &'static str = "minecraft:pale_oak_wall_sign";
    const NAME: &'static str = "Pale Oak Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1844;
    const MAX_STATE_ID: u32 = 1849;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Soul Fire
pub struct SoulFire;

impl BlockDef for SoulFire {
    const ID: u32 = 197;
    const STRING_ID: &'static str = "minecraft:soul_fire";
    const NAME: &'static str = "Soul Fire";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 10;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1850;
    const MAX_STATE_ID: u32 = 1865;
    type State = super::states::AgeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Candle
pub struct MagentaCandle;

impl BlockDef for MagentaCandle {
    const ID: u32 = 947;
    const STRING_ID: &'static str = "minecraft:magenta_candle";
    const NAME: &'static str = "Magenta Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1866;
    const MAX_STATE_ID: u32 = 1873;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Slab
pub struct MangroveDoubleSlab;

impl BlockDef for MangroveDoubleSlab {
    const ID: u32 = 607;
    const STRING_ID: &'static str = "minecraft:mangrove_double_slab";
    const NAME: &'static str = "Mangrove Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1874;
    const MAX_STATE_ID: u32 = 1875;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Quartz Slab
pub struct SmoothQuartzDoubleSlab;

impl BlockDef for SmoothQuartzDoubleSlab {
    const ID: u32 = 818;
    const STRING_ID: &'static str = "minecraft:smooth_quartz_double_slab";
    const NAME: &'static str = "Smooth Quartz Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1876;
    const MAX_STATE_ID: u32 = 1877;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Stained Glass
pub struct LightGrayStainedGlass;

impl BlockDef for LightGrayStainedGlass {
    const ID: u32 = 308;
    const STRING_ID: &'static str = "minecraft:light_gray_stained_glass";
    const NAME: &'static str = "Light Gray Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1878;
    const MAX_STATE_ID: u32 = 1878;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Obsidian
pub struct Obsidian;

impl BlockDef for Obsidian {
    const ID: u32 = 49;
    const STRING_ID: &'static str = "minecraft:obsidian";
    const NAME: &'static str = "Obsidian";
    const HARDNESS: f32 = 50.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 1879;
    const MAX_STATE_ID: u32 = 1879;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Stained Glass Pane
pub struct LightGrayStainedGlassPane;

impl BlockDef for LightGrayStainedGlassPane {
    const ID: u32 = 508;
    const STRING_ID: &'static str = "minecraft:light_gray_stained_glass_pane";
    const NAME: &'static str = "Light Gray Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1880;
    const MAX_STATE_ID: u32 = 1880;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Slab
pub struct DarkOakSlab;

impl BlockDef for DarkOakSlab {
    const ID: u32 = 605;
    const STRING_ID: &'static str = "minecraft:dark_oak_slab";
    const NAME: &'static str = "Dark Oak Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1881;
    const MAX_STATE_ID: u32 = 1882;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Brick Wall
pub struct DeepslateBrickWall;

impl BlockDef for DeepslateBrickWall {
    const ID: u32 = 1167;
    const STRING_ID: &'static str = "minecraft:deepslate_brick_wall";
    const NAME: &'static str = "Deepslate Brick Wall";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 1883;
    const MAX_STATE_ID: u32 = 2044;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper Grate
pub struct WaxedExposedCopperGrate;

impl BlockDef for WaxedExposedCopperGrate {
    const ID: u32 = 1097;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_grate";
    const NAME: &'static str = "Waxed Exposed Copper Grate";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2045;
    const MAX_STATE_ID: u32 = 2045;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Cut Copper Slab
pub struct OxidizedDoubleCutCopperSlab;

impl BlockDef for OxidizedDoubleCutCopperSlab {
    const ID: u32 = 8090;
    const STRING_ID: &'static str = "minecraft:oxidized_double_cut_copper_slab";
    const NAME: &'static str = "Oxidized Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2046;
    const MAX_STATE_ID: u32 = 2047;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper
pub struct ExposedCopper;

impl BlockDef for ExposedCopper {
    const ID: u32 = 1035;
    const STRING_ID: &'static str = "minecraft:exposed_copper";
    const NAME: &'static str = "Exposed Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2048;
    const MAX_STATE_ID: u32 = 2048;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Deepslate Slab
pub struct PolishedDeepslateDoubleSlab;

impl BlockDef for PolishedDeepslateDoubleSlab {
    const ID: u32 = 1158;
    const STRING_ID: &'static str = "minecraft:polished_deepslate_double_slab";
    const NAME: &'static str = "Polished Deepslate Slab";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2049;
    const MAX_STATE_ID: u32 = 2050;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Copper Bars
pub struct WaxedCopperBars;

impl BlockDef for WaxedCopperBars {
    const ID: u32 = 346;
    const STRING_ID: &'static str = "minecraft:waxed_copper_bars";
    const NAME: &'static str = "Waxed Copper Bars";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2051;
    const MAX_STATE_ID: u32 = 2051;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Button
pub struct StoneButton;

impl BlockDef for StoneButton {
    const ID: u32 = 77;
    const STRING_ID: &'static str = "minecraft:stone_button";
    const NAME: &'static str = "Stone Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2052;
    const MAX_STATE_ID: u32 = 2063;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Nether Brick Slab
pub struct RedNetherBrickDoubleSlab;

impl BlockDef for RedNetherBrickDoubleSlab {
    const ID: u32 = 821;
    const STRING_ID: &'static str = "minecraft:red_nether_brick_double_slab";
    const NAME: &'static str = "Red Nether Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2064;
    const MAX_STATE_ID: u32 = 2065;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Copper Bulb
pub struct WaxedCopperBulb;

impl BlockDef for WaxedCopperBulb {
    const ID: u32 = 1104;
    const STRING_ID: &'static str = "minecraft:waxed_copper_bulb";
    const NAME: &'static str = "Waxed Copper Bulb";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2066;
    const MAX_STATE_ID: u32 = 2069;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sponge
pub struct Sponge;

impl BlockDef for Sponge {
    const ID: u32 = 19;
    const STRING_ID: &'static str = "minecraft:sponge";
    const NAME: &'static str = "Sponge";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2070;
    const MAX_STATE_ID: u32 = 2070;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Cut Copper Slab
pub struct ExposedDoubleCutCopperSlab;

impl BlockDef for ExposedDoubleCutCopperSlab {
    const ID: u32 = 8095;
    const STRING_ID: &'static str = "minecraft:exposed_double_cut_copper_slab";
    const NAME: &'static str = "Exposed Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2071;
    const MAX_STATE_ID: u32 = 2072;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Fence
pub struct BambooFence;

impl BlockDef for BambooFence {
    const ID: u32 = 645;
    const STRING_ID: &'static str = "minecraft:bamboo_fence";
    const NAME: &'static str = "Bamboo Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2073;
    const MAX_STATE_ID: u32 = 2073;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Stairs
pub struct NormalStoneStairs;

impl BlockDef for NormalStoneStairs {
    const ID: u32 = 803;
    const STRING_ID: &'static str = "minecraft:normal_stone_stairs";
    const NAME: &'static str = "Stone Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2074;
    const MAX_STATE_ID: u32 = 2081;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Diorite Slab
pub struct DioriteDoubleSlab;

impl BlockDef for DioriteDoubleSlab {
    const ID: u32 = 8067;
    const STRING_ID: &'static str = "minecraft:diorite_double_slab";
    const NAME: &'static str = "Diorite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2082;
    const MAX_STATE_ID: u32 = 2083;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Stone Brick Slab
pub struct EndStoneBrickSlab;

impl BlockDef for EndStoneBrickSlab {
    const ID: u32 = 816;
    const STRING_ID: &'static str = "minecraft:end_stone_brick_slab";
    const NAME: &'static str = "End Stone Brick Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 9.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2084;
    const MAX_STATE_ID: u32 = 2085;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Terracotta
pub struct HardenedClay;

impl BlockDef for HardenedClay {
    const ID: u32 = 172;
    const STRING_ID: &'static str = "minecraft:hardened_clay";
    const NAME: &'static str = "Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2086;
    const MAX_STATE_ID: u32 = 2086;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Hanging Sign
pub struct BirchHangingSign;

impl BlockDef for BirchHangingSign {
    const ID: u32 = 236;
    const STRING_ID: &'static str = "minecraft:birch_hanging_sign";
    const NAME: &'static str = "Birch Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2087;
    const MAX_STATE_ID: u32 = 2470;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Jungle Log
pub struct StrippedJungleLog;

impl BlockDef for StrippedJungleLog {
    const ID: u32 = 63;
    const STRING_ID: &'static str = "minecraft:stripped_jungle_log";
    const NAME: &'static str = "Stripped Jungle Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2471;
    const MAX_STATE_ID: u32 = 2473;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper Golem Statue
pub struct OxidizedCopperGolemStatue;

impl BlockDef for OxidizedCopperGolemStatue {
    const ID: u32 = 1119;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_golem_statue";
    const NAME: &'static str = "Oxidized Copper Golem Statue";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2474;
    const MAX_STATE_ID: u32 = 2477;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock9;

impl BlockDef for LightBlock9 {
    const ID: u32 = 525;
    const STRING_ID: &'static str = "minecraft:light_block_9";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2478;
    const MAX_STATE_ID: u32 = 2478;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock8;

impl BlockDef for LightBlock8 {
    const ID: u32 = 8012;
    const STRING_ID: &'static str = "minecraft:light_block_8";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2479;
    const MAX_STATE_ID: u32 = 2479;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock7;

impl BlockDef for LightBlock7 {
    const ID: u32 = 8013;
    const STRING_ID: &'static str = "minecraft:light_block_7";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2480;
    const MAX_STATE_ID: u32 = 2480;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock6;

impl BlockDef for LightBlock6 {
    const ID: u32 = 8014;
    const STRING_ID: &'static str = "minecraft:light_block_6";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2481;
    const MAX_STATE_ID: u32 = 2481;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock5;

impl BlockDef for LightBlock5 {
    const ID: u32 = 8015;
    const STRING_ID: &'static str = "minecraft:light_block_5";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2482;
    const MAX_STATE_ID: u32 = 2482;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock4;

impl BlockDef for LightBlock4 {
    const ID: u32 = 8016;
    const STRING_ID: &'static str = "minecraft:light_block_4";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2483;
    const MAX_STATE_ID: u32 = 2483;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock3;

impl BlockDef for LightBlock3 {
    const ID: u32 = 8017;
    const STRING_ID: &'static str = "minecraft:light_block_3";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2484;
    const MAX_STATE_ID: u32 = 2484;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock2;

impl BlockDef for LightBlock2 {
    const ID: u32 = 8018;
    const STRING_ID: &'static str = "minecraft:light_block_2";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2485;
    const MAX_STATE_ID: u32 = 2485;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock1;

impl BlockDef for LightBlock1 {
    const ID: u32 = 8019;
    const STRING_ID: &'static str = "minecraft:light_block_1";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2486;
    const MAX_STATE_ID: u32 = 2486;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock0;

impl BlockDef for LightBlock0 {
    const ID: u32 = 8020;
    const STRING_ID: &'static str = "minecraft:light_block_0";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2487;
    const MAX_STATE_ID: u32 = 2487;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Door
pub struct PaleOakDoor;

impl BlockDef for PaleOakDoor {
    const ID: u32 = 652;
    const STRING_ID: &'static str = "minecraft:pale_oak_door";
    const NAME: &'static str = "Pale Oak Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2488;
    const MAX_STATE_ID: u32 = 2519;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Sapling
pub struct OakSapling;

impl BlockDef for OakSapling {
    const ID: u32 = 25;
    const STRING_ID: &'static str = "minecraft:oak_sapling";
    const NAME: &'static str = "Oak Sapling";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2520;
    const MAX_STATE_ID: u32 = 2521;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Slab
pub struct PolishedBlackstoneDoubleSlab;

impl BlockDef for PolishedBlackstoneDoubleSlab {
    const ID: u32 = 937;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_double_slab";
    const NAME: &'static str = "Polished Blackstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2522;
    const MAX_STATE_ID: u32 = 2523;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Terracotta
pub struct LightGrayTerracotta;

impl BlockDef for LightGrayTerracotta {
    const ID: u32 = 492;
    const STRING_ID: &'static str = "minecraft:light_gray_terracotta";
    const NAME: &'static str = "Light Gray Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2524;
    const MAX_STATE_ID: u32 = 2524;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smoker
pub struct Smoker;

impl BlockDef for Smoker {
    const ID: u32 = 840;
    const STRING_ID: &'static str = "minecraft:smoker";
    const NAME: &'static str = "Smoker";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2525;
    const MAX_STATE_ID: u32 = 2528;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Stained Glass
pub struct BrownStainedGlass;

impl BlockDef for BrownStainedGlass {
    const ID: u32 = 312;
    const STRING_ID: &'static str = "minecraft:brown_stained_glass";
    const NAME: &'static str = "Brown Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2529;
    const MAX_STATE_ID: u32 = 2529;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Andesite
pub struct Andesite;

impl BlockDef for Andesite {
    const ID: u32 = 6;
    const STRING_ID: &'static str = "minecraft:andesite";
    const NAME: &'static str = "Andesite";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2530;
    const MAX_STATE_ID: u32 = 2530;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Fire Coral
pub struct FireCoral;

impl BlockDef for FireCoral {
    const ID: u32 = 766;
    const STRING_ID: &'static str = "minecraft:fire_coral";
    const NAME: &'static str = "Fire Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 2531;
    const MAX_STATE_ID: u32 = 2531;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone
pub struct Stone;

impl BlockDef for Stone {
    const ID: u32 = 1;
    const STRING_ID: &'static str = "minecraft:stone";
    const NAME: &'static str = "Stone";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2532;
    const MAX_STATE_ID: u32 = 2532;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Sandstone Slab
pub struct SmoothSandstoneSlab;

impl BlockDef for SmoothSandstoneSlab {
    const ID: u32 = 817;
    const STRING_ID: &'static str = "minecraft:smooth_sandstone_slab";
    const NAME: &'static str = "Smooth Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2533;
    const MAX_STATE_ID: u32 = 2534;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Log
pub struct BirchLog;

impl BlockDef for BirchLog {
    const ID: u32 = 51;
    const STRING_ID: &'static str = "minecraft:birch_log";
    const NAME: &'static str = "Birch Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2535;
    const MAX_STATE_ID: u32 = 2537;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Glass Pane
pub struct HardGlassPane;

impl BlockDef for HardGlassPane {
    const ID: u32 = 190;
    const STRING_ID: &'static str = "minecraft:hard_glass_pane";
    const NAME: &'static str = "Hard Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2538;
    const MAX_STATE_ID: u32 = 2538;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff Brick Wall
pub struct TuffBrickWall;

impl BlockDef for TuffBrickWall {
    const ID: u32 = 996;
    const STRING_ID: &'static str = "minecraft:tuff_brick_wall";
    const NAME: &'static str = "Tuff Brick Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2539;
    const MAX_STATE_ID: u32 = 2700;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purpur Slab
pub struct PurpurSlab;

impl BlockDef for PurpurSlab {
    const ID: u32 = 623;
    const STRING_ID: &'static str = "minecraft:purpur_slab";
    const NAME: &'static str = "Purpur Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2701;
    const MAX_STATE_ID: u32 = 2702;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brain Coral
pub struct BrainCoral;

impl BlockDef for BrainCoral {
    const ID: u32 = 764;
    const STRING_ID: &'static str = "minecraft:brain_coral";
    const NAME: &'static str = "Brain Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 2703;
    const MAX_STATE_ID: u32 = 2703;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Spruce Wood
pub struct StrippedSpruceWood;

impl BlockDef for StrippedSpruceWood {
    const ID: u32 = 80;
    const STRING_ID: &'static str = "minecraft:stripped_spruce_wood";
    const NAME: &'static str = "Stripped Spruce Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2704;
    const MAX_STATE_ID: u32 = 2706;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Wool
pub struct OrangeWool;

impl BlockDef for OrangeWool {
    const ID: u32 = 141;
    const STRING_ID: &'static str = "minecraft:orange_wool";
    const NAME: &'static str = "Orange Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2707;
    const MAX_STATE_ID: u32 = 2707;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Brick Slab
pub struct PolishedBlackstoneBrickDoubleSlab;

impl BlockDef for PolishedBlackstoneBrickDoubleSlab {
    const ID: u32 = 932;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_brick_double_slab";
    const NAME: &'static str = "Polished Blackstone Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2708;
    const MAX_STATE_ID: u32 = 2709;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Slab
pub struct CrimsonDoubleSlab;

impl BlockDef for CrimsonDoubleSlab {
    const ID: u32 = 885;
    const STRING_ID: &'static str = "minecraft:crimson_double_slab";
    const NAME: &'static str = "Crimson Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2710;
    const MAX_STATE_ID: u32 = 2711;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Respawn Anchor
pub struct RespawnAnchor;

impl BlockDef for RespawnAnchor {
    const ID: u32 = 918;
    const STRING_ID: &'static str = "minecraft:respawn_anchor";
    const NAME: &'static str = "Respawn Anchor";
    const HARDNESS: f32 = 50.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2712;
    const MAX_STATE_ID: u32 = 2716;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Concrete
pub struct LightGrayConcrete;

impl BlockDef for LightGrayConcrete {
    const ID: u32 = 718;
    const STRING_ID: &'static str = "minecraft:light_gray_concrete";
    const NAME: &'static str = "Light Gray Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2717;
    const MAX_STATE_ID: u32 = 2717;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Candle
pub struct GreenCandle;

impl BlockDef for GreenCandle {
    const ID: u32 = 958;
    const STRING_ID: &'static str = "minecraft:green_candle";
    const NAME: &'static str = "Green Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2718;
    const MAX_STATE_ID: u32 = 2725;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper
pub struct WaxedExposedCopper;

impl BlockDef for WaxedExposedCopper {
    const ID: u32 = 1039;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper";
    const NAME: &'static str = "Waxed Exposed Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2726;
    const MAX_STATE_ID: u32 = 2726;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Sandstone Slab
pub struct RedSandstoneDoubleSlab;

impl BlockDef for RedSandstoneDoubleSlab {
    const ID: u32 = 621;
    const STRING_ID: &'static str = "minecraft:red_sandstone_double_slab";
    const NAME: &'static str = "Red Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2727;
    const MAX_STATE_ID: u32 = 2728;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Wood
pub struct BirchWood;

impl BlockDef for BirchWood {
    const ID: u32 = 73;
    const STRING_ID: &'static str = "minecraft:birch_wood";
    const NAME: &'static str = "Birch Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2729;
    const MAX_STATE_ID: u32 = 2731;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Sand
pub struct RedSand;

impl BlockDef for RedSand {
    const ID: u32 = 39;
    const STRING_ID: &'static str = "minecraft:red_sand";
    const NAME: &'static str = "Red Sand";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2732;
    const MAX_STATE_ID: u32 = 2732;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hay Bale
pub struct HayBlock;

impl BlockDef for HayBlock {
    const ID: u32 = 170;
    const STRING_ID: &'static str = "minecraft:hay_block";
    const NAME: &'static str = "Hay Bale";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2733;
    const MAX_STATE_ID: u32 = 2744;
    type State = super::states::DeprecatedPillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Wood
pub struct JungleWood;

impl BlockDef for JungleWood {
    const ID: u32 = 74;
    const STRING_ID: &'static str = "minecraft:jungle_wood";
    const NAME: &'static str = "Jungle Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2745;
    const MAX_STATE_ID: u32 = 2747;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper
pub struct WaxedWeatheredCopper;

impl BlockDef for WaxedWeatheredCopper {
    const ID: u32 = 1040;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper";
    const NAME: &'static str = "Waxed Weathered Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2748;
    const MAX_STATE_ID: u32 = 2748;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Infested Cracked Stone Bricks
pub struct InfestedCrackedStoneBricks;

impl BlockDef for InfestedCrackedStoneBricks {
    const ID: u32 = 336;
    const STRING_ID: &'static str = "minecraft:infested_cracked_stone_bricks";
    const NAME: &'static str = "Infested Cracked Stone Bricks";
    const HARDNESS: f32 = 0.75_f32;
    const RESISTANCE: f32 = 0.75_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2749;
    const MAX_STATE_ID: u32 = 2749;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Cut Copper Slab
pub struct WaxedOxidizedCutCopperSlab;

impl BlockDef for WaxedOxidizedCutCopperSlab {
    const ID: u32 = 8094;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_cut_copper_slab";
    const NAME: &'static str = "Waxed Oxidized Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2750;
    const MAX_STATE_ID: u32 = 2751;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Leaves
pub struct OakLeaves;

impl BlockDef for OakLeaves {
    const ID: u32 = 88;
    const STRING_ID: &'static str = "minecraft:oak_leaves";
    const NAME: &'static str = "Oak Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 2752;
    const MAX_STATE_ID: u32 = 2755;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Resin Clump
pub struct ResinClump;

impl BlockDef for ResinClump {
    const ID: u32 = 368;
    const STRING_ID: &'static str = "minecraft:resin_clump";
    const NAME: &'static str = "Resin Clump";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 2756;
    const MAX_STATE_ID: u32 = 2819;
    type State = super::states::MultiFaceState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brain Coral Fan
pub struct BrainCoralFan;

impl BlockDef for BrainCoralFan {
    const ID: u32 = 774;
    const STRING_ID: &'static str = "minecraft:brain_coral_fan";
    const NAME: &'static str = "Brain Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 2820;
    const MAX_STATE_ID: u32 = 2821;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Cyan Candle
pub struct CyanCandleCake;

impl BlockDef for CyanCandleCake {
    const ID: u32 = 971;
    const STRING_ID: &'static str = "minecraft:cyan_candle_cake";
    const NAME: &'static str = "Cake with Cyan Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2822;
    const MAX_STATE_ID: u32 = 2823;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Tuff Wall
pub struct PolishedTuffWall;

impl BlockDef for PolishedTuffWall {
    const ID: u32 = 991;
    const STRING_ID: &'static str = "minecraft:polished_tuff_wall";
    const NAME: &'static str = "Polished Tuff Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2824;
    const MAX_STATE_ID: u32 = 2985;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Stairs
pub struct BambooStairs;

impl BlockDef for BambooStairs {
    const ID: u32 = 521;
    const STRING_ID: &'static str = "minecraft:bamboo_stairs";
    const NAME: &'static str = "Bamboo Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2986;
    const MAX_STATE_ID: u32 = 2993;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Infested Mossy Stone Bricks
pub struct InfestedMossyStoneBricks;

impl BlockDef for InfestedMossyStoneBricks {
    const ID: u32 = 335;
    const STRING_ID: &'static str = "minecraft:infested_mossy_stone_bricks";
    const NAME: &'static str = "Infested Mossy Stone Bricks";
    const HARDNESS: f32 = 0.75_f32;
    const RESISTANCE: f32 = 0.75_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2994;
    const MAX_STATE_ID: u32 = 2994;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Torch
pub struct Torch;

impl BlockDef for Torch {
    const ID: u32 = 50;
    const STRING_ID: &'static str = "minecraft:torch";
    const NAME: &'static str = "Torch";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 14;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 2995;
    const MAX_STATE_ID: u32 = 3000;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mud Brick Wall
pub struct MudBrickWall;

impl BlockDef for MudBrickWall {
    const ID: u32 = 830;
    const STRING_ID: &'static str = "minecraft:mud_brick_wall";
    const NAME: &'static str = "Mud Brick Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3001;
    const MAX_STATE_ID: u32 = 3162;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Honey Block
pub struct HoneyBlock;

impl BlockDef for HoneyBlock {
    const ID: u32 = 913;
    const STRING_ID: &'static str = "minecraft:honey_block";
    const NAME: &'static str = "Honey Block";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 3163;
    const MAX_STATE_ID: u32 = 3163;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Underwater Tnt
pub struct UnderwaterTnt;

impl BlockDef for UnderwaterTnt {
    const ID: u32 = 3339;
    const STRING_ID: &'static str = "minecraft:underwater_tnt";
    const NAME: &'static str = "Underwater Tnt";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3164;
    const MAX_STATE_ID: u32 = 3165;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dripstone Block
pub struct DripstoneBlock;

impl BlockDef for DripstoneBlock {
    const ID: u32 = 1132;
    const STRING_ID: &'static str = "minecraft:dripstone_block";
    const NAME: &'static str = "Dripstone Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3166;
    const MAX_STATE_ID: u32 = 3166;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Vines
pub struct Vine;

impl BlockDef for Vine {
    const ID: u32 = 106;
    const STRING_ID: &'static str = "minecraft:vine";
    const NAME: &'static str = "Vines";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3167;
    const MAX_STATE_ID: u32 = 3182;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Sandstone Slab
pub struct RedSandstoneSlab;

impl BlockDef for RedSandstoneSlab {
    const ID: u32 = 8052;
    const STRING_ID: &'static str = "minecraft:red_sandstone_slab";
    const NAME: &'static str = "Red Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3183;
    const MAX_STATE_ID: u32 = 3184;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Trapdoor
pub struct CherryTrapdoor;

impl BlockDef for CherryTrapdoor {
    const ID: u32 = 321;
    const STRING_ID: &'static str = "minecraft:cherry_trapdoor";
    const NAME: &'static str = "Cherry Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3185;
    const MAX_STATE_ID: u32 = 3200;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blackstone Slab
pub struct BlackstoneSlab;

impl BlockDef for BlackstoneSlab {
    const ID: u32 = 8072;
    const STRING_ID: &'static str = "minecraft:blackstone_slab";
    const NAME: &'static str = "Blackstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3201;
    const MAX_STATE_ID: u32 = 3202;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gold Ore
pub struct GoldOre;

impl BlockDef for GoldOre {
    const ID: u32 = 14;
    const STRING_ID: &'static str = "minecraft:gold_ore";
    const NAME: &'static str = "Gold Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3203;
    const MAX_STATE_ID: u32 = 3203;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Glazed Terracotta
pub struct YellowGlazedTerracotta;

impl BlockDef for YellowGlazedTerracotta {
    const ID: u32 = 224;
    const STRING_ID: &'static str = "minecraft:yellow_glazed_terracotta";
    const NAME: &'static str = "Yellow Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3204;
    const MAX_STATE_ID: u32 = 3209;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stonecutter
pub struct Stonecutter;

impl BlockDef for Stonecutter {
    const ID: u32 = 245;
    const STRING_ID: &'static str = "minecraft:stonecutter";
    const NAME: &'static str = "Stonecutter";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3210;
    const MAX_STATE_ID: u32 = 3210;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dried Ghast
pub struct DriedGhast;

impl BlockDef for DriedGhast {
    const ID: u32 = 747;
    const STRING_ID: &'static str = "minecraft:dried_ghast";
    const NAME: &'static str = "Dried Ghast";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3211;
    const MAX_STATE_ID: u32 = 3226;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Planks
pub struct WarpedPlanks;

impl BlockDef for WarpedPlanks {
    const ID: u32 = 884;
    const STRING_ID: &'static str = "minecraft:warped_planks";
    const NAME: &'static str = "Warped Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3227;
    const MAX_STATE_ID: u32 = 3227;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Piston
pub struct Piston;

impl BlockDef for Piston {
    const ID: u32 = 33;
    const STRING_ID: &'static str = "minecraft:piston";
    const NAME: &'static str = "Piston";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3228;
    const MAX_STATE_ID: u32 = 3233;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Carpet
pub struct BrownCarpet;

impl BlockDef for BrownCarpet {
    const ID: u32 = 550;
    const STRING_ID: &'static str = "minecraft:brown_carpet";
    const NAME: &'static str = "Brown Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3234;
    const MAX_STATE_ID: u32 = 3234;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Brick Stairs
pub struct StoneBrickStairs;

impl BlockDef for StoneBrickStairs {
    const ID: u32 = 109;
    const STRING_ID: &'static str = "minecraft:stone_brick_stairs";
    const NAME: &'static str = "Stone Brick Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3235;
    const MAX_STATE_ID: u32 = 3242;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Bubble Coral Block
pub struct DeadBubbleCoralBlock;

impl BlockDef for DeadBubbleCoralBlock {
    const ID: u32 = 750;
    const STRING_ID: &'static str = "minecraft:dead_bubble_coral_block";
    const NAME: &'static str = "Dead Bubble Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3243;
    const MAX_STATE_ID: u32 = 3243;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Candle
pub struct GrayCandle;

impl BlockDef for GrayCandle {
    const ID: u32 = 952;
    const STRING_ID: &'static str = "minecraft:gray_candle";
    const NAME: &'static str = "Gray Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3244;
    const MAX_STATE_ID: u32 = 3251;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Fence
pub struct CherryFence;

impl BlockDef for CherryFence {
    const ID: u32 = 641;
    const STRING_ID: &'static str = "minecraft:cherry_fence";
    const NAME: &'static str = "Cherry Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3252;
    const MAX_STATE_ID: u32 = 3252;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Planks
pub struct MangrovePlanks;

impl BlockDef for MangrovePlanks {
    const ID: u32 = 22;
    const STRING_ID: &'static str = "minecraft:mangrove_planks";
    const NAME: &'static str = "Mangrove Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3253;
    const MAX_STATE_ID: u32 = 3253;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Invisible Bedrock
pub struct InvisibleBedrock;

impl BlockDef for InvisibleBedrock {
    const ID: u32 = 3429;
    const STRING_ID: &'static str = "minecraft:invisible_bedrock";
    const NAME: &'static str = "Invisible Bedrock";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3254;
    const MAX_STATE_ID: u32 = 3254;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Terracotta
pub struct RedTerracotta;

impl BlockDef for RedTerracotta {
    const ID: u32 = 498;
    const STRING_ID: &'static str = "minecraft:red_terracotta";
    const NAME: &'static str = "Red Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3255;
    const MAX_STATE_ID: u32 = 3255;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Diorite Wall
pub struct DioriteWall;

impl BlockDef for DioriteWall {
    const ID: u32 = 836;
    const STRING_ID: &'static str = "minecraft:diorite_wall";
    const NAME: &'static str = "Diorite Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3256;
    const MAX_STATE_ID: u32 = 3417;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Fire Coral Block
pub struct DeadFireCoralBlock;

impl BlockDef for DeadFireCoralBlock {
    const ID: u32 = 751;
    const STRING_ID: &'static str = "minecraft:dead_fire_coral_block";
    const NAME: &'static str = "Dead Fire Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3418;
    const MAX_STATE_ID: u32 = 3418;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper Bulb
pub struct OxidizedCopperBulb;

impl BlockDef for OxidizedCopperBulb {
    const ID: u32 = 1103;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_bulb";
    const NAME: &'static str = "Oxidized Copper Bulb";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3419;
    const MAX_STATE_ID: u32 = 3422;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Wool
pub struct MagentaWool;

impl BlockDef for MagentaWool {
    const ID: u32 = 142;
    const STRING_ID: &'static str = "minecraft:magenta_wool";
    const NAME: &'static str = "Magenta Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3423;
    const MAX_STATE_ID: u32 = 3423;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper Bars
pub struct OxidizedCopperBars;

impl BlockDef for OxidizedCopperBars {
    const ID: u32 = 345;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_bars";
    const NAME: &'static str = "Oxidized Copper Bars";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3424;
    const MAX_STATE_ID: u32 = 3424;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Glazed Terracotta
pub struct MagentaGlazedTerracotta;

impl BlockDef for MagentaGlazedTerracotta {
    const ID: u32 = 222;
    const STRING_ID: &'static str = "minecraft:magenta_glazed_terracotta";
    const NAME: &'static str = "Magenta Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3425;
    const MAX_STATE_ID: u32 = 3430;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Quartz Slab
pub struct QuartzDoubleSlab;

impl BlockDef for QuartzDoubleSlab {
    const ID: u32 = 620;
    const STRING_ID: &'static str = "minecraft:quartz_double_slab";
    const NAME: &'static str = "Quartz Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3431;
    const MAX_STATE_ID: u32 = 3432;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Brick Wall
pub struct PolishedBlackstoneBrickWall;

impl BlockDef for PolishedBlackstoneBrickWall {
    const ID: u32 = 934;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_brick_wall";
    const NAME: &'static str = "Polished Blackstone Brick Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3433;
    const MAX_STATE_ID: u32 = 3594;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Slab
pub struct MangroveSlab;

impl BlockDef for MangroveSlab {
    const ID: u32 = 8038;
    const STRING_ID: &'static str = "minecraft:mangrove_slab";
    const NAME: &'static str = "Mangrove Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3595;
    const MAX_STATE_ID: u32 = 3596;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Glazed Terracotta
pub struct OrangeGlazedTerracotta;

impl BlockDef for OrangeGlazedTerracotta {
    const ID: u32 = 221;
    const STRING_ID: &'static str = "minecraft:orange_glazed_terracotta";
    const NAME: &'static str = "Orange Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3597;
    const MAX_STATE_ID: u32 = 3602;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Brown Stained Glass Pane
pub struct HardBrownStainedGlassPane;

impl BlockDef for HardBrownStainedGlassPane {
    const ID: u32 = 3778;
    const STRING_ID: &'static str = "minecraft:hard_brown_stained_glass_pane";
    const NAME: &'static str = "Hard Brown Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3603;
    const MAX_STATE_ID: u32 = 3603;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Basalt
pub struct SmoothBasalt;

impl BlockDef for SmoothBasalt {
    const ID: u32 = 1172;
    const STRING_ID: &'static str = "minecraft:smooth_basalt";
    const NAME: &'static str = "Smooth Basalt";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3604;
    const MAX_STATE_ID: u32 = 3604;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lily Pad
pub struct Waterlily;

impl BlockDef for Waterlily {
    const ID: u32 = 111;
    const STRING_ID: &'static str = "minecraft:waterlily";
    const NAME: &'static str = "Lily Pad";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3605;
    const MAX_STATE_ID: u32 = 3605;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Pale Oak Wood
pub struct StrippedPaleOakWood;

impl BlockDef for StrippedPaleOakWood {
    const ID: u32 = 86;
    const STRING_ID: &'static str = "minecraft:stripped_pale_oak_wood";
    const NAME: &'static str = "Stripped Pale Oak Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3606;
    const MAX_STATE_ID: u32 = 3608;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Light Blue Stained Glass
pub struct HardLightBlueStainedGlass;

impl BlockDef for HardLightBlueStainedGlass {
    const ID: u32 = 3784;
    const STRING_ID: &'static str = "minecraft:hard_light_blue_stained_glass";
    const NAME: &'static str = "Hard Light Blue Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3609;
    const MAX_STATE_ID: u32 = 3609;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Emerald
pub struct EmeraldBlock;

impl BlockDef for EmeraldBlock {
    const ID: u32 = 133;
    const STRING_ID: &'static str = "minecraft:emerald_block";
    const NAME: &'static str = "Block of Emerald";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3610;
    const MAX_STATE_ID: u32 = 3610;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Suspicious Sand
pub struct SuspiciousSand;

impl BlockDef for SuspiciousSand {
    const ID: u32 = 38;
    const STRING_ID: &'static str = "minecraft:suspicious_sand";
    const NAME: &'static str = "Suspicious Sand";
    const HARDNESS: f32 = 0.25_f32;
    const RESISTANCE: f32 = 0.25_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3611;
    const MAX_STATE_ID: u32 = 3618;
    type State = super::states::BrushableState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Cobblestone Wall
pub struct MossyCobblestoneWall;

impl BlockDef for MossyCobblestoneWall {
    const ID: u32 = 410;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone_wall";
    const NAME: &'static str = "Mossy Cobblestone Wall";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3619;
    const MAX_STATE_ID: u32 = 3780;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Heavy Weighted Pressure Plate
pub struct HeavyWeightedPressurePlate;

impl BlockDef for HeavyWeightedPressurePlate {
    const ID: u32 = 148;
    const STRING_ID: &'static str = "minecraft:heavy_weighted_pressure_plate";
    const NAME: &'static str = "Heavy Weighted Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3781;
    const MAX_STATE_ID: u32 = 3796;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Stained Glass
pub struct PurpleStainedGlass;

impl BlockDef for PurpleStainedGlass {
    const ID: u32 = 310;
    const STRING_ID: &'static str = "minecraft:purple_stained_glass";
    const NAME: &'static str = "Purple Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3797;
    const MAX_STATE_ID: u32 = 3797;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lightning Rod
pub struct LightningRod;

impl BlockDef for LightningRod {
    const ID: u32 = 1124;
    const STRING_ID: &'static str = "minecraft:lightning_rod";
    const NAME: &'static str = "Lightning Rod";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3798;
    const MAX_STATE_ID: u32 = 3809;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Leaves
pub struct AcaciaLeaves;

impl BlockDef for AcaciaLeaves {
    const ID: u32 = 92;
    const STRING_ID: &'static str = "minecraft:acacia_leaves";
    const NAME: &'static str = "Acacia Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 3810;
    const MAX_STATE_ID: u32 = 3813;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Stained Glass Pane
pub struct BlackStainedGlassPane;

impl BlockDef for BlackStainedGlassPane {
    const ID: u32 = 515;
    const STRING_ID: &'static str = "minecraft:black_stained_glass_pane";
    const NAME: &'static str = "Black Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3814;
    const MAX_STATE_ID: u32 = 3814;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobblestone Wall
pub struct CobblestoneWall;

impl BlockDef for CobblestoneWall {
    const ID: u32 = 139;
    const STRING_ID: &'static str = "minecraft:cobblestone_wall";
    const NAME: &'static str = "Cobblestone Wall";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3815;
    const MAX_STATE_ID: u32 = 3976;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Underwater Torch
pub struct UnderwaterTorch;

impl BlockDef for UnderwaterTorch {
    const ID: u32 = 239;
    const STRING_ID: &'static str = "minecraft:underwater_torch";
    const NAME: &'static str = "Underwater Torch";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3977;
    const MAX_STATE_ID: u32 = 3982;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Brick Slab
pub struct DeepslateBrickDoubleSlab;

impl BlockDef for DeepslateBrickDoubleSlab {
    const ID: u32 = 8093;
    const STRING_ID: &'static str = "minecraft:deepslate_brick_double_slab";
    const NAME: &'static str = "Deepslate Brick Slab";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3983;
    const MAX_STATE_ID: u32 = 3984;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Slab
pub struct SpruceDoubleSlab;

impl BlockDef for SpruceDoubleSlab {
    const ID: u32 = 600;
    const STRING_ID: &'static str = "minecraft:spruce_double_slab";
    const NAME: &'static str = "Spruce Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3985;
    const MAX_STATE_ID: u32 = 3986;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Mosaic Slab
pub struct BambooMosaicSlab;

impl BlockDef for BambooMosaicSlab {
    const ID: u32 = 609;
    const STRING_ID: &'static str = "minecraft:bamboo_mosaic_slab";
    const NAME: &'static str = "Bamboo Mosaic Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3987;
    const MAX_STATE_ID: u32 = 3988;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Log
pub struct DarkOakLog;

impl BlockDef for DarkOakLog {
    const ID: u32 = 55;
    const STRING_ID: &'static str = "minecraft:dark_oak_log";
    const NAME: &'static str = "Dark Oak Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 3989;
    const MAX_STATE_ID: u32 = 3991;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Hanging Sign
pub struct AcaciaHangingSign;

impl BlockDef for AcaciaHangingSign {
    const ID: u32 = 237;
    const STRING_ID: &'static str = "minecraft:acacia_hanging_sign";
    const NAME: &'static str = "Acacia Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 3992;
    const MAX_STATE_ID: u32 = 4375;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Ochre Froglight
pub struct OchreFroglight;

impl BlockDef for OchreFroglight {
    const ID: u32 = 1178;
    const STRING_ID: &'static str = "minecraft:ochre_froglight";
    const NAME: &'static str = "Ochre Froglight";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 4376;
    const MAX_STATE_ID: u32 = 4378;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff Wall
pub struct TuffWall;

impl BlockDef for TuffWall {
    const ID: u32 = 987;
    const STRING_ID: &'static str = "minecraft:tuff_wall";
    const NAME: &'static str = "Tuff Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 4379;
    const MAX_STATE_ID: u32 = 4540;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Observer
pub struct Observer;

impl BlockDef for Observer {
    const ID: u32 = 251;
    const STRING_ID: &'static str = "minecraft:observer";
    const NAME: &'static str = "Observer";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 4541;
    const MAX_STATE_ID: u32 = 4552;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Torch
pub struct RedstoneTorch;

impl BlockDef for RedstoneTorch {
    const ID: u32 = 76;
    const STRING_ID: &'static str = "minecraft:redstone_torch";
    const NAME: &'static str = "Redstone Torch";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 7;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 4553;
    const MAX_STATE_ID: u32 = 4558;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Glazed Terracotta
pub struct SilverGlazedTerracotta;

impl BlockDef for SilverGlazedTerracotta {
    const ID: u32 = 228;
    const STRING_ID: &'static str = "minecraft:silver_glazed_terracotta";
    const NAME: &'static str = "Light Gray Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 4559;
    const MAX_STATE_ID: u32 = 4564;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Granite Stairs
pub struct GraniteStairs;

impl BlockDef for GraniteStairs {
    const ID: u32 = 806;
    const STRING_ID: &'static str = "minecraft:granite_stairs";
    const NAME: &'static str = "Granite Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 4565;
    const MAX_STATE_ID: u32 = 4572;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Concrete
pub struct PinkConcrete;

impl BlockDef for PinkConcrete {
    const ID: u32 = 716;
    const STRING_ID: &'static str = "minecraft:pink_concrete";
    const NAME: &'static str = "Pink Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 4573;
    const MAX_STATE_ID: u32 = 4573;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Hanging Sign
pub struct DarkOakHangingSign;

impl BlockDef for DarkOakHangingSign {
    const ID: u32 = 240;
    const STRING_ID: &'static str = "minecraft:dark_oak_hanging_sign";
    const NAME: &'static str = "Dark Oak Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 4574;
    const MAX_STATE_ID: u32 = 4957;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Glowingobsidian
pub struct Glowingobsidian;

impl BlockDef for Glowingobsidian {
    const ID: u32 = 246;
    const STRING_ID: &'static str = "minecraft:glowingobsidian";
    const NAME: &'static str = "Glowingobsidian";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 4958;
    const MAX_STATE_ID: u32 = 4958;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Mushroom
pub struct BrownMushroom;

impl BlockDef for BrownMushroom {
    const ID: u32 = 39;
    const STRING_ID: &'static str = "minecraft:brown_mushroom";
    const NAME: &'static str = "Brown Mushroom";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 1;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 4959;
    const MAX_STATE_ID: u32 = 4959;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cyan Concrete Powder
pub struct CyanConcretePowder;

impl BlockDef for CyanConcretePowder {
    const ID: u32 = 735;
    const STRING_ID: &'static str = "minecraft:cyan_concrete_powder";
    const NAME: &'static str = "Cyan Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 4960;
    const MAX_STATE_ID: u32 = 4960;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Fire Coral Wall Fan
pub struct DeadFireCoralWallFan;

impl BlockDef for DeadFireCoralWallFan {
    const ID: u32 = 781;
    const STRING_ID: &'static str = "minecraft:dead_fire_coral_wall_fan";
    const NAME: &'static str = "Dead Fire Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 4961;
    const MAX_STATE_ID: u32 = 4964;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Glazed Terracotta
pub struct BrownGlazedTerracotta;

impl BlockDef for BrownGlazedTerracotta {
    const ID: u32 = 232;
    const STRING_ID: &'static str = "minecraft:brown_glazed_terracotta";
    const NAME: &'static str = "Brown Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 4965;
    const MAX_STATE_ID: u32 = 4970;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Copper Trapdoor
pub struct WaxedCopperTrapdoor;

impl BlockDef for WaxedCopperTrapdoor {
    const ID: u32 = 1088;
    const STRING_ID: &'static str = "minecraft:waxed_copper_trapdoor";
    const NAME: &'static str = "Waxed Copper Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 4971;
    const MAX_STATE_ID: u32 = 4986;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Shelf
pub struct SpruceShelf;

impl BlockDef for SpruceShelf {
    const ID: u32 = 190;
    const STRING_ID: &'static str = "minecraft:spruce_shelf";
    const NAME: &'static str = "Spruce Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 4987;
    const MAX_STATE_ID: u32 = 5018;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Resin Brick Slab
pub struct ResinBrickDoubleSlab;

impl BlockDef for ResinBrickDoubleSlab {
    const ID: u32 = 378;
    const STRING_ID: &'static str = "minecraft:resin_brick_double_slab";
    const NAME: &'static str = "Resin Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5019;
    const MAX_STATE_ID: u32 = 5020;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper
pub struct OxidizedCopper;

impl BlockDef for OxidizedCopper {
    const ID: u32 = 1037;
    const STRING_ID: &'static str = "minecraft:oxidized_copper";
    const NAME: &'static str = "Oxidized Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5021;
    const MAX_STATE_ID: u32 = 5021;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Ore
pub struct CopperOre;

impl BlockDef for CopperOre {
    const ID: u32 = 1042;
    const STRING_ID: &'static str = "minecraft:copper_ore";
    const NAME: &'static str = "Copper Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5022;
    const MAX_STATE_ID: u32 = 5022;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Planks
pub struct DarkOakPlanks;

impl BlockDef for DarkOakPlanks {
    const ID: u32 = 19;
    const STRING_ID: &'static str = "minecraft:dark_oak_planks";
    const NAME: &'static str = "Dark Oak Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5023;
    const MAX_STATE_ID: u32 = 5023;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Pressure Plate
pub struct BirchPressurePlate;

impl BlockDef for BirchPressurePlate {
    const ID: u32 = 263;
    const STRING_ID: &'static str = "minecraft:birch_pressure_plate";
    const NAME: &'static str = "Birch Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5024;
    const MAX_STATE_ID: u32 = 5039;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Scaffolding
pub struct Scaffolding;

impl BlockDef for Scaffolding {
    const ID: u32 = 837;
    const STRING_ID: &'static str = "minecraft:scaffolding";
    const NAME: &'static str = "Scaffolding";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5040;
    const MAX_STATE_ID: u32 = 5055;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sandstone Stairs
pub struct SandstoneStairs;

impl BlockDef for SandstoneStairs {
    const ID: u32 = 128;
    const STRING_ID: &'static str = "minecraft:sandstone_stairs";
    const NAME: &'static str = "Sandstone Stairs";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5056;
    const MAX_STATE_ID: u32 = 5063;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Green Candle
pub struct GreenCandleCake;

impl BlockDef for GreenCandleCake {
    const ID: u32 = 975;
    const STRING_ID: &'static str = "minecraft:green_candle_cake";
    const NAME: &'static str = "Cake with Green Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5064;
    const MAX_STATE_ID: u32 = 5065;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Stripped Bamboo
pub struct StrippedBambooBlock;

impl BlockDef for StrippedBambooBlock {
    const ID: u32 = 70;
    const STRING_ID: &'static str = "minecraft:stripped_bamboo_block";
    const NAME: &'static str = "Block of Stripped Bamboo";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5066;
    const MAX_STATE_ID: u32 = 5068;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Mushroom Block
pub struct RedMushroomBlock;

impl BlockDef for RedMushroomBlock {
    const ID: u32 = 100;
    const STRING_ID: &'static str = "minecraft:red_mushroom_block";
    const NAME: &'static str = "Red Mushroom Block";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5069;
    const MAX_STATE_ID: u32 = 5084;
    type State = super::states::MushroomState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cracked Stone Bricks
pub struct CrackedStoneBricks;

impl BlockDef for CrackedStoneBricks {
    const ID: u32 = 328;
    const STRING_ID: &'static str = "minecraft:cracked_stone_bricks";
    const NAME: &'static str = "Cracked Stone Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5085;
    const MAX_STATE_ID: u32 = 5085;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sculk Catalyst
pub struct SculkCatalyst;

impl BlockDef for SculkCatalyst {
    const ID: u32 = 1032;
    const STRING_ID: &'static str = "minecraft:sculk_catalyst";
    const NAME: &'static str = "Sculk Catalyst";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 6;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5086;
    const MAX_STATE_ID: u32 = 5087;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobblestone
pub struct Cobblestone;

impl BlockDef for Cobblestone {
    const ID: u32 = 4;
    const STRING_ID: &'static str = "minecraft:cobblestone";
    const NAME: &'static str = "Cobblestone";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5088;
    const MAX_STATE_ID: u32 = 5088;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Lightning Rod
pub struct WaxedLightningRod;

impl BlockDef for WaxedLightningRod {
    const ID: u32 = 1128;
    const STRING_ID: &'static str = "minecraft:waxed_lightning_rod";
    const NAME: &'static str = "Waxed Lightning Rod";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5089;
    const MAX_STATE_ID: u32 = 5100;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Horn Coral
pub struct HornCoral;

impl BlockDef for HornCoral {
    const ID: u32 = 767;
    const STRING_ID: &'static str = "minecraft:horn_coral";
    const NAME: &'static str = "Horn Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 5101;
    const MAX_STATE_ID: u32 = 5101;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Concrete
pub struct YellowConcrete;

impl BlockDef for YellowConcrete {
    const ID: u32 = 714;
    const STRING_ID: &'static str = "minecraft:yellow_concrete";
    const NAME: &'static str = "Yellow Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5102;
    const MAX_STATE_ID: u32 = 5102;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Shelf
pub struct MangroveShelf;

impl BlockDef for MangroveShelf {
    const ID: u32 = 187;
    const STRING_ID: &'static str = "minecraft:mangrove_shelf";
    const NAME: &'static str = "Mangrove Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5103;
    const MAX_STATE_ID: u32 = 5134;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cyan Carpet
pub struct CyanCarpet;

impl BlockDef for CyanCarpet {
    const ID: u32 = 547;
    const STRING_ID: &'static str = "minecraft:cyan_carpet";
    const NAME: &'static str = "Cyan Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5135;
    const MAX_STATE_ID: u32 = 5135;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Shelf
pub struct WarpedShelf;

impl BlockDef for WarpedShelf {
    const ID: u32 = 191;
    const STRING_ID: &'static str = "minecraft:warped_shelf";
    const NAME: &'static str = "Warped Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5136;
    const MAX_STATE_ID: u32 = 5167;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Slab
pub struct OakDoubleSlab;

impl BlockDef for OakDoubleSlab {
    const ID: u32 = 599;
    const STRING_ID: &'static str = "minecraft:oak_double_slab";
    const NAME: &'static str = "Oak Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5168;
    const MAX_STATE_ID: u32 = 5169;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Sandstone Stairs
pub struct SmoothSandstoneStairs;

impl BlockDef for SmoothSandstoneStairs {
    const ID: u32 = 804;
    const STRING_ID: &'static str = "minecraft:smooth_sandstone_stairs";
    const NAME: &'static str = "Smooth Sandstone Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5170;
    const MAX_STATE_ID: u32 = 5177;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Pressure Plate
pub struct JunglePressurePlate;

impl BlockDef for JunglePressurePlate {
    const ID: u32 = 264;
    const STRING_ID: &'static str = "minecraft:jungle_pressure_plate";
    const NAME: &'static str = "Jungle Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5178;
    const MAX_STATE_ID: u32 = 5193;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Copper Slab
pub struct DoubleCutCopperSlab;

impl BlockDef for DoubleCutCopperSlab {
    const ID: u32 = 8089;
    const STRING_ID: &'static str = "minecraft:double_cut_copper_slab";
    const NAME: &'static str = "Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5194;
    const MAX_STATE_ID: u32 = 5195;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chalkboard
pub struct Chalkboard;

impl BlockDef for Chalkboard {
    const ID: u32 = 5373;
    const STRING_ID: &'static str = "minecraft:chalkboard";
    const NAME: &'static str = "Chalkboard";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5196;
    const MAX_STATE_ID: u32 = 5211;
    type State = super::states::DirectionState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Terracotta
pub struct BlueTerracotta;

impl BlockDef for BlueTerracotta {
    const ID: u32 = 495;
    const STRING_ID: &'static str = "minecraft:blue_terracotta";
    const NAME: &'static str = "Blue Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5212;
    const MAX_STATE_ID: u32 = 5212;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sandstone
pub struct Sandstone;

impl BlockDef for Sandstone {
    const ID: u32 = 24;
    const STRING_ID: &'static str = "minecraft:sandstone";
    const NAME: &'static str = "Sandstone";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5213;
    const MAX_STATE_ID: u32 = 5213;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Brown Candle
pub struct BrownCandleCake;

impl BlockDef for BrownCandleCake {
    const ID: u32 = 974;
    const STRING_ID: &'static str = "minecraft:brown_candle_cake";
    const NAME: &'static str = "Cake with Brown Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5214;
    const MAX_STATE_ID: u32 = 5215;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Sign
pub struct AcaciaWallSign;

impl BlockDef for AcaciaWallSign {
    const ID: u32 = 227;
    const STRING_ID: &'static str = "minecraft:acacia_wall_sign";
    const NAME: &'static str = "Acacia Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5216;
    const MAX_STATE_ID: u32 = 5221;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Weighted Pressure Plate
pub struct LightWeightedPressurePlate;

impl BlockDef for LightWeightedPressurePlate {
    const ID: u32 = 147;
    const STRING_ID: &'static str = "minecraft:light_weighted_pressure_plate";
    const NAME: &'static str = "Light Weighted Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5222;
    const MAX_STATE_ID: u32 = 5237;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Shulker Box
pub struct UndyedShulkerBox;

impl BlockDef for UndyedShulkerBox {
    const ID: u32 = 205;
    const STRING_ID: &'static str = "minecraft:undyed_shulker_box";
    const NAME: &'static str = "Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 5238;
    const MAX_STATE_ID: u32 = 5238;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone
pub struct PolishedBlackstone;

impl BlockDef for PolishedBlackstone {
    const ID: u32 = 928;
    const STRING_ID: &'static str = "minecraft:polished_blackstone";
    const NAME: &'static str = "Polished Blackstone";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5239;
    const MAX_STATE_ID: u32 = 5239;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mycelium
pub struct Mycelium;

impl BlockDef for Mycelium {
    const ID: u32 = 110;
    const STRING_ID: &'static str = "minecraft:mycelium";
    const NAME: &'static str = "Mycelium";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5240;
    const MAX_STATE_ID: u32 = 5240;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Lightning Rod
pub struct ExposedLightningRod;

impl BlockDef for ExposedLightningRod {
    const ID: u32 = 1125;
    const STRING_ID: &'static str = "minecraft:exposed_lightning_rod";
    const NAME: &'static str = "Exposed Lightning Rod";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5241;
    const MAX_STATE_ID: u32 = 5252;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo
pub struct Bamboo;

impl BlockDef for Bamboo {
    const ID: u32 = 792;
    const STRING_ID: &'static str = "minecraft:bamboo";
    const NAME: &'static str = "Bamboo";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5253;
    const MAX_STATE_ID: u32 = 5264;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Quartz
pub struct QuartzBlock;

impl BlockDef for QuartzBlock {
    const ID: u32 = 155;
    const STRING_ID: &'static str = "minecraft:quartz_block";
    const NAME: &'static str = "Block of Quartz";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5265;
    const MAX_STATE_ID: u32 = 5267;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Planks
pub struct PaleOakPlanks;

impl BlockDef for PaleOakPlanks {
    const ID: u32 = 21;
    const STRING_ID: &'static str = "minecraft:pale_oak_planks";
    const NAME: &'static str = "Pale Oak Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5268;
    const MAX_STATE_ID: u32 = 5268;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobblestone Stairs
pub struct StoneStairs;

impl BlockDef for StoneStairs {
    const ID: u32 = 67;
    const STRING_ID: &'static str = "minecraft:stone_stairs";
    const NAME: &'static str = "Cobblestone Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5269;
    const MAX_STATE_ID: u32 = 5276;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Chiseled Copper
pub struct WaxedWeatheredChiseledCopper;

impl BlockDef for WaxedWeatheredChiseledCopper {
    const ID: u32 = 1058;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_chiseled_copper";
    const NAME: &'static str = "Waxed Weathered Chiseled Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5277;
    const MAX_STATE_ID: u32 = 5277;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Stained Glass
pub struct GrayStainedGlass;

impl BlockDef for GrayStainedGlass {
    const ID: u32 = 307;
    const STRING_ID: &'static str = "minecraft:gray_stained_glass";
    const NAME: &'static str = "Gray Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5278;
    const MAX_STATE_ID: u32 = 5278;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Terracotta
pub struct GreenTerracotta;

impl BlockDef for GreenTerracotta {
    const ID: u32 = 497;
    const STRING_ID: &'static str = "minecraft:green_terracotta";
    const NAME: &'static str = "Green Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5279;
    const MAX_STATE_ID: u32 = 5279;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Brick Slab
pub struct DeepslateBrickSlab;

impl BlockDef for DeepslateBrickSlab {
    const ID: u32 = 1166;
    const STRING_ID: &'static str = "minecraft:deepslate_brick_slab";
    const NAME: &'static str = "Deepslate Brick Slab";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5280;
    const MAX_STATE_ID: u32 = 5281;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Stairs
pub struct WarpedStairs;

impl BlockDef for WarpedStairs {
    const ID: u32 = 896;
    const STRING_ID: &'static str = "minecraft:warped_stairs";
    const NAME: &'static str = "Warped Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5282;
    const MAX_STATE_ID: u32 = 5289;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smithing Table
pub struct SmithingTable;

impl BlockDef for SmithingTable {
    const ID: u32 = 846;
    const STRING_ID: &'static str = "minecraft:smithing_table";
    const NAME: &'static str = "Smithing Table";
    const HARDNESS: f32 = 2.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5290;
    const MAX_STATE_ID: u32 = 5290;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Player Head
pub struct PlayerHead;

impl BlockDef for PlayerHead {
    const ID: u32 = 459;
    const STRING_ID: &'static str = "minecraft:player_head";
    const NAME: &'static str = "Player Head";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5291;
    const MAX_STATE_ID: u32 = 5296;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper Grate
pub struct WeatheredCopperGrate;

impl BlockDef for WeatheredCopperGrate {
    const ID: u32 = 1094;
    const STRING_ID: &'static str = "minecraft:weathered_copper_grate";
    const NAME: &'static str = "Weathered Copper Grate";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5297;
    const MAX_STATE_ID: u32 = 5297;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Poppy
pub struct Poppy;

impl BlockDef for Poppy {
    const ID: u32 = 160;
    const STRING_ID: &'static str = "minecraft:poppy";
    const NAME: &'static str = "Poppy";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5298;
    const MAX_STATE_ID: u32 = 5298;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff Brick Slab
pub struct TuffBrickSlab;

impl BlockDef for TuffBrickSlab {
    const ID: u32 = 994;
    const STRING_ID: &'static str = "minecraft:tuff_brick_slab";
    const NAME: &'static str = "Tuff Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5299;
    const MAX_STATE_ID: u32 = 5300;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Chain
pub struct CopperChain;

impl BlockDef for CopperChain {
    const ID: u32 = 351;
    const STRING_ID: &'static str = "minecraft:copper_chain";
    const NAME: &'static str = "Copper Chain";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5301;
    const MAX_STATE_ID: u32 = 5303;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Chest
pub struct CopperChest;

impl BlockDef for CopperChest {
    const ID: u32 = 1108;
    const STRING_ID: &'static str = "minecraft:copper_chest";
    const NAME: &'static str = "Copper Chest";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5304;
    const MAX_STATE_ID: u32 = 5307;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Stone Bricks
pub struct MossyStoneBricks;

impl BlockDef for MossyStoneBricks {
    const ID: u32 = 327;
    const STRING_ID: &'static str = "minecraft:mossy_stone_bricks";
    const NAME: &'static str = "Mossy Stone Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5308;
    const MAX_STATE_ID: u32 = 5308;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Wool
pub struct GreenWool;

impl BlockDef for GreenWool {
    const ID: u32 = 153;
    const STRING_ID: &'static str = "minecraft:green_wool";
    const NAME: &'static str = "Green Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5309;
    const MAX_STATE_ID: u32 = 5309;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Carpet
pub struct GreenCarpet;

impl BlockDef for GreenCarpet {
    const ID: u32 = 551;
    const STRING_ID: &'static str = "minecraft:green_carpet";
    const NAME: &'static str = "Green Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5310;
    const MAX_STATE_ID: u32 = 5310;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Prismarine Brick Slab
pub struct PrismarineBrickSlab;

impl BlockDef for PrismarineBrickSlab {
    const ID: u32 = 534;
    const STRING_ID: &'static str = "minecraft:prismarine_brick_slab";
    const NAME: &'static str = "Prismarine Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5311;
    const MAX_STATE_ID: u32 = 5312;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Door
pub struct WoodenDoor;

impl BlockDef for WoodenDoor {
    const ID: u32 = 64;
    const STRING_ID: &'static str = "minecraft:wooden_door";
    const NAME: &'static str = "Oak Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5313;
    const MAX_STATE_ID: u32 = 5344;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pitcher Plant
pub struct PitcherPlant;

impl BlockDef for PitcherPlant {
    const ID: u32 = 664;
    const STRING_ID: &'static str = "minecraft:pitcher_plant";
    const NAME: &'static str = "Pitcher Plant";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5345;
    const MAX_STATE_ID: u32 = 5346;
    type State = super::states::DoublePlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Compound Creator
pub struct CompoundCreator;

impl BlockDef for CompoundCreator {
    const ID: u32 = 5526;
    const STRING_ID: &'static str = "minecraft:compound_creator";
    const NAME: &'static str = "Compound Creator";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5347;
    const MAX_STATE_ID: u32 = 5350;
    type State = super::states::DirectionState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Pressure Plate
pub struct SprucePressurePlate;

impl BlockDef for SprucePressurePlate {
    const ID: u32 = 262;
    const STRING_ID: &'static str = "minecraft:spruce_pressure_plate";
    const NAME: &'static str = "Spruce Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5351;
    const MAX_STATE_ID: u32 = 5366;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Netherite
pub struct NetheriteBlock;

impl BlockDef for NetheriteBlock {
    const ID: u32 = 915;
    const STRING_ID: &'static str = "minecraft:netherite_block";
    const NAME: &'static str = "Block of Netherite";
    const HARDNESS: f32 = 50.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5367;
    const MAX_STATE_ID: u32 = 5367;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Wool
pub struct PinkWool;

impl BlockDef for PinkWool {
    const ID: u32 = 146;
    const STRING_ID: &'static str = "minecraft:pink_wool";
    const NAME: &'static str = "Pink Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5368;
    const MAX_STATE_ID: u32 = 5368;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Redstone
pub struct RedstoneBlock;

impl BlockDef for RedstoneBlock {
    const ID: u32 = 152;
    const STRING_ID: &'static str = "minecraft:redstone_block";
    const NAME: &'static str = "Block of Redstone";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5369;
    const MAX_STATE_ID: u32 = 5369;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Fence Gate
pub struct BirchFenceGate;

impl BlockDef for BirchFenceGate {
    const ID: u32 = 184;
    const STRING_ID: &'static str = "minecraft:birch_fence_gate";
    const NAME: &'static str = "Birch Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5370;
    const MAX_STATE_ID: u32 = 5385;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Wire
pub struct RedstoneWire;

impl BlockDef for RedstoneWire {
    const ID: u32 = 55;
    const STRING_ID: &'static str = "minecraft:redstone_wire";
    const NAME: &'static str = "Redstone Wire";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5386;
    const MAX_STATE_ID: u32 = 5401;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Quartz Pillar
pub struct QuartzPillar;

impl BlockDef for QuartzPillar {
    const ID: u32 = 480;
    const STRING_ID: &'static str = "minecraft:quartz_pillar";
    const NAME: &'static str = "Quartz Pillar";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5402;
    const MAX_STATE_ID: u32 = 5404;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Cut Copper
pub struct WaxedExposedCutCopper;

impl BlockDef for WaxedExposedCutCopper {
    const ID: u32 = 1049;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_cut_copper";
    const NAME: &'static str = "Waxed Exposed Cut Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5405;
    const MAX_STATE_ID: u32 = 5405;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lava
pub struct Lava;

impl BlockDef for Lava {
    const ID: u32 = 11;
    const STRING_ID: &'static str = "minecraft:lava";
    const NAME: &'static str = "Lava";
    const HARDNESS: f32 = 100.0_f32;
    const RESISTANCE: f32 = 100.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 5406;
    const MAX_STATE_ID: u32 = 5421;
    type State = super::states::LiquidState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Hanging Sign
pub struct JungleHangingSign;

impl BlockDef for JungleHangingSign {
    const ID: u32 = 239;
    const STRING_ID: &'static str = "minecraft:jungle_hanging_sign";
    const NAME: &'static str = "Jungle Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5422;
    const MAX_STATE_ID: u32 = 5805;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Slab
pub struct BirchSlab;

impl BlockDef for BirchSlab {
    const ID: u32 = 601;
    const STRING_ID: &'static str = "minecraft:birch_slab";
    const NAME: &'static str = "Birch Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5806;
    const MAX_STATE_ID: u32 = 5807;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Loom
pub struct Loom;

impl BlockDef for Loom {
    const ID: u32 = 838;
    const STRING_ID: &'static str = "minecraft:loom";
    const NAME: &'static str = "Loom";
    const HARDNESS: f32 = 2.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5808;
    const MAX_STATE_ID: u32 = 5811;
    type State = super::states::DirectionState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper Lantern
pub struct WaxedWeatheredCopperLantern;

impl BlockDef for WaxedWeatheredCopperLantern {
    const ID: u32 = 857;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_lantern";
    const NAME: &'static str = "Waxed Weathered Copper Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5812;
    const MAX_STATE_ID: u32 = 5813;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Tube Coral Block
pub struct DeadTubeCoralBlock;

impl BlockDef for DeadTubeCoralBlock {
    const ID: u32 = 748;
    const STRING_ID: &'static str = "minecraft:dead_tube_coral_block";
    const NAME: &'static str = "Dead Tube Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5814;
    const MAX_STATE_ID: u32 = 5814;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Stone
pub struct EndStone;

impl BlockDef for EndStone {
    const ID: u32 = 121;
    const STRING_ID: &'static str = "minecraft:end_stone";
    const NAME: &'static str = "End Stone";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 9.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5815;
    const MAX_STATE_ID: u32 = 5815;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Tuff Slab
pub struct PolishedTuffDoubleSlab;

impl BlockDef for PolishedTuffDoubleSlab {
    const ID: u32 = 8076;
    const STRING_ID: &'static str = "minecraft:polished_tuff_double_slab";
    const NAME: &'static str = "Polished Tuff Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5816;
    const MAX_STATE_ID: u32 = 5817;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Door
pub struct CrimsonDoor;

impl BlockDef for CrimsonDoor {
    const ID: u32 = 899;
    const STRING_ID: &'static str = "minecraft:crimson_door";
    const NAME: &'static str = "Crimson Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5818;
    const MAX_STATE_ID: u32 = 5849;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Pressure Plate
pub struct MangrovePressurePlate;

impl BlockDef for MangrovePressurePlate {
    const ID: u32 = 269;
    const STRING_ID: &'static str = "minecraft:mangrove_pressure_plate";
    const NAME: &'static str = "Mangrove Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5850;
    const MAX_STATE_ID: u32 = 5865;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Shelf
pub struct JungleShelf;

impl BlockDef for JungleShelf {
    const ID: u32 = 186;
    const STRING_ID: &'static str = "minecraft:jungle_shelf";
    const NAME: &'static str = "Jungle Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5866;
    const MAX_STATE_ID: u32 = 5897;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Slab
pub struct JungleSlab;

impl BlockDef for JungleSlab {
    const ID: u32 = 602;
    const STRING_ID: &'static str = "minecraft:jungle_slab";
    const NAME: &'static str = "Jungle Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5898;
    const MAX_STATE_ID: u32 = 5899;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Stained Glass Pane
pub struct LightBlueStainedGlassPane;

impl BlockDef for LightBlueStainedGlassPane {
    const ID: u32 = 503;
    const STRING_ID: &'static str = "minecraft:light_blue_stained_glass_pane";
    const NAME: &'static str = "Light Blue Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5900;
    const MAX_STATE_ID: u32 = 5900;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Glowstone
pub struct Glowstone;

impl BlockDef for Glowstone {
    const ID: u32 = 89;
    const STRING_ID: &'static str = "minecraft:glowstone";
    const NAME: &'static str = "Glowstone";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5901;
    const MAX_STATE_ID: u32 = 5901;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Pressure Plate
pub struct StonePressurePlate;

impl BlockDef for StonePressurePlate {
    const ID: u32 = 70;
    const STRING_ID: &'static str = "minecraft:stone_pressure_plate";
    const NAME: &'static str = "Stone Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5902;
    const MAX_STATE_ID: u32 = 5917;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Cut Copper Stairs
pub struct WaxedExposedCutCopperStairs;

impl BlockDef for WaxedExposedCutCopperStairs {
    const ID: u32 = 1065;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_cut_copper_stairs";
    const NAME: &'static str = "Waxed Exposed Cut Copper Stairs";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5918;
    const MAX_STATE_ID: u32 = 5925;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard White Stained Glass
pub struct HardWhiteStainedGlass;

impl BlockDef for HardWhiteStainedGlass {
    const ID: u32 = 6105;
    const STRING_ID: &'static str = "minecraft:hard_white_stained_glass";
    const NAME: &'static str = "Hard White Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5926;
    const MAX_STATE_ID: u32 = 5926;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mud Brick Slab
pub struct MudBrickSlab;

impl BlockDef for MudBrickSlab {
    const ID: u32 = 8049;
    const STRING_ID: &'static str = "minecraft:mud_brick_slab";
    const NAME: &'static str = "Mud Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5927;
    const MAX_STATE_ID: u32 = 5928;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Lightning Rod
pub struct WaxedExposedLightningRod;

impl BlockDef for WaxedExposedLightningRod {
    const ID: u32 = 1129;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_lightning_rod";
    const NAME: &'static str = "Waxed Exposed Lightning Rod";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5929;
    const MAX_STATE_ID: u32 = 5940;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper Lantern
pub struct ExposedCopperLantern;

impl BlockDef for ExposedCopperLantern {
    const ID: u32 = 852;
    const STRING_ID: &'static str = "minecraft:exposed_copper_lantern";
    const NAME: &'static str = "Exposed Copper Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5941;
    const MAX_STATE_ID: u32 = 5942;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Farmland
pub struct Farmland;

impl BlockDef for Farmland {
    const ID: u32 = 60;
    const STRING_ID: &'static str = "minecraft:farmland";
    const NAME: &'static str = "Farmland";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5943;
    const MAX_STATE_ID: u32 = 5950;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Brain Coral Wall Fan
pub struct DeadBrainCoralWallFan;

impl BlockDef for DeadBrainCoralWallFan {
    const ID: u32 = 779;
    const STRING_ID: &'static str = "minecraft:dead_brain_coral_wall_fan";
    const NAME: &'static str = "Dead Brain Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 5951;
    const MAX_STATE_ID: u32 = 5954;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Red Sandstone
pub struct CutRedSandstone;

impl BlockDef for CutRedSandstone {
    const ID: u32 = 597;
    const STRING_ID: &'static str = "minecraft:cut_red_sandstone";
    const NAME: &'static str = "Cut Red Sandstone";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 5955;
    const MAX_STATE_ID: u32 = 5955;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Rail
pub struct Rail;

impl BlockDef for Rail {
    const ID: u32 = 66;
    const STRING_ID: &'static str = "minecraft:rail";
    const NAME: &'static str = "Rail";
    const HARDNESS: f32 = 0.7_f32;
    const RESISTANCE: f32 = 0.7_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5956;
    const MAX_STATE_ID: u32 = 5965;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blackstone Wall
pub struct BlackstoneWall;

impl BlockDef for BlackstoneWall {
    const ID: u32 = 926;
    const STRING_ID: &'static str = "minecraft:blackstone_wall";
    const NAME: &'static str = "Blackstone Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 5966;
    const MAX_STATE_ID: u32 = 6127;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Bricks
pub struct StoneBricks;

impl BlockDef for StoneBricks {
    const ID: u32 = 326;
    const STRING_ID: &'static str = "minecraft:stone_bricks";
    const NAME: &'static str = "Stone Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6128;
    const MAX_STATE_ID: u32 = 6128;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Cobblestone Stairs
pub struct MossyCobblestoneStairs;

impl BlockDef for MossyCobblestoneStairs {
    const ID: u32 = 801;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone_stairs";
    const NAME: &'static str = "Mossy Cobblestone Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6129;
    const MAX_STATE_ID: u32 = 6136;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Magenta Stained Glass
pub struct HardMagentaStainedGlass;

impl BlockDef for HardMagentaStainedGlass {
    const ID: u32 = 6316;
    const STRING_ID: &'static str = "minecraft:hard_magenta_stained_glass";
    const NAME: &'static str = "Hard Magenta Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6137;
    const MAX_STATE_ID: u32 = 6137;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Detector Rail
pub struct DetectorRail;

impl BlockDef for DetectorRail {
    const ID: u32 = 28;
    const STRING_ID: &'static str = "minecraft:detector_rail";
    const NAME: &'static str = "Detector Rail";
    const HARDNESS: f32 = 0.7_f32;
    const RESISTANCE: f32 = 0.7_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6138;
    const MAX_STATE_ID: u32 = 6149;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Orchid
pub struct BlueOrchid;

impl BlockDef for BlueOrchid {
    const ID: u32 = 161;
    const STRING_ID: &'static str = "minecraft:blue_orchid";
    const NAME: &'static str = "Blue Orchid";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6150;
    const MAX_STATE_ID: u32 = 6150;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Stained Glass Pane
pub struct GreenStainedGlassPane;

impl BlockDef for GreenStainedGlassPane {
    const ID: u32 = 513;
    const STRING_ID: &'static str = "minecraft:green_stained_glass_pane";
    const NAME: &'static str = "Green Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6151;
    const MAX_STATE_ID: u32 = 6151;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Granite Stairs
pub struct PolishedGraniteStairs;

impl BlockDef for PolishedGraniteStairs {
    const ID: u32 = 797;
    const STRING_ID: &'static str = "minecraft:polished_granite_stairs";
    const NAME: &'static str = "Polished Granite Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6152;
    const MAX_STATE_ID: u32 = 6159;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Leaves
pub struct BirchLeaves;

impl BlockDef for BirchLeaves {
    const ID: u32 = 90;
    const STRING_ID: &'static str = "minecraft:birch_leaves";
    const NAME: &'static str = "Birch Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6160;
    const MAX_STATE_ID: u32 = 6163;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Terracotta
pub struct PinkTerracotta;

impl BlockDef for PinkTerracotta {
    const ID: u32 = 490;
    const STRING_ID: &'static str = "minecraft:pink_terracotta";
    const NAME: &'static str = "Pink Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6164;
    const MAX_STATE_ID: u32 = 6164;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Slab
pub struct DarkOakDoubleSlab;

impl BlockDef for DarkOakDoubleSlab {
    const ID: u32 = 8036;
    const STRING_ID: &'static str = "minecraft:dark_oak_double_slab";
    const NAME: &'static str = "Dark Oak Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6165;
    const MAX_STATE_ID: u32 = 6166;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Infested Cobblestone
pub struct InfestedCobblestone;

impl BlockDef for InfestedCobblestone {
    const ID: u32 = 333;
    const STRING_ID: &'static str = "minecraft:infested_cobblestone";
    const NAME: &'static str = "Infested Cobblestone";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 0.75_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6167;
    const MAX_STATE_ID: u32 = 6167;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Pink Candle
pub struct PinkCandleCake;

impl BlockDef for PinkCandleCake {
    const ID: u32 = 968;
    const STRING_ID: &'static str = "minecraft:pink_candle_cake";
    const NAME: &'static str = "Cake with Pink Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6168;
    const MAX_STATE_ID: u32 = 6169;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cracked Deepslate Tiles
pub struct CrackedDeepslateTiles;

impl BlockDef for CrackedDeepslateTiles {
    const ID: u32 = 1170;
    const STRING_ID: &'static str = "minecraft:cracked_deepslate_tiles";
    const NAME: &'static str = "Cracked Deepslate Tiles";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6170;
    const MAX_STATE_ID: u32 = 6170;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brain Coral Wall Fan
pub struct BrainCoralWallFan;

impl BlockDef for BrainCoralWallFan {
    const ID: u32 = 784;
    const STRING_ID: &'static str = "minecraft:brain_coral_wall_fan";
    const NAME: &'static str = "Brain Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6171;
    const MAX_STATE_ID: u32 = 6174;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Wood
pub struct MangroveWood;

impl BlockDef for MangroveWood {
    const ID: u32 = 78;
    const STRING_ID: &'static str = "minecraft:mangrove_wood";
    const NAME: &'static str = "Mangrove Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6175;
    const MAX_STATE_ID: u32 = 6177;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper Golem Statue
pub struct WaxedExposedCopperGolemStatue;

impl BlockDef for WaxedExposedCopperGolemStatue {
    const ID: u32 = 1121;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_golem_statue";
    const NAME: &'static str = "Waxed Exposed Copper Golem Statue";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6178;
    const MAX_STATE_ID: u32 = 6181;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Glazed Terracotta
pub struct RedGlazedTerracotta;

impl BlockDef for RedGlazedTerracotta {
    const ID: u32 = 234;
    const STRING_ID: &'static str = "minecraft:red_glazed_terracotta";
    const NAME: &'static str = "Red Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6182;
    const MAX_STATE_ID: u32 = 6187;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper Chest
pub struct WaxedOxidizedCopperChest;

impl BlockDef for WaxedOxidizedCopperChest {
    const ID: u32 = 1115;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_chest";
    const NAME: &'static str = "Waxed Oxidized Copper Chest";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6188;
    const MAX_STATE_ID: u32 = 6191;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper Chain
pub struct WaxedOxidizedCopperChain;

impl BlockDef for WaxedOxidizedCopperChain {
    const ID: u32 = 358;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_chain";
    const NAME: &'static str = "Waxed Oxidized Copper Chain";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6192;
    const MAX_STATE_ID: u32 = 6194;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Fence Gate
pub struct DarkOakFenceGate;

impl BlockDef for DarkOakFenceGate {
    const ID: u32 = 186;
    const STRING_ID: &'static str = "minecraft:dark_oak_fence_gate";
    const NAME: &'static str = "Dark Oak Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6195;
    const MAX_STATE_ID: u32 = 6210;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Cobblestone Slab
pub struct MossyCobblestoneSlab;

impl BlockDef for MossyCobblestoneSlab {
    const ID: u32 = 815;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone_slab";
    const NAME: &'static str = "Mossy Cobblestone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6211;
    const MAX_STATE_ID: u32 = 6212;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Mosaic Slab
pub struct BambooMosaicDoubleSlab;

impl BlockDef for BambooMosaicDoubleSlab {
    const ID: u32 = 8040;
    const STRING_ID: &'static str = "minecraft:bamboo_mosaic_double_slab";
    const NAME: &'static str = "Bamboo Mosaic Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6213;
    const MAX_STATE_ID: u32 = 6214;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobblestone Slab
pub struct CobblestoneSlab;

impl BlockDef for CobblestoneSlab {
    const ID: u32 = 615;
    const STRING_ID: &'static str = "minecraft:cobblestone_slab";
    const NAME: &'static str = "Cobblestone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6215;
    const MAX_STATE_ID: u32 = 6216;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Nylium
pub struct CrimsonNylium;

impl BlockDef for CrimsonNylium {
    const ID: u32 = 875;
    const STRING_ID: &'static str = "minecraft:crimson_nylium";
    const NAME: &'static str = "Crimson Nylium";
    const HARDNESS: f32 = 0.4_f32;
    const RESISTANCE: f32 = 0.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6217;
    const MAX_STATE_ID: u32 = 6217;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Structure Void
pub struct StructureVoid;

impl BlockDef for StructureVoid {
    const ID: u32 = 217;
    const STRING_ID: &'static str = "minecraft:structure_void";
    const NAME: &'static str = "Structure Void";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6218;
    const MAX_STATE_ID: u32 = 6218;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper Bars
pub struct WaxedExposedCopperBars;

impl BlockDef for WaxedExposedCopperBars {
    const ID: u32 = 347;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_bars";
    const NAME: &'static str = "Waxed Exposed Copper Bars";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6219;
    const MAX_STATE_ID: u32 = 6219;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Concrete
pub struct PurpleConcrete;

impl BlockDef for PurpleConcrete {
    const ID: u32 = 720;
    const STRING_ID: &'static str = "minecraft:purple_concrete";
    const NAME: &'static str = "Purple Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6220;
    const MAX_STATE_ID: u32 = 6220;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper Bulb
pub struct WaxedExposedCopperBulb;

impl BlockDef for WaxedExposedCopperBulb {
    const ID: u32 = 1105;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_bulb";
    const NAME: &'static str = "Waxed Exposed Copper Bulb";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6221;
    const MAX_STATE_ID: u32 = 6224;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Brick Slab
pub struct PolishedBlackstoneBrickSlab;

impl BlockDef for PolishedBlackstoneBrickSlab {
    const ID: u32 = 8073;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_brick_slab";
    const NAME: &'static str = "Polished Blackstone Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6225;
    const MAX_STATE_ID: u32 = 6226;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Slab
pub struct NormalStoneSlab;

impl BlockDef for NormalStoneSlab {
    const ID: u32 = 610;
    const STRING_ID: &'static str = "minecraft:normal_stone_slab";
    const NAME: &'static str = "Stone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6227;
    const MAX_STATE_ID: u32 = 6228;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Yellow Stained Glass Pane
pub struct HardYellowStainedGlassPane;

impl BlockDef for HardYellowStainedGlassPane {
    const ID: u32 = 6416;
    const STRING_ID: &'static str = "minecraft:hard_yellow_stained_glass_pane";
    const NAME: &'static str = "Hard Yellow Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6229;
    const MAX_STATE_ID: u32 = 6229;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Sapling
pub struct SpruceSapling;

impl BlockDef for SpruceSapling {
    const ID: u32 = 26;
    const STRING_ID: &'static str = "minecraft:spruce_sapling";
    const NAME: &'static str = "Spruce Sapling";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6230;
    const MAX_STATE_ID: u32 = 6231;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Terracotta
pub struct YellowTerracotta;

impl BlockDef for YellowTerracotta {
    const ID: u32 = 488;
    const STRING_ID: &'static str = "minecraft:yellow_terracotta";
    const NAME: &'static str = "Yellow Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6232;
    const MAX_STATE_ID: u32 = 6232;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Snow Block
pub struct Snow;

impl BlockDef for Snow {
    const ID: u32 = 80;
    const STRING_ID: &'static str = "minecraft:snow";
    const NAME: &'static str = "Snow Block";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6233;
    const MAX_STATE_ID: u32 = 6233;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sand
pub struct Sand;

impl BlockDef for Sand {
    const ID: u32 = 12;
    const STRING_ID: &'static str = "minecraft:sand";
    const NAME: &'static str = "Sand";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6234;
    const MAX_STATE_ID: u32 = 6234;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Daylight Detector
pub struct DaylightDetector;

impl BlockDef for DaylightDetector {
    const ID: u32 = 151;
    const STRING_ID: &'static str = "minecraft:daylight_detector";
    const NAME: &'static str = "Daylight Detector";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6235;
    const MAX_STATE_ID: u32 = 6250;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Sign
pub struct MangroveStandingSign;

impl BlockDef for MangroveStandingSign {
    const ID: u32 = 218;
    const STRING_ID: &'static str = "minecraft:mangrove_standing_sign";
    const NAME: &'static str = "Mangrove Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6251;
    const MAX_STATE_ID: u32 = 6266;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Mangrove Wood
pub struct StrippedMangroveWood;

impl BlockDef for StrippedMangroveWood {
    const ID: u32 = 87;
    const STRING_ID: &'static str = "minecraft:stripped_mangrove_wood";
    const NAME: &'static str = "Stripped Mangrove Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6267;
    const MAX_STATE_ID: u32 = 6269;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Conduit
pub struct Conduit;

impl BlockDef for Conduit {
    const ID: u32 = 790;
    const STRING_ID: &'static str = "minecraft:conduit";
    const NAME: &'static str = "Conduit";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6270;
    const MAX_STATE_ID: u32 = 6270;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Slime Block
pub struct Slime;

impl BlockDef for Slime {
    const ID: u32 = 165;
    const STRING_ID: &'static str = "minecraft:slime";
    const NAME: &'static str = "Slime Block";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6271;
    const MAX_STATE_ID: u32 = 6271;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Torch
pub struct CopperTorch;

impl BlockDef for CopperTorch {
    const ID: u32 = 292;
    const STRING_ID: &'static str = "minecraft:copper_torch";
    const NAME: &'static str = "Copper Torch";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 14;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6272;
    const MAX_STATE_ID: u32 = 6277;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bone Block
pub struct BoneBlock;

impl BlockDef for BoneBlock {
    const ID: u32 = 216;
    const STRING_ID: &'static str = "minecraft:bone_block";
    const NAME: &'static str = "Bone Block";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6278;
    const MAX_STATE_ID: u32 = 6289;
    type State = super::states::DeprecatedPillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Frame
pub struct Frame;

impl BlockDef for Frame {
    const ID: u32 = 199;
    const STRING_ID: &'static str = "minecraft:frame";
    const NAME: &'static str = "Frame";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6290;
    const MAX_STATE_ID: u32 = 6313;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Log
pub struct SpruceLog;

impl BlockDef for SpruceLog {
    const ID: u32 = 50;
    const STRING_ID: &'static str = "minecraft:spruce_log";
    const NAME: &'static str = "Spruce Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6314;
    const MAX_STATE_ID: u32 = 6316;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Lapis Lazuli
pub struct LapisBlock;

impl BlockDef for LapisBlock {
    const ID: u32 = 22;
    const STRING_ID: &'static str = "minecraft:lapis_block";
    const NAME: &'static str = "Block of Lapis Lazuli";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6317;
    const MAX_STATE_ID: u32 = 6317;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Coal Ore
pub struct CoalOre;

impl BlockDef for CoalOre {
    const ID: u32 = 16;
    const STRING_ID: &'static str = "minecraft:coal_ore";
    const NAME: &'static str = "Coal Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6318;
    const MAX_STATE_ID: u32 = 6318;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Stone Brick Slab
pub struct MossyStoneBrickDoubleSlab;

impl BlockDef for MossyStoneBrickDoubleSlab {
    const ID: u32 = 8057;
    const STRING_ID: &'static str = "minecraft:mossy_stone_brick_double_slab";
    const NAME: &'static str = "Mossy Stone Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6319;
    const MAX_STATE_ID: u32 = 6320;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Red Sandstone Slab
pub struct CutRedSandstoneDoubleSlab;

impl BlockDef for CutRedSandstoneDoubleSlab {
    const ID: u32 = 622;
    const STRING_ID: &'static str = "minecraft:cut_red_sandstone_double_slab";
    const NAME: &'static str = "Cut Red Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6321;
    const MAX_STATE_ID: u32 = 6322;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Client Request Placeholder Block
pub struct ClientRequestPlaceholderBlock;

impl BlockDef for ClientRequestPlaceholderBlock {
    const ID: u32 = 6512;
    const STRING_ID: &'static str = "minecraft:client_request_placeholder_block";
    const NAME: &'static str = "Client Request Placeholder Block";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6323;
    const MAX_STATE_ID: u32 = 6323;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Shelf
pub struct BambooShelf;

impl BlockDef for BambooShelf {
    const ID: u32 = 181;
    const STRING_ID: &'static str = "minecraft:bamboo_shelf";
    const NAME: &'static str = "Bamboo Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6324;
    const MAX_STATE_ID: u32 = 6355;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Ore
pub struct RedstoneOre;

impl BlockDef for RedstoneOre {
    const ID: u32 = 73;
    const STRING_ID: &'static str = "minecraft:redstone_ore";
    const NAME: &'static str = "Redstone Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6356;
    const MAX_STATE_ID: u32 = 6356;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Slab
pub struct BambooDoubleSlab;

impl BlockDef for BambooDoubleSlab {
    const ID: u32 = 608;
    const STRING_ID: &'static str = "minecraft:bamboo_double_slab";
    const NAME: &'static str = "Bamboo Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6357;
    const MAX_STATE_ID: u32 = 6358;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Copper Chest
pub struct WaxedCopperChest;

impl BlockDef for WaxedCopperChest {
    const ID: u32 = 1112;
    const STRING_ID: &'static str = "minecraft:waxed_copper_chest";
    const NAME: &'static str = "Waxed Copper Chest";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6359;
    const MAX_STATE_ID: u32 = 6362;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Stained Glass
pub struct GreenStainedGlass;

impl BlockDef for GreenStainedGlass {
    const ID: u32 = 313;
    const STRING_ID: &'static str = "minecraft:green_stained_glass";
    const NAME: &'static str = "Green Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6363;
    const MAX_STATE_ID: u32 = 6363;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Copper Chain
pub struct WaxedCopperChain;

impl BlockDef for WaxedCopperChain {
    const ID: u32 = 355;
    const STRING_ID: &'static str = "minecraft:waxed_copper_chain";
    const NAME: &'static str = "Waxed Copper Chain";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6364;
    const MAX_STATE_ID: u32 = 6366;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bubble Coral Block
pub struct BubbleCoralBlock;

impl BlockDef for BubbleCoralBlock {
    const ID: u32 = 755;
    const STRING_ID: &'static str = "minecraft:bubble_coral_block";
    const NAME: &'static str = "Bubble Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6367;
    const MAX_STATE_ID: u32 = 6367;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Infested Chiseled Stone Bricks
pub struct InfestedChiseledStoneBricks;

impl BlockDef for InfestedChiseledStoneBricks {
    const ID: u32 = 337;
    const STRING_ID: &'static str = "minecraft:infested_chiseled_stone_bricks";
    const NAME: &'static str = "Infested Chiseled Stone Bricks";
    const HARDNESS: f32 = 0.75_f32;
    const RESISTANCE: f32 = 0.75_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6368;
    const MAX_STATE_ID: u32 = 6368;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Brick Fence
pub struct NetherBrickFence;

impl BlockDef for NetherBrickFence {
    const ID: u32 = 113;
    const STRING_ID: &'static str = "minecraft:nether_brick_fence";
    const NAME: &'static str = "Nether Brick Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6369;
    const MAX_STATE_ID: u32 = 6369;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Tulip
pub struct PinkTulip;

impl BlockDef for PinkTulip {
    const ID: u32 = 167;
    const STRING_ID: &'static str = "minecraft:pink_tulip";
    const NAME: &'static str = "Pink Tulip";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6370;
    const MAX_STATE_ID: u32 = 6370;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Slab
pub struct OakSlab;

impl BlockDef for OakSlab {
    const ID: u32 = 8030;
    const STRING_ID: &'static str = "minecraft:oak_slab";
    const NAME: &'static str = "Oak Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6371;
    const MAX_STATE_ID: u32 = 6372;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Pale Oak Log
pub struct StrippedPaleOakLog;

impl BlockDef for StrippedPaleOakLog {
    const ID: u32 = 67;
    const STRING_ID: &'static str = "minecraft:stripped_pale_oak_log";
    const NAME: &'static str = "Stripped Pale Oak Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6373;
    const MAX_STATE_ID: u32 = 6375;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Tile Slab
pub struct DeepslateTileSlab;

impl BlockDef for DeepslateTileSlab {
    const ID: u32 = 1162;
    const STRING_ID: &'static str = "minecraft:deepslate_tile_slab";
    const NAME: &'static str = "Deepslate Tile Slab";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6376;
    const MAX_STATE_ID: u32 = 6377;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Concrete Powder
pub struct PinkConcretePowder;

impl BlockDef for PinkConcretePowder {
    const ID: u32 = 732;
    const STRING_ID: &'static str = "minecraft:pink_concrete_powder";
    const NAME: &'static str = "Pink Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6378;
    const MAX_STATE_ID: u32 = 6378;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Slab
pub struct PaleOakSlab;

impl BlockDef for PaleOakSlab {
    const ID: u32 = 606;
    const STRING_ID: &'static str = "minecraft:pale_oak_slab";
    const NAME: &'static str = "Pale Oak Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6379;
    const MAX_STATE_ID: u32 = 6380;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Tube Coral
pub struct DeadTubeCoral;

impl BlockDef for DeadTubeCoral {
    const ID: u32 = 758;
    const STRING_ID: &'static str = "minecraft:dead_tube_coral";
    const NAME: &'static str = "Dead Tube Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6381;
    const MAX_STATE_ID: u32 = 6381;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Wart Block
pub struct NetherWartBlock;

impl BlockDef for NetherWartBlock {
    const ID: u32 = 214;
    const STRING_ID: &'static str = "minecraft:nether_wart_block";
    const NAME: &'static str = "Nether Wart Block";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6382;
    const MAX_STATE_ID: u32 = 6382;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Prismarine Slab
pub struct PrismarineSlab;

impl BlockDef for PrismarineSlab {
    const ID: u32 = 533;
    const STRING_ID: &'static str = "minecraft:prismarine_slab";
    const NAME: &'static str = "Prismarine Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6383;
    const MAX_STATE_ID: u32 = 6384;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Prismarine Slab
pub struct PrismarineDoubleSlab;

impl BlockDef for PrismarineDoubleSlab {
    const ID: u32 = 8027;
    const STRING_ID: &'static str = "minecraft:prismarine_double_slab";
    const NAME: &'static str = "Prismarine Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6385;
    const MAX_STATE_ID: u32 = 6386;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Door
pub struct CherryDoor;

impl BlockDef for CherryDoor {
    const ID: u32 = 650;
    const STRING_ID: &'static str = "minecraft:cherry_door";
    const NAME: &'static str = "Cherry Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6387;
    const MAX_STATE_ID: u32 = 6418;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Colored Torch Blue
pub struct ColoredTorchBlue;

impl BlockDef for ColoredTorchBlue {
    const ID: u32 = 6770;
    const STRING_ID: &'static str = "minecraft:colored_torch_blue";
    const NAME: &'static str = "Colored Torch Blue";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6419;
    const MAX_STATE_ID: u32 = 6424;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Hyphae
pub struct CrimsonHyphae;

impl BlockDef for CrimsonHyphae {
    const ID: u32 = 873;
    const STRING_ID: &'static str = "minecraft:crimson_hyphae";
    const NAME: &'static str = "Crimson Hyphae";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6425;
    const MAX_STATE_ID: u32 = 6427;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Stairs
pub struct PolishedBlackstoneStairs;

impl BlockDef for PolishedBlackstoneStairs {
    const ID: u32 = 936;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_stairs";
    const NAME: &'static str = "Polished Blackstone Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6428;
    const MAX_STATE_ID: u32 = 6435;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Cut Copper Stairs
pub struct WeatheredCutCopperStairs;

impl BlockDef for WeatheredCutCopperStairs {
    const ID: u32 = 1062;
    const STRING_ID: &'static str = "minecraft:weathered_cut_copper_stairs";
    const NAME: &'static str = "Weathered Cut Copper Stairs";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6436;
    const MAX_STATE_ID: u32 = 6443;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Small Dripleaf
pub struct SmallDripleafBlock;

impl BlockDef for SmallDripleafBlock {
    const ID: u32 = 1147;
    const STRING_ID: &'static str = "minecraft:small_dripleaf_block";
    const NAME: &'static str = "Small Dripleaf";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6444;
    const MAX_STATE_ID: u32 = 6451;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Stained Glass
pub struct PinkStainedGlass;

impl BlockDef for PinkStainedGlass {
    const ID: u32 = 306;
    const STRING_ID: &'static str = "minecraft:pink_stained_glass";
    const NAME: &'static str = "Pink Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6452;
    const MAX_STATE_ID: u32 = 6452;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper Grate
pub struct WaxedWeatheredCopperGrate;

impl BlockDef for WaxedWeatheredCopperGrate {
    const ID: u32 = 1098;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_grate";
    const NAME: &'static str = "Waxed Weathered Copper Grate";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6453;
    const MAX_STATE_ID: u32 = 6453;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Button
pub struct SpruceButton;

impl BlockDef for SpruceButton {
    const ID: u32 = 444;
    const STRING_ID: &'static str = "minecraft:spruce_button";
    const NAME: &'static str = "Spruce Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6454;
    const MAX_STATE_ID: u32 = 6465;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Log
pub struct AcaciaLog;

impl BlockDef for AcaciaLog {
    const ID: u32 = 53;
    const STRING_ID: &'static str = "minecraft:acacia_log";
    const NAME: &'static str = "Acacia Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6466;
    const MAX_STATE_ID: u32 = 6468;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Trapdoor
pub struct CrimsonTrapdoor;

impl BlockDef for CrimsonTrapdoor {
    const ID: u32 = 891;
    const STRING_ID: &'static str = "minecraft:crimson_trapdoor";
    const NAME: &'static str = "Crimson Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6469;
    const MAX_STATE_ID: u32 = 6484;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Basalt
pub struct Basalt;

impl BlockDef for Basalt {
    const ID: u32 = 288;
    const STRING_ID: &'static str = "minecraft:basalt";
    const NAME: &'static str = "Basalt";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6485;
    const MAX_STATE_ID: u32 = 6487;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Cyan Stained Glass
pub struct HardCyanStainedGlass;

impl BlockDef for HardCyanStainedGlass {
    const ID: u32 = 6847;
    const STRING_ID: &'static str = "minecraft:hard_cyan_stained_glass";
    const NAME: &'static str = "Hard Cyan Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6488;
    const MAX_STATE_ID: u32 = 6488;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Slab
pub struct NormalStoneDoubleSlab;

impl BlockDef for NormalStoneDoubleSlab {
    const ID: u32 = 8041;
    const STRING_ID: &'static str = "minecraft:normal_stone_double_slab";
    const NAME: &'static str = "Stone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6489;
    const MAX_STATE_ID: u32 = 6490;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Brick Slab
pub struct StoneBrickDoubleSlab;

impl BlockDef for StoneBrickDoubleSlab {
    const ID: u32 = 617;
    const STRING_ID: &'static str = "minecraft:stone_brick_double_slab";
    const NAME: &'static str = "Stone Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6491;
    const MAX_STATE_ID: u32 = 6492;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Terracotta
pub struct LightBlueTerracotta;

impl BlockDef for LightBlueTerracotta {
    const ID: u32 = 487;
    const STRING_ID: &'static str = "minecraft:light_blue_terracotta";
    const NAME: &'static str = "Light Blue Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6493;
    const MAX_STATE_ID: u32 = 6493;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Lamp
pub struct LitRedstoneLamp;

impl BlockDef for LitRedstoneLamp {
    const ID: u32 = 124;
    const STRING_ID: &'static str = "minecraft:lit_redstone_lamp";
    const NAME: &'static str = "Redstone Lamp";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6494;
    const MAX_STATE_ID: u32 = 6494;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Golem Statue
pub struct CopperGolemStatue;

impl BlockDef for CopperGolemStatue {
    const ID: u32 = 1116;
    const STRING_ID: &'static str = "minecraft:copper_golem_statue";
    const NAME: &'static str = "Copper Golem Statue";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6495;
    const MAX_STATE_ID: u32 = 6498;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Blue Stained Glass
pub struct HardBlueStainedGlass;

impl BlockDef for HardBlueStainedGlass {
    const ID: u32 = 6858;
    const STRING_ID: &'static str = "minecraft:hard_blue_stained_glass";
    const NAME: &'static str = "Hard Blue Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6499;
    const MAX_STATE_ID: u32 = 6499;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Purple Stained Glass
pub struct HardPurpleStainedGlass;

impl BlockDef for HardPurpleStainedGlass {
    const ID: u32 = 6859;
    const STRING_ID: &'static str = "minecraft:hard_purple_stained_glass";
    const NAME: &'static str = "Hard Purple Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6500;
    const MAX_STATE_ID: u32 = 6500;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Diamond Ore
pub struct DiamondOre;

impl BlockDef for DiamondOre {
    const ID: u32 = 56;
    const STRING_ID: &'static str = "minecraft:diamond_ore";
    const NAME: &'static str = "Diamond Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6501;
    const MAX_STATE_ID: u32 = 6501;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Roots
pub struct WarpedRoots;

impl BlockDef for WarpedRoots {
    const ID: u32 = 869;
    const STRING_ID: &'static str = "minecraft:warped_roots";
    const NAME: &'static str = "Warped Roots";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6502;
    const MAX_STATE_ID: u32 = 6502;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Concrete
pub struct MagentaConcrete;

impl BlockDef for MagentaConcrete {
    const ID: u32 = 712;
    const STRING_ID: &'static str = "minecraft:magenta_concrete";
    const NAME: &'static str = "Magenta Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6503;
    const MAX_STATE_ID: u32 = 6503;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Prismarine
pub struct DarkPrismarine;

impl BlockDef for DarkPrismarine {
    const ID: u32 = 529;
    const STRING_ID: &'static str = "minecraft:dark_prismarine";
    const NAME: &'static str = "Dark Prismarine";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6504;
    const MAX_STATE_ID: u32 = 6504;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sticky Piston
pub struct StickyPiston;

impl BlockDef for StickyPiston {
    const ID: u32 = 29;
    const STRING_ID: &'static str = "minecraft:sticky_piston";
    const NAME: &'static str = "Sticky Piston";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6505;
    const MAX_STATE_ID: u32 = 6510;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Ender Chest
pub struct EnderChest;

impl BlockDef for EnderChest {
    const ID: u32 = 130;
    const STRING_ID: &'static str = "minecraft:ender_chest";
    const NAME: &'static str = "Ender Chest";
    const HARDNESS: f32 = 22.5_f32;
    const RESISTANCE: f32 = 600.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 7;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6511;
    const MAX_STATE_ID: u32 = 6514;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Medium Amethyst Bud
pub struct MediumAmethystBud;

impl BlockDef for MediumAmethystBud {
    const ID: u32 = 982;
    const STRING_ID: &'static str = "minecraft:medium_amethyst_bud";
    const NAME: &'static str = "Medium Amethyst Bud";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 2;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6515;
    const MAX_STATE_ID: u32 = 6520;
    type State = super::states::BlockFaceState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Shulker Box
pub struct PinkShulkerBox;

impl BlockDef for PinkShulkerBox {
    const ID: u32 = 684;
    const STRING_ID: &'static str = "minecraft:pink_shulker_box";
    const NAME: &'static str = "Pink Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6521;
    const MAX_STATE_ID: u32 = 6521;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Slab
pub struct WarpedDoubleSlab;

impl BlockDef for WarpedDoubleSlab {
    const ID: u32 = 886;
    const STRING_ID: &'static str = "minecraft:warped_double_slab";
    const NAME: &'static str = "Warped Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6522;
    const MAX_STATE_ID: u32 = 6523;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Sign
pub struct JungleWallSign;

impl BlockDef for JungleWallSign {
    const ID: u32 = 229;
    const STRING_ID: &'static str = "minecraft:jungle_wall_sign";
    const NAME: &'static str = "Jungle Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6524;
    const MAX_STATE_ID: u32 = 6529;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sculk Sensor
pub struct SculkSensor;

impl BlockDef for SculkSensor {
    const ID: u32 = 1028;
    const STRING_ID: &'static str = "minecraft:sculk_sensor";
    const NAME: &'static str = "Sculk Sensor";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 1;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6530;
    const MAX_STATE_ID: u32 = 6532;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Bulb
pub struct CopperBulb;

impl BlockDef for CopperBulb {
    const ID: u32 = 1100;
    const STRING_ID: &'static str = "minecraft:copper_bulb";
    const NAME: &'static str = "Copper Bulb";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6533;
    const MAX_STATE_ID: u32 = 6536;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Bars
pub struct CopperBars;

impl BlockDef for CopperBars {
    const ID: u32 = 342;
    const STRING_ID: &'static str = "minecraft:copper_bars";
    const NAME: &'static str = "Copper Bars";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6537;
    const MAX_STATE_ID: u32 = 6537;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Shelf
pub struct OakShelf;

impl BlockDef for OakShelf {
    const ID: u32 = 188;
    const STRING_ID: &'static str = "minecraft:oak_shelf";
    const NAME: &'static str = "Oak Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6538;
    const MAX_STATE_ID: u32 = 6569;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Diorite Stairs
pub struct DioriteStairs;

impl BlockDef for DioriteStairs {
    const ID: u32 = 810;
    const STRING_ID: &'static str = "minecraft:diorite_stairs";
    const NAME: &'static str = "Diorite Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6570;
    const MAX_STATE_ID: u32 = 6577;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Leaves
pub struct SpruceLeaves;

impl BlockDef for SpruceLeaves {
    const ID: u32 = 89;
    const STRING_ID: &'static str = "minecraft:spruce_leaves";
    const NAME: &'static str = "Spruce Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6578;
    const MAX_STATE_ID: u32 = 6581;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Frogspawn
pub struct FrogSpawn;

impl BlockDef for FrogSpawn {
    const ID: u32 = 1181;
    const STRING_ID: &'static str = "minecraft:frog_spawn";
    const NAME: &'static str = "Frogspawn";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6582;
    const MAX_STATE_ID: u32 = 6582;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Door
pub struct AcaciaDoor;

impl BlockDef for AcaciaDoor {
    const ID: u32 = 196;
    const STRING_ID: &'static str = "minecraft:acacia_door";
    const NAME: &'static str = "Acacia Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6583;
    const MAX_STATE_ID: u32 = 6614;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Sandstone Slab
pub struct SmoothSandstoneDoubleSlab;

impl BlockDef for SmoothSandstoneDoubleSlab {
    const ID: u32 = 8061;
    const STRING_ID: &'static str = "minecraft:smooth_sandstone_double_slab";
    const NAME: &'static str = "Smooth Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6615;
    const MAX_STATE_ID: u32 = 6616;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Shulker Box
pub struct RedShulkerBox;

impl BlockDef for RedShulkerBox {
    const ID: u32 = 692;
    const STRING_ID: &'static str = "minecraft:red_shulker_box";
    const NAME: &'static str = "Red Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6617;
    const MAX_STATE_ID: u32 = 6617;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Cherry Log
pub struct StrippedCherryLog;

impl BlockDef for StrippedCherryLog {
    const ID: u32 = 65;
    const STRING_ID: &'static str = "minecraft:stripped_cherry_log";
    const NAME: &'static str = "Stripped Cherry Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6618;
    const MAX_STATE_ID: u32 = 6620;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Button
pub struct CrimsonButton;

impl BlockDef for CrimsonButton {
    const ID: u32 = 897;
    const STRING_ID: &'static str = "minecraft:crimson_button";
    const NAME: &'static str = "Crimson Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6621;
    const MAX_STATE_ID: u32 = 6632;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Planks
pub struct AcaciaPlanks;

impl BlockDef for AcaciaPlanks {
    const ID: u32 = 17;
    const STRING_ID: &'static str = "minecraft:acacia_planks";
    const NAME: &'static str = "Acacia Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6633;
    const MAX_STATE_ID: u32 = 6633;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Fire Coral Block
pub struct FireCoralBlock;

impl BlockDef for FireCoralBlock {
    const ID: u32 = 756;
    const STRING_ID: &'static str = "minecraft:fire_coral_block";
    const NAME: &'static str = "Fire Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6634;
    const MAX_STATE_ID: u32 = 6634;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Concrete Powder
pub struct MagentaConcretePowder;

impl BlockDef for MagentaConcretePowder {
    const ID: u32 = 728;
    const STRING_ID: &'static str = "minecraft:magenta_concrete_powder";
    const NAME: &'static str = "Magenta Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6635;
    const MAX_STATE_ID: u32 = 6635;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Iron Door
pub struct IronDoor;

impl BlockDef for IronDoor {
    const ID: u32 = 71;
    const STRING_ID: &'static str = "minecraft:iron_door";
    const NAME: &'static str = "Iron Door";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 5.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6636;
    const MAX_STATE_ID: u32 = 6667;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Honeycomb Block
pub struct HoneycombBlock;

impl BlockDef for HoneycombBlock {
    const ID: u32 = 914;
    const STRING_ID: &'static str = "minecraft:honeycomb_block";
    const NAME: &'static str = "Honeycomb Block";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6668;
    const MAX_STATE_ID: u32 = 6668;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Brick Stairs
pub struct PolishedBlackstoneBrickStairs;

impl BlockDef for PolishedBlackstoneBrickStairs {
    const ID: u32 = 933;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_brick_stairs";
    const NAME: &'static str = "Polished Blackstone Brick Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6669;
    const MAX_STATE_ID: u32 = 6676;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Trapdoor
pub struct MangroveTrapdoor;

impl BlockDef for MangroveTrapdoor {
    const ID: u32 = 324;
    const STRING_ID: &'static str = "minecraft:mangrove_trapdoor";
    const NAME: &'static str = "Mangrove Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6677;
    const MAX_STATE_ID: u32 = 6692;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Quartz Ore
pub struct QuartzOre;

impl BlockDef for QuartzOre {
    const ID: u32 = 153;
    const STRING_ID: &'static str = "minecraft:quartz_ore";
    const NAME: &'static str = "Nether Quartz Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6693;
    const MAX_STATE_ID: u32 = 6693;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Daylight Detector
pub struct DaylightDetectorInverted;

impl BlockDef for DaylightDetectorInverted {
    const ID: u32 = 178;
    const STRING_ID: &'static str = "minecraft:daylight_detector_inverted";
    const NAME: &'static str = "Daylight Detector";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6694;
    const MAX_STATE_ID: u32 = 6709;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Barrel
pub struct Barrel;

impl BlockDef for Barrel {
    const ID: u32 = 839;
    const STRING_ID: &'static str = "minecraft:barrel";
    const NAME: &'static str = "Barrel";
    const HARDNESS: f32 = 2.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6710;
    const MAX_STATE_ID: u32 = 6721;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Quartz Block
pub struct SmoothQuartz;

impl BlockDef for SmoothQuartz {
    const ID: u32 = 626;
    const STRING_ID: &'static str = "minecraft:smooth_quartz";
    const NAME: &'static str = "Smooth Quartz Block";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6722;
    const MAX_STATE_ID: u32 = 6724;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Coarse Dirt
pub struct CoarseDirt;

impl BlockDef for CoarseDirt {
    const ID: u32 = 10;
    const STRING_ID: &'static str = "minecraft:coarse_dirt";
    const NAME: &'static str = "Coarse Dirt";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6725;
    const MAX_STATE_ID: u32 = 6725;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chorus Flower
pub struct ChorusFlower;

impl BlockDef for ChorusFlower {
    const ID: u32 = 200;
    const STRING_ID: &'static str = "minecraft:chorus_flower";
    const NAME: &'static str = "Chorus Flower";
    const HARDNESS: f32 = 0.4_f32;
    const RESISTANCE: f32 = 0.4_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6726;
    const MAX_STATE_ID: u32 = 6731;
    type State = super::states::AgeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Stained Glass
pub struct OrangeStainedGlass;

impl BlockDef for OrangeStainedGlass {
    const ID: u32 = 301;
    const STRING_ID: &'static str = "minecraft:orange_stained_glass";
    const NAME: &'static str = "Orange Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6732;
    const MAX_STATE_ID: u32 = 6732;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Stained Glass Pane
pub struct WhiteStainedGlassPane;

impl BlockDef for WhiteStainedGlassPane {
    const ID: u32 = 500;
    const STRING_ID: &'static str = "minecraft:white_stained_glass_pane";
    const NAME: &'static str = "White Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6733;
    const MAX_STATE_ID: u32 = 6733;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Birch Wood
pub struct StrippedBirchWood;

impl BlockDef for StrippedBirchWood {
    const ID: u32 = 81;
    const STRING_ID: &'static str = "minecraft:stripped_birch_wood";
    const NAME: &'static str = "Stripped Birch Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6734;
    const MAX_STATE_ID: u32 = 6736;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cracked Nether Bricks
pub struct CrackedNetherBricks;

impl BlockDef for CrackedNetherBricks {
    const ID: u32 = 942;
    const STRING_ID: &'static str = "minecraft:cracked_nether_bricks";
    const NAME: &'static str = "Cracked Nether Bricks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6737;
    const MAX_STATE_ID: u32 = 6737;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Repeater
pub struct PoweredRepeater;

impl BlockDef for PoweredRepeater {
    const ID: u32 = 94;
    const STRING_ID: &'static str = "minecraft:powered_repeater";
    const NAME: &'static str = "Redstone Repeater";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6738;
    const MAX_STATE_ID: u32 = 6753;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Candle
pub struct LightBlueCandle;

impl BlockDef for LightBlueCandle {
    const ID: u32 = 948;
    const STRING_ID: &'static str = "minecraft:light_blue_candle";
    const NAME: &'static str = "Light Blue Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6754;
    const MAX_STATE_ID: u32 = 6761;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Lime Stained Glass Pane
pub struct HardLimeStainedGlassPane;

impl BlockDef for HardLimeStainedGlassPane {
    const ID: u32 = 7295;
    const STRING_ID: &'static str = "minecraft:hard_lime_stained_glass_pane";
    const NAME: &'static str = "Hard Lime Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6762;
    const MAX_STATE_ID: u32 = 6762;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pumpkin
pub struct Pumpkin;

impl BlockDef for Pumpkin {
    const ID: u32 = 86;
    const STRING_ID: &'static str = "minecraft:pumpkin";
    const NAME: &'static str = "Pumpkin";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6763;
    const MAX_STATE_ID: u32 = 6766;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element Constructor
pub struct ElementConstructor;

impl BlockDef for ElementConstructor {
    const ID: u32 = 7300;
    const STRING_ID: &'static str = "minecraft:element_constructor";
    const NAME: &'static str = "Element Constructor";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6767;
    const MAX_STATE_ID: u32 = 6770;
    type State = super::states::DirectionState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Tiles
pub struct DeepslateTiles;

impl BlockDef for DeepslateTiles {
    const ID: u32 = 1160;
    const STRING_ID: &'static str = "minecraft:deepslate_tiles";
    const NAME: &'static str = "Deepslate Tiles";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6771;
    const MAX_STATE_ID: u32 = 6771;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Stone
pub struct SmoothStone;

impl BlockDef for SmoothStone {
    const ID: u32 = 624;
    const STRING_ID: &'static str = "minecraft:smooth_stone";
    const NAME: &'static str = "Smooth Stone";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6772;
    const MAX_STATE_ID: u32 = 6772;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Light Gray Stained Glass Pane
pub struct HardLightGrayStainedGlassPane;

impl BlockDef for HardLightGrayStainedGlassPane {
    const ID: u32 = 7306;
    const STRING_ID: &'static str = "minecraft:hard_light_gray_stained_glass_pane";
    const NAME: &'static str = "Hard Light Gray Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6773;
    const MAX_STATE_ID: u32 = 6773;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Terracotta
pub struct GrayTerracotta;

impl BlockDef for GrayTerracotta {
    const ID: u32 = 491;
    const STRING_ID: &'static str = "minecraft:gray_terracotta";
    const NAME: &'static str = "Gray Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6774;
    const MAX_STATE_ID: u32 = 6774;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper Trapdoor
pub struct OxidizedCopperTrapdoor;

impl BlockDef for OxidizedCopperTrapdoor {
    const ID: u32 = 1087;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_trapdoor";
    const NAME: &'static str = "Oxidized Copper Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6775;
    const MAX_STATE_ID: u32 = 6790;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Granite Slab
pub struct GraniteSlab;

impl BlockDef for GraniteSlab {
    const ID: u32 = 819;
    const STRING_ID: &'static str = "minecraft:granite_slab";
    const NAME: &'static str = "Granite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6791;
    const MAX_STATE_ID: u32 = 6792;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Tulip
pub struct WhiteTulip;

impl BlockDef for WhiteTulip {
    const ID: u32 = 166;
    const STRING_ID: &'static str = "minecraft:white_tulip";
    const NAME: &'static str = "White Tulip";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6793;
    const MAX_STATE_ID: u32 = 6793;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Concrete
pub struct LimeConcrete;

impl BlockDef for LimeConcrete {
    const ID: u32 = 715;
    const STRING_ID: &'static str = "minecraft:lime_concrete";
    const NAME: &'static str = "Lime Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6794;
    const MAX_STATE_ID: u32 = 6794;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Black Candle
pub struct BlackCandleCake;

impl BlockDef for BlackCandleCake {
    const ID: u32 = 977;
    const STRING_ID: &'static str = "minecraft:black_candle_cake";
    const NAME: &'static str = "Cake with Black Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6795;
    const MAX_STATE_ID: u32 = 6796;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Mushroom
pub struct RedMushroom;

impl BlockDef for RedMushroom {
    const ID: u32 = 40;
    const STRING_ID: &'static str = "minecraft:red_mushroom";
    const NAME: &'static str = "Red Mushroom";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6797;
    const MAX_STATE_ID: u32 = 6797;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gilded Blackstone
pub struct GildedBlackstone;

impl BlockDef for GildedBlackstone {
    const ID: u32 = 935;
    const STRING_ID: &'static str = "minecraft:gilded_blackstone";
    const NAME: &'static str = "Gilded Blackstone";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6798;
    const MAX_STATE_ID: u32 = 6798;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Yellow Stained Glass
pub struct HardYellowStainedGlass;

impl BlockDef for HardYellowStainedGlass {
    const ID: u32 = 7332;
    const STRING_ID: &'static str = "minecraft:hard_yellow_stained_glass";
    const NAME: &'static str = "Hard Yellow Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6799;
    const MAX_STATE_ID: u32 = 6799;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Terracotta
pub struct MagentaTerracotta;

impl BlockDef for MagentaTerracotta {
    const ID: u32 = 486;
    const STRING_ID: &'static str = "minecraft:magenta_terracotta";
    const NAME: &'static str = "Magenta Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 6800;
    const MAX_STATE_ID: u32 = 6800;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Cut Copper Stairs
pub struct ExposedCutCopperStairs;

impl BlockDef for ExposedCutCopperStairs {
    const ID: u32 = 1061;
    const STRING_ID: &'static str = "minecraft:exposed_cut_copper_stairs";
    const NAME: &'static str = "Exposed Cut Copper Stairs";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6801;
    const MAX_STATE_ID: u32 = 6808;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Stairs
pub struct MangroveStairs;

impl BlockDef for MangroveStairs {
    const ID: u32 = 520;
    const STRING_ID: &'static str = "minecraft:mangrove_stairs";
    const NAME: &'static str = "Mangrove Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6809;
    const MAX_STATE_ID: u32 = 6816;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Diorite Slab
pub struct PolishedDioriteSlab;

impl BlockDef for PolishedDioriteSlab {
    const ID: u32 = 814;
    const STRING_ID: &'static str = "minecraft:polished_diorite_slab";
    const NAME: &'static str = "Polished Diorite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6817;
    const MAX_STATE_ID: u32 = 6818;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Reserved6
pub struct Reserved6;

impl BlockDef for Reserved6 {
    const ID: u32 = 255;
    const STRING_ID: &'static str = "minecraft:reserved6";
    const NAME: &'static str = "Reserved6";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6819;
    const MAX_STATE_ID: u32 = 6819;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Copper Stairs
pub struct CutCopperStairs;

impl BlockDef for CutCopperStairs {
    const ID: u32 = 1060;
    const STRING_ID: &'static str = "minecraft:cut_copper_stairs";
    const NAME: &'static str = "Cut Copper Stairs";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6820;
    const MAX_STATE_ID: u32 = 6827;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Test Instance Block
pub struct Unknown;

impl BlockDef for Unknown {
    const ID: u32 = 908;
    const STRING_ID: &'static str = "minecraft:unknown";
    const NAME: &'static str = "Test Instance Block";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 6828;
    const MAX_STATE_ID: u32 = 6828;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lab Table
pub struct LabTable;

impl BlockDef for LabTable {
    const ID: u32 = 7362;
    const STRING_ID: &'static str = "minecraft:lab_table";
    const NAME: &'static str = "Lab Table";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6829;
    const MAX_STATE_ID: u32 = 6832;
    type State = super::states::DirectionState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper Lantern
pub struct WaxedOxidizedCopperLantern;

impl BlockDef for WaxedOxidizedCopperLantern {
    const ID: u32 = 858;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_lantern";
    const NAME: &'static str = "Waxed Oxidized Copper Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6833;
    const MAX_STATE_ID: u32 = 6834;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Button
pub struct CherryButton;

impl BlockDef for CherryButton {
    const ID: u32 = 448;
    const STRING_ID: &'static str = "minecraft:cherry_button";
    const NAME: &'static str = "Cherry Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6835;
    const MAX_STATE_ID: u32 = 6846;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Yellow Candle
pub struct YellowCandleCake;

impl BlockDef for YellowCandleCake {
    const ID: u32 = 966;
    const STRING_ID: &'static str = "minecraft:yellow_candle_cake";
    const NAME: &'static str = "Cake with Yellow Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6847;
    const MAX_STATE_ID: u32 = 6848;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Fence Gate
pub struct MangroveFenceGate;

impl BlockDef for MangroveFenceGate {
    const ID: u32 = 635;
    const STRING_ID: &'static str = "minecraft:mangrove_fence_gate";
    const NAME: &'static str = "Mangrove Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6849;
    const MAX_STATE_ID: u32 = 6864;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sunflower
pub struct Sunflower;

impl BlockDef for Sunflower {
    const ID: u32 = 557;
    const STRING_ID: &'static str = "minecraft:sunflower";
    const NAME: &'static str = "Sunflower";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6865;
    const MAX_STATE_ID: u32 = 6866;
    type State = super::states::DoublePlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Petals
pub struct PinkPetals;

impl BlockDef for PinkPetals {
    const ID: u32 = 1141;
    const STRING_ID: &'static str = "minecraft:pink_petals";
    const NAME: &'static str = "Pink Petals";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6867;
    const MAX_STATE_ID: u32 = 6898;
    type State = super::states::PetalsState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Hanging Sign
pub struct BambooHangingSign;

impl BlockDef for BambooHangingSign {
    const ID: u32 = 245;
    const STRING_ID: &'static str = "minecraft:bamboo_hanging_sign";
    const NAME: &'static str = "Bamboo Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 6899;
    const MAX_STATE_ID: u32 = 7282;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Infested Deepslate
pub struct InfestedDeepslate;

impl BlockDef for InfestedDeepslate {
    const ID: u32 = 1171;
    const STRING_ID: &'static str = "minecraft:infested_deepslate";
    const NAME: &'static str = "Infested Deepslate";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 0.75_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7283;
    const MAX_STATE_ID: u32 = 7285;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Soul Torch
pub struct SoulTorch;

impl BlockDef for SoulTorch {
    const ID: u32 = 290;
    const STRING_ID: &'static str = "minecraft:soul_torch";
    const NAME: &'static str = "Soul Torch";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 10;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7286;
    const MAX_STATE_ID: u32 = 7291;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Podzol
pub struct Podzol;

impl BlockDef for Podzol {
    const ID: u32 = 243;
    const STRING_ID: &'static str = "minecraft:podzol";
    const NAME: &'static str = "Podzol";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7292;
    const MAX_STATE_ID: u32 = 7292;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Copper
pub struct CopperBlock;

impl BlockDef for CopperBlock {
    const ID: u32 = 1034;
    const STRING_ID: &'static str = "minecraft:copper_block";
    const NAME: &'static str = "Block of Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7293;
    const MAX_STATE_ID: u32 = 7293;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Ore
pub struct LitRedstoneOre;

impl BlockDef for LitRedstoneOre {
    const ID: u32 = 74;
    const STRING_ID: &'static str = "minecraft:lit_redstone_ore";
    const NAME: &'static str = "Redstone Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7294;
    const MAX_STATE_ID: u32 = 7294;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Tile Stairs
pub struct DeepslateTileStairs;

impl BlockDef for DeepslateTileStairs {
    const ID: u32 = 1161;
    const STRING_ID: &'static str = "minecraft:deepslate_tile_stairs";
    const NAME: &'static str = "Deepslate Tile Stairs";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7295;
    const MAX_STATE_ID: u32 = 7302;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Fence Gate
pub struct CrimsonFenceGate;

impl BlockDef for CrimsonFenceGate {
    const ID: u32 = 893;
    const STRING_ID: &'static str = "minecraft:crimson_fence_gate";
    const NAME: &'static str = "Crimson Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7303;
    const MAX_STATE_ID: u32 = 7318;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Bush
pub struct Deadbush;

impl BlockDef for Deadbush {
    const ID: u32 = 32;
    const STRING_ID: &'static str = "minecraft:deadbush";
    const NAME: &'static str = "Dead Bush";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7319;
    const MAX_STATE_ID: u32 = 7319;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Cut Copper Slab
pub struct WaxedWeatheredDoubleCutCopperSlab;

impl BlockDef for WaxedWeatheredDoubleCutCopperSlab {
    const ID: u32 = 8086;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_double_cut_copper_slab";
    const NAME: &'static str = "Waxed Weathered Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7320;
    const MAX_STATE_ID: u32 = 7321;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Bricks
pub struct PolishedBlackstoneBricks;

impl BlockDef for PolishedBlackstoneBricks {
    const ID: u32 = 929;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_bricks";
    const NAME: &'static str = "Polished Blackstone Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7322;
    const MAX_STATE_ID: u32 = 7322;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Candle
pub struct RedCandle;

impl BlockDef for RedCandle {
    const ID: u32 = 959;
    const STRING_ID: &'static str = "minecraft:red_candle";
    const NAME: &'static str = "Red Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7323;
    const MAX_STATE_ID: u32 = 7330;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Copper
pub struct CutCopper;

impl BlockDef for CutCopper {
    const ID: u32 = 1044;
    const STRING_ID: &'static str = "minecraft:cut_copper";
    const NAME: &'static str = "Cut Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7331;
    const MAX_STATE_ID: u32 = 7331;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper Golem Statue
pub struct WaxedWeatheredCopperGolemStatue;

impl BlockDef for WaxedWeatheredCopperGolemStatue {
    const ID: u32 = 1122;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_golem_statue";
    const NAME: &'static str = "Waxed Weathered Copper Golem Statue";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7332;
    const MAX_STATE_ID: u32 = 7335;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Iron Ore
pub struct IronOre;

impl BlockDef for IronOre {
    const ID: u32 = 15;
    const STRING_ID: &'static str = "minecraft:iron_ore";
    const NAME: &'static str = "Iron Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7336;
    const MAX_STATE_ID: u32 = 7336;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Door
pub struct SpruceDoor;

impl BlockDef for SpruceDoor {
    const ID: u32 = 193;
    const STRING_ID: &'static str = "minecraft:spruce_door";
    const NAME: &'static str = "Spruce Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7337;
    const MAX_STATE_ID: u32 = 7368;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Frosted Ice
pub struct FrostedIce;

impl BlockDef for FrostedIce {
    const ID: u32 = 207;
    const STRING_ID: &'static str = "minecraft:frosted_ice";
    const NAME: &'static str = "Frosted Ice";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 7369;
    const MAX_STATE_ID: u32 = 7372;
    type State = super::states::AgeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chipped Anvil
pub struct ChippedAnvil;

impl BlockDef for ChippedAnvil {
    const ID: u32 = 468;
    const STRING_ID: &'static str = "minecraft:chipped_anvil";
    const NAME: &'static str = "Chipped Anvil";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7373;
    const MAX_STATE_ID: u32 = 7376;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Large Amethyst Bud
pub struct LargeAmethystBud;

impl BlockDef for LargeAmethystBud {
    const ID: u32 = 981;
    const STRING_ID: &'static str = "minecraft:large_amethyst_bud";
    const NAME: &'static str = "Large Amethyst Bud";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 4;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7377;
    const MAX_STATE_ID: u32 = 7382;
    type State = super::states::BlockFaceState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper Door
pub struct ExposedCopperDoor;

impl BlockDef for ExposedCopperDoor {
    const ID: u32 = 1077;
    const STRING_ID: &'static str = "minecraft:exposed_copper_door";
    const NAME: &'static str = "Exposed Copper Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7383;
    const MAX_STATE_ID: u32 = 7414;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Suspicious Gravel
pub struct SuspiciousGravel;

impl BlockDef for SuspiciousGravel {
    const ID: u32 = 41;
    const STRING_ID: &'static str = "minecraft:suspicious_gravel";
    const NAME: &'static str = "Suspicious Gravel";
    const HARDNESS: f32 = 0.25_f32;
    const RESISTANCE: f32 = 0.25_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7415;
    const MAX_STATE_ID: u32 = 7422;
    type State = super::states::BrushableState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Trapdoor
pub struct WarpedTrapdoor;

impl BlockDef for WarpedTrapdoor {
    const ID: u32 = 892;
    const STRING_ID: &'static str = "minecraft:warped_trapdoor";
    const NAME: &'static str = "Warped Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7423;
    const MAX_STATE_ID: u32 = 7438;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Water
pub struct FlowingWater;

impl BlockDef for FlowingWater {
    const ID: u32 = 8;
    const STRING_ID: &'static str = "minecraft:flowing_water";
    const NAME: &'static str = "Water";
    const HARDNESS: f32 = 100.0_f32;
    const RESISTANCE: f32 = 100.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 7439;
    const MAX_STATE_ID: u32 = 7454;
    type State = super::states::LiquidState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bricks
pub struct BrickBlock;

impl BlockDef for BrickBlock {
    const ID: u32 = 45;
    const STRING_ID: &'static str = "minecraft:brick_block";
    const NAME: &'static str = "Bricks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7455;
    const MAX_STATE_ID: u32 = 7455;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Glass
pub struct HardGlass;

impl BlockDef for HardGlass {
    const ID: u32 = 253;
    const STRING_ID: &'static str = "minecraft:hard_glass";
    const NAME: &'static str = "Hard Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7456;
    const MAX_STATE_ID: u32 = 7456;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper Trapdoor
pub struct WaxedWeatheredCopperTrapdoor;

impl BlockDef for WaxedWeatheredCopperTrapdoor {
    const ID: u32 = 1090;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_trapdoor";
    const NAME: &'static str = "Waxed Weathered Copper Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7457;
    const MAX_STATE_ID: u32 = 7472;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Quartz Stairs
pub struct QuartzStairs;

impl BlockDef for QuartzStairs {
    const ID: u32 = 156;
    const STRING_ID: &'static str = "minecraft:quartz_stairs";
    const NAME: &'static str = "Quartz Stairs";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7473;
    const MAX_STATE_ID: u32 = 7480;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cave Vines Plant
pub struct CaveVines;

impl BlockDef for CaveVines {
    const ID: u32 = 8087;
    const STRING_ID: &'static str = "minecraft:cave_vines";
    const NAME: &'static str = "Cave Vines Plant";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7481;
    const MAX_STATE_ID: u32 = 7506;
    type State = super::states::GrowingPlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Stained Glass Pane
pub struct MagentaStainedGlassPane;

impl BlockDef for MagentaStainedGlassPane {
    const ID: u32 = 502;
    const STRING_ID: &'static str = "minecraft:magenta_stained_glass_pane";
    const NAME: &'static str = "Magenta Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7507;
    const MAX_STATE_ID: u32 = 7507;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Iron Bars
pub struct IronBars;

impl BlockDef for IronBars {
    const ID: u32 = 101;
    const STRING_ID: &'static str = "minecraft:iron_bars";
    const NAME: &'static str = "Iron Bars";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7508;
    const MAX_STATE_ID: u32 = 7508;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Terracotta
pub struct WhiteTerracotta;

impl BlockDef for WhiteTerracotta {
    const ID: u32 = 484;
    const STRING_ID: &'static str = "minecraft:white_terracotta";
    const NAME: &'static str = "White Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7509;
    const MAX_STATE_ID: u32 = 7509;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Oak Wood
pub struct StrippedOakWood;

impl BlockDef for StrippedOakWood {
    const ID: u32 = 79;
    const STRING_ID: &'static str = "minecraft:stripped_oak_wood";
    const NAME: &'static str = "Stripped Oak Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7510;
    const MAX_STATE_ID: u32 = 7512;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Carpet
pub struct LightBlueCarpet;

impl BlockDef for LightBlueCarpet {
    const ID: u32 = 541;
    const STRING_ID: &'static str = "minecraft:light_blue_carpet";
    const NAME: &'static str = "Light Blue Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7513;
    const MAX_STATE_ID: u32 = 7513;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Hanging Sign
pub struct OakHangingSign;

impl BlockDef for OakHangingSign {
    const ID: u32 = 234;
    const STRING_ID: &'static str = "minecraft:oak_hanging_sign";
    const NAME: &'static str = "Oak Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7514;
    const MAX_STATE_ID: u32 = 7897;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Concrete Powder
pub struct WhiteConcretePowder;

impl BlockDef for WhiteConcretePowder {
    const ID: u32 = 726;
    const STRING_ID: &'static str = "minecraft:white_concrete_powder";
    const NAME: &'static str = "White Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7898;
    const MAX_STATE_ID: u32 = 7898;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Attached Melon Stem
pub struct MelonStem;

impl BlockDef for MelonStem {
    const ID: u32 = 105;
    const STRING_ID: &'static str = "minecraft:melon_stem";
    const NAME: &'static str = "Attached Melon Stem";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7899;
    const MAX_STATE_ID: u32 = 7946;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Planks
pub struct CrimsonPlanks;

impl BlockDef for CrimsonPlanks {
    const ID: u32 = 883;
    const STRING_ID: &'static str = "minecraft:crimson_planks";
    const NAME: &'static str = "Crimson Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7947;
    const MAX_STATE_ID: u32 = 7947;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Dark Oak Wood
pub struct StrippedDarkOakWood;

impl BlockDef for StrippedDarkOakWood {
    const ID: u32 = 85;
    const STRING_ID: &'static str = "minecraft:stripped_dark_oak_wood";
    const NAME: &'static str = "Stripped Dark Oak Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7948;
    const MAX_STATE_ID: u32 = 7950;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Cut Copper
pub struct WaxedWeatheredCutCopper;

impl BlockDef for WaxedWeatheredCutCopper {
    const ID: u32 = 1050;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_cut_copper";
    const NAME: &'static str = "Waxed Weathered Cut Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7951;
    const MAX_STATE_ID: u32 = 7951;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Stained Glass
pub struct WhiteStainedGlass;

impl BlockDef for WhiteStainedGlass {
    const ID: u32 = 300;
    const STRING_ID: &'static str = "minecraft:white_stained_glass";
    const NAME: &'static str = "White Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7952;
    const MAX_STATE_ID: u32 = 7952;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Horn Coral Wall Fan
pub struct HornCoralWallFan;

impl BlockDef for HornCoralWallFan {
    const ID: u32 = 787;
    const STRING_ID: &'static str = "minecraft:horn_coral_wall_fan";
    const NAME: &'static str = "Horn Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 7953;
    const MAX_STATE_ID: u32 = 7956;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Wood
pub struct OakWood;

impl BlockDef for OakWood {
    const ID: u32 = 71;
    const STRING_ID: &'static str = "minecraft:oak_wood";
    const NAME: &'static str = "Oak Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7957;
    const MAX_STATE_ID: u32 = 7959;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Stained Glass Pane
pub struct PurpleStainedGlassPane;

impl BlockDef for PurpleStainedGlassPane {
    const ID: u32 = 510;
    const STRING_ID: &'static str = "minecraft:purple_stained_glass_pane";
    const NAME: &'static str = "Purple Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7960;
    const MAX_STATE_ID: u32 = 7960;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper Trapdoor
pub struct WaxedOxidizedCopperTrapdoor;

impl BlockDef for WaxedOxidizedCopperTrapdoor {
    const ID: u32 = 1091;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_trapdoor";
    const NAME: &'static str = "Waxed Oxidized Copper Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7961;
    const MAX_STATE_ID: u32 = 7976;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Sign
pub struct WallSign;

impl BlockDef for WallSign {
    const ID: u32 = 68;
    const STRING_ID: &'static str = "minecraft:wall_sign";
    const NAME: &'static str = "Oak Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 7977;
    const MAX_STATE_ID: u32 = 7982;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jukebox
pub struct Jukebox;

impl BlockDef for Jukebox {
    const ID: u32 = 84;
    const STRING_ID: &'static str = "minecraft:jukebox";
    const NAME: &'static str = "Jukebox";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7983;
    const MAX_STATE_ID: u32 = 7983;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Cherry Wood
pub struct StrippedCherryWood;

impl BlockDef for StrippedCherryWood {
    const ID: u32 = 84;
    const STRING_ID: &'static str = "minecraft:stripped_cherry_wood";
    const NAME: &'static str = "Stripped Cherry Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7984;
    const MAX_STATE_ID: u32 = 7986;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jigsaw Block
pub struct Jigsaw;

impl BlockDef for Jigsaw {
    const ID: u32 = 906;
    const STRING_ID: &'static str = "minecraft:jigsaw";
    const NAME: &'static str = "Jigsaw Block";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 7987;
    const MAX_STATE_ID: u32 = 8010;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper Golem Statue
pub struct WaxedOxidizedCopperGolemStatue;

impl BlockDef for WaxedOxidizedCopperGolemStatue {
    const ID: u32 = 1123;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_golem_statue";
    const NAME: &'static str = "Waxed Oxidized Copper Golem Statue";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8011;
    const MAX_STATE_ID: u32 = 8014;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Prismarine Wall
pub struct PrismarineWall;

impl BlockDef for PrismarineWall {
    const ID: u32 = 825;
    const STRING_ID: &'static str = "minecraft:prismarine_wall";
    const NAME: &'static str = "Prismarine Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8015;
    const MAX_STATE_ID: u32 = 8176;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Border Block
pub struct BorderBlock;

impl BlockDef for BorderBlock {
    const ID: u32 = 212;
    const STRING_ID: &'static str = "minecraft:border_block";
    const NAME: &'static str = "Border Block";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8177;
    const MAX_STATE_ID: u32 = 8338;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Shroomlight
pub struct Shroomlight;

impl BlockDef for Shroomlight {
    const ID: u32 = 877;
    const STRING_ID: &'static str = "minecraft:shroomlight";
    const NAME: &'static str = "Shroomlight";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8339;
    const MAX_STATE_ID: u32 = 8339;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Fence Gate
pub struct BambooFenceGate;

impl BlockDef for BambooFenceGate {
    const ID: u32 = 636;
    const STRING_ID: &'static str = "minecraft:bamboo_fence_gate";
    const NAME: &'static str = "Bamboo Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8340;
    const MAX_STATE_ID: u32 = 8355;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cornflower
pub struct Cornflower;

impl BlockDef for Cornflower {
    const ID: u32 = 169;
    const STRING_ID: &'static str = "minecraft:cornflower";
    const NAME: &'static str = "Cornflower";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8356;
    const MAX_STATE_ID: u32 = 8356;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Polished Blackstone
pub struct ChiseledPolishedBlackstone;

impl BlockDef for ChiseledPolishedBlackstone {
    const ID: u32 = 931;
    const STRING_ID: &'static str = "minecraft:chiseled_polished_blackstone";
    const NAME: &'static str = "Chiseled Polished Blackstone";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8357;
    const MAX_STATE_ID: u32 = 8357;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Stairs
pub struct DarkOakStairs;

impl BlockDef for DarkOakStairs {
    const ID: u32 = 164;
    const STRING_ID: &'static str = "minecraft:dark_oak_stairs";
    const NAME: &'static str = "Dark Oak Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8358;
    const MAX_STATE_ID: u32 = 8365;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Tile Wall
pub struct DeepslateTileWall;

impl BlockDef for DeepslateTileWall {
    const ID: u32 = 1163;
    const STRING_ID: &'static str = "minecraft:deepslate_tile_wall";
    const NAME: &'static str = "Deepslate Tile Wall";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8366;
    const MAX_STATE_ID: u32 = 8527;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Glass Pane
pub struct GlassPane;

impl BlockDef for GlassPane {
    const ID: u32 = 102;
    const STRING_ID: &'static str = "minecraft:glass_pane";
    const NAME: &'static str = "Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8528;
    const MAX_STATE_ID: u32 = 8528;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Deepslate
pub struct ChiseledDeepslate;

impl BlockDef for ChiseledDeepslate {
    const ID: u32 = 1168;
    const STRING_ID: &'static str = "minecraft:chiseled_deepslate";
    const NAME: &'static str = "Chiseled Deepslate";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8529;
    const MAX_STATE_ID: u32 = 8529;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Copper Slab
pub struct CutCopperSlab;

impl BlockDef for CutCopperSlab {
    const ID: u32 = 1068;
    const STRING_ID: &'static str = "minecraft:cut_copper_slab";
    const NAME: &'static str = "Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8530;
    const MAX_STATE_ID: u32 = 8531;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Stained Glass
pub struct RedStainedGlass;

impl BlockDef for RedStainedGlass {
    const ID: u32 = 314;
    const STRING_ID: &'static str = "minecraft:red_stained_glass";
    const NAME: &'static str = "Red Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8532;
    const MAX_STATE_ID: u32 = 8532;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Wood
pub struct PaleOakWood;

impl BlockDef for PaleOakWood {
    const ID: u32 = 20;
    const STRING_ID: &'static str = "minecraft:pale_oak_wood";
    const NAME: &'static str = "Pale Oak Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8533;
    const MAX_STATE_ID: u32 = 8535;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Infested Stone Bricks
pub struct InfestedStoneBricks;

impl BlockDef for InfestedStoneBricks {
    const ID: u32 = 334;
    const STRING_ID: &'static str = "minecraft:infested_stone_bricks";
    const NAME: &'static str = "Infested Stone Bricks";
    const HARDNESS: f32 = 0.75_f32;
    const RESISTANCE: f32 = 0.75_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8536;
    const MAX_STATE_ID: u32 = 8536;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Pressure Plate
pub struct AcaciaPressurePlate;

impl BlockDef for AcaciaPressurePlate {
    const ID: u32 = 265;
    const STRING_ID: &'static str = "minecraft:acacia_pressure_plate";
    const NAME: &'static str = "Acacia Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8537;
    const MAX_STATE_ID: u32 = 8552;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Lightning Rod
pub struct WeatheredLightningRod;

impl BlockDef for WeatheredLightningRod {
    const ID: u32 = 1126;
    const STRING_ID: &'static str = "minecraft:weathered_lightning_rod";
    const NAME: &'static str = "Weathered Lightning Rod";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8553;
    const MAX_STATE_ID: u32 = 8564;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Trapdoor
pub struct BambooTrapdoor;

impl BlockDef for BambooTrapdoor {
    const ID: u32 = 325;
    const STRING_ID: &'static str = "minecraft:bamboo_trapdoor";
    const NAME: &'static str = "Bamboo Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8565;
    const MAX_STATE_ID: u32 = 8580;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Chiseled Copper
pub struct OxidizedChiseledCopper;

impl BlockDef for OxidizedChiseledCopper {
    const ID: u32 = 1055;
    const STRING_ID: &'static str = "minecraft:oxidized_chiseled_copper";
    const NAME: &'static str = "Oxidized Chiseled Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8581;
    const MAX_STATE_ID: u32 = 8581;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Sign
pub struct MangroveWallSign;

impl BlockDef for MangroveWallSign {
    const ID: u32 = 232;
    const STRING_ID: &'static str = "minecraft:mangrove_wall_sign";
    const NAME: &'static str = "Mangrove Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8582;
    const MAX_STATE_ID: u32 = 8587;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Raw Copper
pub struct RawCopperBlock;

impl BlockDef for RawCopperBlock {
    const ID: u32 = 1174;
    const STRING_ID: &'static str = "minecraft:raw_copper_block";
    const NAME: &'static str = "Block of Raw Copper";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8588;
    const MAX_STATE_ID: u32 = 8588;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tall Dry Grass
pub struct TallDryGrass;

impl BlockDef for TallDryGrass {
    const ID: u32 = 135;
    const STRING_ID: &'static str = "minecraft:tall_dry_grass";
    const NAME: &'static str = "Tall Dry Grass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8589;
    const MAX_STATE_ID: u32 = 8589;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Cut Copper Slab
pub struct OxidizedCutCopperSlab;

impl BlockDef for OxidizedCutCopperSlab {
    const ID: u32 = 1071;
    const STRING_ID: &'static str = "minecraft:oxidized_cut_copper_slab";
    const NAME: &'static str = "Oxidized Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8590;
    const MAX_STATE_ID: u32 = 8591;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Horn Coral Block
pub struct HornCoralBlock;

impl BlockDef for HornCoralBlock {
    const ID: u32 = 757;
    const STRING_ID: &'static str = "minecraft:horn_coral_block";
    const NAME: &'static str = "Horn Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8592;
    const MAX_STATE_ID: u32 = 8592;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Shelf
pub struct DarkOakShelf;

impl BlockDef for DarkOakShelf {
    const ID: u32 = 185;
    const STRING_ID: &'static str = "minecraft:dark_oak_shelf";
    const NAME: &'static str = "Dark Oak Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8593;
    const MAX_STATE_ID: u32 = 8624;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Beetroots
pub struct Beetroot;

impl BlockDef for Beetroot {
    const ID: u32 = 244;
    const STRING_ID: &'static str = "minecraft:beetroot";
    const NAME: &'static str = "Beetroots";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8625;
    const MAX_STATE_ID: u32 = 8632;
    type State = super::states::CropState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Light Gray Candle
pub struct LightGrayCandleCake;

impl BlockDef for LightGrayCandleCake {
    const ID: u32 = 970;
    const STRING_ID: &'static str = "minecraft:light_gray_candle_cake";
    const NAME: &'static str = "Cake with Light Gray Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8633;
    const MAX_STATE_ID: u32 = 8634;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Candle
pub struct WhiteCandle;

impl BlockDef for WhiteCandle {
    const ID: u32 = 945;
    const STRING_ID: &'static str = "minecraft:white_candle";
    const NAME: &'static str = "White Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8635;
    const MAX_STATE_ID: u32 = 8642;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Andesite Stairs
pub struct AndesiteStairs;

impl BlockDef for AndesiteStairs {
    const ID: u32 = 807;
    const STRING_ID: &'static str = "minecraft:andesite_stairs";
    const NAME: &'static str = "Andesite Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8643;
    const MAX_STATE_ID: u32 = 8650;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Planks
pub struct BirchPlanks;

impl BlockDef for BirchPlanks {
    const ID: u32 = 15;
    const STRING_ID: &'static str = "minecraft:birch_planks";
    const NAME: &'static str = "Birch Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8651;
    const MAX_STATE_ID: u32 = 8651;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Powered Rail
pub struct GoldenRail;

impl BlockDef for GoldenRail {
    const ID: u32 = 27;
    const STRING_ID: &'static str = "minecraft:golden_rail";
    const NAME: &'static str = "Powered Rail";
    const HARDNESS: f32 = 0.7_f32;
    const RESISTANCE: f32 = 0.7_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8652;
    const MAX_STATE_ID: u32 = 8663;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cyan Wool
pub struct CyanWool;

impl BlockDef for CyanWool {
    const ID: u32 = 149;
    const STRING_ID: &'static str = "minecraft:cyan_wool";
    const NAME: &'static str = "Cyan Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8664;
    const MAX_STATE_ID: u32 = 8664;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Petrified Oak Slab
pub struct PetrifiedOakDoubleSlab;

impl BlockDef for PetrifiedOakDoubleSlab {
    const ID: u32 = 614;
    const STRING_ID: &'static str = "minecraft:petrified_oak_double_slab";
    const NAME: &'static str = "Petrified Oak Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8665;
    const MAX_STATE_ID: u32 = 8666;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deprecated Anvil
pub struct DeprecatedAnvil;

impl BlockDef for DeprecatedAnvil {
    const ID: u32 = 9205;
    const STRING_ID: &'static str = "minecraft:deprecated_anvil";
    const NAME: &'static str = "Deprecated Anvil";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8667;
    const MAX_STATE_ID: u32 = 8670;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Sign
pub struct DarkoakWallSign;

impl BlockDef for DarkoakWallSign {
    const ID: u32 = 230;
    const STRING_ID: &'static str = "minecraft:darkoak_wall_sign";
    const NAME: &'static str = "Dark Oak Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8671;
    const MAX_STATE_ID: u32 = 8676;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Leaves
pub struct JungleLeaves;

impl BlockDef for JungleLeaves {
    const ID: u32 = 91;
    const STRING_ID: &'static str = "minecraft:jungle_leaves";
    const NAME: &'static str = "Jungle Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 8677;
    const MAX_STATE_ID: u32 = 8680;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Shulker Box
pub struct GrayShulkerBox;

impl BlockDef for GrayShulkerBox {
    const ID: u32 = 685;
    const STRING_ID: &'static str = "minecraft:gray_shulker_box";
    const NAME: &'static str = "Gray Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 8681;
    const MAX_STATE_ID: u32 = 8681;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Sandstone Stairs
pub struct RedSandstoneStairs;

impl BlockDef for RedSandstoneStairs {
    const ID: u32 = 180;
    const STRING_ID: &'static str = "minecraft:red_sandstone_stairs";
    const NAME: &'static str = "Red Sandstone Stairs";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8682;
    const MAX_STATE_ID: u32 = 8689;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cyan Glazed Terracotta
pub struct CyanGlazedTerracotta;

impl BlockDef for CyanGlazedTerracotta {
    const ID: u32 = 229;
    const STRING_ID: &'static str = "minecraft:cyan_glazed_terracotta";
    const NAME: &'static str = "Cyan Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8690;
    const MAX_STATE_ID: u32 = 8695;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cracked Deepslate Bricks
pub struct CrackedDeepslateBricks;

impl BlockDef for CrackedDeepslateBricks {
    const ID: u32 = 1169;
    const STRING_ID: &'static str = "minecraft:cracked_deepslate_bricks";
    const NAME: &'static str = "Cracked Deepslate Bricks";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8696;
    const MAX_STATE_ID: u32 = 8696;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Fire Coral Wall Fan
pub struct FireCoralWallFan;

impl BlockDef for FireCoralWallFan {
    const ID: u32 = 786;
    const STRING_ID: &'static str = "minecraft:fire_coral_wall_fan";
    const NAME: &'static str = "Fire Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 8697;
    const MAX_STATE_ID: u32 = 8700;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Fence Gate
pub struct JungleFenceGate;

impl BlockDef for JungleFenceGate {
    const ID: u32 = 185;
    const STRING_ID: &'static str = "minecraft:jungle_fence_gate";
    const NAME: &'static str = "Jungle Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8701;
    const MAX_STATE_ID: u32 = 8716;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper Grate
pub struct ExposedCopperGrate;

impl BlockDef for ExposedCopperGrate {
    const ID: u32 = 1093;
    const STRING_ID: &'static str = "minecraft:exposed_copper_grate";
    const NAME: &'static str = "Exposed Copper Grate";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8717;
    const MAX_STATE_ID: u32 = 8717;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Copper Grate
pub struct WaxedCopperGrate;

impl BlockDef for WaxedCopperGrate {
    const ID: u32 = 1096;
    const STRING_ID: &'static str = "minecraft:waxed_copper_grate";
    const NAME: &'static str = "Waxed Copper Grate";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8718;
    const MAX_STATE_ID: u32 = 8718;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Light Blue Stained Glass Pane
pub struct HardLightBlueStainedGlassPane;

impl BlockDef for HardLightBlueStainedGlassPane {
    const ID: u32 = 9257;
    const STRING_ID: &'static str = "minecraft:hard_light_blue_stained_glass_pane";
    const NAME: &'static str = "Hard Light Blue Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8719;
    const MAX_STATE_ID: u32 = 8719;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Trapdoor
pub struct JungleTrapdoor;

impl BlockDef for JungleTrapdoor {
    const ID: u32 = 319;
    const STRING_ID: &'static str = "minecraft:jungle_trapdoor";
    const NAME: &'static str = "Jungle Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8720;
    const MAX_STATE_ID: u32 = 8735;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Rooted Dirt
pub struct DirtWithRoots;

impl BlockDef for DirtWithRoots {
    const ID: u32 = 1149;
    const STRING_ID: &'static str = "minecraft:dirt_with_roots";
    const NAME: &'static str = "Rooted Dirt";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8736;
    const MAX_STATE_ID: u32 = 8736;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Coal
pub struct CoalBlock;

impl BlockDef for CoalBlock {
    const ID: u32 = 173;
    const STRING_ID: &'static str = "minecraft:coal_block";
    const NAME: &'static str = "Block of Coal";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8737;
    const MAX_STATE_ID: u32 = 8737;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Wool
pub struct WhiteWool;

impl BlockDef for WhiteWool {
    const ID: u32 = 140;
    const STRING_ID: &'static str = "minecraft:white_wool";
    const NAME: &'static str = "White Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8738;
    const MAX_STATE_ID: u32 = 8738;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Fence Gate
pub struct WarpedFenceGate;

impl BlockDef for WarpedFenceGate {
    const ID: u32 = 894;
    const STRING_ID: &'static str = "minecraft:warped_fence_gate";
    const NAME: &'static str = "Warped Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8739;
    const MAX_STATE_ID: u32 = 8754;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Sandstone Slab
pub struct CutSandstoneSlab;

impl BlockDef for CutSandstoneSlab {
    const ID: u32 = 8044;
    const STRING_ID: &'static str = "minecraft:cut_sandstone_slab";
    const NAME: &'static str = "Cut Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8755;
    const MAX_STATE_ID: u32 = 8756;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Skeleton Skull
pub struct SkeletonSkull;

impl BlockDef for SkeletonSkull {
    const ID: u32 = 453;
    const STRING_ID: &'static str = "minecraft:skeleton_skull";
    const NAME: &'static str = "Skeleton Skull";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8757;
    const MAX_STATE_ID: u32 = 8762;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper Chest
pub struct ExposedCopperChest;

impl BlockDef for ExposedCopperChest {
    const ID: u32 = 1109;
    const STRING_ID: &'static str = "minecraft:exposed_copper_chest";
    const NAME: &'static str = "Exposed Copper Chest";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8763;
    const MAX_STATE_ID: u32 = 8766;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper Chain
pub struct ExposedCopperChain;

impl BlockDef for ExposedCopperChain {
    const ID: u32 = 352;
    const STRING_ID: &'static str = "minecraft:exposed_copper_chain";
    const NAME: &'static str = "Exposed Copper Chain";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8767;
    const MAX_STATE_ID: u32 = 8769;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Composter
pub struct Composter;

impl BlockDef for Composter {
    const ID: u32 = 909;
    const STRING_ID: &'static str = "minecraft:composter";
    const NAME: &'static str = "Composter";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8770;
    const MAX_STATE_ID: u32 = 8778;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Cut Copper Slab
pub struct WaxedDoubleCutCopperSlab;

impl BlockDef for WaxedDoubleCutCopperSlab {
    const ID: u32 = 8081;
    const STRING_ID: &'static str = "minecraft:waxed_double_cut_copper_slab";
    const NAME: &'static str = "Waxed Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8779;
    const MAX_STATE_ID: u32 = 8780;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Kelp Plant
pub struct Kelp;

impl BlockDef for Kelp {
    const ID: u32 = 743;
    const STRING_ID: &'static str = "minecraft:kelp";
    const NAME: &'static str = "Kelp Plant";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 8781;
    const MAX_STATE_ID: u32 = 8806;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper Door
pub struct WaxedExposedCopperDoor;

impl BlockDef for WaxedExposedCopperDoor {
    const ID: u32 = 1081;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_door";
    const NAME: &'static str = "Waxed Exposed Copper Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8807;
    const MAX_STATE_ID: u32 = 8838;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Bricks
pub struct DeepslateBricks;

impl BlockDef for DeepslateBricks {
    const ID: u32 = 1164;
    const STRING_ID: &'static str = "minecraft:deepslate_bricks";
    const NAME: &'static str = "Deepslate Bricks";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8839;
    const MAX_STATE_ID: u32 = 8839;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Glazed Terracotta
pub struct BlueGlazedTerracotta;

impl BlockDef for BlueGlazedTerracotta {
    const ID: u32 = 231;
    const STRING_ID: &'static str = "minecraft:blue_glazed_terracotta";
    const NAME: &'static str = "Blue Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8840;
    const MAX_STATE_ID: u32 = 8845;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Glazed Terracotta
pub struct LightBlueGlazedTerracotta;

impl BlockDef for LightBlueGlazedTerracotta {
    const ID: u32 = 223;
    const STRING_ID: &'static str = "minecraft:light_blue_glazed_terracotta";
    const NAME: &'static str = "Light Blue Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8846;
    const MAX_STATE_ID: u32 = 8851;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Rose Bush
pub struct RoseBush;

impl BlockDef for RoseBush {
    const ID: u32 = 559;
    const STRING_ID: &'static str = "minecraft:rose_bush";
    const NAME: &'static str = "Rose Bush";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8852;
    const MAX_STATE_ID: u32 = 8853;
    type State = super::states::DoublePlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Flowering Azalea
pub struct FloweringAzalea;

impl BlockDef for FloweringAzalea {
    const ID: u32 = 1139;
    const STRING_ID: &'static str = "minecraft:flowering_azalea";
    const NAME: &'static str = "Flowering Azalea";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8854;
    const MAX_STATE_ID: u32 = 8854;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Cut Copper
pub struct OxidizedCutCopper;

impl BlockDef for OxidizedCutCopper {
    const ID: u32 = 1047;
    const STRING_ID: &'static str = "minecraft:oxidized_cut_copper";
    const NAME: &'static str = "Oxidized Cut Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8855;
    const MAX_STATE_ID: u32 = 8855;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Wool
pub struct BlueWool;

impl BlockDef for BlueWool {
    const ID: u32 = 151;
    const STRING_ID: &'static str = "minecraft:blue_wool";
    const NAME: &'static str = "Blue Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 8856;
    const MAX_STATE_ID: u32 = 8856;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Hanging Sign
pub struct PaleOakHangingSign;

impl BlockDef for PaleOakHangingSign {
    const ID: u32 = 241;
    const STRING_ID: &'static str = "minecraft:pale_oak_hanging_sign";
    const NAME: &'static str = "Pale Oak Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 8857;
    const MAX_STATE_ID: u32 = 9240;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weeping Vines Plant
pub struct WeepingVines;

impl BlockDef for WeepingVines {
    const ID: u32 = 879;
    const STRING_ID: &'static str = "minecraft:weeping_vines";
    const NAME: &'static str = "Weeping Vines Plant";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9241;
    const MAX_STATE_ID: u32 = 9266;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chorus Plant
pub struct ChorusPlant;

impl BlockDef for ChorusPlant {
    const ID: u32 = 240;
    const STRING_ID: &'static str = "minecraft:chorus_plant";
    const NAME: &'static str = "Chorus Plant";
    const HARDNESS: f32 = 0.4_f32;
    const RESISTANCE: f32 = 0.4_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 9267;
    const MAX_STATE_ID: u32 = 9267;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Water
pub struct Water;

impl BlockDef for Water {
    const ID: u32 = 9;
    const STRING_ID: &'static str = "minecraft:water";
    const NAME: &'static str = "Water";
    const HARDNESS: f32 = 100.0_f32;
    const RESISTANCE: f32 = 100.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 9268;
    const MAX_STATE_ID: u32 = 9283;
    type State = super::states::LiquidState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mud Brick Stairs
pub struct MudBrickStairs;

impl BlockDef for MudBrickStairs {
    const ID: u32 = 372;
    const STRING_ID: &'static str = "minecraft:mud_brick_stairs";
    const NAME: &'static str = "Mud Brick Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9284;
    const MAX_STATE_ID: u32 = 9291;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Repeater
pub struct UnpoweredRepeater;

impl BlockDef for UnpoweredRepeater {
    const ID: u32 = 93;
    const STRING_ID: &'static str = "minecraft:unpowered_repeater";
    const NAME: &'static str = "Redstone Repeater";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9292;
    const MAX_STATE_ID: u32 = 9307;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Brick Wall
pub struct StoneBrickWall;

impl BlockDef for StoneBrickWall {
    const ID: u32 = 829;
    const STRING_ID: &'static str = "minecraft:stone_brick_wall";
    const NAME: &'static str = "Stone Brick Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9308;
    const MAX_STATE_ID: u32 = 9469;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Red Sandstone Stairs
pub struct SmoothRedSandstoneStairs;

impl BlockDef for SmoothRedSandstoneStairs {
    const ID: u32 = 798;
    const STRING_ID: &'static str = "minecraft:smooth_red_sandstone_stairs";
    const NAME: &'static str = "Smooth Red Sandstone Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9470;
    const MAX_STATE_ID: u32 = 9477;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 100
pub struct Element100;

impl BlockDef for Element100 {
    const ID: u32 = 10018;
    const STRING_ID: &'static str = "minecraft:element_100";
    const NAME: &'static str = "Element 100";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9478;
    const MAX_STATE_ID: u32 = 9478;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 101
pub struct Element101;

impl BlockDef for Element101 {
    const ID: u32 = 10019;
    const STRING_ID: &'static str = "minecraft:element_101";
    const NAME: &'static str = "Element 101";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9479;
    const MAX_STATE_ID: u32 = 9479;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 102
pub struct Element102;

impl BlockDef for Element102 {
    const ID: u32 = 10020;
    const STRING_ID: &'static str = "minecraft:element_102";
    const NAME: &'static str = "Element 102";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9480;
    const MAX_STATE_ID: u32 = 9480;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 103
pub struct Element103;

impl BlockDef for Element103 {
    const ID: u32 = 10021;
    const STRING_ID: &'static str = "minecraft:element_103";
    const NAME: &'static str = "Element 103";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9481;
    const MAX_STATE_ID: u32 = 9481;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 104
pub struct Element104;

impl BlockDef for Element104 {
    const ID: u32 = 10022;
    const STRING_ID: &'static str = "minecraft:element_104";
    const NAME: &'static str = "Element 104";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9482;
    const MAX_STATE_ID: u32 = 9482;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 105
pub struct Element105;

impl BlockDef for Element105 {
    const ID: u32 = 10023;
    const STRING_ID: &'static str = "minecraft:element_105";
    const NAME: &'static str = "Element 105";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9483;
    const MAX_STATE_ID: u32 = 9483;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 106
pub struct Element106;

impl BlockDef for Element106 {
    const ID: u32 = 10024;
    const STRING_ID: &'static str = "minecraft:element_106";
    const NAME: &'static str = "Element 106";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9484;
    const MAX_STATE_ID: u32 = 9484;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 107
pub struct Element107;

impl BlockDef for Element107 {
    const ID: u32 = 10025;
    const STRING_ID: &'static str = "minecraft:element_107";
    const NAME: &'static str = "Element 107";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9485;
    const MAX_STATE_ID: u32 = 9485;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 108
pub struct Element108;

impl BlockDef for Element108 {
    const ID: u32 = 10026;
    const STRING_ID: &'static str = "minecraft:element_108";
    const NAME: &'static str = "Element 108";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9486;
    const MAX_STATE_ID: u32 = 9486;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 109
pub struct Element109;

impl BlockDef for Element109 {
    const ID: u32 = 10027;
    const STRING_ID: &'static str = "minecraft:element_109";
    const NAME: &'static str = "Element 109";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9487;
    const MAX_STATE_ID: u32 = 9487;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 113
pub struct Element113;

impl BlockDef for Element113 {
    const ID: u32 = 10028;
    const STRING_ID: &'static str = "minecraft:element_113";
    const NAME: &'static str = "Element 113";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9488;
    const MAX_STATE_ID: u32 = 9488;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 112
pub struct Element112;

impl BlockDef for Element112 {
    const ID: u32 = 10029;
    const STRING_ID: &'static str = "minecraft:element_112";
    const NAME: &'static str = "Element 112";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9489;
    const MAX_STATE_ID: u32 = 9489;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 111
pub struct Element111;

impl BlockDef for Element111 {
    const ID: u32 = 10030;
    const STRING_ID: &'static str = "minecraft:element_111";
    const NAME: &'static str = "Element 111";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9490;
    const MAX_STATE_ID: u32 = 9490;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 110
pub struct Element110;

impl BlockDef for Element110 {
    const ID: u32 = 10031;
    const STRING_ID: &'static str = "minecraft:element_110";
    const NAME: &'static str = "Element 110";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9491;
    const MAX_STATE_ID: u32 = 9491;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 117
pub struct Element117;

impl BlockDef for Element117 {
    const ID: u32 = 10032;
    const STRING_ID: &'static str = "minecraft:element_117";
    const NAME: &'static str = "Element 117";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9492;
    const MAX_STATE_ID: u32 = 9492;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 116
pub struct Element116;

impl BlockDef for Element116 {
    const ID: u32 = 10033;
    const STRING_ID: &'static str = "minecraft:element_116";
    const NAME: &'static str = "Element 116";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9493;
    const MAX_STATE_ID: u32 = 9493;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 115
pub struct Element115;

impl BlockDef for Element115 {
    const ID: u32 = 10034;
    const STRING_ID: &'static str = "minecraft:element_115";
    const NAME: &'static str = "Element 115";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9494;
    const MAX_STATE_ID: u32 = 9494;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 114
pub struct Element114;

impl BlockDef for Element114 {
    const ID: u32 = 10035;
    const STRING_ID: &'static str = "minecraft:element_114";
    const NAME: &'static str = "Element 114";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9495;
    const MAX_STATE_ID: u32 = 9495;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 118
pub struct Element118;

impl BlockDef for Element118 {
    const ID: u32 = 10036;
    const STRING_ID: &'static str = "minecraft:element_118";
    const NAME: &'static str = "Element 118";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9496;
    const MAX_STATE_ID: u32 = 9496;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Andesite Wall
pub struct AndesiteWall;

impl BlockDef for AndesiteWall {
    const ID: u32 = 832;
    const STRING_ID: &'static str = "minecraft:andesite_wall";
    const NAME: &'static str = "Andesite Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9497;
    const MAX_STATE_ID: u32 = 9658;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Glazed Terracotta
pub struct WhiteGlazedTerracotta;

impl BlockDef for WhiteGlazedTerracotta {
    const ID: u32 = 220;
    const STRING_ID: &'static str = "minecraft:white_glazed_terracotta";
    const NAME: &'static str = "White Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9659;
    const MAX_STATE_ID: u32 = 9664;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Warped Hyphae
pub struct StrippedWarpedHyphae;

impl BlockDef for StrippedWarpedHyphae {
    const ID: u32 = 865;
    const STRING_ID: &'static str = "minecraft:stripped_warped_hyphae";
    const NAME: &'static str = "Stripped Warped Hyphae";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9665;
    const MAX_STATE_ID: u32 = 9667;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Moving Piston
pub struct MovingBlock;

impl BlockDef for MovingBlock {
    const ID: u32 = 156;
    const STRING_ID: &'static str = "minecraft:moving_block";
    const NAME: &'static str = "Moving Piston";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9668;
    const MAX_STATE_ID: u32 = 9668;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Trapped Chest
pub struct TrappedChest;

impl BlockDef for TrappedChest {
    const ID: u32 = 146;
    const STRING_ID: &'static str = "minecraft:trapped_chest";
    const NAME: &'static str = "Trapped Chest";
    const HARDNESS: f32 = 2.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9669;
    const MAX_STATE_ID: u32 = 9672;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Trapdoor
pub struct AcaciaTrapdoor;

impl BlockDef for AcaciaTrapdoor {
    const ID: u32 = 320;
    const STRING_ID: &'static str = "minecraft:acacia_trapdoor";
    const NAME: &'static str = "Acacia Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9673;
    const MAX_STATE_ID: u32 = 9688;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper Chest
pub struct WeatheredCopperChest;

impl BlockDef for WeatheredCopperChest {
    const ID: u32 = 1110;
    const STRING_ID: &'static str = "minecraft:weathered_copper_chest";
    const NAME: &'static str = "Weathered Copper Chest";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9689;
    const MAX_STATE_ID: u32 = 9692;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brain Coral Block
pub struct BrainCoralBlock;

impl BlockDef for BrainCoralBlock {
    const ID: u32 = 754;
    const STRING_ID: &'static str = "minecraft:brain_coral_block";
    const NAME: &'static str = "Brain Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9693;
    const MAX_STATE_ID: u32 = 9693;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper Chain
pub struct WeatheredCopperChain;

impl BlockDef for WeatheredCopperChain {
    const ID: u32 = 353;
    const STRING_ID: &'static str = "minecraft:weathered_copper_chain";
    const NAME: &'static str = "Weathered Copper Chain";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9694;
    const MAX_STATE_ID: u32 = 9696;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Sign
pub struct StandingSign;

impl BlockDef for StandingSign {
    const ID: u32 = 63;
    const STRING_ID: &'static str = "minecraft:standing_sign";
    const NAME: &'static str = "Oak Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9697;
    const MAX_STATE_ID: u32 = 9712;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Planks
pub struct BambooPlanks;

impl BlockDef for BambooPlanks {
    const ID: u32 = 23;
    const STRING_ID: &'static str = "minecraft:bamboo_planks";
    const NAME: &'static str = "Bamboo Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9713;
    const MAX_STATE_ID: u32 = 9713;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Glow Lichen
pub struct GlowLichen;

impl BlockDef for GlowLichen {
    const ID: u32 = 367;
    const STRING_ID: &'static str = "minecraft:glow_lichen";
    const NAME: &'static str = "Glow Lichen";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9714;
    const MAX_STATE_ID: u32 = 9777;
    type State = super::states::MultiFaceState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purpur Pillar
pub struct PurpurPillar;

impl BlockDef for PurpurPillar {
    const ID: u32 = 659;
    const STRING_ID: &'static str = "minecraft:purpur_pillar";
    const NAME: &'static str = "Purpur Pillar";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9778;
    const MAX_STATE_ID: u32 = 9780;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Banner
pub struct WallBanner;

impl BlockDef for WallBanner {
    const ID: u32 = 177;
    const STRING_ID: &'static str = "minecraft:wall_banner";
    const NAME: &'static str = "Black Banner";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9781;
    const MAX_STATE_ID: u32 = 9786;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Twisting Vines Plant
pub struct TwistingVines;

impl BlockDef for TwistingVines {
    const ID: u32 = 881;
    const STRING_ID: &'static str = "minecraft:twisting_vines";
    const NAME: &'static str = "Twisting Vines Plant";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9787;
    const MAX_STATE_ID: u32 = 9812;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Copper
pub struct ChiseledCopper;

impl BlockDef for ChiseledCopper {
    const ID: u32 = 1052;
    const STRING_ID: &'static str = "minecraft:chiseled_copper";
    const NAME: &'static str = "Chiseled Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9813;
    const MAX_STATE_ID: u32 = 9813;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Slab
pub struct AcaciaDoubleSlab;

impl BlockDef for AcaciaDoubleSlab {
    const ID: u32 = 603;
    const STRING_ID: &'static str = "minecraft:acacia_double_slab";
    const NAME: &'static str = "Acacia Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9814;
    const MAX_STATE_ID: u32 = 9815;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Door
pub struct DarkOakDoor;

impl BlockDef for DarkOakDoor {
    const ID: u32 = 197;
    const STRING_ID: &'static str = "minecraft:dark_oak_door";
    const NAME: &'static str = "Dark Oak Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9816;
    const MAX_STATE_ID: u32 = 9847;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Fence
pub struct OakFence;

impl BlockDef for OakFence {
    const ID: u32 = 284;
    const STRING_ID: &'static str = "minecraft:oak_fence";
    const NAME: &'static str = "Oak Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9848;
    const MAX_STATE_ID: u32 = 9848;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Moss Block
pub struct PaleMossBlock;

impl BlockDef for PaleMossBlock {
    const ID: u32 = 1188;
    const STRING_ID: &'static str = "minecraft:pale_moss_block";
    const NAME: &'static str = "Pale Moss Block";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9849;
    const MAX_STATE_ID: u32 = 9849;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Soul Lantern
pub struct SoulLantern;

impl BlockDef for SoulLantern {
    const ID: u32 = 850;
    const STRING_ID: &'static str = "minecraft:soul_lantern";
    const NAME: &'static str = "Soul Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 10;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9850;
    const MAX_STATE_ID: u32 = 9851;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dirt
pub struct Dirt;

impl BlockDef for Dirt {
    const ID: u32 = 3;
    const STRING_ID: &'static str = "minecraft:dirt";
    const NAME: &'static str = "Dirt";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9852;
    const MAX_STATE_ID: u32 = 9852;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Stained Glass
pub struct BlueStainedGlass;

impl BlockDef for BlueStainedGlass {
    const ID: u32 = 311;
    const STRING_ID: &'static str = "minecraft:blue_stained_glass";
    const NAME: &'static str = "Blue Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9853;
    const MAX_STATE_ID: u32 = 9853;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deny
pub struct Deny;

impl BlockDef for Deny {
    const ID: u32 = 211;
    const STRING_ID: &'static str = "minecraft:deny";
    const NAME: &'static str = "Deny";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9854;
    const MAX_STATE_ID: u32 = 9854;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bee Nest
pub struct BeeNest;

impl BlockDef for BeeNest {
    const ID: u32 = 911;
    const STRING_ID: &'static str = "minecraft:bee_nest";
    const NAME: &'static str = "Bee Nest";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9855;
    const MAX_STATE_ID: u32 = 9878;
    type State = super::states::BeehiveState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bubble Column
pub struct BubbleColumn;

impl BlockDef for BubbleColumn {
    const ID: u32 = 796;
    const STRING_ID: &'static str = "minecraft:bubble_column";
    const NAME: &'static str = "Bubble Column";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 9879;
    const MAX_STATE_ID: u32 = 9880;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Campfire
pub struct Campfire;

impl BlockDef for Campfire {
    const ID: u32 = 859;
    const STRING_ID: &'static str = "minecraft:campfire";
    const NAME: &'static str = "Campfire";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9881;
    const MAX_STATE_ID: u32 = 9888;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Stone Slab
pub struct SmoothStoneDoubleSlab;

impl BlockDef for SmoothStoneDoubleSlab {
    const ID: u32 = 611;
    const STRING_ID: &'static str = "minecraft:smooth_stone_double_slab";
    const NAME: &'static str = "Smooth Stone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9889;
    const MAX_STATE_ID: u32 = 9890;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Stained Glass
pub struct LightBlueStainedGlass;

impl BlockDef for LightBlueStainedGlass {
    const ID: u32 = 303;
    const STRING_ID: &'static str = "minecraft:light_blue_stained_glass";
    const NAME: &'static str = "Light Blue Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9891;
    const MAX_STATE_ID: u32 = 9891;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Soul Soil
pub struct SoulSoil;

impl BlockDef for SoulSoil {
    const ID: u32 = 287;
    const STRING_ID: &'static str = "minecraft:soul_soil";
    const NAME: &'static str = "Soul Soil";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9892;
    const MAX_STATE_ID: u32 = 9892;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Soul Sand
pub struct SoulSand;

impl BlockDef for SoulSand {
    const ID: u32 = 88;
    const STRING_ID: &'static str = "minecraft:soul_sand";
    const NAME: &'static str = "Soul Sand";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 9893;
    const MAX_STATE_ID: u32 = 9893;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Granite Wall
pub struct GraniteWall;

impl BlockDef for GraniteWall {
    const ID: u32 = 828;
    const STRING_ID: &'static str = "minecraft:granite_wall";
    const NAME: &'static str = "Granite Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 9894;
    const MAX_STATE_ID: u32 = 10055;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Hanging Sign
pub struct SpruceHangingSign;

impl BlockDef for SpruceHangingSign {
    const ID: u32 = 235;
    const STRING_ID: &'static str = "minecraft:spruce_hanging_sign";
    const NAME: &'static str = "Spruce Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10056;
    const MAX_STATE_ID: u32 = 10439;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Diorite
pub struct PolishedDiorite;

impl BlockDef for PolishedDiorite {
    const ID: u32 = 5;
    const STRING_ID: &'static str = "minecraft:polished_diorite";
    const NAME: &'static str = "Polished Diorite";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10440;
    const MAX_STATE_ID: u32 = 10440;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Reinforced Deepslate
pub struct ReinforcedDeepslate;

impl BlockDef for ReinforcedDeepslate {
    const ID: u32 = 1182;
    const STRING_ID: &'static str = "minecraft:reinforced_deepslate";
    const NAME: &'static str = "Reinforced Deepslate";
    const HARDNESS: f32 = 55.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10441;
    const MAX_STATE_ID: u32 = 10441;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Fletching Table
pub struct FletchingTable;

impl BlockDef for FletchingTable {
    const ID: u32 = 843;
    const STRING_ID: &'static str = "minecraft:fletching_table";
    const NAME: &'static str = "Fletching Table";
    const HARDNESS: f32 = 2.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10442;
    const MAX_STATE_ID: u32 = 10442;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Leaves
pub struct CherryLeaves;

impl BlockDef for CherryLeaves {
    const ID: u32 = 93;
    const STRING_ID: &'static str = "minecraft:cherry_leaves";
    const NAME: &'static str = "Cherry Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 10443;
    const MAX_STATE_ID: u32 = 10446;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Creeper Head
pub struct CreeperHead;

impl BlockDef for CreeperHead {
    const ID: u32 = 461;
    const STRING_ID: &'static str = "minecraft:creeper_head";
    const NAME: &'static str = "Creeper Head";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10447;
    const MAX_STATE_ID: u32 = 10452;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Glazed Terracotta
pub struct BlackGlazedTerracotta;

impl BlockDef for BlackGlazedTerracotta {
    const ID: u32 = 235;
    const STRING_ID: &'static str = "minecraft:black_glazed_terracotta";
    const NAME: &'static str = "Black Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10453;
    const MAX_STATE_ID: u32 = 10458;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Cut Copper Stairs
pub struct WaxedOxidizedCutCopperStairs;

impl BlockDef for WaxedOxidizedCutCopperStairs {
    const ID: u32 = 1067;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_cut_copper_stairs";
    const NAME: &'static str = "Waxed Oxidized Cut Copper Stairs";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10459;
    const MAX_STATE_ID: u32 = 10466;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper Bulb
pub struct WaxedWeatheredCopperBulb;

impl BlockDef for WaxedWeatheredCopperBulb {
    const ID: u32 = 1106;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_bulb";
    const NAME: &'static str = "Waxed Weathered Copper Bulb";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10467;
    const MAX_STATE_ID: u32 = 10470;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dragon Head
pub struct DragonHead;

impl BlockDef for DragonHead {
    const ID: u32 = 463;
    const STRING_ID: &'static str = "minecraft:dragon_head";
    const NAME: &'static str = "Dragon Head";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10471;
    const MAX_STATE_ID: u32 = 10476;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper Bars
pub struct WaxedWeatheredCopperBars;

impl BlockDef for WaxedWeatheredCopperBars {
    const ID: u32 = 348;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_bars";
    const NAME: &'static str = "Waxed Weathered Copper Bars";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10477;
    const MAX_STATE_ID: u32 = 10477;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Calibrated Sculk Sensor
pub struct CalibratedSculkSensor;

impl BlockDef for CalibratedSculkSensor {
    const ID: u32 = 1029;
    const STRING_ID: &'static str = "minecraft:calibrated_sculk_sensor";
    const NAME: &'static str = "Calibrated Sculk Sensor";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 1;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10478;
    const MAX_STATE_ID: u32 = 10489;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Prismarine Slab
pub struct DarkPrismarineSlab;

impl BlockDef for DarkPrismarineSlab {
    const ID: u32 = 8029;
    const STRING_ID: &'static str = "minecraft:dark_prismarine_slab";
    const NAME: &'static str = "Dark Prismarine Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10490;
    const MAX_STATE_ID: u32 = 10491;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Trapdoor
pub struct CopperTrapdoor;

impl BlockDef for CopperTrapdoor {
    const ID: u32 = 1084;
    const STRING_ID: &'static str = "minecraft:copper_trapdoor";
    const NAME: &'static str = "Copper Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10492;
    const MAX_STATE_ID: u32 = 10507;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Acacia Log
pub struct StrippedAcaciaLog;

impl BlockDef for StrippedAcaciaLog {
    const ID: u32 = 64;
    const STRING_ID: &'static str = "minecraft:stripped_acacia_log";
    const NAME: &'static str = "Stripped Acacia Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10508;
    const MAX_STATE_ID: u32 = 10510;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobbled Deepslate Slab
pub struct CobbledDeepslateDoubleSlab;

impl BlockDef for CobbledDeepslateDoubleSlab {
    const ID: u32 = 8079;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate_double_slab";
    const NAME: &'static str = "Cobbled Deepslate Slab";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10511;
    const MAX_STATE_ID: u32 = 10512;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Fence
pub struct WarpedFence;

impl BlockDef for WarpedFence {
    const ID: u32 = 890;
    const STRING_ID: &'static str = "minecraft:warped_fence";
    const NAME: &'static str = "Warped Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10513;
    const MAX_STATE_ID: u32 = 10513;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crafting Table
pub struct CraftingTable;

impl BlockDef for CraftingTable {
    const ID: u32 = 58;
    const STRING_ID: &'static str = "minecraft:crafting_table";
    const NAME: &'static str = "Crafting Table";
    const HARDNESS: f32 = 2.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10514;
    const MAX_STATE_ID: u32 = 10514;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sea Pickle
pub struct SeaPickle;

impl BlockDef for SeaPickle {
    const ID: u32 = 788;
    const STRING_ID: &'static str = "minecraft:sea_pickle";
    const NAME: &'static str = "Sea Pickle";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 6;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 10515;
    const MAX_STATE_ID: u32 = 10522;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Sign
pub struct CherryStandingSign;

impl BlockDef for CherryStandingSign {
    const ID: u32 = 214;
    const STRING_ID: &'static str = "minecraft:cherry_standing_sign";
    const NAME: &'static str = "Cherry Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10523;
    const MAX_STATE_ID: u32 = 10538;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Shelf
pub struct PaleOakShelf;

impl BlockDef for PaleOakShelf {
    const ID: u32 = 189;
    const STRING_ID: &'static str = "minecraft:pale_oak_shelf";
    const NAME: &'static str = "Pale Oak Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10539;
    const MAX_STATE_ID: u32 = 10570;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Concrete Powder
pub struct BrownConcretePowder;

impl BlockDef for BrownConcretePowder {
    const ID: u32 = 738;
    const STRING_ID: &'static str = "minecraft:brown_concrete_powder";
    const NAME: &'static str = "Brown Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10571;
    const MAX_STATE_ID: u32 = 10571;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Hanging Sign
pub struct MangroveHangingSign;

impl BlockDef for MangroveHangingSign {
    const ID: u32 = 244;
    const STRING_ID: &'static str = "minecraft:mangrove_hanging_sign";
    const NAME: &'static str = "Mangrove Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10572;
    const MAX_STATE_ID: u32 = 10955;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper Trapdoor
pub struct WaxedExposedCopperTrapdoor;

impl BlockDef for WaxedExposedCopperTrapdoor {
    const ID: u32 = 1089;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_trapdoor";
    const NAME: &'static str = "Waxed Exposed Copper Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10956;
    const MAX_STATE_ID: u32 = 10971;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Candle
pub struct BrownCandle;

impl BlockDef for BrownCandle {
    const ID: u32 = 957;
    const STRING_ID: &'static str = "minecraft:brown_candle";
    const NAME: &'static str = "Brown Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10972;
    const MAX_STATE_ID: u32 = 10979;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Stone Brick Stairs
pub struct MossyStoneBrickStairs;

impl BlockDef for MossyStoneBrickStairs {
    const ID: u32 = 799;
    const STRING_ID: &'static str = "minecraft:mossy_stone_brick_stairs";
    const NAME: &'static str = "Mossy Stone Brick Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10980;
    const MAX_STATE_ID: u32 = 10987;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Rod
pub struct EndRod;

impl BlockDef for EndRod {
    const ID: u32 = 208;
    const STRING_ID: &'static str = "minecraft:end_rod";
    const NAME: &'static str = "End Rod";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 14;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10988;
    const MAX_STATE_ID: u32 = 10993;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Stem
pub struct CrimsonStem;

impl BlockDef for CrimsonStem {
    const ID: u32 = 871;
    const STRING_ID: &'static str = "minecraft:crimson_stem";
    const NAME: &'static str = "Crimson Stem";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10994;
    const MAX_STATE_ID: u32 = 10996;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Concrete
pub struct GreenConcrete;

impl BlockDef for GreenConcrete {
    const ID: u32 = 723;
    const STRING_ID: &'static str = "minecraft:green_concrete";
    const NAME: &'static str = "Green Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 10997;
    const MAX_STATE_ID: u32 = 10997;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff Brick Slab
pub struct TuffBrickDoubleSlab;

impl BlockDef for TuffBrickDoubleSlab {
    const ID: u32 = 8077;
    const STRING_ID: &'static str = "minecraft:tuff_brick_double_slab";
    const NAME: &'static str = "Tuff Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 10998;
    const MAX_STATE_ID: u32 = 10999;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Slab
pub struct CrimsonSlab;

impl BlockDef for CrimsonSlab {
    const ID: u32 = 8070;
    const STRING_ID: &'static str = "minecraft:crimson_slab";
    const NAME: &'static str = "Crimson Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11000;
    const MAX_STATE_ID: u32 = 11001;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Hyphae
pub struct WarpedHyphae;

impl BlockDef for WarpedHyphae {
    const ID: u32 = 864;
    const STRING_ID: &'static str = "minecraft:warped_hyphae";
    const NAME: &'static str = "Warped Hyphae";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11002;
    const MAX_STATE_ID: u32 = 11004;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Wart Block
pub struct WarpedWartBlock;

impl BlockDef for WarpedWartBlock {
    const ID: u32 = 868;
    const STRING_ID: &'static str = "minecraft:warped_wart_block";
    const NAME: &'static str = "Warped Wart Block";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11005;
    const MAX_STATE_ID: u32 = 11005;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Shulker Box
pub struct LightGrayShulkerBox;

impl BlockDef for LightGrayShulkerBox {
    const ID: u32 = 686;
    const STRING_ID: &'static str = "minecraft:light_gray_shulker_box";
    const NAME: &'static str = "Light Gray Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 11006;
    const MAX_STATE_ID: u32 = 11006;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Resin Bricks
pub struct ResinBricks;

impl BlockDef for ResinBricks {
    const ID: u32 = 376;
    const STRING_ID: &'static str = "minecraft:resin_bricks";
    const NAME: &'static str = "Resin Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11007;
    const MAX_STATE_ID: u32 = 11007;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Carrots
pub struct Carrots;

impl BlockDef for Carrots {
    const ID: u32 = 141;
    const STRING_ID: &'static str = "minecraft:carrots";
    const NAME: &'static str = "Carrots";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11008;
    const MAX_STATE_ID: u32 = 11015;
    type State = super::states::CropState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff Stairs
pub struct TuffStairs;

impl BlockDef for TuffStairs {
    const ID: u32 = 986;
    const STRING_ID: &'static str = "minecraft:tuff_stairs";
    const NAME: &'static str = "Tuff Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11016;
    const MAX_STATE_ID: u32 = 11023;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Carpet
pub struct YellowCarpet;

impl BlockDef for YellowCarpet {
    const ID: u32 = 542;
    const STRING_ID: &'static str = "minecraft:yellow_carpet";
    const NAME: &'static str = "Yellow Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11024;
    const MAX_STATE_ID: u32 = 11024;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cyan Stained Glass
pub struct CyanStainedGlass;

impl BlockDef for CyanStainedGlass {
    const ID: u32 = 309;
    const STRING_ID: &'static str = "minecraft:cyan_stained_glass";
    const NAME: &'static str = "Cyan Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11025;
    const MAX_STATE_ID: u32 = 11025;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Stained Glass
pub struct BlackStainedGlass;

impl BlockDef for BlackStainedGlass {
    const ID: u32 = 315;
    const STRING_ID: &'static str = "minecraft:black_stained_glass";
    const NAME: &'static str = "Black Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11026;
    const MAX_STATE_ID: u32 = 11026;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper Door
pub struct WaxedOxidizedCopperDoor;

impl BlockDef for WaxedOxidizedCopperDoor {
    const ID: u32 = 1083;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_door";
    const NAME: &'static str = "Waxed Oxidized Copper Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11027;
    const MAX_STATE_ID: u32 = 11058;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Horn Coral
pub struct DeadHornCoral;

impl BlockDef for DeadHornCoral {
    const ID: u32 = 762;
    const STRING_ID: &'static str = "minecraft:dead_horn_coral";
    const NAME: &'static str = "Dead Horn Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 11059;
    const MAX_STATE_ID: u32 = 11059;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Andesite Slab
pub struct AndesiteDoubleSlab;

impl BlockDef for AndesiteDoubleSlab {
    const ID: u32 = 820;
    const STRING_ID: &'static str = "minecraft:andesite_double_slab";
    const NAME: &'static str = "Andesite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11060;
    const MAX_STATE_ID: u32 = 11061;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Grass Block
pub struct GrassBlock;

impl BlockDef for GrassBlock {
    const ID: u32 = 8;
    const STRING_ID: &'static str = "minecraft:grass_block";
    const NAME: &'static str = "Grass Block";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11062;
    const MAX_STATE_ID: u32 = 11062;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tripwire Hook
pub struct TripwireHook;

impl BlockDef for TripwireHook {
    const ID: u32 = 131;
    const STRING_ID: &'static str = "minecraft:tripwire_hook";
    const NAME: &'static str = "Tripwire Hook";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11063;
    const MAX_STATE_ID: u32 = 11078;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cave Vines Plant
pub struct CaveVinesBodyWithBerries;

impl BlockDef for CaveVinesBodyWithBerries {
    const ID: u32 = 1136;
    const STRING_ID: &'static str = "minecraft:cave_vines_body_with_berries";
    const NAME: &'static str = "Cave Vines Plant";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11079;
    const MAX_STATE_ID: u32 = 11104;
    type State = super::states::GrowingPlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Pressure Plate
pub struct DarkOakPressurePlate;

impl BlockDef for DarkOakPressurePlate {
    const ID: u32 = 267;
    const STRING_ID: &'static str = "minecraft:dark_oak_pressure_plate";
    const NAME: &'static str = "Dark Oak Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11105;
    const MAX_STATE_ID: u32 = 11120;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Door
pub struct CopperDoor;

impl BlockDef for CopperDoor {
    const ID: u32 = 1076;
    const STRING_ID: &'static str = "minecraft:copper_door";
    const NAME: &'static str = "Copper Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11121;
    const MAX_STATE_ID: u32 = 11152;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Black Stained Glass
pub struct HardBlackStainedGlass;

impl BlockDef for HardBlackStainedGlass {
    const ID: u32 = 11699;
    const STRING_ID: &'static str = "minecraft:hard_black_stained_glass";
    const NAME: &'static str = "Hard Black Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11153;
    const MAX_STATE_ID: u32 = 11153;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Birch Log
pub struct StrippedBirchLog;

impl BlockDef for StrippedBirchLog {
    const ID: u32 = 62;
    const STRING_ID: &'static str = "minecraft:stripped_birch_log";
    const NAME: &'static str = "Stripped Birch Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11154;
    const MAX_STATE_ID: u32 = 11156;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tinted Glass
pub struct TintedGlass;

impl BlockDef for TintedGlass {
    const ID: u32 = 1026;
    const STRING_ID: &'static str = "minecraft:tinted_glass";
    const NAME: &'static str = "Tinted Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11157;
    const MAX_STATE_ID: u32 = 11157;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Big Dripleaf
pub struct BigDripleaf;

impl BlockDef for BigDripleaf {
    const ID: u32 = 1145;
    const STRING_ID: &'static str = "minecraft:big_dripleaf";
    const NAME: &'static str = "Big Dripleaf";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11158;
    const MAX_STATE_ID: u32 = 11189;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Sandstone
pub struct CutSandstone;

impl BlockDef for CutSandstone {
    const ID: u32 = 108;
    const STRING_ID: &'static str = "minecraft:cut_sandstone";
    const NAME: &'static str = "Cut Sandstone";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11190;
    const MAX_STATE_ID: u32 = 11190;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Hanging Sign
pub struct WarpedHangingSign;

impl BlockDef for WarpedHangingSign {
    const ID: u32 = 243;
    const STRING_ID: &'static str = "minecraft:warped_hanging_sign";
    const NAME: &'static str = "Warped Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11191;
    const MAX_STATE_ID: u32 = 11574;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Wool
pub struct LimeWool;

impl BlockDef for LimeWool {
    const ID: u32 = 145;
    const STRING_ID: &'static str = "minecraft:lime_wool";
    const NAME: &'static str = "Lime Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11575;
    const MAX_STATE_ID: u32 = 11575;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Blue Candle
pub struct BlueCandleCake;

impl BlockDef for BlueCandleCake {
    const ID: u32 = 973;
    const STRING_ID: &'static str = "minecraft:blue_candle_cake";
    const NAME: &'static str = "Cake with Blue Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11576;
    const MAX_STATE_ID: u32 = 11577;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sweet Berry Bush
pub struct SweetBerryBush;

impl BlockDef for SweetBerryBush {
    const ID: u32 = 861;
    const STRING_ID: &'static str = "minecraft:sweet_berry_bush";
    const NAME: &'static str = "Sweet Berry Bush";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11578;
    const MAX_STATE_ID: u32 = 11585;
    type State = super::states::CropState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Slab
pub struct PolishedBlackstoneSlab;

impl BlockDef for PolishedBlackstoneSlab {
    const ID: u32 = 8074;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_slab";
    const NAME: &'static str = "Polished Blackstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11586;
    const MAX_STATE_ID: u32 = 11587;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sugar Cane
pub struct Reeds;

impl BlockDef for Reeds {
    const ID: u32 = 83;
    const STRING_ID: &'static str = "minecraft:reeds";
    const NAME: &'static str = "Sugar Cane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11588;
    const MAX_STATE_ID: u32 = 11603;
    type State = super::states::AgeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Shulker Box
pub struct BlackShulkerBox;

impl BlockDef for BlackShulkerBox {
    const ID: u32 = 693;
    const STRING_ID: &'static str = "minecraft:black_shulker_box";
    const NAME: &'static str = "Black Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 11604;
    const MAX_STATE_ID: u32 = 11604;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper Golem Statue
pub struct WeatheredCopperGolemStatue;

impl BlockDef for WeatheredCopperGolemStatue {
    const ID: u32 = 1118;
    const STRING_ID: &'static str = "minecraft:weathered_copper_golem_statue";
    const NAME: &'static str = "Weathered Copper Golem Statue";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11605;
    const MAX_STATE_ID: u32 = 11608;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Sapling
pub struct JungleSapling;

impl BlockDef for JungleSapling {
    const ID: u32 = 28;
    const STRING_ID: &'static str = "minecraft:jungle_sapling";
    const NAME: &'static str = "Jungle Sapling";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11609;
    const MAX_STATE_ID: u32 = 11610;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Sandstone
pub struct ChiseledSandstone;

impl BlockDef for ChiseledSandstone {
    const ID: u32 = 107;
    const STRING_ID: &'static str = "minecraft:chiseled_sandstone";
    const NAME: &'static str = "Chiseled Sandstone";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11611;
    const MAX_STATE_ID: u32 = 11611;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Barrier
pub struct Barrier;

impl BlockDef for Barrier {
    const ID: u32 = 524;
    const STRING_ID: &'static str = "minecraft:barrier";
    const NAME: &'static str = "Barrier";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11612;
    const MAX_STATE_ID: u32 = 11612;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Torchflower Crop
pub struct TorchflowerCrop;

impl BlockDef for TorchflowerCrop {
    const ID: u32 = 662;
    const STRING_ID: &'static str = "minecraft:torchflower_crop";
    const NAME: &'static str = "Torchflower Crop";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11613;
    const MAX_STATE_ID: u32 = 11620;
    type State = super::states::CropState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Carpet
pub struct BlackCarpet;

impl BlockDef for BlackCarpet {
    const ID: u32 = 553;
    const STRING_ID: &'static str = "minecraft:black_carpet";
    const NAME: &'static str = "Black Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11621;
    const MAX_STATE_ID: u32 = 11621;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Log
pub struct PaleOakLog;

impl BlockDef for PaleOakLog {
    const ID: u32 = 56;
    const STRING_ID: &'static str = "minecraft:pale_oak_log";
    const NAME: &'static str = "Pale Oak Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11622;
    const MAX_STATE_ID: u32 = 11624;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Sign
pub struct JungleStandingSign;

impl BlockDef for JungleStandingSign {
    const ID: u32 = 215;
    const STRING_ID: &'static str = "minecraft:jungle_standing_sign";
    const NAME: &'static str = "Jungle Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11625;
    const MAX_STATE_ID: u32 = 11640;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Slab
pub struct CherryDoubleSlab;

impl BlockDef for CherryDoubleSlab {
    const ID: u32 = 604;
    const STRING_ID: &'static str = "minecraft:cherry_double_slab";
    const NAME: &'static str = "Cherry Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11641;
    const MAX_STATE_ID: u32 = 11642;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Cut Copper Slab
pub struct WeatheredCutCopperSlab;

impl BlockDef for WeatheredCutCopperSlab {
    const ID: u32 = 8078;
    const STRING_ID: &'static str = "minecraft:weathered_cut_copper_slab";
    const NAME: &'static str = "Weathered Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11643;
    const MAX_STATE_ID: u32 = 11644;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper Lantern
pub struct OxidizedCopperLantern;

impl BlockDef for OxidizedCopperLantern {
    const ID: u32 = 854;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_lantern";
    const NAME: &'static str = "Oxidized Copper Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11645;
    const MAX_STATE_ID: u32 = 11646;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Leaves
pub struct DarkOakLeaves;

impl BlockDef for DarkOakLeaves {
    const ID: u32 = 94;
    const STRING_ID: &'static str = "minecraft:dark_oak_leaves";
    const NAME: &'static str = "Dark Oak Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 11647;
    const MAX_STATE_ID: u32 = 11650;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Brick Slab
pub struct NetherBrickSlab;

impl BlockDef for NetherBrickSlab {
    const ID: u32 = 619;
    const STRING_ID: &'static str = "minecraft:nether_brick_slab";
    const NAME: &'static str = "Nether Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11651;
    const MAX_STATE_ID: u32 = 11652;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Fire
pub struct Fire;

impl BlockDef for Fire {
    const ID: u32 = 51;
    const STRING_ID: &'static str = "minecraft:fire";
    const NAME: &'static str = "Fire";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11653;
    const MAX_STATE_ID: u32 = 11668;
    type State = super::states::AgeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Fern
pub struct Fern;

impl BlockDef for Fern {
    const ID: u32 = 131;
    const STRING_ID: &'static str = "minecraft:fern";
    const NAME: &'static str = "Fern";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11669;
    const MAX_STATE_ID: u32 = 11669;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purpur Slab
pub struct PurpurDoubleSlab;

impl BlockDef for PurpurDoubleSlab {
    const ID: u32 = 8054;
    const STRING_ID: &'static str = "minecraft:purpur_double_slab";
    const NAME: &'static str = "Purpur Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11670;
    const MAX_STATE_ID: u32 = 11671;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Torchflower
pub struct Torchflower;

impl BlockDef for Torchflower {
    const ID: u32 = 159;
    const STRING_ID: &'static str = "minecraft:torchflower";
    const NAME: &'static str = "Torchflower";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11672;
    const MAX_STATE_ID: u32 = 11672;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Short Dry Grass
pub struct ShortDryGrass;

impl BlockDef for ShortDryGrass {
    const ID: u32 = 134;
    const STRING_ID: &'static str = "minecraft:short_dry_grass";
    const NAME: &'static str = "Short Dry Grass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11673;
    const MAX_STATE_ID: u32 = 11673;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Infested Stone
pub struct InfestedStone;

impl BlockDef for InfestedStone {
    const ID: u32 = 332;
    const STRING_ID: &'static str = "minecraft:infested_stone";
    const NAME: &'static str = "Infested Stone";
    const HARDNESS: f32 = 0.75_f32;
    const RESISTANCE: f32 = 0.75_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11674;
    const MAX_STATE_ID: u32 = 11674;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Hanging Moss
pub struct PaleHangingMoss;

impl BlockDef for PaleHangingMoss {
    const ID: u32 = 1190;
    const STRING_ID: &'static str = "minecraft:pale_hanging_moss";
    const NAME: &'static str = "Pale Hanging Moss";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11675;
    const MAX_STATE_ID: u32 = 11676;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Moss Carpet
pub struct PaleMossCarpet;

impl BlockDef for PaleMossCarpet {
    const ID: u32 = 1189;
    const STRING_ID: &'static str = "minecraft:pale_moss_carpet";
    const NAME: &'static str = "Pale Moss Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11677;
    const MAX_STATE_ID: u32 = 11838;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Portal Frame
pub struct EndPortalFrame;

impl BlockDef for EndPortalFrame {
    const ID: u32 = 120;
    const STRING_ID: &'static str = "minecraft:end_portal_frame";
    const NAME: &'static str = "End Portal Frame";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 1;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11839;
    const MAX_STATE_ID: u32 = 11846;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Pressure Plate
pub struct BambooPressurePlate;

impl BlockDef for BambooPressurePlate {
    const ID: u32 = 270;
    const STRING_ID: &'static str = "minecraft:bamboo_pressure_plate";
    const NAME: &'static str = "Bamboo Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11847;
    const MAX_STATE_ID: u32 = 11862;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Prismarine
pub struct Prismarine;

impl BlockDef for Prismarine {
    const ID: u32 = 168;
    const STRING_ID: &'static str = "minecraft:prismarine";
    const NAME: &'static str = "Prismarine";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11863;
    const MAX_STATE_ID: u32 = 11863;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Magenta Candle
pub struct MagentaCandleCake;

impl BlockDef for MagentaCandleCake {
    const ID: u32 = 964;
    const STRING_ID: &'static str = "minecraft:magenta_candle_cake";
    const NAME: &'static str = "Cake with Magenta Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11864;
    const MAX_STATE_ID: u32 = 11865;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper Trapdoor
pub struct ExposedCopperTrapdoor;

impl BlockDef for ExposedCopperTrapdoor {
    const ID: u32 = 1085;
    const STRING_ID: &'static str = "minecraft:exposed_copper_trapdoor";
    const NAME: &'static str = "Exposed Copper Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11866;
    const MAX_STATE_ID: u32 = 11881;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mushroom Stem
pub struct MushroomStem;

impl BlockDef for MushroomStem {
    const ID: u32 = 340;
    const STRING_ID: &'static str = "minecraft:mushroom_stem";
    const NAME: &'static str = "Mushroom Stem";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11882;
    const MAX_STATE_ID: u32 = 11897;
    type State = super::states::MushroomState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Terracotta
pub struct BlackTerracotta;

impl BlockDef for BlackTerracotta {
    const ID: u32 = 499;
    const STRING_ID: &'static str = "minecraft:black_terracotta";
    const NAME: &'static str = "Black Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11898;
    const MAX_STATE_ID: u32 = 11898;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Resin Brick Stairs
pub struct ResinBrickStairs;

impl BlockDef for ResinBrickStairs {
    const ID: u32 = 377;
    const STRING_ID: &'static str = "minecraft:resin_brick_stairs";
    const NAME: &'static str = "Resin Brick Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11899;
    const MAX_STATE_ID: u32 = 11906;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Gold Ore
pub struct DeepslateGoldOre;

impl BlockDef for DeepslateGoldOre {
    const ID: u32 = 43;
    const STRING_ID: &'static str = "minecraft:deepslate_gold_ore";
    const NAME: &'static str = "Deepslate Gold Ore";
    const HARDNESS: f32 = 4.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11907;
    const MAX_STATE_ID: u32 = 11907;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Ancient Debris
pub struct AncientDebris;

impl BlockDef for AncientDebris {
    const ID: u32 = 916;
    const STRING_ID: &'static str = "minecraft:ancient_debris";
    const NAME: &'static str = "Ancient Debris";
    const HARDNESS: f32 = 30.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11908;
    const MAX_STATE_ID: u32 = 11908;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Vault
pub struct Vault;

impl BlockDef for Vault {
    const ID: u32 = 1186;
    const STRING_ID: &'static str = "minecraft:vault";
    const NAME: &'static str = "Vault";
    const HARDNESS: f32 = 50.0_f32;
    const RESISTANCE: f32 = 50.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 6;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 11909;
    const MAX_STATE_ID: u32 = 11940;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Beehive
pub struct Beehive;

impl BlockDef for Beehive {
    const ID: u32 = 912;
    const STRING_ID: &'static str = "minecraft:beehive";
    const NAME: &'static str = "Beehive";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 11941;
    const MAX_STATE_ID: u32 = 11964;
    type State = super::states::BeehiveState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Orange Stained Glass
pub struct HardOrangeStainedGlass;

impl BlockDef for HardOrangeStainedGlass {
    const ID: u32 = 12519;
    const STRING_ID: &'static str = "minecraft:hard_orange_stained_glass";
    const NAME: &'static str = "Hard Orange Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11965;
    const MAX_STATE_ID: u32 = 11965;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Door
pub struct JungleDoor;

impl BlockDef for JungleDoor {
    const ID: u32 = 195;
    const STRING_ID: &'static str = "minecraft:jungle_door";
    const NAME: &'static str = "Jungle Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11966;
    const MAX_STATE_ID: u32 = 11997;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Glass
pub struct Glass;

impl BlockDef for Glass {
    const ID: u32 = 20;
    const STRING_ID: &'static str = "minecraft:glass";
    const NAME: &'static str = "Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11998;
    const MAX_STATE_ID: u32 = 11998;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Wither Rose
pub struct WitherRose;

impl BlockDef for WitherRose {
    const ID: u32 = 170;
    const STRING_ID: &'static str = "minecraft:wither_rose";
    const NAME: &'static str = "Wither Rose";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 11999;
    const MAX_STATE_ID: u32 = 11999;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Brick Slab
pub struct NetherBrickDoubleSlab;

impl BlockDef for NetherBrickDoubleSlab {
    const ID: u32 = 8050;
    const STRING_ID: &'static str = "minecraft:nether_brick_double_slab";
    const NAME: &'static str = "Nether Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12000;
    const MAX_STATE_ID: u32 = 12001;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Cut Copper
pub struct ExposedCutCopper;

impl BlockDef for ExposedCutCopper {
    const ID: u32 = 1045;
    const STRING_ID: &'static str = "minecraft:exposed_cut_copper";
    const NAME: &'static str = "Exposed Cut Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12002;
    const MAX_STATE_ID: u32 = 12002;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Cut Copper Stairs
pub struct WaxedWeatheredCutCopperStairs;

impl BlockDef for WaxedWeatheredCutCopperStairs {
    const ID: u32 = 1066;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_cut_copper_stairs";
    const NAME: &'static str = "Waxed Weathered Cut Copper Stairs";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12003;
    const MAX_STATE_ID: u32 = 12010;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Roots
pub struct MangroveRoots;

impl BlockDef for MangroveRoots {
    const ID: u32 = 58;
    const STRING_ID: &'static str = "minecraft:mangrove_roots";
    const NAME: &'static str = "Mangrove Roots";
    const HARDNESS: f32 = 0.7_f32;
    const RESISTANCE: f32 = 0.7_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12011;
    const MAX_STATE_ID: u32 = 12011;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Candle
pub struct YellowCandle;

impl BlockDef for YellowCandle {
    const ID: u32 = 949;
    const STRING_ID: &'static str = "minecraft:yellow_candle";
    const NAME: &'static str = "Yellow Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12012;
    const MAX_STATE_ID: u32 = 12019;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Stairs
pub struct AcaciaStairs;

impl BlockDef for AcaciaStairs {
    const ID: u32 = 163;
    const STRING_ID: &'static str = "minecraft:acacia_stairs";
    const NAME: &'static str = "Acacia Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12020;
    const MAX_STATE_ID: u32 = 12027;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Mosaic Stairs
pub struct BambooMosaicStairs;

impl BlockDef for BambooMosaicStairs {
    const ID: u32 = 522;
    const STRING_ID: &'static str = "minecraft:bamboo_mosaic_stairs";
    const NAME: &'static str = "Bamboo Mosaic Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12028;
    const MAX_STATE_ID: u32 = 12035;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Concrete
pub struct BrownConcrete;

impl BlockDef for BrownConcrete {
    const ID: u32 = 722;
    const STRING_ID: &'static str = "minecraft:brown_concrete";
    const NAME: &'static str = "Brown Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12036;
    const MAX_STATE_ID: u32 = 12036;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Slab
pub struct CherrySlab;

impl BlockDef for CherrySlab {
    const ID: u32 = 8035;
    const STRING_ID: &'static str = "minecraft:cherry_slab";
    const NAME: &'static str = "Cherry Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12037;
    const MAX_STATE_ID: u32 = 12038;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Resin Bricks
pub struct ChiseledResinBricks;

impl BlockDef for ChiseledResinBricks {
    const ID: u32 = 380;
    const STRING_ID: &'static str = "minecraft:chiseled_resin_bricks";
    const NAME: &'static str = "Chiseled Resin Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12039;
    const MAX_STATE_ID: u32 = 12039;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bubble Coral
pub struct BubbleCoral;

impl BlockDef for BubbleCoral {
    const ID: u32 = 765;
    const STRING_ID: &'static str = "minecraft:bubble_coral";
    const NAME: &'static str = "Bubble Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12040;
    const MAX_STATE_ID: u32 = 12040;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Shulker Box
pub struct OrangeShulkerBox;

impl BlockDef for OrangeShulkerBox {
    const ID: u32 = 679;
    const STRING_ID: &'static str = "minecraft:orange_shulker_box";
    const NAME: &'static str = "Orange Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12041;
    const MAX_STATE_ID: u32 = 12041;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Candle
pub struct LightGrayCandle;

impl BlockDef for LightGrayCandle {
    const ID: u32 = 953;
    const STRING_ID: &'static str = "minecraft:light_gray_candle";
    const NAME: &'static str = "Light Gray Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12042;
    const MAX_STATE_ID: u32 = 12049;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Pressure Plate
pub struct PolishedBlackstonePressurePlate;

impl BlockDef for PolishedBlackstonePressurePlate {
    const ID: u32 = 938;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_pressure_plate";
    const NAME: &'static str = "Polished Blackstone Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12050;
    const MAX_STATE_ID: u32 = 12065;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Sign
pub struct AcaciaStandingSign;

impl BlockDef for AcaciaStandingSign {
    const ID: u32 = 213;
    const STRING_ID: &'static str = "minecraft:acacia_standing_sign";
    const NAME: &'static str = "Acacia Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12066;
    const MAX_STATE_ID: u32 = 12081;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Granite Slab
pub struct PolishedGraniteSlab;

impl BlockDef for PolishedGraniteSlab {
    const ID: u32 = 811;
    const STRING_ID: &'static str = "minecraft:polished_granite_slab";
    const NAME: &'static str = "Polished Granite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12082;
    const MAX_STATE_ID: u32 = 12083;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Red Sandstone Slab
pub struct SmoothRedSandstoneDoubleSlab;

impl BlockDef for SmoothRedSandstoneDoubleSlab {
    const ID: u32 = 812;
    const STRING_ID: &'static str = "minecraft:smooth_red_sandstone_double_slab";
    const NAME: &'static str = "Smooth Red Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12084;
    const MAX_STATE_ID: u32 = 12085;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff Brick Stairs
pub struct TuffBrickStairs;

impl BlockDef for TuffBrickStairs {
    const ID: u32 = 995;
    const STRING_ID: &'static str = "minecraft:tuff_brick_stairs";
    const NAME: &'static str = "Tuff Brick Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12086;
    const MAX_STATE_ID: u32 = 12093;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Shulker Box
pub struct BlueShulkerBox;

impl BlockDef for BlueShulkerBox {
    const ID: u32 = 689;
    const STRING_ID: &'static str = "minecraft:blue_shulker_box";
    const NAME: &'static str = "Blue Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12094;
    const MAX_STATE_ID: u32 = 12094;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper Bulb
pub struct ExposedCopperBulb;

impl BlockDef for ExposedCopperBulb {
    const ID: u32 = 1101;
    const STRING_ID: &'static str = "minecraft:exposed_copper_bulb";
    const NAME: &'static str = "Exposed Copper Bulb";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12095;
    const MAX_STATE_ID: u32 = 12098;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper Bars
pub struct ExposedCopperBars;

impl BlockDef for ExposedCopperBars {
    const ID: u32 = 343;
    const STRING_ID: &'static str = "minecraft:exposed_copper_bars";
    const NAME: &'static str = "Exposed Copper Bars";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12099;
    const MAX_STATE_ID: u32 = 12099;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Fire Coral
pub struct DeadFireCoral;

impl BlockDef for DeadFireCoral {
    const ID: u32 = 761;
    const STRING_ID: &'static str = "minecraft:dead_fire_coral";
    const NAME: &'static str = "Dead Fire Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12100;
    const MAX_STATE_ID: u32 = 12100;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stone Brick Slab
pub struct StoneBrickSlab;

impl BlockDef for StoneBrickSlab {
    const ID: u32 = 8048;
    const STRING_ID: &'static str = "minecraft:stone_brick_slab";
    const NAME: &'static str = "Stone Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12101;
    const MAX_STATE_ID: u32 = 12102;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Stairs
pub struct CrimsonStairs;

impl BlockDef for CrimsonStairs {
    const ID: u32 = 895;
    const STRING_ID: &'static str = "minecraft:crimson_stairs";
    const NAME: &'static str = "Crimson Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12103;
    const MAX_STATE_ID: u32 = 12110;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper Bars
pub struct WaxedOxidizedCopperBars;

impl BlockDef for WaxedOxidizedCopperBars {
    const ID: u32 = 349;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_bars";
    const NAME: &'static str = "Waxed Oxidized Copper Bars";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12111;
    const MAX_STATE_ID: u32 = 12111;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Spruce Log
pub struct StrippedSpruceLog;

impl BlockDef for StrippedSpruceLog {
    const ID: u32 = 61;
    const STRING_ID: &'static str = "minecraft:stripped_spruce_log";
    const NAME: &'static str = "Stripped Spruce Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12112;
    const MAX_STATE_ID: u32 = 12114;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper Bulb
pub struct WaxedOxidizedCopperBulb;

impl BlockDef for WaxedOxidizedCopperBulb {
    const ID: u32 = 1107;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_bulb";
    const NAME: &'static str = "Waxed Oxidized Copper Bulb";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12115;
    const MAX_STATE_ID: u32 = 12118;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Attached Pumpkin Stem
pub struct PumpkinStem;

impl BlockDef for PumpkinStem {
    const ID: u32 = 104;
    const STRING_ID: &'static str = "minecraft:pumpkin_stem";
    const NAME: &'static str = "Attached Pumpkin Stem";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12119;
    const MAX_STATE_ID: u32 = 12166;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Flowering Azalea Leaves
pub struct AzaleaLeavesFlowered;

impl BlockDef for AzaleaLeavesFlowered {
    const ID: u32 = 98;
    const STRING_ID: &'static str = "minecraft:azalea_leaves_flowered";
    const NAME: &'static str = "Flowering Azalea Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12167;
    const MAX_STATE_ID: u32 = 12170;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Magenta Stained Glass Pane
pub struct HardMagentaStainedGlassPane;

impl BlockDef for HardMagentaStainedGlassPane {
    const ID: u32 = 12726;
    const STRING_ID: &'static str = "minecraft:hard_magenta_stained_glass_pane";
    const NAME: &'static str = "Hard Magenta Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12171;
    const MAX_STATE_ID: u32 = 12171;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Piston Head
pub struct StickyPistonArmCollision;

impl BlockDef for StickyPistonArmCollision {
    const ID: u32 = 8002;
    const STRING_ID: &'static str = "minecraft:sticky_piston_arm_collision";
    const NAME: &'static str = "Piston Head";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12172;
    const MAX_STATE_ID: u32 = 12177;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Nylium
pub struct WarpedNylium;

impl BlockDef for WarpedNylium {
    const ID: u32 = 866;
    const STRING_ID: &'static str = "minecraft:warped_nylium";
    const NAME: &'static str = "Warped Nylium";
    const HARDNESS: f32 = 0.4_f32;
    const RESISTANCE: f32 = 0.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12178;
    const MAX_STATE_ID: u32 = 12178;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Emerald Ore
pub struct DeepslateEmeraldOre;

impl BlockDef for DeepslateEmeraldOre {
    const ID: u32 = 399;
    const STRING_ID: &'static str = "minecraft:deepslate_emerald_ore";
    const NAME: &'static str = "Deepslate Emerald Ore";
    const HARDNESS: f32 = 4.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12179;
    const MAX_STATE_ID: u32 = 12179;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Sapling
pub struct AcaciaSapling;

impl BlockDef for AcaciaSapling {
    const ID: u32 = 29;
    const STRING_ID: &'static str = "minecraft:acacia_sapling";
    const NAME: &'static str = "Acacia Sapling";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12180;
    const MAX_STATE_ID: u32 = 12181;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Quartz Bricks
pub struct QuartzBricks;

impl BlockDef for QuartzBricks {
    const ID: u32 = 943;
    const STRING_ID: &'static str = "minecraft:quartz_bricks";
    const NAME: &'static str = "Quartz Bricks";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12182;
    const MAX_STATE_ID: u32 = 12182;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Andesite Slab
pub struct AndesiteSlab;

impl BlockDef for AndesiteSlab {
    const ID: u32 = 8064;
    const STRING_ID: &'static str = "minecraft:andesite_slab";
    const NAME: &'static str = "Andesite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12183;
    const MAX_STATE_ID: u32 = 12184;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Comparator
pub struct UnpoweredComparator;

impl BlockDef for UnpoweredComparator {
    const ID: u32 = 149;
    const STRING_ID: &'static str = "minecraft:unpowered_comparator";
    const NAME: &'static str = "Redstone Comparator";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12185;
    const MAX_STATE_ID: u32 = 12200;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Candle
pub struct LimeCandle;

impl BlockDef for LimeCandle {
    const ID: u32 = 950;
    const STRING_ID: &'static str = "minecraft:lime_candle";
    const NAME: &'static str = "Lime Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12201;
    const MAX_STATE_ID: u32 = 12208;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Structure Block
pub struct StructureBlock;

impl BlockDef for StructureBlock {
    const ID: u32 = 252;
    const STRING_ID: &'static str = "minecraft:structure_block";
    const NAME: &'static str = "Structure Block";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12209;
    const MAX_STATE_ID: u32 = 12214;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Stone Brick Stairs
pub struct EndBrickStairs;

impl BlockDef for EndBrickStairs {
    const ID: u32 = 802;
    const STRING_ID: &'static str = "minecraft:end_brick_stairs";
    const NAME: &'static str = "End Stone Brick Stairs";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 9.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12215;
    const MAX_STATE_ID: u32 = 12222;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Terracotta
pub struct PurpleTerracotta;

impl BlockDef for PurpleTerracotta {
    const ID: u32 = 494;
    const STRING_ID: &'static str = "minecraft:purple_terracotta";
    const NAME: &'static str = "Purple Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12223;
    const MAX_STATE_ID: u32 = 12223;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Target
pub struct Target;

impl BlockDef for Target {
    const ID: u32 = 910;
    const STRING_ID: &'static str = "minecraft:target";
    const NAME: &'static str = "Target";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12224;
    const MAX_STATE_ID: u32 = 12224;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Button
pub struct WoodenButton;

impl BlockDef for WoodenButton {
    const ID: u32 = 143;
    const STRING_ID: &'static str = "minecraft:wooden_button";
    const NAME: &'static str = "Oak Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12225;
    const MAX_STATE_ID: u32 = 12236;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Door
pub struct MangroveDoor;

impl BlockDef for MangroveDoor {
    const ID: u32 = 653;
    const STRING_ID: &'static str = "minecraft:mangrove_door";
    const NAME: &'static str = "Mangrove Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12237;
    const MAX_STATE_ID: u32 = 12268;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Stone Brick Slab
pub struct EndStoneBrickDoubleSlab;

impl BlockDef for EndStoneBrickDoubleSlab {
    const ID: u32 = 8060;
    const STRING_ID: &'static str = "minecraft:end_stone_brick_double_slab";
    const NAME: &'static str = "End Stone Brick Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 9.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12269;
    const MAX_STATE_ID: u32 = 12270;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Lime Stained Glass
pub struct HardLimeStainedGlass;

impl BlockDef for HardLimeStainedGlass {
    const ID: u32 = 12827;
    const STRING_ID: &'static str = "minecraft:hard_lime_stained_glass";
    const NAME: &'static str = "Hard Lime Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12271;
    const MAX_STATE_ID: u32 = 12271;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper Door
pub struct WeatheredCopperDoor;

impl BlockDef for WeatheredCopperDoor {
    const ID: u32 = 1078;
    const STRING_ID: &'static str = "minecraft:weathered_copper_door";
    const NAME: &'static str = "Weathered Copper Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12272;
    const MAX_STATE_ID: u32 = 12303;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pearlescent Froglight
pub struct PearlescentFroglight;

impl BlockDef for PearlescentFroglight {
    const ID: u32 = 1180;
    const STRING_ID: &'static str = "minecraft:pearlescent_froglight";
    const NAME: &'static str = "Pearlescent Froglight";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12304;
    const MAX_STATE_ID: u32 = 12306;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Button
pub struct BambooButton;

impl BlockDef for BambooButton {
    const ID: u32 = 452;
    const STRING_ID: &'static str = "minecraft:bamboo_button";
    const NAME: &'static str = "Bamboo Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12307;
    const MAX_STATE_ID: u32 = 12318;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tall Grass
pub struct TallGrass;

impl BlockDef for TallGrass {
    const ID: u32 = 561;
    const STRING_ID: &'static str = "minecraft:tall_grass";
    const NAME: &'static str = "Tall Grass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12319;
    const MAX_STATE_ID: u32 = 12320;
    type State = super::states::DoublePlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper Lantern
pub struct WeatheredCopperLantern;

impl BlockDef for WeatheredCopperLantern {
    const ID: u32 = 853;
    const STRING_ID: &'static str = "minecraft:weathered_copper_lantern";
    const NAME: &'static str = "Weathered Copper Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12321;
    const MAX_STATE_ID: u32 = 12322;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock12;

impl BlockDef for LightBlock12 {
    const ID: u32 = 8021;
    const STRING_ID: &'static str = "minecraft:light_block_12";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12323;
    const MAX_STATE_ID: u32 = 12323;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock13;

impl BlockDef for LightBlock13 {
    const ID: u32 = 8022;
    const STRING_ID: &'static str = "minecraft:light_block_13";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12324;
    const MAX_STATE_ID: u32 = 12324;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock10;

impl BlockDef for LightBlock10 {
    const ID: u32 = 8023;
    const STRING_ID: &'static str = "minecraft:light_block_10";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12325;
    const MAX_STATE_ID: u32 = 12325;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock11;

impl BlockDef for LightBlock11 {
    const ID: u32 = 8024;
    const STRING_ID: &'static str = "minecraft:light_block_11";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12326;
    const MAX_STATE_ID: u32 = 12326;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock14;

impl BlockDef for LightBlock14 {
    const ID: u32 = 8025;
    const STRING_ID: &'static str = "minecraft:light_block_14";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12327;
    const MAX_STATE_ID: u32 = 12327;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light
pub struct LightBlock15;

impl BlockDef for LightBlock15 {
    const ID: u32 = 8026;
    const STRING_ID: &'static str = "minecraft:light_block_15";
    const NAME: &'static str = "Light";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12328;
    const MAX_STATE_ID: u32 = 12328;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Sprouts
pub struct NetherSprouts;

impl BlockDef for NetherSprouts {
    const ID: u32 = 870;
    const STRING_ID: &'static str = "minecraft:nether_sprouts";
    const NAME: &'static str = "Nether Sprouts";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12329;
    const MAX_STATE_ID: u32 = 12329;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cyan Stained Glass Pane
pub struct CyanStainedGlassPane;

impl BlockDef for CyanStainedGlassPane {
    const ID: u32 = 509;
    const STRING_ID: &'static str = "minecraft:cyan_stained_glass_pane";
    const NAME: &'static str = "Cyan Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12330;
    const MAX_STATE_ID: u32 = 12330;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Horn Coral Block
pub struct DeadHornCoralBlock;

impl BlockDef for DeadHornCoralBlock {
    const ID: u32 = 752;
    const STRING_ID: &'static str = "minecraft:dead_horn_coral_block";
    const NAME: &'static str = "Dead Horn Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12331;
    const MAX_STATE_ID: u32 = 12331;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Verdant Froglight
pub struct VerdantFroglight;

impl BlockDef for VerdantFroglight {
    const ID: u32 = 1179;
    const STRING_ID: &'static str = "minecraft:verdant_froglight";
    const NAME: &'static str = "Verdant Froglight";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12332;
    const MAX_STATE_ID: u32 = 12334;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Gray Stained Glass Pane
pub struct HardGrayStainedGlassPane;

impl BlockDef for HardGrayStainedGlassPane {
    const ID: u32 = 12891;
    const STRING_ID: &'static str = "minecraft:hard_gray_stained_glass_pane";
    const NAME: &'static str = "Hard Gray Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12335;
    const MAX_STATE_ID: u32 = 12335;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Resin
pub struct ResinBlock;

impl BlockDef for ResinBlock {
    const ID: u32 = 375;
    const STRING_ID: &'static str = "minecraft:resin_block";
    const NAME: &'static str = "Block of Resin";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12336;
    const MAX_STATE_ID: u32 = 12336;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Slab
pub struct WarpedSlab;

impl BlockDef for WarpedSlab {
    const ID: u32 = 8071;
    const STRING_ID: &'static str = "minecraft:warped_slab";
    const NAME: &'static str = "Warped Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12337;
    const MAX_STATE_ID: u32 = 12338;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Stem
pub struct WarpedStem;

impl BlockDef for WarpedStem {
    const ID: u32 = 862;
    const STRING_ID: &'static str = "minecraft:warped_stem";
    const NAME: &'static str = "Warped Stem";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12339;
    const MAX_STATE_ID: u32 = 12341;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Horn Coral Fan
pub struct HornCoralFan;

impl BlockDef for HornCoralFan {
    const ID: u32 = 777;
    const STRING_ID: &'static str = "minecraft:horn_coral_fan";
    const NAME: &'static str = "Horn Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12342;
    const MAX_STATE_ID: u32 = 12343;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Shulker Box
pub struct GreenShulkerBox;

impl BlockDef for GreenShulkerBox {
    const ID: u32 = 691;
    const STRING_ID: &'static str = "minecraft:green_shulker_box";
    const NAME: &'static str = "Green Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12344;
    const MAX_STATE_ID: u32 = 12344;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Large Fern
pub struct LargeFern;

impl BlockDef for LargeFern {
    const ID: u32 = 562;
    const STRING_ID: &'static str = "minecraft:large_fern";
    const NAME: &'static str = "Large Fern";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12345;
    const MAX_STATE_ID: u32 = 12346;
    type State = super::states::DoublePlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Crimson Hyphae
pub struct StrippedCrimsonHyphae;

impl BlockDef for StrippedCrimsonHyphae {
    const ID: u32 = 874;
    const STRING_ID: &'static str = "minecraft:stripped_crimson_hyphae";
    const NAME: &'static str = "Stripped Crimson Hyphae";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12347;
    const MAX_STATE_ID: u32 = 12349;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cocoa
pub struct Cocoa;

impl BlockDef for Cocoa {
    const ID: u32 = 127;
    const STRING_ID: &'static str = "minecraft:cocoa";
    const NAME: &'static str = "Cocoa";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12350;
    const MAX_STATE_ID: u32 = 12361;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lever
pub struct Lever;

impl BlockDef for Lever {
    const ID: u32 = 69;
    const STRING_ID: &'static str = "minecraft:lever";
    const NAME: &'static str = "Lever";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12362;
    const MAX_STATE_ID: u32 = 12377;
    type State = super::states::LeverState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Slab
pub struct BambooSlab;

impl BlockDef for BambooSlab {
    const ID: u32 = 8039;
    const STRING_ID: &'static str = "minecraft:bamboo_slab";
    const NAME: &'static str = "Bamboo Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12378;
    const MAX_STATE_ID: u32 = 12379;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Green Stained Glass
pub struct HardGreenStainedGlass;

impl BlockDef for HardGreenStainedGlass {
    const ID: u32 = 12944;
    const STRING_ID: &'static str = "minecraft:hard_green_stained_glass";
    const NAME: &'static str = "Hard Green Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12380;
    const MAX_STATE_ID: u32 = 12380;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brick Stairs
pub struct BrickStairs;

impl BlockDef for BrickStairs {
    const ID: u32 = 108;
    const STRING_ID: &'static str = "minecraft:brick_stairs";
    const NAME: &'static str = "Brick Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12381;
    const MAX_STATE_ID: u32 = 12388;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Colored Torch Green
pub struct ColoredTorchGreen;

impl BlockDef for ColoredTorchGreen {
    const ID: u32 = 12953;
    const STRING_ID: &'static str = "minecraft:colored_torch_green";
    const NAME: &'static str = "Colored Torch Green";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12389;
    const MAX_STATE_ID: u32 = 12394;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper Trapdoor
pub struct WeatheredCopperTrapdoor;

impl BlockDef for WeatheredCopperTrapdoor {
    const ID: u32 = 1086;
    const STRING_ID: &'static str = "minecraft:weathered_copper_trapdoor";
    const NAME: &'static str = "Weathered Copper Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12395;
    const MAX_STATE_ID: u32 = 12410;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Red Sandstone Slab
pub struct SmoothRedSandstoneSlab;

impl BlockDef for SmoothRedSandstoneSlab {
    const ID: u32 = 8056;
    const STRING_ID: &'static str = "minecraft:smooth_red_sandstone_slab";
    const NAME: &'static str = "Smooth Red Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12411;
    const MAX_STATE_ID: u32 = 12412;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Moss Block
pub struct MossBlock;

impl BlockDef for MossBlock {
    const ID: u32 = 1144;
    const STRING_ID: &'static str = "minecraft:moss_block";
    const NAME: &'static str = "Moss Block";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12413;
    const MAX_STATE_ID: u32 = 12413;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Concrete Powder
pub struct PurpleConcretePowder;

impl BlockDef for PurpleConcretePowder {
    const ID: u32 = 736;
    const STRING_ID: &'static str = "minecraft:purple_concrete_powder";
    const NAME: &'static str = "Purple Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12414;
    const MAX_STATE_ID: u32 = 12414;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Glazed Terracotta
pub struct PinkGlazedTerracotta;

impl BlockDef for PinkGlazedTerracotta {
    const ID: u32 = 226;
    const STRING_ID: &'static str = "minecraft:pink_glazed_terracotta";
    const NAME: &'static str = "Pink Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12415;
    const MAX_STATE_ID: u32 = 12420;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Short Grass
pub struct ShortGrass;

impl BlockDef for ShortGrass {
    const ID: u32 = 130;
    const STRING_ID: &'static str = "minecraft:short_grass";
    const NAME: &'static str = "Short Grass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12421;
    const MAX_STATE_ID: u32 = 12421;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Cut Copper Slab
pub struct WaxedWeatheredCutCopperSlab;

impl BlockDef for WaxedWeatheredCutCopperSlab {
    const ID: u32 = 1074;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_cut_copper_slab";
    const NAME: &'static str = "Waxed Weathered Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12422;
    const MAX_STATE_ID: u32 = 12423;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Fire Coral Fan
pub struct FireCoralFan;

impl BlockDef for FireCoralFan {
    const ID: u32 = 776;
    const STRING_ID: &'static str = "minecraft:fire_coral_fan";
    const NAME: &'static str = "Fire Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12424;
    const MAX_STATE_ID: u32 = 12425;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Trapdoor
pub struct SpruceTrapdoor;

impl BlockDef for SpruceTrapdoor {
    const ID: u32 = 317;
    const STRING_ID: &'static str = "minecraft:spruce_trapdoor";
    const NAME: &'static str = "Spruce Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12426;
    const MAX_STATE_ID: u32 = 12441;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chain Command Block
pub struct ChainCommandBlock;

impl BlockDef for ChainCommandBlock {
    const ID: u32 = 189;
    const STRING_ID: &'static str = "minecraft:chain_command_block";
    const NAME: &'static str = "Chain Command Block";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12442;
    const MAX_STATE_ID: u32 = 12453;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Sandstone
pub struct RedSandstone;

impl BlockDef for RedSandstone {
    const ID: u32 = 179;
    const STRING_ID: &'static str = "minecraft:red_sandstone";
    const NAME: &'static str = "Red Sandstone";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12454;
    const MAX_STATE_ID: u32 = 12454;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Nether Brick Slab
pub struct RedNetherBrickSlab;

impl BlockDef for RedNetherBrickSlab {
    const ID: u32 = 8065;
    const STRING_ID: &'static str = "minecraft:red_nether_brick_slab";
    const NAME: &'static str = "Red Nether Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12455;
    const MAX_STATE_ID: u32 = 12456;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Chiseled Copper
pub struct ExposedChiseledCopper;

impl BlockDef for ExposedChiseledCopper {
    const ID: u32 = 1053;
    const STRING_ID: &'static str = "minecraft:exposed_chiseled_copper";
    const NAME: &'static str = "Exposed Chiseled Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12457;
    const MAX_STATE_ID: u32 = 12457;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Fence Gate
pub struct SpruceFenceGate;

impl BlockDef for SpruceFenceGate {
    const ID: u32 = 183;
    const STRING_ID: &'static str = "minecraft:spruce_fence_gate";
    const NAME: &'static str = "Spruce Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12458;
    const MAX_STATE_ID: u32 = 12473;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Cut Copper Slab
pub struct ExposedCutCopperSlab;

impl BlockDef for ExposedCutCopperSlab {
    const ID: u32 = 1069;
    const STRING_ID: &'static str = "minecraft:exposed_cut_copper_slab";
    const NAME: &'static str = "Exposed Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12474;
    const MAX_STATE_ID: u32 = 12475;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Nether Brick Stairs
pub struct RedNetherBrickStairs;

impl BlockDef for RedNetherBrickStairs {
    const ID: u32 = 808;
    const STRING_ID: &'static str = "minecraft:red_nether_brick_stairs";
    const NAME: &'static str = "Red Nether Brick Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12476;
    const MAX_STATE_ID: u32 = 12483;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Glazed Terracotta
pub struct GreenGlazedTerracotta;

impl BlockDef for GreenGlazedTerracotta {
    const ID: u32 = 233;
    const STRING_ID: &'static str = "minecraft:green_glazed_terracotta";
    const NAME: &'static str = "Green Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12484;
    const MAX_STATE_ID: u32 = 12489;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Planks
pub struct JunglePlanks;

impl BlockDef for JunglePlanks {
    const ID: u32 = 16;
    const STRING_ID: &'static str = "minecraft:jungle_planks";
    const NAME: &'static str = "Jungle Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12490;
    const MAX_STATE_ID: u32 = 12490;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Redstone Ore
pub struct DeepslateRedstoneOre;

impl BlockDef for DeepslateRedstoneOre {
    const ID: u32 = 272;
    const STRING_ID: &'static str = "minecraft:deepslate_redstone_ore";
    const NAME: &'static str = "Deepslate Redstone Ore";
    const HARDNESS: f32 = 4.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12491;
    const MAX_STATE_ID: u32 = 12491;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Brain Coral Block
pub struct DeadBrainCoralBlock;

impl BlockDef for DeadBrainCoralBlock {
    const ID: u32 = 749;
    const STRING_ID: &'static str = "minecraft:dead_brain_coral_block";
    const NAME: &'static str = "Dead Brain Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12492;
    const MAX_STATE_ID: u32 = 12492;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Fence
pub struct MangroveFence;

impl BlockDef for MangroveFence {
    const ID: u32 = 644;
    const STRING_ID: &'static str = "minecraft:mangrove_fence";
    const NAME: &'static str = "Mangrove Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12493;
    const MAX_STATE_ID: u32 = 12493;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper Grate
pub struct OxidizedCopperGrate;

impl BlockDef for OxidizedCopperGrate {
    const ID: u32 = 1095;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_grate";
    const NAME: &'static str = "Oxidized Copper Grate";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12494;
    const MAX_STATE_ID: u32 = 12494;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Anvil
pub struct Anvil;

impl BlockDef for Anvil {
    const ID: u32 = 145;
    const STRING_ID: &'static str = "minecraft:anvil";
    const NAME: &'static str = "Anvil";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12495;
    const MAX_STATE_ID: u32 = 12498;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Trapdoor
pub struct BirchTrapdoor;

impl BlockDef for BirchTrapdoor {
    const ID: u32 = 318;
    const STRING_ID: &'static str = "minecraft:birch_trapdoor";
    const NAME: &'static str = "Birch Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12499;
    const MAX_STATE_ID: u32 = 12514;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff Bricks
pub struct TuffBricks;

impl BlockDef for TuffBricks {
    const ID: u32 = 993;
    const STRING_ID: &'static str = "minecraft:tuff_bricks";
    const NAME: &'static str = "Tuff Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12515;
    const MAX_STATE_ID: u32 = 12515;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Leaves
pub struct MangroveLeaves;

impl BlockDef for MangroveLeaves {
    const ID: u32 = 96;
    const STRING_ID: &'static str = "minecraft:mangrove_leaves";
    const NAME: &'static str = "Mangrove Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12516;
    const MAX_STATE_ID: u32 = 12519;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobbled Deepslate
pub struct CobbledDeepslate;

impl BlockDef for CobbledDeepslate {
    const ID: u32 = 1152;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate";
    const NAME: &'static str = "Cobbled Deepslate";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12520;
    const MAX_STATE_ID: u32 = 12520;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Quartz Slab
pub struct QuartzSlab;

impl BlockDef for QuartzSlab {
    const ID: u32 = 8051;
    const STRING_ID: &'static str = "minecraft:quartz_slab";
    const NAME: &'static str = "Quartz Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12521;
    const MAX_STATE_ID: u32 = 12522;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bookshelf
pub struct Bookshelf;

impl BlockDef for Bookshelf {
    const ID: u32 = 47;
    const STRING_ID: &'static str = "minecraft:bookshelf";
    const NAME: &'static str = "Bookshelf";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12523;
    const MAX_STATE_ID: u32 = 12523;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mud
pub struct Mud;

impl BlockDef for Mud {
    const ID: u32 = 1150;
    const STRING_ID: &'static str = "minecraft:mud";
    const NAME: &'static str = "Mud";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12524;
    const MAX_STATE_ID: u32 = 12524;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jack o'Lantern
pub struct LitPumpkin;

impl BlockDef for LitPumpkin {
    const ID: u32 = 91;
    const STRING_ID: &'static str = "minecraft:lit_pumpkin";
    const NAME: &'static str = "Jack o'Lantern";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12525;
    const MAX_STATE_ID: u32 = 12528;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Ice
pub struct Ice;

impl BlockDef for Ice {
    const ID: u32 = 79;
    const STRING_ID: &'static str = "minecraft:ice";
    const NAME: &'static str = "Ice";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12529;
    const MAX_STATE_ID: u32 = 12529;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Air
pub struct Air;

impl BlockDef for Air {
    const ID: u32 = 0;
    const STRING_ID: &'static str = "minecraft:air";
    const NAME: &'static str = "Air";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12530;
    const MAX_STATE_ID: u32 = 12530;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Bed
pub struct Bed;

impl BlockDef for Bed {
    const ID: u32 = 26;
    const STRING_ID: &'static str = "minecraft:bed";
    const NAME: &'static str = "Black Bed";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12531;
    const MAX_STATE_ID: u32 = 12546;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Concrete
pub struct BlackConcrete;

impl BlockDef for BlackConcrete {
    const ID: u32 = 725;
    const STRING_ID: &'static str = "minecraft:black_concrete";
    const NAME: &'static str = "Black Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12547;
    const MAX_STATE_ID: u32 = 12547;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// TNT
pub struct Tnt;

impl BlockDef for Tnt {
    const ID: u32 = 46;
    const STRING_ID: &'static str = "minecraft:tnt";
    const NAME: &'static str = "TNT";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12548;
    const MAX_STATE_ID: u32 = 12549;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Purple Candle
pub struct PurpleCandleCake;

impl BlockDef for PurpleCandleCake {
    const ID: u32 = 972;
    const STRING_ID: &'static str = "minecraft:purple_candle_cake";
    const NAME: &'static str = "Cake with Purple Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12550;
    const MAX_STATE_ID: u32 = 12551;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobweb
pub struct Web;

impl BlockDef for Web {
    const ID: u32 = 30;
    const STRING_ID: &'static str = "minecraft:web";
    const NAME: &'static str = "Cobweb";
    const HARDNESS: f32 = 4.0_f32;
    const RESISTANCE: f32 = 4.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12552;
    const MAX_STATE_ID: u32 = 12552;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Tube Coral Fan
pub struct DeadTubeCoralFan;

impl BlockDef for DeadTubeCoralFan {
    const ID: u32 = 768;
    const STRING_ID: &'static str = "minecraft:dead_tube_coral_fan";
    const NAME: &'static str = "Dead Tube Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12553;
    const MAX_STATE_ID: u32 = 12554;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper Chest
pub struct OxidizedCopperChest;

impl BlockDef for OxidizedCopperChest {
    const ID: u32 = 1111;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_chest";
    const NAME: &'static str = "Oxidized Copper Chest";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12555;
    const MAX_STATE_ID: u32 = 12558;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper Chain
pub struct OxidizedCopperChain;

impl BlockDef for OxidizedCopperChain {
    const ID: u32 = 354;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_chain";
    const NAME: &'static str = "Oxidized Copper Chain";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12559;
    const MAX_STATE_ID: u32 = 12561;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Sign
pub struct PaleOakStandingSign;

impl BlockDef for PaleOakStandingSign {
    const ID: u32 = 217;
    const STRING_ID: &'static str = "minecraft:pale_oak_standing_sign";
    const NAME: &'static str = "Pale Oak Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12562;
    const MAX_STATE_ID: u32 = 12577;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Diorite Stairs
pub struct PolishedDioriteStairs;

impl BlockDef for PolishedDioriteStairs {
    const ID: u32 = 800;
    const STRING_ID: &'static str = "minecraft:polished_diorite_stairs";
    const NAME: &'static str = "Polished Diorite Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12578;
    const MAX_STATE_ID: u32 = 12585;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Concrete Powder
pub struct BlueConcretePowder;

impl BlockDef for BlueConcretePowder {
    const ID: u32 = 737;
    const STRING_ID: &'static str = "minecraft:blue_concrete_powder";
    const NAME: &'static str = "Blue Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12586;
    const MAX_STATE_ID: u32 = 12586;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Concrete
pub struct OrangeConcrete;

impl BlockDef for OrangeConcrete {
    const ID: u32 = 711;
    const STRING_ID: &'static str = "minecraft:orange_concrete";
    const NAME: &'static str = "Orange Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12587;
    const MAX_STATE_ID: u32 = 12587;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crying Obsidian
pub struct CryingObsidian;

impl BlockDef for CryingObsidian {
    const ID: u32 = 917;
    const STRING_ID: &'static str = "minecraft:crying_obsidian";
    const NAME: &'static str = "Crying Obsidian";
    const HARDNESS: f32 = 50.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 10;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12588;
    const MAX_STATE_ID: u32 = 12588;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Carpet
pub struct LimeCarpet;

impl BlockDef for LimeCarpet {
    const ID: u32 = 543;
    const STRING_ID: &'static str = "minecraft:lime_carpet";
    const NAME: &'static str = "Lime Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12589;
    const MAX_STATE_ID: u32 = 12589;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Closed Eyeblossom
pub struct ClosedEyeblossom;

impl BlockDef for ClosedEyeblossom {
    const ID: u32 = 1192;
    const STRING_ID: &'static str = "minecraft:closed_eyeblossom";
    const NAME: &'static str = "Closed Eyeblossom";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12590;
    const MAX_STATE_ID: u32 = 12590;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Fire Coral Fan
pub struct DeadFireCoralFan;

impl BlockDef for DeadFireCoralFan {
    const ID: u32 = 771;
    const STRING_ID: &'static str = "minecraft:dead_fire_coral_fan";
    const NAME: &'static str = "Dead Fire Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12591;
    const MAX_STATE_ID: u32 = 12592;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Decorated Pot
pub struct DecoratedPot;

impl BlockDef for DecoratedPot {
    const ID: u32 = 1183;
    const STRING_ID: &'static str = "minecraft:decorated_pot";
    const NAME: &'static str = "Decorated Pot";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12593;
    const MAX_STATE_ID: u32 = 12596;
    type State = super::states::DirectionState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Granite Slab
pub struct GraniteDoubleSlab;

impl BlockDef for GraniteDoubleSlab {
    const ID: u32 = 8063;
    const STRING_ID: &'static str = "minecraft:granite_double_slab";
    const NAME: &'static str = "Granite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12597;
    const MAX_STATE_ID: u32 = 12598;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Enchanting Table
pub struct EnchantingTable;

impl BlockDef for EnchantingTable {
    const ID: u32 = 116;
    const STRING_ID: &'static str = "minecraft:enchanting_table";
    const NAME: &'static str = "Enchanting Table";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 7;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12599;
    const MAX_STATE_ID: u32 = 12599;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Wall
pub struct PolishedBlackstoneWall;

impl BlockDef for PolishedBlackstoneWall {
    const ID: u32 = 940;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_wall";
    const NAME: &'static str = "Polished Blackstone Wall";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12600;
    const MAX_STATE_ID: u32 = 12761;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Cut Copper Slab
pub struct WaxedExposedDoubleCutCopperSlab;

impl BlockDef for WaxedExposedDoubleCutCopperSlab {
    const ID: u32 = 1073;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_double_cut_copper_slab";
    const NAME: &'static str = "Waxed Exposed Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12762;
    const MAX_STATE_ID: u32 = 12763;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bubble Coral Wall Fan
pub struct BubbleCoralWallFan;

impl BlockDef for BubbleCoralWallFan {
    const ID: u32 = 785;
    const STRING_ID: &'static str = "minecraft:bubble_coral_wall_fan";
    const NAME: &'static str = "Bubble Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12764;
    const MAX_STATE_ID: u32 = 12767;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Tulip
pub struct OrangeTulip;

impl BlockDef for OrangeTulip {
    const ID: u32 = 165;
    const STRING_ID: &'static str = "minecraft:orange_tulip";
    const NAME: &'static str = "Orange Tulip";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12768;
    const MAX_STATE_ID: u32 = 12768;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Shulker Box
pub struct BrownShulkerBox;

impl BlockDef for BrownShulkerBox {
    const ID: u32 = 690;
    const STRING_ID: &'static str = "minecraft:brown_shulker_box";
    const NAME: &'static str = "Brown Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12769;
    const MAX_STATE_ID: u32 = 12769;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Azalea
pub struct Azalea;

impl BlockDef for Azalea {
    const ID: u32 = 1138;
    const STRING_ID: &'static str = "minecraft:azalea";
    const NAME: &'static str = "Azalea";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12770;
    const MAX_STATE_ID: u32 = 12770;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mud Bricks
pub struct MudBricks;

impl BlockDef for MudBricks {
    const ID: u32 = 331;
    const STRING_ID: &'static str = "minecraft:mud_bricks";
    const NAME: &'static str = "Mud Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12771;
    const MAX_STATE_ID: u32 = 12771;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Sign
pub struct BirchWallSign;

impl BlockDef for BirchWallSign {
    const ID: u32 = 226;
    const STRING_ID: &'static str = "minecraft:birch_wall_sign";
    const NAME: &'static str = "Birch Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12772;
    const MAX_STATE_ID: u32 = 12777;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Sign
pub struct BambooWallSign;

impl BlockDef for BambooWallSign {
    const ID: u32 = 233;
    const STRING_ID: &'static str = "minecraft:bamboo_wall_sign";
    const NAME: &'static str = "Bamboo Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12778;
    const MAX_STATE_ID: u32 = 12783;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Wood
pub struct AcaciaWood;

impl BlockDef for AcaciaWood {
    const ID: u32 = 75;
    const STRING_ID: &'static str = "minecraft:acacia_wood";
    const NAME: &'static str = "Acacia Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12784;
    const MAX_STATE_ID: u32 = 12786;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Stained Glass Pane
pub struct GrayStainedGlassPane;

impl BlockDef for GrayStainedGlassPane {
    const ID: u32 = 507;
    const STRING_ID: &'static str = "minecraft:gray_stained_glass_pane";
    const NAME: &'static str = "Gray Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12787;
    const MAX_STATE_ID: u32 = 12787;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hopper
pub struct Hopper;

impl BlockDef for Hopper {
    const ID: u32 = 154;
    const STRING_ID: &'static str = "minecraft:hopper";
    const NAME: &'static str = "Hopper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 4.8_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12788;
    const MAX_STATE_ID: u32 = 12799;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Red Stained Glass
pub struct HardRedStainedGlass;

impl BlockDef for HardRedStainedGlass {
    const ID: u32 = 13526;
    const STRING_ID: &'static str = "minecraft:hard_red_stained_glass";
    const NAME: &'static str = "Hard Red Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12800;
    const MAX_STATE_ID: u32 = 12800;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bell
pub struct Bell;

impl BlockDef for Bell {
    const ID: u32 = 848;
    const STRING_ID: &'static str = "minecraft:bell";
    const NAME: &'static str = "Bell";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 5.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12801;
    const MAX_STATE_ID: u32 = 12832;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lectern
pub struct Lectern;

impl BlockDef for Lectern {
    const ID: u32 = 845;
    const STRING_ID: &'static str = "minecraft:lectern";
    const NAME: &'static str = "Lectern";
    const HARDNESS: f32 = 2.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12833;
    const MAX_STATE_ID: u32 = 12840;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bush
pub struct Bush;

impl BlockDef for Bush {
    const ID: u32 = 133;
    const STRING_ID: &'static str = "minecraft:bush";
    const NAME: &'static str = "Bush";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12841;
    const MAX_STATE_ID: u32 = 12841;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Crimson Stem
pub struct StrippedCrimsonStem;

impl BlockDef for StrippedCrimsonStem {
    const ID: u32 = 872;
    const STRING_ID: &'static str = "minecraft:stripped_crimson_stem";
    const NAME: &'static str = "Stripped Crimson Stem";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12842;
    const MAX_STATE_ID: u32 = 12844;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Black Banner
pub struct StandingBanner;

impl BlockDef for StandingBanner {
    const ID: u32 = 176;
    const STRING_ID: &'static str = "minecraft:standing_banner";
    const NAME: &'static str = "Black Banner";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12845;
    const MAX_STATE_ID: u32 = 12860;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Shulker Box
pub struct LightBlueShulkerBox;

impl BlockDef for LightBlueShulkerBox {
    const ID: u32 = 681;
    const STRING_ID: &'static str = "minecraft:light_blue_shulker_box";
    const NAME: &'static str = "Light Blue Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 12861;
    const MAX_STATE_ID: u32 = 12861;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Stairs
pub struct JungleStairs;

impl BlockDef for JungleStairs {
    const ID: u32 = 136;
    const STRING_ID: &'static str = "minecraft:jungle_stairs";
    const NAME: &'static str = "Jungle Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12862;
    const MAX_STATE_ID: u32 = 12869;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Propagule
pub struct MangrovePropagule;

impl BlockDef for MangrovePropagule {
    const ID: u32 = 33;
    const STRING_ID: &'static str = "minecraft:mangrove_propagule";
    const NAME: &'static str = "Mangrove Propagule";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12870;
    const MAX_STATE_ID: u32 = 12879;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cactus
pub struct Cactus;

impl BlockDef for Cactus {
    const ID: u32 = 81;
    const STRING_ID: &'static str = "minecraft:cactus";
    const NAME: &'static str = "Cactus";
    const HARDNESS: f32 = 0.4_f32;
    const RESISTANCE: f32 = 0.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12880;
    const MAX_STATE_ID: u32 = 12895;
    type State = super::states::AgeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Budding Amethyst
pub struct BuddingAmethyst;

impl BlockDef for BuddingAmethyst {
    const ID: u32 = 979;
    const STRING_ID: &'static str = "minecraft:budding_amethyst";
    const NAME: &'static str = "Budding Amethyst";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 12896;
    const MAX_STATE_ID: u32 = 12896;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sniffer Egg
pub struct SnifferEgg;

impl BlockDef for SnifferEgg {
    const ID: u32 = 746;
    const STRING_ID: &'static str = "minecraft:sniffer_egg";
    const NAME: &'static str = "Sniffer Egg";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12897;
    const MAX_STATE_ID: u32 = 12899;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Diorite Slab
pub struct PolishedDioriteDoubleSlab;

impl BlockDef for PolishedDioriteDoubleSlab {
    const ID: u32 = 8058;
    const STRING_ID: &'static str = "minecraft:polished_diorite_double_slab";
    const NAME: &'static str = "Polished Diorite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12900;
    const MAX_STATE_ID: u32 = 12901;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Stairs
pub struct BirchStairs;

impl BlockDef for BirchStairs {
    const ID: u32 = 135;
    const STRING_ID: &'static str = "minecraft:birch_stairs";
    const NAME: &'static str = "Birch Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12902;
    const MAX_STATE_ID: u32 = 12909;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Brick Wall
pub struct NetherBrickWall;

impl BlockDef for NetherBrickWall {
    const ID: u32 = 831;
    const STRING_ID: &'static str = "minecraft:nether_brick_wall";
    const NAME: &'static str = "Nether Brick Wall";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 12910;
    const MAX_STATE_ID: u32 = 13071;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Glazed Terracotta
pub struct PurpleGlazedTerracotta;

impl BlockDef for PurpleGlazedTerracotta {
    const ID: u32 = 219;
    const STRING_ID: &'static str = "minecraft:purple_glazed_terracotta";
    const NAME: &'static str = "Purple Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13072;
    const MAX_STATE_ID: u32 = 13077;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Green Concrete Powder
pub struct GreenConcretePowder;

impl BlockDef for GreenConcretePowder {
    const ID: u32 = 739;
    const STRING_ID: &'static str = "minecraft:green_concrete_powder";
    const NAME: &'static str = "Green Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13078;
    const MAX_STATE_ID: u32 = 13078;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bedrock
pub struct Bedrock;

impl BlockDef for Bedrock {
    const ID: u32 = 7;
    const STRING_ID: &'static str = "minecraft:bedrock";
    const NAME: &'static str = "Bedrock";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13079;
    const MAX_STATE_ID: u32 = 13080;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Slab
pub struct SpruceSlab;

impl BlockDef for SpruceSlab {
    const ID: u32 = 8031;
    const STRING_ID: &'static str = "minecraft:spruce_slab";
    const NAME: &'static str = "Spruce Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13081;
    const MAX_STATE_ID: u32 = 13082;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blackstone Stairs
pub struct BlackstoneStairs;

impl BlockDef for BlackstoneStairs {
    const ID: u32 = 925;
    const STRING_ID: &'static str = "minecraft:blackstone_stairs";
    const NAME: &'static str = "Blackstone Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13083;
    const MAX_STATE_ID: u32 = 13090;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Ice
pub struct BlueIce;

impl BlockDef for BlueIce {
    const ID: u32 = 789;
    const STRING_ID: &'static str = "minecraft:blue_ice";
    const NAME: &'static str = "Blue Ice";
    const HARDNESS: f32 = 2.8_f32;
    const RESISTANCE: f32 = 2.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13091;
    const MAX_STATE_ID: u32 = 13091;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cyan Shulker Box
pub struct CyanShulkerBox;

impl BlockDef for CyanShulkerBox {
    const ID: u32 = 687;
    const STRING_ID: &'static str = "minecraft:cyan_shulker_box";
    const NAME: &'static str = "Cyan Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 13092;
    const MAX_STATE_ID: u32 = 13092;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Red Stained Glass Pane
pub struct HardRedStainedGlassPane;

impl BlockDef for HardRedStainedGlassPane {
    const ID: u32 = 13819;
    const STRING_ID: &'static str = "minecraft:hard_red_stained_glass_pane";
    const NAME: &'static str = "Hard Red Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13093;
    const MAX_STATE_ID: u32 = 13093;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Andesite Stairs
pub struct PolishedAndesiteStairs;

impl BlockDef for PolishedAndesiteStairs {
    const ID: u32 = 809;
    const STRING_ID: &'static str = "minecraft:polished_andesite_stairs";
    const NAME: &'static str = "Polished Andesite Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13094;
    const MAX_STATE_ID: u32 = 13101;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Horn Coral Wall Fan
pub struct DeadHornCoralWallFan;

impl BlockDef for DeadHornCoralWallFan {
    const ID: u32 = 782;
    const STRING_ID: &'static str = "minecraft:dead_horn_coral_wall_fan";
    const NAME: &'static str = "Dead Horn Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 13102;
    const MAX_STATE_ID: u32 = 13105;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Piglin Head
pub struct PiglinHead;

impl BlockDef for PiglinHead {
    const ID: u32 = 465;
    const STRING_ID: &'static str = "minecraft:piglin_head";
    const NAME: &'static str = "Piglin Head";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13106;
    const MAX_STATE_ID: u32 = 13111;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sculk
pub struct Sculk;

impl BlockDef for Sculk {
    const ID: u32 = 1030;
    const STRING_ID: &'static str = "minecraft:sculk";
    const NAME: &'static str = "Sculk";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13112;
    const MAX_STATE_ID: u32 = 13112;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Purple Stained Glass Pane
pub struct HardPurpleStainedGlassPane;

impl BlockDef for HardPurpleStainedGlassPane {
    const ID: u32 = 13839;
    const STRING_ID: &'static str = "minecraft:hard_purple_stained_glass_pane";
    const NAME: &'static str = "Hard Purple Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13113;
    const MAX_STATE_ID: u32 = 13113;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Netherrack
pub struct Netherrack;

impl BlockDef for Netherrack {
    const ID: u32 = 87;
    const STRING_ID: &'static str = "minecraft:netherrack";
    const NAME: &'static str = "Netherrack";
    const HARDNESS: f32 = 0.4_f32;
    const RESISTANCE: f32 = 0.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13114;
    const MAX_STATE_ID: u32 = 13114;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Candle
pub struct PurpleCandle;

impl BlockDef for PurpleCandle {
    const ID: u32 = 955;
    const STRING_ID: &'static str = "minecraft:purple_candle";
    const NAME: &'static str = "Purple Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13115;
    const MAX_STATE_ID: u32 = 13122;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Sign
pub struct SpruceStandingSign;

impl BlockDef for SpruceStandingSign {
    const ID: u32 = 211;
    const STRING_ID: &'static str = "minecraft:spruce_standing_sign";
    const NAME: &'static str = "Spruce Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13123;
    const MAX_STATE_ID: u32 = 13138;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mangrove Button
pub struct MangroveButton;

impl BlockDef for MangroveButton {
    const ID: u32 = 451;
    const STRING_ID: &'static str = "minecraft:mangrove_button";
    const NAME: &'static str = "Mangrove Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13139;
    const MAX_STATE_ID: u32 = 13150;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Carpet
pub struct OrangeCarpet;

impl BlockDef for OrangeCarpet {
    const ID: u32 = 539;
    const STRING_ID: &'static str = "minecraft:orange_carpet";
    const NAME: &'static str = "Orange Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13151;
    const MAX_STATE_ID: u32 = 13151;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Horn Coral Fan
pub struct DeadHornCoralFan;

impl BlockDef for DeadHornCoralFan {
    const ID: u32 = 772;
    const STRING_ID: &'static str = "minecraft:dead_horn_coral_fan";
    const NAME: &'static str = "Dead Horn Coral Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 13152;
    const MAX_STATE_ID: u32 = 13153;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lantern
pub struct Lantern;

impl BlockDef for Lantern {
    const ID: u32 = 849;
    const STRING_ID: &'static str = "minecraft:lantern";
    const NAME: &'static str = "Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13154;
    const MAX_STATE_ID: u32 = 13155;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Shelf
pub struct CrimsonShelf;

impl BlockDef for CrimsonShelf {
    const ID: u32 = 184;
    const STRING_ID: &'static str = "minecraft:crimson_shelf";
    const NAME: &'static str = "Crimson Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13156;
    const MAX_STATE_ID: u32 = 13187;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Copper Door
pub struct WaxedWeatheredCopperDoor;

impl BlockDef for WaxedWeatheredCopperDoor {
    const ID: u32 = 1082;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_copper_door";
    const NAME: &'static str = "Waxed Weathered Copper Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13188;
    const MAX_STATE_ID: u32 = 13219;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Stained Glass Pane
pub struct RedStainedGlassPane;

impl BlockDef for RedStainedGlassPane {
    const ID: u32 = 514;
    const STRING_ID: &'static str = "minecraft:red_stained_glass_pane";
    const NAME: &'static str = "Red Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13220;
    const MAX_STATE_ID: u32 = 13220;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blast Furnace
pub struct LitBlastFurnace;

impl BlockDef for LitBlastFurnace {
    const ID: u32 = 841;
    const STRING_ID: &'static str = "minecraft:lit_blast_furnace";
    const NAME: &'static str = "Blast Furnace";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13221;
    const MAX_STATE_ID: u32 = 13224;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Lightning Rod
pub struct WaxedOxidizedLightningRod;

impl BlockDef for WaxedOxidizedLightningRod {
    const ID: u32 = 1131;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_lightning_rod";
    const NAME: &'static str = "Waxed Oxidized Lightning Rod";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13225;
    const MAX_STATE_ID: u32 = 13236;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Stained Glass Pane
pub struct PinkStainedGlassPane;

impl BlockDef for PinkStainedGlassPane {
    const ID: u32 = 506;
    const STRING_ID: &'static str = "minecraft:pink_stained_glass_pane";
    const NAME: &'static str = "Pink Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13237;
    const MAX_STATE_ID: u32 = 13237;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Wool
pub struct LightBlueWool;

impl BlockDef for LightBlueWool {
    const ID: u32 = 143;
    const STRING_ID: &'static str = "minecraft:light_blue_wool";
    const NAME: &'static str = "Light Blue Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13238;
    const MAX_STATE_ID: u32 = 13238;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Allow
pub struct Allow;

impl BlockDef for Allow {
    const ID: u32 = 210;
    const STRING_ID: &'static str = "minecraft:allow";
    const NAME: &'static str = "Allow";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13239;
    const MAX_STATE_ID: u32 = 13239;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Fence
pub struct DarkOakFence;

impl BlockDef for DarkOakFence {
    const ID: u32 = 642;
    const STRING_ID: &'static str = "minecraft:dark_oak_fence";
    const NAME: &'static str = "Dark Oak Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13240;
    const MAX_STATE_ID: u32 = 13240;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deprecated Purpur Block 2
pub struct DeprecatedPurpurBlock2;

impl BlockDef for DeprecatedPurpurBlock2 {
    const ID: u32 = 13969;
    const STRING_ID: &'static str = "minecraft:deprecated_purpur_block_2";
    const NAME: &'static str = "Deprecated Purpur Block 2";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13241;
    const MAX_STATE_ID: u32 = 13243;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deprecated Purpur Block 1
pub struct DeprecatedPurpurBlock1;

impl BlockDef for DeprecatedPurpurBlock1 {
    const ID: u32 = 13972;
    const STRING_ID: &'static str = "minecraft:deprecated_purpur_block_1";
    const NAME: &'static str = "Deprecated Purpur Block 1";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13244;
    const MAX_STATE_ID: u32 = 13246;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Door
pub struct BirchDoor;

impl BlockDef for BirchDoor {
    const ID: u32 = 194;
    const STRING_ID: &'static str = "minecraft:birch_door";
    const NAME: &'static str = "Birch Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13247;
    const MAX_STATE_ID: u32 = 13278;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Shelf
pub struct CherryShelf;

impl BlockDef for CherryShelf {
    const ID: u32 = 183;
    const STRING_ID: &'static str = "minecraft:cherry_shelf";
    const NAME: &'static str = "Cherry Shelf";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13279;
    const MAX_STATE_ID: u32 = 13310;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chest
pub struct Chest;

impl BlockDef for Chest {
    const ID: u32 = 54;
    const STRING_ID: &'static str = "minecraft:chest";
    const NAME: &'static str = "Chest";
    const HARDNESS: f32 = 2.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13311;
    const MAX_STATE_ID: u32 = 13314;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Wood
pub struct CherryWood;

impl BlockDef for CherryWood {
    const ID: u32 = 76;
    const STRING_ID: &'static str = "minecraft:cherry_wood";
    const NAME: &'static str = "Cherry Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13315;
    const MAX_STATE_ID: u32 = 13317;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Clay
pub struct Clay;

impl BlockDef for Clay {
    const ID: u32 = 82;
    const STRING_ID: &'static str = "minecraft:clay";
    const NAME: &'static str = "Clay";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13318;
    const MAX_STATE_ID: u32 = 13318;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Stairs
pub struct CherryStairs;

impl BlockDef for CherryStairs {
    const ID: u32 = 517;
    const STRING_ID: &'static str = "minecraft:cherry_stairs";
    const NAME: &'static str = "Cherry Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13319;
    const MAX_STATE_ID: u32 = 13326;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake
pub struct Cake;

impl BlockDef for Cake {
    const ID: u32 = 92;
    const STRING_ID: &'static str = "minecraft:cake";
    const NAME: &'static str = "Cake";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13327;
    const MAX_STATE_ID: u32 = 13333;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Hanging Sign
pub struct CrimsonHangingSign;

impl BlockDef for CrimsonHangingSign {
    const ID: u32 = 242;
    const STRING_ID: &'static str = "minecraft:crimson_hanging_sign";
    const NAME: &'static str = "Crimson Hanging Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13334;
    const MAX_STATE_ID: u32 = 13717;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sculk Vein
pub struct SculkVein;

impl BlockDef for SculkVein {
    const ID: u32 = 1031;
    const STRING_ID: &'static str = "minecraft:sculk_vein";
    const NAME: &'static str = "Sculk Vein";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 13718;
    const MAX_STATE_ID: u32 = 13781;
    type State = super::states::MultiFaceState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Brain Coral
pub struct DeadBrainCoral;

impl BlockDef for DeadBrainCoral {
    const ID: u32 = 759;
    const STRING_ID: &'static str = "minecraft:dead_brain_coral";
    const NAME: &'static str = "Dead Brain Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 13782;
    const MAX_STATE_ID: u32 = 13782;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Coal Ore
pub struct DeepslateCoalOre;

impl BlockDef for DeepslateCoalOre {
    const ID: u32 = 47;
    const STRING_ID: &'static str = "minecraft:deepslate_coal_ore";
    const NAME: &'static str = "Deepslate Coal Ore";
    const HARDNESS: f32 = 4.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13783;
    const MAX_STATE_ID: u32 = 13783;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Cut Copper
pub struct WeatheredCutCopper;

impl BlockDef for WeatheredCutCopper {
    const ID: u32 = 1046;
    const STRING_ID: &'static str = "minecraft:weathered_cut_copper";
    const NAME: &'static str = "Weathered Cut Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13784;
    const MAX_STATE_ID: u32 = 13784;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Sign
pub struct WarpedStandingSign;

impl BlockDef for WarpedStandingSign {
    const ID: u32 = 902;
    const STRING_ID: &'static str = "minecraft:warped_standing_sign";
    const NAME: &'static str = "Warped Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13785;
    const MAX_STATE_ID: u32 = 13800;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Andesite Slab
pub struct PolishedAndesiteDoubleSlab;

impl BlockDef for PolishedAndesiteDoubleSlab {
    const ID: u32 = 822;
    const STRING_ID: &'static str = "minecraft:polished_andesite_double_slab";
    const NAME: &'static str = "Polished Andesite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13801;
    const MAX_STATE_ID: u32 = 13802;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cracked Polished Blackstone Bricks
pub struct CrackedPolishedBlackstoneBricks;

impl BlockDef for CrackedPolishedBlackstoneBricks {
    const ID: u32 = 930;
    const STRING_ID: &'static str = "minecraft:cracked_polished_blackstone_bricks";
    const NAME: &'static str = "Cracked Polished Blackstone Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13803;
    const MAX_STATE_ID: u32 = 13803;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Sign
pub struct BambooStandingSign;

impl BlockDef for BambooStandingSign {
    const ID: u32 = 219;
    const STRING_ID: &'static str = "minecraft:bamboo_standing_sign";
    const NAME: &'static str = "Bamboo Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13804;
    const MAX_STATE_ID: u32 = 13819;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lava
pub struct FlowingLava;

impl BlockDef for FlowingLava {
    const ID: u32 = 10;
    const STRING_ID: &'static str = "minecraft:flowing_lava";
    const NAME: &'static str = "Lava";
    const HARDNESS: f32 = 100.0_f32;
    const RESISTANCE: f32 = 100.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 13820;
    const MAX_STATE_ID: u32 = 13835;
    type State = super::states::LiquidState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Wither Skeleton Skull
pub struct WitherSkeletonSkull;

impl BlockDef for WitherSkeletonSkull {
    const ID: u32 = 455;
    const STRING_ID: &'static str = "minecraft:wither_skeleton_skull";
    const NAME: &'static str = "Wither Skeleton Skull";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13836;
    const MAX_STATE_ID: u32 = 13841;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Tuff
pub struct PolishedTuff;

impl BlockDef for PolishedTuff {
    const ID: u32 = 988;
    const STRING_ID: &'static str = "minecraft:polished_tuff";
    const NAME: &'static str = "Polished Tuff";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13842;
    const MAX_STATE_ID: u32 = 13842;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magenta Stained Glass
pub struct MagentaStainedGlass;

impl BlockDef for MagentaStainedGlass {
    const ID: u32 = 302;
    const STRING_ID: &'static str = "minecraft:magenta_stained_glass";
    const NAME: &'static str = "Magenta Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13843;
    const MAX_STATE_ID: u32 = 13843;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard White Stained Glass Pane
pub struct HardWhiteStainedGlassPane;

impl BlockDef for HardWhiteStainedGlassPane {
    const ID: u32 = 14573;
    const STRING_ID: &'static str = "minecraft:hard_white_stained_glass_pane";
    const NAME: &'static str = "Hard White Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13844;
    const MAX_STATE_ID: u32 = 13844;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Button
pub struct AcaciaButton;

impl BlockDef for AcaciaButton {
    const ID: u32 = 447;
    const STRING_ID: &'static str = "minecraft:acacia_button";
    const NAME: &'static str = "Acacia Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13845;
    const MAX_STATE_ID: u32 = 13856;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Cyan Stained Glass Pane
pub struct HardCyanStainedGlassPane;

impl BlockDef for HardCyanStainedGlassPane {
    const ID: u32 = 14586;
    const STRING_ID: &'static str = "minecraft:hard_cyan_stained_glass_pane";
    const NAME: &'static str = "Hard Cyan Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13857;
    const MAX_STATE_ID: u32 = 13857;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Furnace
pub struct LitFurnace;

impl BlockDef for LitFurnace {
    const ID: u32 = 62;
    const STRING_ID: &'static str = "minecraft:lit_furnace";
    const NAME: &'static str = "Furnace";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13858;
    const MAX_STATE_ID: u32 = 13861;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Nether Bricks
pub struct ChiseledNetherBricks;

impl BlockDef for ChiseledNetherBricks {
    const ID: u32 = 941;
    const STRING_ID: &'static str = "minecraft:chiseled_nether_bricks";
    const NAME: &'static str = "Chiseled Nether Bricks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13862;
    const MAX_STATE_ID: u32 = 13862;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Warped Button
pub struct WarpedButton;

impl BlockDef for WarpedButton {
    const ID: u32 = 898;
    const STRING_ID: &'static str = "minecraft:warped_button";
    const NAME: &'static str = "Warped Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13863;
    const MAX_STATE_ID: u32 = 13874;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Concrete Powder
pub struct RedConcretePowder;

impl BlockDef for RedConcretePowder {
    const ID: u32 = 740;
    const STRING_ID: &'static str = "minecraft:red_concrete_powder";
    const NAME: &'static str = "Red Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13875;
    const MAX_STATE_ID: u32 = 13875;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Concrete Powder
pub struct LightGrayConcretePowder;

impl BlockDef for LightGrayConcretePowder {
    const ID: u32 = 734;
    const STRING_ID: &'static str = "minecraft:light_gray_concrete_powder";
    const NAME: &'static str = "Light Gray Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13876;
    const MAX_STATE_ID: u32 = 13876;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Lapis Lazuli Ore
pub struct DeepslateLapisOre;

impl BlockDef for DeepslateLapisOre {
    const ID: u32 = 103;
    const STRING_ID: &'static str = "minecraft:deepslate_lapis_ore";
    const NAME: &'static str = "Deepslate Lapis Lazuli Ore";
    const HARDNESS: f32 = 4.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13877;
    const MAX_STATE_ID: u32 = 13877;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Bubble Coral
pub struct DeadBubbleCoral;

impl BlockDef for DeadBubbleCoral {
    const ID: u32 = 760;
    const STRING_ID: &'static str = "minecraft:dead_bubble_coral";
    const NAME: &'static str = "Dead Bubble Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 13878;
    const MAX_STATE_ID: u32 = 13878;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Sapling
pub struct CherrySapling;

impl BlockDef for CherrySapling {
    const ID: u32 = 30;
    const STRING_ID: &'static str = "minecraft:cherry_sapling";
    const NAME: &'static str = "Cherry Sapling";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13879;
    const MAX_STATE_ID: u32 = 13880;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Log
pub struct CherryLog;

impl BlockDef for CherryLog {
    const ID: u32 = 54;
    const STRING_ID: &'static str = "minecraft:cherry_log";
    const NAME: &'static str = "Cherry Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13881;
    const MAX_STATE_ID: u32 = 13883;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Prismarine Stairs
pub struct PrismarineStairs;

impl BlockDef for PrismarineStairs {
    const ID: u32 = 530;
    const STRING_ID: &'static str = "minecraft:prismarine_stairs";
    const NAME: &'static str = "Prismarine Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13884;
    const MAX_STATE_ID: u32 = 13891;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Carpet
pub struct WhiteCarpet;

impl BlockDef for WhiteCarpet {
    const ID: u32 = 538;
    const STRING_ID: &'static str = "minecraft:white_carpet";
    const NAME: &'static str = "White Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13892;
    const MAX_STATE_ID: u32 = 13892;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cyan Concrete
pub struct CyanConcrete;

impl BlockDef for CyanConcrete {
    const ID: u32 = 719;
    const STRING_ID: &'static str = "minecraft:cyan_concrete";
    const NAME: &'static str = "Cyan Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13893;
    const MAX_STATE_ID: u32 = 13893;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Tuff Stairs
pub struct PolishedTuffStairs;

impl BlockDef for PolishedTuffStairs {
    const ID: u32 = 990;
    const STRING_ID: &'static str = "minecraft:polished_tuff_stairs";
    const NAME: &'static str = "Polished Tuff Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13894;
    const MAX_STATE_ID: u32 = 13901;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dragon Egg
pub struct DragonEgg;

impl BlockDef for DragonEgg {
    const ID: u32 = 122;
    const STRING_ID: &'static str = "minecraft:dragon_egg";
    const NAME: &'static str = "Dragon Egg";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 9.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 1;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13902;
    const MAX_STATE_ID: u32 = 13902;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blue Concrete
pub struct BlueConcrete;

impl BlockDef for BlueConcrete {
    const ID: u32 = 721;
    const STRING_ID: &'static str = "minecraft:blue_concrete";
    const NAME: &'static str = "Blue Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13903;
    const MAX_STATE_ID: u32 = 13903;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Bricks
pub struct NetherBrick;

impl BlockDef for NetherBrick {
    const ID: u32 = 112;
    const STRING_ID: &'static str = "minecraft:nether_brick";
    const NAME: &'static str = "Nether Bricks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13904;
    const MAX_STATE_ID: u32 = 13904;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Iron Ore
pub struct DeepslateIronOre;

impl BlockDef for DeepslateIronOre {
    const ID: u32 = 45;
    const STRING_ID: &'static str = "minecraft:deepslate_iron_ore";
    const NAME: &'static str = "Deepslate Iron Ore";
    const HARDNESS: f32 = 4.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13905;
    const MAX_STATE_ID: u32 = 13905;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 1
pub struct Element1;

impl BlockDef for Element1 {
    const ID: u32 = 14637;
    const STRING_ID: &'static str = "minecraft:element_1";
    const NAME: &'static str = "Element 1";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13906;
    const MAX_STATE_ID: u32 = 13906;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 0
pub struct Element0;

impl BlockDef for Element0 {
    const ID: u32 = 36;
    const STRING_ID: &'static str = "minecraft:element_0";
    const NAME: &'static str = "Element 0";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13907;
    const MAX_STATE_ID: u32 = 13907;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 3
pub struct Element3;

impl BlockDef for Element3 {
    const ID: u32 = 14639;
    const STRING_ID: &'static str = "minecraft:element_3";
    const NAME: &'static str = "Element 3";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13908;
    const MAX_STATE_ID: u32 = 13908;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 2
pub struct Element2;

impl BlockDef for Element2 {
    const ID: u32 = 14640;
    const STRING_ID: &'static str = "minecraft:element_2";
    const NAME: &'static str = "Element 2";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13909;
    const MAX_STATE_ID: u32 = 13909;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 5
pub struct Element5;

impl BlockDef for Element5 {
    const ID: u32 = 14641;
    const STRING_ID: &'static str = "minecraft:element_5";
    const NAME: &'static str = "Element 5";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13910;
    const MAX_STATE_ID: u32 = 13910;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 4
pub struct Element4;

impl BlockDef for Element4 {
    const ID: u32 = 14642;
    const STRING_ID: &'static str = "minecraft:element_4";
    const NAME: &'static str = "Element 4";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13911;
    const MAX_STATE_ID: u32 = 13911;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 7
pub struct Element7;

impl BlockDef for Element7 {
    const ID: u32 = 14643;
    const STRING_ID: &'static str = "minecraft:element_7";
    const NAME: &'static str = "Element 7";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13912;
    const MAX_STATE_ID: u32 = 13912;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 6
pub struct Element6;

impl BlockDef for Element6 {
    const ID: u32 = 14644;
    const STRING_ID: &'static str = "minecraft:element_6";
    const NAME: &'static str = "Element 6";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13913;
    const MAX_STATE_ID: u32 = 13913;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 9
pub struct Element9;

impl BlockDef for Element9 {
    const ID: u32 = 14645;
    const STRING_ID: &'static str = "minecraft:element_9";
    const NAME: &'static str = "Element 9";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13914;
    const MAX_STATE_ID: u32 = 13914;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 8
pub struct Element8;

impl BlockDef for Element8 {
    const ID: u32 = 14646;
    const STRING_ID: &'static str = "minecraft:element_8";
    const NAME: &'static str = "Element 8";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13915;
    const MAX_STATE_ID: u32 = 13915;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxeye Daisy
pub struct OxeyeDaisy;

impl BlockDef for OxeyeDaisy {
    const ID: u32 = 168;
    const STRING_ID: &'static str = "minecraft:oxeye_daisy";
    const NAME: &'static str = "Oxeye Daisy";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13916;
    const MAX_STATE_ID: u32 = 13916;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Camera
pub struct Camera;

impl BlockDef for Camera {
    const ID: u32 = 242;
    const STRING_ID: &'static str = "minecraft:camera";
    const NAME: &'static str = "Camera";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13917;
    const MAX_STATE_ID: u32 = 13917;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Wheat Crops
pub struct Wheat;

impl BlockDef for Wheat {
    const ID: u32 = 59;
    const STRING_ID: &'static str = "minecraft:wheat";
    const NAME: &'static str = "Wheat Crops";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13918;
    const MAX_STATE_ID: u32 = 13925;
    type State = super::states::CropState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Cut Copper
pub struct WaxedCutCopper;

impl BlockDef for WaxedCutCopper {
    const ID: u32 = 1048;
    const STRING_ID: &'static str = "minecraft:waxed_cut_copper";
    const NAME: &'static str = "Waxed Cut Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13926;
    const MAX_STATE_ID: u32 = 13926;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Iron Chain
pub struct IronChain;

impl BlockDef for IronChain {
    const ID: u32 = 350;
    const STRING_ID: &'static str = "minecraft:iron_chain";
    const NAME: &'static str = "Iron Chain";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13927;
    const MAX_STATE_ID: u32 = 13929;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Resin Brick Slab
pub struct ResinBrickSlab;

impl BlockDef for ResinBrickSlab {
    const ID: u32 = 8008;
    const STRING_ID: &'static str = "minecraft:resin_brick_slab";
    const NAME: &'static str = "Resin Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13930;
    const MAX_STATE_ID: u32 = 13931;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Heavy Core
pub struct HeavyCore;

impl BlockDef for HeavyCore {
    const ID: u32 = 1187;
    const STRING_ID: &'static str = "minecraft:heavy_core";
    const NAME: &'static str = "Heavy Core";
    const HARDNESS: f32 = 10.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13932;
    const MAX_STATE_ID: u32 = 13932;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobbled Deepslate Slab
pub struct CobbledDeepslateSlab;

impl BlockDef for CobbledDeepslateSlab {
    const ID: u32 = 1154;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate_slab";
    const NAME: &'static str = "Cobbled Deepslate Slab";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13933;
    const MAX_STATE_ID: u32 = 13934;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lilac
pub struct Lilac;

impl BlockDef for Lilac {
    const ID: u32 = 558;
    const STRING_ID: &'static str = "minecraft:lilac";
    const NAME: &'static str = "Lilac";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13935;
    const MAX_STATE_ID: u32 = 13936;
    type State = super::states::DoublePlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Trapdoor
pub struct PaleOakTrapdoor;

impl BlockDef for PaleOakTrapdoor {
    const ID: u32 = 323;
    const STRING_ID: &'static str = "minecraft:pale_oak_trapdoor";
    const NAME: &'static str = "Pale Oak Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13937;
    const MAX_STATE_ID: u32 = 13952;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Quartz Block
pub struct ChiseledQuartzBlock;

impl BlockDef for ChiseledQuartzBlock {
    const ID: u32 = 479;
    const STRING_ID: &'static str = "minecraft:chiseled_quartz_block";
    const NAME: &'static str = "Chiseled Quartz Block";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 13953;
    const MAX_STATE_ID: u32 = 13955;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spore Blossom
pub struct SporeBlossom;

impl BlockDef for SporeBlossom {
    const ID: u32 = 1137;
    const STRING_ID: &'static str = "minecraft:spore_blossom";
    const NAME: &'static str = "Spore Blossom";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13956;
    const MAX_STATE_ID: u32 = 13956;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Exposed Copper Lantern
pub struct WaxedExposedCopperLantern;

impl BlockDef for WaxedExposedCopperLantern {
    const ID: u32 = 856;
    const STRING_ID: &'static str = "minecraft:waxed_exposed_copper_lantern";
    const NAME: &'static str = "Waxed Exposed Copper Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13957;
    const MAX_STATE_ID: u32 = 13958;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Sign
pub struct CrimsonStandingSign;

impl BlockDef for CrimsonStandingSign {
    const ID: u32 = 901;
    const STRING_ID: &'static str = "minecraft:crimson_standing_sign";
    const NAME: &'static str = "Crimson Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13959;
    const MAX_STATE_ID: u32 = 13974;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Sign
pub struct DarkoakStandingSign;

impl BlockDef for DarkoakStandingSign {
    const ID: u32 = 216;
    const STRING_ID: &'static str = "minecraft:darkoak_standing_sign";
    const NAME: &'static str = "Dark Oak Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13975;
    const MAX_STATE_ID: u32 = 13990;
    type State = super::states::StandingSignState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Cut Copper Slab
pub struct WeatheredDoubleCutCopperSlab;

impl BlockDef for WeatheredDoubleCutCopperSlab {
    const ID: u32 = 1070;
    const STRING_ID: &'static str = "minecraft:weathered_double_cut_copper_slab";
    const NAME: &'static str = "Weathered Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13991;
    const MAX_STATE_ID: u32 = 13992;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Stairs
pub struct PaleOakStairs;

impl BlockDef for PaleOakStairs {
    const ID: u32 = 519;
    const STRING_ID: &'static str = "minecraft:pale_oak_stairs";
    const NAME: &'static str = "Pale Oak Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 13993;
    const MAX_STATE_ID: u32 = 14000;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Emerald Ore
pub struct EmeraldOre;

impl BlockDef for EmeraldOre {
    const ID: u32 = 129;
    const STRING_ID: &'static str = "minecraft:emerald_ore";
    const NAME: &'static str = "Emerald Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14001;
    const MAX_STATE_ID: u32 = 14001;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Mushroom Block
pub struct BrownMushroomBlock;

impl BlockDef for BrownMushroomBlock {
    const ID: u32 = 99;
    const STRING_ID: &'static str = "minecraft:brown_mushroom_block";
    const NAME: &'static str = "Brown Mushroom Block";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14002;
    const MAX_STATE_ID: u32 = 14017;
    type State = super::states::MushroomState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Concrete Powder
pub struct GrayConcretePowder;

impl BlockDef for GrayConcretePowder {
    const ID: u32 = 733;
    const STRING_ID: &'static str = "minecraft:gray_concrete_powder";
    const NAME: &'static str = "Gray Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14018;
    const MAX_STATE_ID: u32 = 14018;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Petrified Oak Slab
pub struct PetrifiedOakSlab;

impl BlockDef for PetrifiedOakSlab {
    const ID: u32 = 8045;
    const STRING_ID: &'static str = "minecraft:petrified_oak_slab";
    const NAME: &'static str = "Petrified Oak Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14019;
    const MAX_STATE_ID: u32 = 14020;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Concrete
pub struct GrayConcrete;

impl BlockDef for GrayConcrete {
    const ID: u32 = 717;
    const STRING_ID: &'static str = "minecraft:gray_concrete";
    const NAME: &'static str = "Gray Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14021;
    const MAX_STATE_ID: u32 = 14021;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Candle
pub struct PinkCandle;

impl BlockDef for PinkCandle {
    const ID: u32 = 951;
    const STRING_ID: &'static str = "minecraft:pink_candle";
    const NAME: &'static str = "Pink Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14022;
    const MAX_STATE_ID: u32 = 14029;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Nether Brick Wall
pub struct RedNetherBrickWall;

impl BlockDef for RedNetherBrickWall {
    const ID: u32 = 833;
    const STRING_ID: &'static str = "minecraft:red_nether_brick_wall";
    const NAME: &'static str = "Red Nether Brick Wall";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14030;
    const MAX_STATE_ID: u32 = 14191;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Shulker Box
pub struct PurpleShulkerBox;

impl BlockDef for PurpleShulkerBox {
    const ID: u32 = 688;
    const STRING_ID: &'static str = "minecraft:purple_shulker_box";
    const NAME: &'static str = "Purple Shulker Box";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 14192;
    const MAX_STATE_ID: u32 = 14192;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Carved Pumpkin
pub struct CarvedPumpkin;

impl BlockDef for CarvedPumpkin {
    const ID: u32 = 296;
    const STRING_ID: &'static str = "minecraft:carved_pumpkin";
    const NAME: &'static str = "Carved Pumpkin";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14193;
    const MAX_STATE_ID: u32 = 14196;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dropper
pub struct Dropper;

impl BlockDef for Dropper {
    const ID: u32 = 125;
    const STRING_ID: &'static str = "minecraft:dropper";
    const NAME: &'static str = "Dropper";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14197;
    const MAX_STATE_ID: u32 = 14208;
    type State = super::states::DispenserState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Sign
pub struct SpruceWallSign;

impl BlockDef for SpruceWallSign {
    const ID: u32 = 225;
    const STRING_ID: &'static str = "minecraft:spruce_wall_sign";
    const NAME: &'static str = "Spruce Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14209;
    const MAX_STATE_ID: u32 = 14214;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Warped Stem
pub struct StrippedWarpedStem;

impl BlockDef for StrippedWarpedStem {
    const ID: u32 = 863;
    const STRING_ID: &'static str = "minecraft:stripped_warped_stem";
    const NAME: &'static str = "Stripped Warped Stem";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14215;
    const MAX_STATE_ID: u32 = 14217;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Candle
pub struct Candle;

impl BlockDef for Candle {
    const ID: u32 = 944;
    const STRING_ID: &'static str = "minecraft:candle";
    const NAME: &'static str = "Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14218;
    const MAX_STATE_ID: u32 = 14225;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Andesite Slab
pub struct PolishedAndesiteSlab;

impl BlockDef for PolishedAndesiteSlab {
    const ID: u32 = 8066;
    const STRING_ID: &'static str = "minecraft:polished_andesite_slab";
    const NAME: &'static str = "Polished Andesite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14226;
    const MAX_STATE_ID: u32 = 14227;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pointed Dripstone
pub struct PointedDripstone;

impl BlockDef for PointedDripstone {
    const ID: u32 = 1133;
    const STRING_ID: &'static str = "minecraft:pointed_dripstone";
    const NAME: &'static str = "Pointed Dripstone";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14228;
    const MAX_STATE_ID: u32 = 14237;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Carpet
pub struct RedCarpet;

impl BlockDef for RedCarpet {
    const ID: u32 = 552;
    const STRING_ID: &'static str = "minecraft:red_carpet";
    const NAME: &'static str = "Red Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14238;
    const MAX_STATE_ID: u32 = 14238;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Netherreactor
pub struct Netherreactor;

impl BlockDef for Netherreactor {
    const ID: u32 = 247;
    const STRING_ID: &'static str = "minecraft:netherreactor";
    const NAME: &'static str = "Netherreactor";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14239;
    const MAX_STATE_ID: u32 = 14239;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cut Red Sandstone Slab
pub struct CutRedSandstoneSlab;

impl BlockDef for CutRedSandstoneSlab {
    const ID: u32 = 8053;
    const STRING_ID: &'static str = "minecraft:cut_red_sandstone_slab";
    const NAME: &'static str = "Cut Red Sandstone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14240;
    const MAX_STATE_ID: u32 = 14241;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Brick Stairs
pub struct DeepslateBrickStairs;

impl BlockDef for DeepslateBrickStairs {
    const ID: u32 = 1165;
    const STRING_ID: &'static str = "minecraft:deepslate_brick_stairs";
    const NAME: &'static str = "Deepslate Brick Stairs";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14242;
    const MAX_STATE_ID: u32 = 14249;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Prismarine Stairs
pub struct DarkPrismarineStairs;

impl BlockDef for DarkPrismarineStairs {
    const ID: u32 = 532;
    const STRING_ID: &'static str = "minecraft:dark_prismarine_stairs";
    const NAME: &'static str = "Dark Prismarine Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14250;
    const MAX_STATE_ID: u32 = 14257;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Creaking Heart
pub struct CreakingHeart;

impl BlockDef for CreakingHeart {
    const ID: u32 = 199;
    const STRING_ID: &'static str = "minecraft:creaking_heart";
    const NAME: &'static str = "Creaking Heart";
    const HARDNESS: f32 = 10.0_f32;
    const RESISTANCE: f32 = 10.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14258;
    const MAX_STATE_ID: u32 = 14275;
    type State = super::states::CreakingHeartBlockState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Button
pub struct PaleOakButton;

impl BlockDef for PaleOakButton {
    const ID: u32 = 450;
    const STRING_ID: &'static str = "minecraft:pale_oak_button";
    const NAME: &'static str = "Pale Oak Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14276;
    const MAX_STATE_ID: u32 = 14287;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Tuff Bricks
pub struct ChiseledTuffBricks;

impl BlockDef for ChiseledTuffBricks {
    const ID: u32 = 997;
    const STRING_ID: &'static str = "minecraft:chiseled_tuff_bricks";
    const NAME: &'static str = "Chiseled Tuff Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14288;
    const MAX_STATE_ID: u32 = 14288;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Blue Concrete
pub struct LightBlueConcrete;

impl BlockDef for LightBlueConcrete {
    const ID: u32 = 713;
    const STRING_ID: &'static str = "minecraft:light_blue_concrete";
    const NAME: &'static str = "Light Blue Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14289;
    const MAX_STATE_ID: u32 = 14289;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Exposed Copper Golem Statue
pub struct ExposedCopperGolemStatue;

impl BlockDef for ExposedCopperGolemStatue {
    const ID: u32 = 1117;
    const STRING_ID: &'static str = "minecraft:exposed_copper_golem_statue";
    const NAME: &'static str = "Exposed Copper Golem Statue";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14290;
    const MAX_STATE_ID: u32 = 14293;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Tulip
pub struct RedTulip;

impl BlockDef for RedTulip {
    const ID: u32 = 164;
    const STRING_ID: &'static str = "minecraft:red_tulip";
    const NAME: &'static str = "Red Tulip";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14294;
    const MAX_STATE_ID: u32 = 14294;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chemical Heat
pub struct ChemicalHeat;

impl BlockDef for ChemicalHeat {
    const ID: u32 = 192;
    const STRING_ID: &'static str = "minecraft:chemical_heat";
    const NAME: &'static str = "Chemical Heat";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14295;
    const MAX_STATE_ID: u32 = 14295;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tripwire
pub struct TripWire;

impl BlockDef for TripWire {
    const ID: u32 = 402;
    const STRING_ID: &'static str = "minecraft:trip_wire";
    const NAME: &'static str = "Tripwire";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14296;
    const MAX_STATE_ID: u32 = 14311;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cauldron
pub struct Cauldron;

impl BlockDef for Cauldron {
    const ID: u32 = 118;
    const STRING_ID: &'static str = "minecraft:cauldron";
    const NAME: &'static str = "Cauldron";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14312;
    const MAX_STATE_ID: u32 = 14332;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cave Vines
pub struct CaveVinesHeadWithBerries;

impl BlockDef for CaveVinesHeadWithBerries {
    const ID: u32 = 1135;
    const STRING_ID: &'static str = "minecraft:cave_vines_head_with_berries";
    const NAME: &'static str = "Cave Vines";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14333;
    const MAX_STATE_ID: u32 = 14358;
    type State = super::states::GrowingPlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tube Coral Block
pub struct TubeCoralBlock;

impl BlockDef for TubeCoralBlock {
    const ID: u32 = 753;
    const STRING_ID: &'static str = "minecraft:tube_coral_block";
    const NAME: &'static str = "Tube Coral Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14359;
    const MAX_STATE_ID: u32 = 14359;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Red Sandstone
pub struct ChiseledRedSandstone;

impl BlockDef for ChiseledRedSandstone {
    const ID: u32 = 596;
    const STRING_ID: &'static str = "minecraft:chiseled_red_sandstone";
    const NAME: &'static str = "Chiseled Red Sandstone";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14360;
    const MAX_STATE_ID: u32 = 14360;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dead Tube Coral Wall Fan
pub struct DeadTubeCoralWallFan;

impl BlockDef for DeadTubeCoralWallFan {
    const ID: u32 = 778;
    const STRING_ID: &'static str = "minecraft:dead_tube_coral_wall_fan";
    const NAME: &'static str = "Dead Tube Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 14361;
    const MAX_STATE_ID: u32 = 14364;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Sapling
pub struct BirchSapling;

impl BlockDef for BirchSapling {
    const ID: u32 = 27;
    const STRING_ID: &'static str = "minecraft:birch_sapling";
    const NAME: &'static str = "Birch Sapling";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14365;
    const MAX_STATE_ID: u32 = 14366;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dark Oak Trapdoor
pub struct DarkOakTrapdoor;

impl BlockDef for DarkOakTrapdoor {
    const ID: u32 = 322;
    const STRING_ID: &'static str = "minecraft:dark_oak_trapdoor";
    const NAME: &'static str = "Dark Oak Trapdoor";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14367;
    const MAX_STATE_ID: u32 = 14382;
    type State = super::states::TrapdoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Pink Stained Glass Pane
pub struct HardPinkStainedGlassPane;

impl BlockDef for HardPinkStainedGlassPane {
    const ID: u32 = 15115;
    const STRING_ID: &'static str = "minecraft:hard_pink_stained_glass_pane";
    const NAME: &'static str = "Hard Pink Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14383;
    const MAX_STATE_ID: u32 = 14383;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Terracotta
pub struct OrangeTerracotta;

impl BlockDef for OrangeTerracotta {
    const ID: u32 = 485;
    const STRING_ID: &'static str = "minecraft:orange_terracotta";
    const NAME: &'static str = "Orange Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14384;
    const MAX_STATE_ID: u32 = 14384;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brick Slab
pub struct BrickSlab;

impl BlockDef for BrickSlab {
    const ID: u32 = 8047;
    const STRING_ID: &'static str = "minecraft:brick_slab";
    const NAME: &'static str = "Brick Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14385;
    const MAX_STATE_ID: u32 = 14386;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper
pub struct WaxedOxidizedCopper;

impl BlockDef for WaxedOxidizedCopper {
    const ID: u32 = 1041;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper";
    const NAME: &'static str = "Waxed Oxidized Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14387;
    const MAX_STATE_ID: u32 = 14387;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Planks
pub struct OakPlanks;

impl BlockDef for OakPlanks {
    const ID: u32 = 13;
    const STRING_ID: &'static str = "minecraft:oak_planks";
    const NAME: &'static str = "Oak Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14388;
    const MAX_STATE_ID: u32 = 14388;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Oak Log
pub struct StrippedOakLog;

impl BlockDef for StrippedOakLog {
    const ID: u32 = 68;
    const STRING_ID: &'static str = "minecraft:stripped_oak_log";
    const NAME: &'static str = "Stripped Oak Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14389;
    const MAX_STATE_ID: u32 = 14391;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Stone Slab
pub struct SmoothStoneSlab;

impl BlockDef for SmoothStoneSlab {
    const ID: u32 = 8042;
    const STRING_ID: &'static str = "minecraft:smooth_stone_slab";
    const NAME: &'static str = "Smooth Stone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14392;
    const MAX_STATE_ID: u32 = 14393;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Andesite
pub struct PolishedAndesite;

impl BlockDef for PolishedAndesite {
    const ID: u32 = 7;
    const STRING_ID: &'static str = "minecraft:polished_andesite";
    const NAME: &'static str = "Polished Andesite";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14394;
    const MAX_STATE_ID: u32 = 14394;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sea Lantern
pub struct SeaLantern;

impl BlockDef for SeaLantern {
    const ID: u32 = 536;
    const STRING_ID: &'static str = "minecraft:sea_lantern";
    const NAME: &'static str = "Sea Lantern";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14395;
    const MAX_STATE_ID: u32 = 14395;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brewing Stand
pub struct BrewingStand;

impl BlockDef for BrewingStand {
    const ID: u32 = 117;
    const STRING_ID: &'static str = "minecraft:brewing_stand";
    const NAME: &'static str = "Brewing Stand";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 1;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14396;
    const MAX_STATE_ID: u32 = 14403;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Shoot
pub struct BambooSapling;

impl BlockDef for BambooSapling {
    const ID: u32 = 791;
    const STRING_ID: &'static str = "minecraft:bamboo_sapling";
    const NAME: &'static str = "Bamboo Shoot";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14404;
    const MAX_STATE_ID: u32 = 14405;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper Bulb
pub struct WeatheredCopperBulb;

impl BlockDef for WeatheredCopperBulb {
    const ID: u32 = 1102;
    const STRING_ID: &'static str = "minecraft:weathered_copper_bulb";
    const NAME: &'static str = "Weathered Copper Bulb";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14406;
    const MAX_STATE_ID: u32 = 14409;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper Bars
pub struct WeatheredCopperBars;

impl BlockDef for WeatheredCopperBars {
    const ID: u32 = 344;
    const STRING_ID: &'static str = "minecraft:weathered_copper_bars";
    const NAME: &'static str = "Weathered Copper Bars";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14410;
    const MAX_STATE_ID: u32 = 14410;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blast Furnace
pub struct BlastFurnace;

impl BlockDef for BlastFurnace {
    const ID: u32 = 8069;
    const STRING_ID: &'static str = "minecraft:blast_furnace";
    const NAME: &'static str = "Blast Furnace";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14411;
    const MAX_STATE_ID: u32 = 14414;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Roots
pub struct CrimsonRoots;

impl BlockDef for CrimsonRoots {
    const ID: u32 = 882;
    const STRING_ID: &'static str = "minecraft:crimson_roots";
    const NAME: &'static str = "Crimson Roots";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14415;
    const MAX_STATE_ID: u32 = 14415;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Slab
pub struct AcaciaSlab;

impl BlockDef for AcaciaSlab {
    const ID: u32 = 8034;
    const STRING_ID: &'static str = "minecraft:acacia_slab";
    const NAME: &'static str = "Acacia Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14416;
    const MAX_STATE_ID: u32 = 14417;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stonecutter
pub struct StonecutterBlock;

impl BlockDef for StonecutterBlock {
    const ID: u32 = 847;
    const STRING_ID: &'static str = "minecraft:stonecutter_block";
    const NAME: &'static str = "Stonecutter";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14418;
    const MAX_STATE_ID: u32 = 14421;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Quartz Slab
pub struct SmoothQuartzSlab;

impl BlockDef for SmoothQuartzSlab {
    const ID: u32 = 8062;
    const STRING_ID: &'static str = "minecraft:smooth_quartz_slab";
    const NAME: &'static str = "Smooth Quartz Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14422;
    const MAX_STATE_ID: u32 = 14423;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Concrete Powder
pub struct YellowConcretePowder;

impl BlockDef for YellowConcretePowder {
    const ID: u32 = 730;
    const STRING_ID: &'static str = "minecraft:yellow_concrete_powder";
    const NAME: &'static str = "Yellow Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14424;
    const MAX_STATE_ID: u32 = 14424;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with White Candle
pub struct WhiteCandleCake;

impl BlockDef for WhiteCandleCake {
    const ID: u32 = 962;
    const STRING_ID: &'static str = "minecraft:white_candle_cake";
    const NAME: &'static str = "Cake with White Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14425;
    const MAX_STATE_ID: u32 = 14426;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Candle
pub struct CandleCake;

impl BlockDef for CandleCake {
    const ID: u32 = 961;
    const STRING_ID: &'static str = "minecraft:candle_cake";
    const NAME: &'static str = "Cake with Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14427;
    const MAX_STATE_ID: u32 = 14428;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Stained Glass Pane
pub struct LimeStainedGlassPane;

impl BlockDef for LimeStainedGlassPane {
    const ID: u32 = 505;
    const STRING_ID: &'static str = "minecraft:lime_stained_glass_pane";
    const NAME: &'static str = "Lime Stained Glass Pane";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14429;
    const MAX_STATE_ID: u32 = 14429;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// End Portal
pub struct EndPortal;

impl BlockDef for EndPortal {
    const ID: u32 = 119;
    const STRING_ID: &'static str = "minecraft:end_portal";
    const NAME: &'static str = "End Portal";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14430;
    const MAX_STATE_ID: u32 = 14430;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Yellow Stained Glass
pub struct YellowStainedGlass;

impl BlockDef for YellowStainedGlass {
    const ID: u32 = 304;
    const STRING_ID: &'static str = "minecraft:yellow_stained_glass";
    const NAME: &'static str = "Yellow Stained Glass";
    const HARDNESS: f32 = 0.3_f32;
    const RESISTANCE: f32 = 0.3_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14431;
    const MAX_STATE_ID: u32 = 14431;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Jungle Slab
pub struct JungleDoubleSlab;

impl BlockDef for JungleDoubleSlab {
    const ID: u32 = 8033;
    const STRING_ID: &'static str = "minecraft:jungle_double_slab";
    const NAME: &'static str = "Jungle Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14432;
    const MAX_STATE_ID: u32 = 14433;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Granite Slab
pub struct PolishedGraniteDoubleSlab;

impl BlockDef for PolishedGraniteDoubleSlab {
    const ID: u32 = 8055;
    const STRING_ID: &'static str = "minecraft:polished_granite_double_slab";
    const NAME: &'static str = "Polished Granite Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14434;
    const MAX_STATE_ID: u32 = 14435;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Wood
pub struct SpruceWood;

impl BlockDef for SpruceWood {
    const ID: u32 = 72;
    const STRING_ID: &'static str = "minecraft:spruce_wood";
    const NAME: &'static str = "Spruce Wood";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14436;
    const MAX_STATE_ID: u32 = 14438;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Blackstone
pub struct Blackstone;

impl BlockDef for Blackstone {
    const ID: u32 = 924;
    const STRING_ID: &'static str = "minecraft:blackstone";
    const NAME: &'static str = "Blackstone";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14439;
    const MAX_STATE_ID: u32 = 14439;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Fence Gate
pub struct AcaciaFenceGate;

impl BlockDef for AcaciaFenceGate {
    const ID: u32 = 187;
    const STRING_ID: &'static str = "minecraft:acacia_fence_gate";
    const NAME: &'static str = "Acacia Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14440;
    const MAX_STATE_ID: u32 = 14455;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Redstone Ore
pub struct LitDeepslateRedstoneOre;

impl BlockDef for LitDeepslateRedstoneOre {
    const ID: u32 = 8005;
    const STRING_ID: &'static str = "minecraft:lit_deepslate_redstone_ore";
    const NAME: &'static str = "Deepslate Redstone Ore";
    const HARDNESS: f32 = 4.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14456;
    const MAX_STATE_ID: u32 = 14456;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Wildflowers
pub struct Wildflowers;

impl BlockDef for Wildflowers {
    const ID: u32 = 1142;
    const STRING_ID: &'static str = "minecraft:wildflowers";
    const NAME: &'static str = "Wildflowers";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14457;
    const MAX_STATE_ID: u32 = 14488;
    type State = super::states::PetalsState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 10
pub struct Element10;

impl BlockDef for Element10 {
    const ID: u32 = 15231;
    const STRING_ID: &'static str = "minecraft:element_10";
    const NAME: &'static str = "Element 10";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14489;
    const MAX_STATE_ID: u32 = 14489;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 11
pub struct Element11;

impl BlockDef for Element11 {
    const ID: u32 = 15232;
    const STRING_ID: &'static str = "minecraft:element_11";
    const NAME: &'static str = "Element 11";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14490;
    const MAX_STATE_ID: u32 = 14490;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 12
pub struct Element12;

impl BlockDef for Element12 {
    const ID: u32 = 15233;
    const STRING_ID: &'static str = "minecraft:element_12";
    const NAME: &'static str = "Element 12";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14491;
    const MAX_STATE_ID: u32 = 14491;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 13
pub struct Element13;

impl BlockDef for Element13 {
    const ID: u32 = 15234;
    const STRING_ID: &'static str = "minecraft:element_13";
    const NAME: &'static str = "Element 13";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14492;
    const MAX_STATE_ID: u32 = 14492;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 14
pub struct Element14;

impl BlockDef for Element14 {
    const ID: u32 = 15235;
    const STRING_ID: &'static str = "minecraft:element_14";
    const NAME: &'static str = "Element 14";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14493;
    const MAX_STATE_ID: u32 = 14493;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 15
pub struct Element15;

impl BlockDef for Element15 {
    const ID: u32 = 15236;
    const STRING_ID: &'static str = "minecraft:element_15";
    const NAME: &'static str = "Element 15";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14494;
    const MAX_STATE_ID: u32 = 14494;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 16
pub struct Element16;

impl BlockDef for Element16 {
    const ID: u32 = 15237;
    const STRING_ID: &'static str = "minecraft:element_16";
    const NAME: &'static str = "Element 16";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14495;
    const MAX_STATE_ID: u32 = 14495;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 17
pub struct Element17;

impl BlockDef for Element17 {
    const ID: u32 = 15238;
    const STRING_ID: &'static str = "minecraft:element_17";
    const NAME: &'static str = "Element 17";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14496;
    const MAX_STATE_ID: u32 = 14496;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 18
pub struct Element18;

impl BlockDef for Element18 {
    const ID: u32 = 15239;
    const STRING_ID: &'static str = "minecraft:element_18";
    const NAME: &'static str = "Element 18";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14497;
    const MAX_STATE_ID: u32 = 14497;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 19
pub struct Element19;

impl BlockDef for Element19 {
    const ID: u32 = 15240;
    const STRING_ID: &'static str = "minecraft:element_19";
    const NAME: &'static str = "Element 19";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14498;
    const MAX_STATE_ID: u32 = 14498;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 36
pub struct Element36;

impl BlockDef for Element36 {
    const ID: u32 = 15241;
    const STRING_ID: &'static str = "minecraft:element_36";
    const NAME: &'static str = "Element 36";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14499;
    const MAX_STATE_ID: u32 = 14499;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 37
pub struct Element37;

impl BlockDef for Element37 {
    const ID: u32 = 15242;
    const STRING_ID: &'static str = "minecraft:element_37";
    const NAME: &'static str = "Element 37";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14500;
    const MAX_STATE_ID: u32 = 14500;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 34
pub struct Element34;

impl BlockDef for Element34 {
    const ID: u32 = 15243;
    const STRING_ID: &'static str = "minecraft:element_34";
    const NAME: &'static str = "Element 34";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14501;
    const MAX_STATE_ID: u32 = 14501;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 35
pub struct Element35;

impl BlockDef for Element35 {
    const ID: u32 = 15244;
    const STRING_ID: &'static str = "minecraft:element_35";
    const NAME: &'static str = "Element 35";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14502;
    const MAX_STATE_ID: u32 = 14502;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 32
pub struct Element32;

impl BlockDef for Element32 {
    const ID: u32 = 15245;
    const STRING_ID: &'static str = "minecraft:element_32";
    const NAME: &'static str = "Element 32";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14503;
    const MAX_STATE_ID: u32 = 14503;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 33
pub struct Element33;

impl BlockDef for Element33 {
    const ID: u32 = 15246;
    const STRING_ID: &'static str = "minecraft:element_33";
    const NAME: &'static str = "Element 33";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14504;
    const MAX_STATE_ID: u32 = 14504;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 30
pub struct Element30;

impl BlockDef for Element30 {
    const ID: u32 = 15247;
    const STRING_ID: &'static str = "minecraft:element_30";
    const NAME: &'static str = "Element 30";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14505;
    const MAX_STATE_ID: u32 = 14505;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 31
pub struct Element31;

impl BlockDef for Element31 {
    const ID: u32 = 15248;
    const STRING_ID: &'static str = "minecraft:element_31";
    const NAME: &'static str = "Element 31";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14506;
    const MAX_STATE_ID: u32 = 14506;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 38
pub struct Element38;

impl BlockDef for Element38 {
    const ID: u32 = 15249;
    const STRING_ID: &'static str = "minecraft:element_38";
    const NAME: &'static str = "Element 38";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14507;
    const MAX_STATE_ID: u32 = 14507;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 39
pub struct Element39;

impl BlockDef for Element39 {
    const ID: u32 = 15250;
    const STRING_ID: &'static str = "minecraft:element_39";
    const NAME: &'static str = "Element 39";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14508;
    const MAX_STATE_ID: u32 = 14508;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 29
pub struct Element29;

impl BlockDef for Element29 {
    const ID: u32 = 15251;
    const STRING_ID: &'static str = "minecraft:element_29";
    const NAME: &'static str = "Element 29";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14509;
    const MAX_STATE_ID: u32 = 14509;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 28
pub struct Element28;

impl BlockDef for Element28 {
    const ID: u32 = 15252;
    const STRING_ID: &'static str = "minecraft:element_28";
    const NAME: &'static str = "Element 28";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14510;
    const MAX_STATE_ID: u32 = 14510;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 21
pub struct Element21;

impl BlockDef for Element21 {
    const ID: u32 = 15253;
    const STRING_ID: &'static str = "minecraft:element_21";
    const NAME: &'static str = "Element 21";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14511;
    const MAX_STATE_ID: u32 = 14511;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 20
pub struct Element20;

impl BlockDef for Element20 {
    const ID: u32 = 15254;
    const STRING_ID: &'static str = "minecraft:element_20";
    const NAME: &'static str = "Element 20";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14512;
    const MAX_STATE_ID: u32 = 14512;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 23
pub struct Element23;

impl BlockDef for Element23 {
    const ID: u32 = 15255;
    const STRING_ID: &'static str = "minecraft:element_23";
    const NAME: &'static str = "Element 23";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14513;
    const MAX_STATE_ID: u32 = 14513;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 22
pub struct Element22;

impl BlockDef for Element22 {
    const ID: u32 = 15256;
    const STRING_ID: &'static str = "minecraft:element_22";
    const NAME: &'static str = "Element 22";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14514;
    const MAX_STATE_ID: u32 = 14514;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 25
pub struct Element25;

impl BlockDef for Element25 {
    const ID: u32 = 15257;
    const STRING_ID: &'static str = "minecraft:element_25";
    const NAME: &'static str = "Element 25";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14515;
    const MAX_STATE_ID: u32 = 14515;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 24
pub struct Element24;

impl BlockDef for Element24 {
    const ID: u32 = 15258;
    const STRING_ID: &'static str = "minecraft:element_24";
    const NAME: &'static str = "Element 24";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14516;
    const MAX_STATE_ID: u32 = 14516;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 27
pub struct Element27;

impl BlockDef for Element27 {
    const ID: u32 = 15259;
    const STRING_ID: &'static str = "minecraft:element_27";
    const NAME: &'static str = "Element 27";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14517;
    const MAX_STATE_ID: u32 = 14517;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 26
pub struct Element26;

impl BlockDef for Element26 {
    const ID: u32 = 15260;
    const STRING_ID: &'static str = "minecraft:element_26";
    const NAME: &'static str = "Element 26";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14518;
    const MAX_STATE_ID: u32 = 14518;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 58
pub struct Element58;

impl BlockDef for Element58 {
    const ID: u32 = 15261;
    const STRING_ID: &'static str = "minecraft:element_58";
    const NAME: &'static str = "Element 58";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14519;
    const MAX_STATE_ID: u32 = 14519;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 59
pub struct Element59;

impl BlockDef for Element59 {
    const ID: u32 = 15262;
    const STRING_ID: &'static str = "minecraft:element_59";
    const NAME: &'static str = "Element 59";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14520;
    const MAX_STATE_ID: u32 = 14520;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 54
pub struct Element54;

impl BlockDef for Element54 {
    const ID: u32 = 15263;
    const STRING_ID: &'static str = "minecraft:element_54";
    const NAME: &'static str = "Element 54";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14521;
    const MAX_STATE_ID: u32 = 14521;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 55
pub struct Element55;

impl BlockDef for Element55 {
    const ID: u32 = 15264;
    const STRING_ID: &'static str = "minecraft:element_55";
    const NAME: &'static str = "Element 55";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14522;
    const MAX_STATE_ID: u32 = 14522;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 56
pub struct Element56;

impl BlockDef for Element56 {
    const ID: u32 = 15265;
    const STRING_ID: &'static str = "minecraft:element_56";
    const NAME: &'static str = "Element 56";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14523;
    const MAX_STATE_ID: u32 = 14523;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 57
pub struct Element57;

impl BlockDef for Element57 {
    const ID: u32 = 15266;
    const STRING_ID: &'static str = "minecraft:element_57";
    const NAME: &'static str = "Element 57";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14524;
    const MAX_STATE_ID: u32 = 14524;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 50
pub struct Element50;

impl BlockDef for Element50 {
    const ID: u32 = 15267;
    const STRING_ID: &'static str = "minecraft:element_50";
    const NAME: &'static str = "Element 50";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14525;
    const MAX_STATE_ID: u32 = 14525;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 51
pub struct Element51;

impl BlockDef for Element51 {
    const ID: u32 = 15268;
    const STRING_ID: &'static str = "minecraft:element_51";
    const NAME: &'static str = "Element 51";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14526;
    const MAX_STATE_ID: u32 = 14526;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 52
pub struct Element52;

impl BlockDef for Element52 {
    const ID: u32 = 15269;
    const STRING_ID: &'static str = "minecraft:element_52";
    const NAME: &'static str = "Element 52";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14527;
    const MAX_STATE_ID: u32 = 14527;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 53
pub struct Element53;

impl BlockDef for Element53 {
    const ID: u32 = 15270;
    const STRING_ID: &'static str = "minecraft:element_53";
    const NAME: &'static str = "Element 53";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14528;
    const MAX_STATE_ID: u32 = 14528;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 49
pub struct Element49;

impl BlockDef for Element49 {
    const ID: u32 = 15271;
    const STRING_ID: &'static str = "minecraft:element_49";
    const NAME: &'static str = "Element 49";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14529;
    const MAX_STATE_ID: u32 = 14529;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 48
pub struct Element48;

impl BlockDef for Element48 {
    const ID: u32 = 15272;
    const STRING_ID: &'static str = "minecraft:element_48";
    const NAME: &'static str = "Element 48";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14530;
    const MAX_STATE_ID: u32 = 14530;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 47
pub struct Element47;

impl BlockDef for Element47 {
    const ID: u32 = 15273;
    const STRING_ID: &'static str = "minecraft:element_47";
    const NAME: &'static str = "Element 47";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14531;
    const MAX_STATE_ID: u32 = 14531;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 46
pub struct Element46;

impl BlockDef for Element46 {
    const ID: u32 = 15274;
    const STRING_ID: &'static str = "minecraft:element_46";
    const NAME: &'static str = "Element 46";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14532;
    const MAX_STATE_ID: u32 = 14532;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 45
pub struct Element45;

impl BlockDef for Element45 {
    const ID: u32 = 15275;
    const STRING_ID: &'static str = "minecraft:element_45";
    const NAME: &'static str = "Element 45";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14533;
    const MAX_STATE_ID: u32 = 14533;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 44
pub struct Element44;

impl BlockDef for Element44 {
    const ID: u32 = 15276;
    const STRING_ID: &'static str = "minecraft:element_44";
    const NAME: &'static str = "Element 44";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14534;
    const MAX_STATE_ID: u32 = 14534;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 43
pub struct Element43;

impl BlockDef for Element43 {
    const ID: u32 = 15277;
    const STRING_ID: &'static str = "minecraft:element_43";
    const NAME: &'static str = "Element 43";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14535;
    const MAX_STATE_ID: u32 = 14535;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 42
pub struct Element42;

impl BlockDef for Element42 {
    const ID: u32 = 15278;
    const STRING_ID: &'static str = "minecraft:element_42";
    const NAME: &'static str = "Element 42";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14536;
    const MAX_STATE_ID: u32 = 14536;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 41
pub struct Element41;

impl BlockDef for Element41 {
    const ID: u32 = 15279;
    const STRING_ID: &'static str = "minecraft:element_41";
    const NAME: &'static str = "Element 41";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14537;
    const MAX_STATE_ID: u32 = 14537;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 40
pub struct Element40;

impl BlockDef for Element40 {
    const ID: u32 = 15280;
    const STRING_ID: &'static str = "minecraft:element_40";
    const NAME: &'static str = "Element 40";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14538;
    const MAX_STATE_ID: u32 = 14538;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 72
pub struct Element72;

impl BlockDef for Element72 {
    const ID: u32 = 15281;
    const STRING_ID: &'static str = "minecraft:element_72";
    const NAME: &'static str = "Element 72";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14539;
    const MAX_STATE_ID: u32 = 14539;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 73
pub struct Element73;

impl BlockDef for Element73 {
    const ID: u32 = 15282;
    const STRING_ID: &'static str = "minecraft:element_73";
    const NAME: &'static str = "Element 73";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14540;
    const MAX_STATE_ID: u32 = 14540;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 70
pub struct Element70;

impl BlockDef for Element70 {
    const ID: u32 = 15283;
    const STRING_ID: &'static str = "minecraft:element_70";
    const NAME: &'static str = "Element 70";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14541;
    const MAX_STATE_ID: u32 = 14541;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 71
pub struct Element71;

impl BlockDef for Element71 {
    const ID: u32 = 15284;
    const STRING_ID: &'static str = "minecraft:element_71";
    const NAME: &'static str = "Element 71";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14542;
    const MAX_STATE_ID: u32 = 14542;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 76
pub struct Element76;

impl BlockDef for Element76 {
    const ID: u32 = 15285;
    const STRING_ID: &'static str = "minecraft:element_76";
    const NAME: &'static str = "Element 76";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14543;
    const MAX_STATE_ID: u32 = 14543;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 77
pub struct Element77;

impl BlockDef for Element77 {
    const ID: u32 = 15286;
    const STRING_ID: &'static str = "minecraft:element_77";
    const NAME: &'static str = "Element 77";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14544;
    const MAX_STATE_ID: u32 = 14544;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 74
pub struct Element74;

impl BlockDef for Element74 {
    const ID: u32 = 15287;
    const STRING_ID: &'static str = "minecraft:element_74";
    const NAME: &'static str = "Element 74";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14545;
    const MAX_STATE_ID: u32 = 14545;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 75
pub struct Element75;

impl BlockDef for Element75 {
    const ID: u32 = 15288;
    const STRING_ID: &'static str = "minecraft:element_75";
    const NAME: &'static str = "Element 75";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14546;
    const MAX_STATE_ID: u32 = 14546;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 78
pub struct Element78;

impl BlockDef for Element78 {
    const ID: u32 = 15289;
    const STRING_ID: &'static str = "minecraft:element_78";
    const NAME: &'static str = "Element 78";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14547;
    const MAX_STATE_ID: u32 = 14547;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 79
pub struct Element79;

impl BlockDef for Element79 {
    const ID: u32 = 15290;
    const STRING_ID: &'static str = "minecraft:element_79";
    const NAME: &'static str = "Element 79";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14548;
    const MAX_STATE_ID: u32 = 14548;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 65
pub struct Element65;

impl BlockDef for Element65 {
    const ID: u32 = 15291;
    const STRING_ID: &'static str = "minecraft:element_65";
    const NAME: &'static str = "Element 65";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14549;
    const MAX_STATE_ID: u32 = 14549;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 64
pub struct Element64;

impl BlockDef for Element64 {
    const ID: u32 = 15292;
    const STRING_ID: &'static str = "minecraft:element_64";
    const NAME: &'static str = "Element 64";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14550;
    const MAX_STATE_ID: u32 = 14550;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 67
pub struct Element67;

impl BlockDef for Element67 {
    const ID: u32 = 15293;
    const STRING_ID: &'static str = "minecraft:element_67";
    const NAME: &'static str = "Element 67";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14551;
    const MAX_STATE_ID: u32 = 14551;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 66
pub struct Element66;

impl BlockDef for Element66 {
    const ID: u32 = 15294;
    const STRING_ID: &'static str = "minecraft:element_66";
    const NAME: &'static str = "Element 66";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14552;
    const MAX_STATE_ID: u32 = 14552;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 61
pub struct Element61;

impl BlockDef for Element61 {
    const ID: u32 = 15295;
    const STRING_ID: &'static str = "minecraft:element_61";
    const NAME: &'static str = "Element 61";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14553;
    const MAX_STATE_ID: u32 = 14553;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 60
pub struct Element60;

impl BlockDef for Element60 {
    const ID: u32 = 15296;
    const STRING_ID: &'static str = "minecraft:element_60";
    const NAME: &'static str = "Element 60";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14554;
    const MAX_STATE_ID: u32 = 14554;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 63
pub struct Element63;

impl BlockDef for Element63 {
    const ID: u32 = 15297;
    const STRING_ID: &'static str = "minecraft:element_63";
    const NAME: &'static str = "Element 63";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14555;
    const MAX_STATE_ID: u32 = 14555;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 62
pub struct Element62;

impl BlockDef for Element62 {
    const ID: u32 = 15298;
    const STRING_ID: &'static str = "minecraft:element_62";
    const NAME: &'static str = "Element 62";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14556;
    const MAX_STATE_ID: u32 = 14556;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 69
pub struct Element69;

impl BlockDef for Element69 {
    const ID: u32 = 15299;
    const STRING_ID: &'static str = "minecraft:element_69";
    const NAME: &'static str = "Element 69";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14557;
    const MAX_STATE_ID: u32 = 14557;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 68
pub struct Element68;

impl BlockDef for Element68 {
    const ID: u32 = 15300;
    const STRING_ID: &'static str = "minecraft:element_68";
    const NAME: &'static str = "Element 68";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14558;
    const MAX_STATE_ID: u32 = 14558;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 98
pub struct Element98;

impl BlockDef for Element98 {
    const ID: u32 = 15301;
    const STRING_ID: &'static str = "minecraft:element_98";
    const NAME: &'static str = "Element 98";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14559;
    const MAX_STATE_ID: u32 = 14559;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 99
pub struct Element99;

impl BlockDef for Element99 {
    const ID: u32 = 15302;
    const STRING_ID: &'static str = "minecraft:element_99";
    const NAME: &'static str = "Element 99";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14560;
    const MAX_STATE_ID: u32 = 14560;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 90
pub struct Element90;

impl BlockDef for Element90 {
    const ID: u32 = 15303;
    const STRING_ID: &'static str = "minecraft:element_90";
    const NAME: &'static str = "Element 90";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14561;
    const MAX_STATE_ID: u32 = 14561;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 91
pub struct Element91;

impl BlockDef for Element91 {
    const ID: u32 = 15304;
    const STRING_ID: &'static str = "minecraft:element_91";
    const NAME: &'static str = "Element 91";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14562;
    const MAX_STATE_ID: u32 = 14562;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 92
pub struct Element92;

impl BlockDef for Element92 {
    const ID: u32 = 15305;
    const STRING_ID: &'static str = "minecraft:element_92";
    const NAME: &'static str = "Element 92";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14563;
    const MAX_STATE_ID: u32 = 14563;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 93
pub struct Element93;

impl BlockDef for Element93 {
    const ID: u32 = 15306;
    const STRING_ID: &'static str = "minecraft:element_93";
    const NAME: &'static str = "Element 93";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14564;
    const MAX_STATE_ID: u32 = 14564;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 94
pub struct Element94;

impl BlockDef for Element94 {
    const ID: u32 = 15307;
    const STRING_ID: &'static str = "minecraft:element_94";
    const NAME: &'static str = "Element 94";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14565;
    const MAX_STATE_ID: u32 = 14565;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 95
pub struct Element95;

impl BlockDef for Element95 {
    const ID: u32 = 15308;
    const STRING_ID: &'static str = "minecraft:element_95";
    const NAME: &'static str = "Element 95";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14566;
    const MAX_STATE_ID: u32 = 14566;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 96
pub struct Element96;

impl BlockDef for Element96 {
    const ID: u32 = 15309;
    const STRING_ID: &'static str = "minecraft:element_96";
    const NAME: &'static str = "Element 96";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14567;
    const MAX_STATE_ID: u32 = 14567;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 97
pub struct Element97;

impl BlockDef for Element97 {
    const ID: u32 = 15310;
    const STRING_ID: &'static str = "minecraft:element_97";
    const NAME: &'static str = "Element 97";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14568;
    const MAX_STATE_ID: u32 = 14568;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 89
pub struct Element89;

impl BlockDef for Element89 {
    const ID: u32 = 15311;
    const STRING_ID: &'static str = "minecraft:element_89";
    const NAME: &'static str = "Element 89";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14569;
    const MAX_STATE_ID: u32 = 14569;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 88
pub struct Element88;

impl BlockDef for Element88 {
    const ID: u32 = 15312;
    const STRING_ID: &'static str = "minecraft:element_88";
    const NAME: &'static str = "Element 88";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14570;
    const MAX_STATE_ID: u32 = 14570;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 83
pub struct Element83;

impl BlockDef for Element83 {
    const ID: u32 = 15313;
    const STRING_ID: &'static str = "minecraft:element_83";
    const NAME: &'static str = "Element 83";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14571;
    const MAX_STATE_ID: u32 = 14571;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 82
pub struct Element82;

impl BlockDef for Element82 {
    const ID: u32 = 15314;
    const STRING_ID: &'static str = "minecraft:element_82";
    const NAME: &'static str = "Element 82";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14572;
    const MAX_STATE_ID: u32 = 14572;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 81
pub struct Element81;

impl BlockDef for Element81 {
    const ID: u32 = 15315;
    const STRING_ID: &'static str = "minecraft:element_81";
    const NAME: &'static str = "Element 81";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14573;
    const MAX_STATE_ID: u32 = 14573;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 80
pub struct Element80;

impl BlockDef for Element80 {
    const ID: u32 = 15316;
    const STRING_ID: &'static str = "minecraft:element_80";
    const NAME: &'static str = "Element 80";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14574;
    const MAX_STATE_ID: u32 = 14574;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 87
pub struct Element87;

impl BlockDef for Element87 {
    const ID: u32 = 15317;
    const STRING_ID: &'static str = "minecraft:element_87";
    const NAME: &'static str = "Element 87";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14575;
    const MAX_STATE_ID: u32 = 14575;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 86
pub struct Element86;

impl BlockDef for Element86 {
    const ID: u32 = 15318;
    const STRING_ID: &'static str = "minecraft:element_86";
    const NAME: &'static str = "Element 86";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14576;
    const MAX_STATE_ID: u32 = 14576;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 85
pub struct Element85;

impl BlockDef for Element85 {
    const ID: u32 = 15319;
    const STRING_ID: &'static str = "minecraft:element_85";
    const NAME: &'static str = "Element 85";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14577;
    const MAX_STATE_ID: u32 = 14577;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Element 84
pub struct Element84;

impl BlockDef for Element84 {
    const ID: u32 = 15320;
    const STRING_ID: &'static str = "minecraft:element_84";
    const NAME: &'static str = "Element 84";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14578;
    const MAX_STATE_ID: u32 = 14578;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smoker
pub struct LitSmoker;

impl BlockDef for LitSmoker {
    const ID: u32 = 8068;
    const STRING_ID: &'static str = "minecraft:lit_smoker";
    const NAME: &'static str = "Smoker";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14579;
    const MAX_STATE_ID: u32 = 14582;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lapis Lazuli Ore
pub struct LapisOre;

impl BlockDef for LapisOre {
    const ID: u32 = 21;
    const STRING_ID: &'static str = "minecraft:lapis_ore";
    const NAME: &'static str = "Lapis Lazuli Ore";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14583;
    const MAX_STATE_ID: u32 = 14583;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Red Concrete
pub struct RedConcrete;

impl BlockDef for RedConcrete {
    const ID: u32 = 724;
    const STRING_ID: &'static str = "minecraft:red_concrete";
    const NAME: &'static str = "Red Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14584;
    const MAX_STATE_ID: u32 = 14584;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pink Carpet
pub struct PinkCarpet;

impl BlockDef for PinkCarpet {
    const ID: u32 = 544;
    const STRING_ID: &'static str = "minecraft:pink_carpet";
    const NAME: &'static str = "Pink Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14585;
    const MAX_STATE_ID: u32 = 14585;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Quartz Stairs
pub struct SmoothQuartzStairs;

impl BlockDef for SmoothQuartzStairs {
    const ID: u32 = 805;
    const STRING_ID: &'static str = "minecraft:smooth_quartz_stairs";
    const NAME: &'static str = "Smooth Quartz Stairs";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14586;
    const MAX_STATE_ID: u32 = 14593;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Red Candle
pub struct RedCandleCake;

impl BlockDef for RedCandleCake {
    const ID: u32 = 976;
    const STRING_ID: &'static str = "minecraft:red_candle_cake";
    const NAME: &'static str = "Cake with Red Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14594;
    const MAX_STATE_ID: u32 = 14595;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Copper Lantern
pub struct WaxedCopperLantern;

impl BlockDef for WaxedCopperLantern {
    const ID: u32 = 855;
    const STRING_ID: &'static str = "minecraft:waxed_copper_lantern";
    const NAME: &'static str = "Waxed Copper Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14596;
    const MAX_STATE_ID: u32 = 14597;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Azalea Leaves
pub struct AzaleaLeaves;

impl BlockDef for AzaleaLeaves {
    const ID: u32 = 97;
    const STRING_ID: &'static str = "minecraft:azalea_leaves";
    const NAME: &'static str = "Azalea Leaves";
    const HARDNESS: f32 = 0.2_f32;
    const RESISTANCE: f32 = 0.2_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 14598;
    const MAX_STATE_ID: u32 = 14601;
    type State = super::states::LeavesState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purpur Block
pub struct PurpurBlock;

impl BlockDef for PurpurBlock {
    const ID: u32 = 201;
    const STRING_ID: &'static str = "minecraft:purpur_block";
    const NAME: &'static str = "Purpur Block";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14602;
    const MAX_STATE_ID: u32 = 14604;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Sign
pub struct CherryWallSign;

impl BlockDef for CherryWallSign {
    const ID: u32 = 228;
    const STRING_ID: &'static str = "minecraft:cherry_wall_sign";
    const NAME: &'static str = "Cherry Sign";
    const HARDNESS: f32 = 1.0_f32;
    const RESISTANCE: f32 = 1.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14605;
    const MAX_STATE_ID: u32 = 14610;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cyan Candle
pub struct CyanCandle;

impl BlockDef for CyanCandle {
    const ID: u32 = 954;
    const STRING_ID: &'static str = "minecraft:cyan_candle";
    const NAME: &'static str = "Cyan Candle";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14611;
    const MAX_STATE_ID: u32 = 14618;
    type State = super::states::CandleState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Block of Copper
pub struct WaxedCopper;

impl BlockDef for WaxedCopper {
    const ID: u32 = 1038;
    const STRING_ID: &'static str = "minecraft:waxed_copper";
    const NAME: &'static str = "Waxed Block of Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14619;
    const MAX_STATE_ID: u32 = 14619;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Repeating Command Block
pub struct RepeatingCommandBlock;

impl BlockDef for RepeatingCommandBlock {
    const ID: u32 = 188;
    const STRING_ID: &'static str = "minecraft:repeating_command_block";
    const NAME: &'static str = "Repeating Command Block";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14620;
    const MAX_STATE_ID: u32 = 14631;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Colored Torch Purple
pub struct ColoredTorchPurple;

impl BlockDef for ColoredTorchPurple {
    const ID: u32 = 15374;
    const STRING_ID: &'static str = "minecraft:colored_torch_purple";
    const NAME: &'static str = "Colored Torch Purple";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14632;
    const MAX_STATE_ID: u32 = 14637;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Wart
pub struct NetherWart;

impl BlockDef for NetherWart {
    const ID: u32 = 115;
    const STRING_ID: &'static str = "minecraft:nether_wart";
    const NAME: &'static str = "Nether Wart";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14638;
    const MAX_STATE_ID: u32 = 14641;
    type State = super::states::AgeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Carpet
pub struct PurpleCarpet;

impl BlockDef for PurpleCarpet {
    const ID: u32 = 548;
    const STRING_ID: &'static str = "minecraft:purple_carpet";
    const NAME: &'static str = "Purple Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14642;
    const MAX_STATE_ID: u32 = 14642;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Cut Copper Slab
pub struct WaxedOxidizedDoubleCutCopperSlab;

impl BlockDef for WaxedOxidizedDoubleCutCopperSlab {
    const ID: u32 = 1075;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_double_cut_copper_slab";
    const NAME: &'static str = "Waxed Oxidized Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14643;
    const MAX_STATE_ID: u32 = 14644;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Fungus
pub struct CrimsonFungus;

impl BlockDef for CrimsonFungus {
    const ID: u32 = 876;
    const STRING_ID: &'static str = "minecraft:crimson_fungus";
    const NAME: &'static str = "Crimson Fungus";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14645;
    const MAX_STATE_ID: u32 = 14645;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Planks
pub struct CherryPlanks;

impl BlockDef for CherryPlanks {
    const ID: u32 = 18;
    const STRING_ID: &'static str = "minecraft:cherry_planks";
    const NAME: &'static str = "Cherry Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14646;
    const MAX_STATE_ID: u32 = 14646;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Deepslate
pub struct PolishedDeepslate;

impl BlockDef for PolishedDeepslate {
    const ID: u32 = 1156;
    const STRING_ID: &'static str = "minecraft:polished_deepslate";
    const NAME: &'static str = "Polished Deepslate";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14647;
    const MAX_STATE_ID: u32 = 14647;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tuff Slab
pub struct TuffDoubleSlab;

impl BlockDef for TuffDoubleSlab {
    const ID: u32 = 8075;
    const STRING_ID: &'static str = "minecraft:tuff_double_slab";
    const NAME: &'static str = "Tuff Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14648;
    const MAX_STATE_ID: u32 = 14649;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Smooth Red Sandstone
pub struct SmoothRedSandstone;

impl BlockDef for SmoothRedSandstone {
    const ID: u32 = 627;
    const STRING_ID: &'static str = "minecraft:smooth_red_sandstone";
    const NAME: &'static str = "Smooth Red Sandstone";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14650;
    const MAX_STATE_ID: u32 = 14650;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purpur Stairs
pub struct PurpurStairs;

impl BlockDef for PurpurStairs {
    const ID: u32 = 203;
    const STRING_ID: &'static str = "minecraft:purpur_stairs";
    const NAME: &'static str = "Purpur Stairs";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14651;
    const MAX_STATE_ID: u32 = 14658;
    type State = super::states::StairState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tube Coral
pub struct TubeCoral;

impl BlockDef for TubeCoral {
    const ID: u32 = 763;
    const STRING_ID: &'static str = "minecraft:tube_coral";
    const NAME: &'static str = "Tube Coral";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 14659;
    const MAX_STATE_ID: u32 = 14659;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Copper Door
pub struct WaxedCopperDoor;

impl BlockDef for WaxedCopperDoor {
    const ID: u32 = 1080;
    const STRING_ID: &'static str = "minecraft:waxed_copper_door";
    const NAME: &'static str = "Waxed Copper Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14660;
    const MAX_STATE_ID: u32 = 14691;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Nether Portal
pub struct Portal;

impl BlockDef for Portal {
    const ID: u32 = 90;
    const STRING_ID: &'static str = "minecraft:portal";
    const NAME: &'static str = "Nether Portal";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 11;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14692;
    const MAX_STATE_ID: u32 = 14694;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Button
pub struct BirchButton;

impl BlockDef for BirchButton {
    const ID: u32 = 445;
    const STRING_ID: &'static str = "minecraft:birch_button";
    const NAME: &'static str = "Birch Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14695;
    const MAX_STATE_ID: u32 = 14706;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Peony
pub struct Peony;

impl BlockDef for Peony {
    const ID: u32 = 560;
    const STRING_ID: &'static str = "minecraft:peony";
    const NAME: &'static str = "Peony";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14707;
    const MAX_STATE_ID: u32 = 14708;
    type State = super::states::DoublePlantState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Command Block
pub struct CommandBlock;

impl BlockDef for CommandBlock {
    const ID: u32 = 137;
    const STRING_ID: &'static str = "minecraft:command_block";
    const NAME: &'static str = "Command Block";
    const HARDNESS: f32 = -1.0_f32;
    const RESISTANCE: f32 = 3600000.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14709;
    const MAX_STATE_ID: u32 = 14720;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Blackstone Button
pub struct PolishedBlackstoneButton;

impl BlockDef for PolishedBlackstoneButton {
    const ID: u32 = 939;
    const STRING_ID: &'static str = "minecraft:polished_blackstone_button";
    const NAME: &'static str = "Polished Blackstone Button";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14721;
    const MAX_STATE_ID: u32 = 14732;
    type State = super::states::ButtonState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crafter
pub struct Crafter;

impl BlockDef for Crafter {
    const ID: u32 = 1184;
    const STRING_ID: &'static str = "minecraft:crafter";
    const NAME: &'static str = "Crafter";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14733;
    const MAX_STATE_ID: u32 = 14780;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Spruce Planks
pub struct SprucePlanks;

impl BlockDef for SprucePlanks {
    const ID: u32 = 14;
    const STRING_ID: &'static str = "minecraft:spruce_planks";
    const NAME: &'static str = "Spruce Planks";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14781;
    const MAX_STATE_ID: u32 = 14781;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Cobblestone Slab
pub struct MossyCobblestoneDoubleSlab;

impl BlockDef for MossyCobblestoneDoubleSlab {
    const ID: u32 = 8059;
    const STRING_ID: &'static str = "minecraft:mossy_cobblestone_double_slab";
    const NAME: &'static str = "Mossy Cobblestone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14782;
    const MAX_STATE_ID: u32 = 14783;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Furnace
pub struct Furnace;

impl BlockDef for Furnace {
    const ID: u32 = 61;
    const STRING_ID: &'static str = "minecraft:furnace";
    const NAME: &'static str = "Furnace";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14784;
    const MAX_STATE_ID: u32 = 14787;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Info Update2
pub struct InfoUpdate2;

impl BlockDef for InfoUpdate2 {
    const ID: u32 = 249;
    const STRING_ID: &'static str = "minecraft:info_update2";
    const NAME: &'static str = "Info Update2";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14788;
    const MAX_STATE_ID: u32 = 14788;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Amethyst Cluster
pub struct AmethystCluster;

impl BlockDef for AmethystCluster {
    const ID: u32 = 980;
    const STRING_ID: &'static str = "minecraft:amethyst_cluster";
    const NAME: &'static str = "Amethyst Cluster";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 1.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 5;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14789;
    const MAX_STATE_ID: u32 = 14794;
    type State = super::states::BlockFaceState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Chiseled Copper
pub struct WaxedChiseledCopper;

impl BlockDef for WaxedChiseledCopper {
    const ID: u32 = 1056;
    const STRING_ID: &'static str = "minecraft:waxed_chiseled_copper";
    const NAME: &'static str = "Waxed Chiseled Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14795;
    const MAX_STATE_ID: u32 = 14795;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Cut Copper Slab
pub struct WaxedCutCopperSlab;

impl BlockDef for WaxedCutCopperSlab {
    const ID: u32 = 1072;
    const STRING_ID: &'static str = "minecraft:waxed_cut_copper_slab";
    const NAME: &'static str = "Waxed Cut Copper Slab";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14796;
    const MAX_STATE_ID: u32 = 14797;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Polished Deepslate Wall
pub struct PolishedDeepslateWall;

impl BlockDef for PolishedDeepslateWall {
    const ID: u32 = 1159;
    const STRING_ID: &'static str = "minecraft:polished_deepslate_wall";
    const NAME: &'static str = "Polished Deepslate Wall";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14798;
    const MAX_STATE_ID: u32 = 14959;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Prismarine Brick Slab
pub struct PrismarineBrickDoubleSlab;

impl BlockDef for PrismarineBrickDoubleSlab {
    const ID: u32 = 8028;
    const STRING_ID: &'static str = "minecraft:prismarine_brick_double_slab";
    const NAME: &'static str = "Prismarine Brick Slab";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14960;
    const MAX_STATE_ID: u32 = 14961;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dried Kelp Block
pub struct DriedKelpBlock;

impl BlockDef for DriedKelpBlock {
    const ID: u32 = 744;
    const STRING_ID: &'static str = "minecraft:dried_kelp_block";
    const NAME: &'static str = "Dried Kelp Block";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14962;
    const MAX_STATE_ID: u32 = 14962;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Light Gray Stained Glass
pub struct HardLightGrayStainedGlass;

impl BlockDef for HardLightGrayStainedGlass {
    const ID: u32 = 15867;
    const STRING_ID: &'static str = "minecraft:hard_light_gray_stained_glass";
    const NAME: &'static str = "Hard Light Gray Stained Glass";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14963;
    const MAX_STATE_ID: u32 = 14963;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Fence
pub struct CrimsonFence;

impl BlockDef for CrimsonFence {
    const ID: u32 = 889;
    const STRING_ID: &'static str = "minecraft:crimson_fence";
    const NAME: &'static str = "Crimson Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14964;
    const MAX_STATE_ID: u32 = 14964;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Chiseled Tuff
pub struct ChiseledTuff;

impl BlockDef for ChiseledTuff {
    const ID: u32 = 992;
    const STRING_ID: &'static str = "minecraft:chiseled_tuff";
    const NAME: &'static str = "Chiseled Tuff";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14965;
    const MAX_STATE_ID: u32 = 14965;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Concrete Powder
pub struct LimeConcretePowder;

impl BlockDef for LimeConcretePowder {
    const ID: u32 = 731;
    const STRING_ID: &'static str = "minecraft:lime_concrete_powder";
    const NAME: &'static str = "Lime Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14966;
    const MAX_STATE_ID: u32 = 14966;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Turtle Egg
pub struct TurtleEgg;

impl BlockDef for TurtleEgg {
    const ID: u32 = 745;
    const STRING_ID: &'static str = "minecraft:turtle_egg";
    const NAME: &'static str = "Turtle Egg";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14967;
    const MAX_STATE_ID: u32 = 14978;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Magma Block
pub struct Magma;

impl BlockDef for Magma {
    const ID: u32 = 213;
    const STRING_ID: &'static str = "minecraft:magma";
    const NAME: &'static str = "Magma Block";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 3;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14979;
    const MAX_STATE_ID: u32 = 14979;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dispenser
pub struct Dispenser;

impl BlockDef for Dispenser {
    const ID: u32 = 23;
    const STRING_ID: &'static str = "minecraft:dispenser";
    const NAME: &'static str = "Dispenser";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14980;
    const MAX_STATE_ID: u32 = 14991;
    type State = super::states::DispenserState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Brown Terracotta
pub struct BrownTerracotta;

impl BlockDef for BrownTerracotta {
    const ID: u32 = 496;
    const STRING_ID: &'static str = "minecraft:brown_terracotta";
    const NAME: &'static str = "Brown Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14992;
    const MAX_STATE_ID: u32 = 14992;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobblestone Slab
pub struct CobblestoneDoubleSlab;

impl BlockDef for CobblestoneDoubleSlab {
    const ID: u32 = 8046;
    const STRING_ID: &'static str = "minecraft:cobblestone_double_slab";
    const NAME: &'static str = "Cobblestone Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14993;
    const MAX_STATE_ID: u32 = 14994;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Deepslate Diamond Ore
pub struct DeepslateDiamondOre;

impl BlockDef for DeepslateDiamondOre {
    const ID: u32 = 204;
    const STRING_ID: &'static str = "minecraft:deepslate_diamond_ore";
    const NAME: &'static str = "Deepslate Diamond Ore";
    const HARDNESS: f32 = 4.5_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 14995;
    const MAX_STATE_ID: u32 = 14995;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Grindstone
pub struct Grindstone;

impl BlockDef for Grindstone {
    const ID: u32 = 844;
    const STRING_ID: &'static str = "minecraft:grindstone";
    const NAME: &'static str = "Grindstone";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 14996;
    const MAX_STATE_ID: u32 = 15011;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Copper Golem Statue
pub struct WaxedCopperGolemStatue;

impl BlockDef for WaxedCopperGolemStatue {
    const ID: u32 = 1120;
    const STRING_ID: &'static str = "minecraft:waxed_copper_golem_statue";
    const NAME: &'static str = "Waxed Copper Golem Statue";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15012;
    const MAX_STATE_ID: u32 = 15015;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Wool
pub struct LightGrayWool;

impl BlockDef for LightGrayWool {
    const ID: u32 = 148;
    const STRING_ID: &'static str = "minecraft:light_gray_wool";
    const NAME: &'static str = "Light Gray Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15016;
    const MAX_STATE_ID: u32 = 15016;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Soul Campfire
pub struct SoulCampfire;

impl BlockDef for SoulCampfire {
    const ID: u32 = 860;
    const STRING_ID: &'static str = "minecraft:soul_campfire";
    const NAME: &'static str = "Soul Campfire";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 10;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15017;
    const MAX_STATE_ID: u32 = 15024;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Prismarine Bricks
pub struct PrismarineBricks;

impl BlockDef for PrismarineBricks {
    const ID: u32 = 528;
    const STRING_ID: &'static str = "minecraft:prismarine_bricks";
    const NAME: &'static str = "Prismarine Bricks";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15025;
    const MAX_STATE_ID: u32 = 15025;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oak Pressure Plate
pub struct WoodenPressurePlate;

impl BlockDef for WoodenPressurePlate {
    const ID: u32 = 72;
    const STRING_ID: &'static str = "minecraft:wooden_pressure_plate";
    const NAME: &'static str = "Oak Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15026;
    const MAX_STATE_ID: u32 = 15041;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Sandstone Wall
pub struct SandstoneWall;

impl BlockDef for SandstoneWall {
    const ID: u32 = 834;
    const STRING_ID: &'static str = "minecraft:sandstone_wall";
    const NAME: &'static str = "Sandstone Wall";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15042;
    const MAX_STATE_ID: u32 = 15203;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Fence
pub struct BirchFence;

impl BlockDef for BirchFence {
    const ID: u32 = 638;
    const STRING_ID: &'static str = "minecraft:birch_fence";
    const NAME: &'static str = "Birch Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15204;
    const MAX_STATE_ID: u32 = 15204;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Lime Candle
pub struct LimeCandleCake;

impl BlockDef for LimeCandleCake {
    const ID: u32 = 967;
    const STRING_ID: &'static str = "minecraft:lime_candle_cake";
    const NAME: &'static str = "Cake with Lime Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15205;
    const MAX_STATE_ID: u32 = 15206;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Oxidized Copper Grate
pub struct WaxedOxidizedCopperGrate;

impl BlockDef for WaxedOxidizedCopperGrate {
    const ID: u32 = 1099;
    const STRING_ID: &'static str = "minecraft:waxed_oxidized_copper_grate";
    const NAME: &'static str = "Waxed Oxidized Copper Grate";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15207;
    const MAX_STATE_ID: u32 = 15207;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Damaged Anvil
pub struct DamagedAnvil;

impl BlockDef for DamagedAnvil {
    const ID: u32 = 469;
    const STRING_ID: &'static str = "minecraft:damaged_anvil";
    const NAME: &'static str = "Damaged Anvil";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 1200.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15208;
    const MAX_STATE_ID: u32 = 15211;
    type State = super::states::CardinalState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Birch Slab
pub struct BirchDoubleSlab;

impl BlockDef for BirchDoubleSlab {
    const ID: u32 = 8032;
    const STRING_ID: &'static str = "minecraft:birch_double_slab";
    const NAME: &'static str = "Birch Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15212;
    const MAX_STATE_ID: u32 = 15213;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// White Concrete
pub struct WhiteConcrete;

impl BlockDef for WhiteConcrete {
    const ID: u32 = 710;
    const STRING_ID: &'static str = "minecraft:white_concrete";
    const NAME: &'static str = "White Concrete";
    const HARDNESS: f32 = 1.8_f32;
    const RESISTANCE: f32 = 1.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15214;
    const MAX_STATE_ID: u32 = 15214;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Material Reducer
pub struct MaterialReducer;

impl BlockDef for MaterialReducer {
    const ID: u32 = 16121;
    const STRING_ID: &'static str = "minecraft:material_reducer";
    const NAME: &'static str = "Material Reducer";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15215;
    const MAX_STATE_ID: u32 = 15218;
    type State = super::states::DirectionState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Trial Spawner
pub struct TrialSpawner;

impl BlockDef for TrialSpawner {
    const ID: u32 = 1185;
    const STRING_ID: &'static str = "minecraft:trial_spawner";
    const NAME: &'static str = "Trial Spawner";
    const HARDNESS: f32 = 50.0_f32;
    const RESISTANCE: f32 = 50.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 15219;
    const MAX_STATE_ID: u32 = 15230;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Acacia Fence
pub struct AcaciaFence;

impl BlockDef for AcaciaFence {
    const ID: u32 = 640;
    const STRING_ID: &'static str = "minecraft:acacia_fence";
    const NAME: &'static str = "Acacia Fence";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15231;
    const MAX_STATE_ID: u32 = 15231;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dirt Path
pub struct GrassPath;

impl BlockDef for GrassPath {
    const ID: u32 = 198;
    const STRING_ID: &'static str = "minecraft:grass_path";
    const NAME: &'static str = "Dirt Path";
    const HARDNESS: f32 = 0.65_f32;
    const RESISTANCE: f32 = 0.65_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15232;
    const MAX_STATE_ID: u32 = 15232;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Resin Brick Wall
pub struct ResinBrickWall;

impl BlockDef for ResinBrickWall {
    const ID: u32 = 379;
    const STRING_ID: &'static str = "minecraft:resin_brick_wall";
    const NAME: &'static str = "Resin Brick Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15233;
    const MAX_STATE_ID: u32 = 15394;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cobbled Deepslate Wall
pub struct CobbledDeepslateWall;

impl BlockDef for CobbledDeepslateWall {
    const ID: u32 = 1155;
    const STRING_ID: &'static str = "minecraft:cobbled_deepslate_wall";
    const NAME: &'static str = "Cobbled Deepslate Wall";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15395;
    const MAX_STATE_ID: u32 = 15556;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Waxed Weathered Lightning Rod
pub struct WaxedWeatheredLightningRod;

impl BlockDef for WaxedWeatheredLightningRod {
    const ID: u32 = 1130;
    const STRING_ID: &'static str = "minecraft:waxed_weathered_lightning_rod";
    const NAME: &'static str = "Waxed Weathered Lightning Rod";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15557;
    const MAX_STATE_ID: u32 = 15568;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Orange Concrete Powder
pub struct OrangeConcretePowder;

impl BlockDef for OrangeConcretePowder {
    const ID: u32 = 727;
    const STRING_ID: &'static str = "minecraft:orange_concrete_powder";
    const NAME: &'static str = "Orange Concrete Powder";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15569;
    const MAX_STATE_ID: u32 = 15569;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cake with Orange Candle
pub struct OrangeCandleCake;

impl BlockDef for OrangeCandleCake {
    const ID: u32 = 963;
    const STRING_ID: &'static str = "minecraft:orange_candle_cake";
    const NAME: &'static str = "Cake with Orange Candle";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15570;
    const MAX_STATE_ID: u32 = 15571;
    type State = super::states::CandleCakeState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Weathered Copper
pub struct WeatheredCopper;

impl BlockDef for WeatheredCopper {
    const ID: u32 = 1036;
    const STRING_ID: &'static str = "minecraft:weathered_copper";
    const NAME: &'static str = "Weathered Copper";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15572;
    const MAX_STATE_ID: u32 = 15572;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Mossy Stone Brick Wall
pub struct MossyStoneBrickWall;

impl BlockDef for MossyStoneBrickWall {
    const ID: u32 = 827;
    const STRING_ID: &'static str = "minecraft:mossy_stone_brick_wall";
    const NAME: &'static str = "Mossy Stone Brick Wall";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15573;
    const MAX_STATE_ID: u32 = 15734;
    type State = super::states::WallState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Redstone Torch
pub struct UnlitRedstoneTorch;

impl BlockDef for UnlitRedstoneTorch {
    const ID: u32 = 75;
    const STRING_ID: &'static str = "minecraft:unlit_redstone_torch";
    const NAME: &'static str = "Redstone Torch";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 7;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15735;
    const MAX_STATE_ID: u32 = 15740;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Pale Oak Slab
pub struct PaleOakDoubleSlab;

impl BlockDef for PaleOakDoubleSlab {
    const ID: u32 = 8037;
    const STRING_ID: &'static str = "minecraft:pale_oak_double_slab";
    const NAME: &'static str = "Pale Oak Slab";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15741;
    const MAX_STATE_ID: u32 = 15742;
    type State = super::states::SlabState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lime Terracotta
pub struct LimeTerracotta;

impl BlockDef for LimeTerracotta {
    const ID: u32 = 489;
    const STRING_ID: &'static str = "minecraft:lime_terracotta";
    const NAME: &'static str = "Lime Terracotta";
    const HARDNESS: f32 = 1.25_f32;
    const RESISTANCE: f32 = 4.2_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15743;
    const MAX_STATE_ID: u32 = 15743;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cherry Fence Gate
pub struct CherryFenceGate;

impl BlockDef for CherryFenceGate {
    const ID: u32 = 632;
    const STRING_ID: &'static str = "minecraft:cherry_fence_gate";
    const NAME: &'static str = "Cherry Fence Gate";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15744;
    const MAX_STATE_ID: u32 = 15759;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gray Glazed Terracotta
pub struct GrayGlazedTerracotta;

impl BlockDef for GrayGlazedTerracotta {
    const ID: u32 = 227;
    const STRING_ID: &'static str = "minecraft:gray_glazed_terracotta";
    const NAME: &'static str = "Gray Glazed Terracotta";
    const HARDNESS: f32 = 1.4_f32;
    const RESISTANCE: f32 = 1.4_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15760;
    const MAX_STATE_ID: u32 = 15765;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Colored Torch Red
pub struct ColoredTorchRed;

impl BlockDef for ColoredTorchRed {
    const ID: u32 = 16834;
    const STRING_ID: &'static str = "minecraft:colored_torch_red";
    const NAME: &'static str = "Colored Torch Red";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15766;
    const MAX_STATE_ID: u32 = 15771;
    type State = super::states::TorchState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Lodestone
pub struct Lodestone;

impl BlockDef for Lodestone {
    const ID: u32 = 923;
    const STRING_ID: &'static str = "minecraft:lodestone";
    const NAME: &'static str = "Lodestone";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15772;
    const MAX_STATE_ID: u32 = 15772;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Bamboo Mosaic
pub struct BambooMosaic;

impl BlockDef for BambooMosaic {
    const ID: u32 = 24;
    const STRING_ID: &'static str = "minecraft:bamboo_mosaic";
    const NAME: &'static str = "Bamboo Mosaic";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 3.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15773;
    const MAX_STATE_ID: u32 = 15773;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Hard Blue Stained Glass Pane
pub struct HardBlueStainedGlassPane;

impl BlockDef for HardBlueStainedGlassPane {
    const ID: u32 = 16842;
    const STRING_ID: &'static str = "minecraft:hard_blue_stained_glass_pane";
    const NAME: &'static str = "Hard Blue Stained Glass Pane";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15774;
    const MAX_STATE_ID: u32 = 15774;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Raw Iron
pub struct RawIronBlock;

impl BlockDef for RawIronBlock {
    const ID: u32 = 1173;
    const STRING_ID: &'static str = "minecraft:raw_iron_block";
    const NAME: &'static str = "Block of Raw Iron";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15775;
    const MAX_STATE_ID: u32 = 15775;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Light Gray Carpet
pub struct LightGrayCarpet;

impl BlockDef for LightGrayCarpet {
    const ID: u32 = 546;
    const STRING_ID: &'static str = "minecraft:light_gray_carpet";
    const NAME: &'static str = "Light Gray Carpet";
    const HARDNESS: f32 = 0.1_f32;
    const RESISTANCE: f32 = 0.1_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15776;
    const MAX_STATE_ID: u32 = 15776;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Purple Wool
pub struct PurpleWool;

impl BlockDef for PurpleWool {
    const ID: u32 = 150;
    const STRING_ID: &'static str = "minecraft:purple_wool";
    const NAME: &'static str = "Purple Wool";
    const HARDNESS: f32 = 0.8_f32;
    const RESISTANCE: f32 = 0.8_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15777;
    const MAX_STATE_ID: u32 = 15777;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Block of Iron
pub struct IronBlock;

impl BlockDef for IronBlock {
    const ID: u32 = 42;
    const STRING_ID: &'static str = "minecraft:iron_block";
    const NAME: &'static str = "Block of Iron";
    const HARDNESS: f32 = 5.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15778;
    const MAX_STATE_ID: u32 = 15778;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Ladder
pub struct Ladder;

impl BlockDef for Ladder {
    const ID: u32 = 65;
    const STRING_ID: &'static str = "minecraft:ladder";
    const NAME: &'static str = "Ladder";
    const HARDNESS: f32 = 0.4_f32;
    const RESISTANCE: f32 = 0.4_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15779;
    const MAX_STATE_ID: u32 = 15784;
    type State = super::states::FacingState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Crimson Pressure Plate
pub struct CrimsonPressurePlate;

impl BlockDef for CrimsonPressurePlate {
    const ID: u32 = 887;
    const STRING_ID: &'static str = "minecraft:crimson_pressure_plate";
    const NAME: &'static str = "Crimson Pressure Plate";
    const HARDNESS: f32 = 0.5_f32;
    const RESISTANCE: f32 = 0.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15785;
    const MAX_STATE_ID: u32 = 15800;
    type State = super::states::PressurePlateState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Stripped Mangrove Log
pub struct StrippedMangroveLog;

impl BlockDef for StrippedMangroveLog {
    const ID: u32 = 69;
    const STRING_ID: &'static str = "minecraft:stripped_mangrove_log";
    const NAME: &'static str = "Stripped Mangrove Log";
    const HARDNESS: f32 = 2.0_f32;
    const RESISTANCE: f32 = 2.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15801;
    const MAX_STATE_ID: u32 = 15803;
    type State = super::states::PillarState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Copper Lantern
pub struct CopperLantern;

impl BlockDef for CopperLantern {
    const ID: u32 = 851;
    const STRING_ID: &'static str = "minecraft:copper_lantern";
    const NAME: &'static str = "Copper Lantern";
    const HARDNESS: f32 = 3.5_f32;
    const RESISTANCE: f32 = 3.5_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 15;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15804;
    const MAX_STATE_ID: u32 = 15805;
    type State = super::states::LanternState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Gravel
pub struct Gravel;

impl BlockDef for Gravel {
    const ID: u32 = 13;
    const STRING_ID: &'static str = "minecraft:gravel";
    const NAME: &'static str = "Gravel";
    const HARDNESS: f32 = 0.6_f32;
    const RESISTANCE: f32 = 0.6_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15806;
    const MAX_STATE_ID: u32 = 15806;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Cartography Table
pub struct CartographyTable;

impl BlockDef for CartographyTable {
    const ID: u32 = 842;
    const STRING_ID: &'static str = "minecraft:cartography_table";
    const NAME: &'static str = "Cartography Table";
    const HARDNESS: f32 = 2.5_f32;
    const RESISTANCE: f32 = 2.5_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 15807;
    const MAX_STATE_ID: u32 = 15807;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Oxidized Copper Door
pub struct OxidizedCopperDoor;

impl BlockDef for OxidizedCopperDoor {
    const ID: u32 = 1079;
    const STRING_ID: &'static str = "minecraft:oxidized_copper_door";
    const NAME: &'static str = "Oxidized Copper Door";
    const HARDNESS: f32 = 3.0_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15808;
    const MAX_STATE_ID: u32 = 15839;
    type State = super::states::DoorState;
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Tube Coral Wall Fan
pub struct TubeCoralWallFan;

impl BlockDef for TubeCoralWallFan {
    const ID: u32 = 783;
    const STRING_ID: &'static str = "minecraft:tube_coral_wall_fan";
    const NAME: &'static str = "Tube Coral Wall Fan";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 1;
    const MIN_STATE_ID: u32 = 15840;
    const MAX_STATE_ID: u32 = 15843;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// Dandelion
pub struct Dandelion;

impl BlockDef for Dandelion {
    const ID: u32 = 157;
    const STRING_ID: &'static str = "minecraft:dandelion";
    const NAME: &'static str = "Dandelion";
    const HARDNESS: f32 = 0.0_f32;
    const RESISTANCE: f32 = 0.0_f32;
    const IS_TRANSPARENT: bool = true;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 0;
    const MIN_STATE_ID: u32 = 15844;
    const MAX_STATE_ID: u32 = 15844;
    type State = ();
    fn default_state() -> Self::State {
        Default::default()
    }
}

/// All vanilla blocks as dynamic references.
pub static BLOCKS: &[&'static dyn BlockDefDyn] = &[
    &CyanTerracotta,
    &HardPinkStainedGlass,
    &BlueCandle,
    &DarkOakWood,
    &BirchStandingSign,
    &PolishedBasalt,
    &NetherGoldOre,
    &ZombieHead,
    &WaxedWeatheredCopperChain,
    &WaxedWeatheredCopperChest,
    &LeafLitter,
    &WarpedDoor,
    &LightBlueConcretePowder,
    &BambooBlock,
    &PistonArmCollision,
    &WaxedOxidizedChiseledCopper,
    &WetSponge,
    &EndStoneBrickWall,
    &Granite,
    &BlueStainedGlassPane,
    &FenceGate,
    &BirchShelf,
    &PowderSnow,
    &DarkOakButton,
    &DeepslateCopperOre,
    &ChiseledStoneBricks,
    &NetherBrickStairs,
    &YellowShulkerBox,
    &BlackstoneDoubleSlab,
    &LimeStainedGlass,
    &RedWool,
    &JungleButton,
    &SpruceStairs,
    &HardGreenStainedGlassPane,
    &AcaciaShelf,
    &Diorite,
    &PaleOakFenceGate,
    &GrayCandleCake,
    &PolishedTuffSlab,
    &CherryPressurePlate,
    &CherryHangingSign,
    &YellowWool,
    &CrimsonWallSign,
    &YellowStainedGlassPane,
    &EndGateway,
    &AzureBluet,
    &Beacon,
    &RedNetherBrick,
    &BrickWall,
    &CobbledDeepslateStairs,
    &SmoothSandstone,
    &SnowLayer,
    &BrickDoubleSlab,
    &BlackCandle,
    &BlueCarpet,
    &GlowFrame,
    &MudBrickDoubleSlab,
    &HangingRoots,
    &RedSandstoneWall,
    &PrismarineBricksStairs,
    &WaxedOxidizedCutCopper,
    &WaxedExposedCopperChain,
    &WaxedExposedCopperChest,
    &Calcite,
    &DioriteSlab,
    &StrippedDarkOakLog,
    &HardOrangeStainedGlassPane,
    &DeadBubbleCoralFan,
    &JungleLog,
    &BubbleCoralFan,
    &HardBrownStainedGlass,
    &SculkShrieker,
    &GrayWool,
    &OrangeStainedGlassPane,
    &HardBlackStainedGlassPane,
    &GrayCarpet,
    &LilyOfTheValley,
    &LimeGlazedTerracotta,
    &Trapdoor,
    &CactusFlower,
    &DeadBrainCoralFan,
    &InfoUpdate,
    &Seagrass,
    &TubeCoralFan,
    &WaxedExposedCutCopperSlab,
    &RedstoneLamp,
    &MossyCobblestone,
    &Deepslate,
    &MagentaCarpet,
    &PitcherCrop,
    &BrownWool,
    &WaxedExposedChiseledCopper,
    &TuffSlab,
    &WarpedPressurePlate,
    &StrippedAcaciaWood,
    &FireflyBush,
    &DiamondBlock,
    &DarkPrismarineDoubleSlab,
    &OakStairs,
    &HardGrayStainedGlass,
    &OakLog,
    &BrownStainedGlassPane,
    &EndBricks,
    &MagentaShulkerBox,
    &PackedIce,
    &PackedMud,
    &LightBlueCandleCake,
    &MossCarpet,
    &WarpedFungus,
    &OxidizedLightningRod,
    &PolishedDeepslateSlab,
    &BambooDoor,
    &AmethystBlock,
    &DeadBubbleCoralWallFan,
    &GoldBlock,
    &FlowerPot,
    &ChiseledBookshelf,
    &PolishedDeepslateStairs,
    &LimeShulkerBox,
    &WeatheredChiseledCopper,
    &SmallAmethystBud,
    &ActivatorRail,
    &IronTrapdoor,
    &Potatoes,
    &MuddyMangroveRoots,
    &PaleOakPressurePlate,
    &StrippedJungleWood,
    &Noteblock,
    &Tuff,
    &MangroveLog,
    &OxidizedCutCopperStairs,
    &PaleOakFence,
    &PaleOakLeaves,
    &DeepslateTileDoubleSlab,
    &SandstoneSlab,
    &MossyStoneBrickSlab,
    &RawGoldBlock,
    &Allium,
    &WhiteShulkerBox,
    &CopperGrate,
    &BlackWool,
    &OrangeCandle,
    &PoweredComparator,
    &JungleFence,
    &CutSandstoneDoubleSlab,
    &WarpedWallSign,
    &SpruceFence,
    &DarkOakSapling,
    &MelonBlock,
    &BlackConcretePowder,
    &SandstoneDoubleSlab,
    &WaxedCutCopperStairs,
    &OpenEyeblossom,
    &MobSpawner,
    &PaleOakSapling,
    &PolishedGranite,
    &PaleOakWallSign,
    &SoulFire,
    &MagentaCandle,
    &MangroveDoubleSlab,
    &SmoothQuartzDoubleSlab,
    &LightGrayStainedGlass,
    &Obsidian,
    &LightGrayStainedGlassPane,
    &DarkOakSlab,
    &DeepslateBrickWall,
    &WaxedExposedCopperGrate,
    &OxidizedDoubleCutCopperSlab,
    &ExposedCopper,
    &PolishedDeepslateDoubleSlab,
    &WaxedCopperBars,
    &StoneButton,
    &RedNetherBrickDoubleSlab,
    &WaxedCopperBulb,
    &Sponge,
    &ExposedDoubleCutCopperSlab,
    &BambooFence,
    &NormalStoneStairs,
    &DioriteDoubleSlab,
    &EndStoneBrickSlab,
    &HardenedClay,
    &BirchHangingSign,
    &StrippedJungleLog,
    &OxidizedCopperGolemStatue,
    &LightBlock9,
    &LightBlock8,
    &LightBlock7,
    &LightBlock6,
    &LightBlock5,
    &LightBlock4,
    &LightBlock3,
    &LightBlock2,
    &LightBlock1,
    &LightBlock0,
    &PaleOakDoor,
    &OakSapling,
    &PolishedBlackstoneDoubleSlab,
    &LightGrayTerracotta,
    &Smoker,
    &BrownStainedGlass,
    &Andesite,
    &FireCoral,
    &Stone,
    &SmoothSandstoneSlab,
    &BirchLog,
    &HardGlassPane,
    &TuffBrickWall,
    &PurpurSlab,
    &BrainCoral,
    &StrippedSpruceWood,
    &OrangeWool,
    &PolishedBlackstoneBrickDoubleSlab,
    &CrimsonDoubleSlab,
    &RespawnAnchor,
    &LightGrayConcrete,
    &GreenCandle,
    &WaxedExposedCopper,
    &RedSandstoneDoubleSlab,
    &BirchWood,
    &RedSand,
    &HayBlock,
    &JungleWood,
    &WaxedWeatheredCopper,
    &InfestedCrackedStoneBricks,
    &WaxedOxidizedCutCopperSlab,
    &OakLeaves,
    &ResinClump,
    &BrainCoralFan,
    &CyanCandleCake,
    &PolishedTuffWall,
    &BambooStairs,
    &InfestedMossyStoneBricks,
    &Torch,
    &MudBrickWall,
    &HoneyBlock,
    &UnderwaterTnt,
    &DripstoneBlock,
    &Vine,
    &RedSandstoneSlab,
    &CherryTrapdoor,
    &BlackstoneSlab,
    &GoldOre,
    &YellowGlazedTerracotta,
    &Stonecutter,
    &DriedGhast,
    &WarpedPlanks,
    &Piston,
    &BrownCarpet,
    &StoneBrickStairs,
    &DeadBubbleCoralBlock,
    &GrayCandle,
    &CherryFence,
    &MangrovePlanks,
    &InvisibleBedrock,
    &RedTerracotta,
    &DioriteWall,
    &DeadFireCoralBlock,
    &OxidizedCopperBulb,
    &MagentaWool,
    &OxidizedCopperBars,
    &MagentaGlazedTerracotta,
    &QuartzDoubleSlab,
    &PolishedBlackstoneBrickWall,
    &MangroveSlab,
    &OrangeGlazedTerracotta,
    &HardBrownStainedGlassPane,
    &SmoothBasalt,
    &Waterlily,
    &StrippedPaleOakWood,
    &HardLightBlueStainedGlass,
    &EmeraldBlock,
    &SuspiciousSand,
    &MossyCobblestoneWall,
    &HeavyWeightedPressurePlate,
    &PurpleStainedGlass,
    &LightningRod,
    &AcaciaLeaves,
    &BlackStainedGlassPane,
    &CobblestoneWall,
    &UnderwaterTorch,
    &DeepslateBrickDoubleSlab,
    &SpruceDoubleSlab,
    &BambooMosaicSlab,
    &DarkOakLog,
    &AcaciaHangingSign,
    &OchreFroglight,
    &TuffWall,
    &Observer,
    &RedstoneTorch,
    &SilverGlazedTerracotta,
    &GraniteStairs,
    &PinkConcrete,
    &DarkOakHangingSign,
    &Glowingobsidian,
    &BrownMushroom,
    &CyanConcretePowder,
    &DeadFireCoralWallFan,
    &BrownGlazedTerracotta,
    &WaxedCopperTrapdoor,
    &SpruceShelf,
    &ResinBrickDoubleSlab,
    &OxidizedCopper,
    &CopperOre,
    &DarkOakPlanks,
    &BirchPressurePlate,
    &Scaffolding,
    &SandstoneStairs,
    &GreenCandleCake,
    &StrippedBambooBlock,
    &RedMushroomBlock,
    &CrackedStoneBricks,
    &SculkCatalyst,
    &Cobblestone,
    &WaxedLightningRod,
    &HornCoral,
    &YellowConcrete,
    &MangroveShelf,
    &CyanCarpet,
    &WarpedShelf,
    &OakDoubleSlab,
    &SmoothSandstoneStairs,
    &JunglePressurePlate,
    &DoubleCutCopperSlab,
    &Chalkboard,
    &BlueTerracotta,
    &Sandstone,
    &BrownCandleCake,
    &AcaciaWallSign,
    &LightWeightedPressurePlate,
    &UndyedShulkerBox,
    &PolishedBlackstone,
    &Mycelium,
    &ExposedLightningRod,
    &Bamboo,
    &QuartzBlock,
    &PaleOakPlanks,
    &StoneStairs,
    &WaxedWeatheredChiseledCopper,
    &GrayStainedGlass,
    &GreenTerracotta,
    &DeepslateBrickSlab,
    &WarpedStairs,
    &SmithingTable,
    &PlayerHead,
    &WeatheredCopperGrate,
    &Poppy,
    &TuffBrickSlab,
    &CopperChain,
    &CopperChest,
    &MossyStoneBricks,
    &GreenWool,
    &GreenCarpet,
    &PrismarineBrickSlab,
    &WoodenDoor,
    &PitcherPlant,
    &CompoundCreator,
    &SprucePressurePlate,
    &NetheriteBlock,
    &PinkWool,
    &RedstoneBlock,
    &BirchFenceGate,
    &RedstoneWire,
    &QuartzPillar,
    &WaxedExposedCutCopper,
    &Lava,
    &JungleHangingSign,
    &BirchSlab,
    &Loom,
    &WaxedWeatheredCopperLantern,
    &DeadTubeCoralBlock,
    &EndStone,
    &PolishedTuffDoubleSlab,
    &CrimsonDoor,
    &MangrovePressurePlate,
    &JungleShelf,
    &JungleSlab,
    &LightBlueStainedGlassPane,
    &Glowstone,
    &StonePressurePlate,
    &WaxedExposedCutCopperStairs,
    &HardWhiteStainedGlass,
    &MudBrickSlab,
    &WaxedExposedLightningRod,
    &ExposedCopperLantern,
    &Farmland,
    &DeadBrainCoralWallFan,
    &CutRedSandstone,
    &Rail,
    &BlackstoneWall,
    &StoneBricks,
    &MossyCobblestoneStairs,
    &HardMagentaStainedGlass,
    &DetectorRail,
    &BlueOrchid,
    &GreenStainedGlassPane,
    &PolishedGraniteStairs,
    &BirchLeaves,
    &PinkTerracotta,
    &DarkOakDoubleSlab,
    &InfestedCobblestone,
    &PinkCandleCake,
    &CrackedDeepslateTiles,
    &BrainCoralWallFan,
    &MangroveWood,
    &WaxedExposedCopperGolemStatue,
    &RedGlazedTerracotta,
    &WaxedOxidizedCopperChest,
    &WaxedOxidizedCopperChain,
    &DarkOakFenceGate,
    &MossyCobblestoneSlab,
    &BambooMosaicDoubleSlab,
    &CobblestoneSlab,
    &CrimsonNylium,
    &StructureVoid,
    &WaxedExposedCopperBars,
    &PurpleConcrete,
    &WaxedExposedCopperBulb,
    &PolishedBlackstoneBrickSlab,
    &NormalStoneSlab,
    &HardYellowStainedGlassPane,
    &SpruceSapling,
    &YellowTerracotta,
    &Snow,
    &Sand,
    &DaylightDetector,
    &MangroveStandingSign,
    &StrippedMangroveWood,
    &Conduit,
    &Slime,
    &CopperTorch,
    &BoneBlock,
    &Frame,
    &SpruceLog,
    &LapisBlock,
    &CoalOre,
    &MossyStoneBrickDoubleSlab,
    &CutRedSandstoneDoubleSlab,
    &ClientRequestPlaceholderBlock,
    &BambooShelf,
    &RedstoneOre,
    &BambooDoubleSlab,
    &WaxedCopperChest,
    &GreenStainedGlass,
    &WaxedCopperChain,
    &BubbleCoralBlock,
    &InfestedChiseledStoneBricks,
    &NetherBrickFence,
    &PinkTulip,
    &OakSlab,
    &StrippedPaleOakLog,
    &DeepslateTileSlab,
    &PinkConcretePowder,
    &PaleOakSlab,
    &DeadTubeCoral,
    &NetherWartBlock,
    &PrismarineSlab,
    &PrismarineDoubleSlab,
    &CherryDoor,
    &ColoredTorchBlue,
    &CrimsonHyphae,
    &PolishedBlackstoneStairs,
    &WeatheredCutCopperStairs,
    &SmallDripleafBlock,
    &PinkStainedGlass,
    &WaxedWeatheredCopperGrate,
    &SpruceButton,
    &AcaciaLog,
    &CrimsonTrapdoor,
    &Basalt,
    &HardCyanStainedGlass,
    &NormalStoneDoubleSlab,
    &StoneBrickDoubleSlab,
    &LightBlueTerracotta,
    &LitRedstoneLamp,
    &CopperGolemStatue,
    &HardBlueStainedGlass,
    &HardPurpleStainedGlass,
    &DiamondOre,
    &WarpedRoots,
    &MagentaConcrete,
    &DarkPrismarine,
    &StickyPiston,
    &EnderChest,
    &MediumAmethystBud,
    &PinkShulkerBox,
    &WarpedDoubleSlab,
    &JungleWallSign,
    &SculkSensor,
    &CopperBulb,
    &CopperBars,
    &OakShelf,
    &DioriteStairs,
    &SpruceLeaves,
    &FrogSpawn,
    &AcaciaDoor,
    &SmoothSandstoneDoubleSlab,
    &RedShulkerBox,
    &StrippedCherryLog,
    &CrimsonButton,
    &AcaciaPlanks,
    &FireCoralBlock,
    &MagentaConcretePowder,
    &IronDoor,
    &HoneycombBlock,
    &PolishedBlackstoneBrickStairs,
    &MangroveTrapdoor,
    &QuartzOre,
    &DaylightDetectorInverted,
    &Barrel,
    &SmoothQuartz,
    &CoarseDirt,
    &ChorusFlower,
    &OrangeStainedGlass,
    &WhiteStainedGlassPane,
    &StrippedBirchWood,
    &CrackedNetherBricks,
    &PoweredRepeater,
    &LightBlueCandle,
    &HardLimeStainedGlassPane,
    &Pumpkin,
    &ElementConstructor,
    &DeepslateTiles,
    &SmoothStone,
    &HardLightGrayStainedGlassPane,
    &GrayTerracotta,
    &OxidizedCopperTrapdoor,
    &GraniteSlab,
    &WhiteTulip,
    &LimeConcrete,
    &BlackCandleCake,
    &RedMushroom,
    &GildedBlackstone,
    &HardYellowStainedGlass,
    &MagentaTerracotta,
    &ExposedCutCopperStairs,
    &MangroveStairs,
    &PolishedDioriteSlab,
    &Reserved6,
    &CutCopperStairs,
    &Unknown,
    &LabTable,
    &WaxedOxidizedCopperLantern,
    &CherryButton,
    &YellowCandleCake,
    &MangroveFenceGate,
    &Sunflower,
    &PinkPetals,
    &BambooHangingSign,
    &InfestedDeepslate,
    &SoulTorch,
    &Podzol,
    &CopperBlock,
    &LitRedstoneOre,
    &DeepslateTileStairs,
    &CrimsonFenceGate,
    &Deadbush,
    &WaxedWeatheredDoubleCutCopperSlab,
    &PolishedBlackstoneBricks,
    &RedCandle,
    &CutCopper,
    &WaxedWeatheredCopperGolemStatue,
    &IronOre,
    &SpruceDoor,
    &FrostedIce,
    &ChippedAnvil,
    &LargeAmethystBud,
    &ExposedCopperDoor,
    &SuspiciousGravel,
    &WarpedTrapdoor,
    &FlowingWater,
    &BrickBlock,
    &HardGlass,
    &WaxedWeatheredCopperTrapdoor,
    &QuartzStairs,
    &CaveVines,
    &MagentaStainedGlassPane,
    &IronBars,
    &WhiteTerracotta,
    &StrippedOakWood,
    &LightBlueCarpet,
    &OakHangingSign,
    &WhiteConcretePowder,
    &MelonStem,
    &CrimsonPlanks,
    &StrippedDarkOakWood,
    &WaxedWeatheredCutCopper,
    &WhiteStainedGlass,
    &HornCoralWallFan,
    &OakWood,
    &PurpleStainedGlassPane,
    &WaxedOxidizedCopperTrapdoor,
    &WallSign,
    &Jukebox,
    &StrippedCherryWood,
    &Jigsaw,
    &WaxedOxidizedCopperGolemStatue,
    &PrismarineWall,
    &BorderBlock,
    &Shroomlight,
    &BambooFenceGate,
    &Cornflower,
    &ChiseledPolishedBlackstone,
    &DarkOakStairs,
    &DeepslateTileWall,
    &GlassPane,
    &ChiseledDeepslate,
    &CutCopperSlab,
    &RedStainedGlass,
    &PaleOakWood,
    &InfestedStoneBricks,
    &AcaciaPressurePlate,
    &WeatheredLightningRod,
    &BambooTrapdoor,
    &OxidizedChiseledCopper,
    &MangroveWallSign,
    &RawCopperBlock,
    &TallDryGrass,
    &OxidizedCutCopperSlab,
    &HornCoralBlock,
    &DarkOakShelf,
    &Beetroot,
    &LightGrayCandleCake,
    &WhiteCandle,
    &AndesiteStairs,
    &BirchPlanks,
    &GoldenRail,
    &CyanWool,
    &PetrifiedOakDoubleSlab,
    &DeprecatedAnvil,
    &DarkoakWallSign,
    &JungleLeaves,
    &GrayShulkerBox,
    &RedSandstoneStairs,
    &CyanGlazedTerracotta,
    &CrackedDeepslateBricks,
    &FireCoralWallFan,
    &JungleFenceGate,
    &ExposedCopperGrate,
    &WaxedCopperGrate,
    &HardLightBlueStainedGlassPane,
    &JungleTrapdoor,
    &DirtWithRoots,
    &CoalBlock,
    &WhiteWool,
    &WarpedFenceGate,
    &CutSandstoneSlab,
    &SkeletonSkull,
    &ExposedCopperChest,
    &ExposedCopperChain,
    &Composter,
    &WaxedDoubleCutCopperSlab,
    &Kelp,
    &WaxedExposedCopperDoor,
    &DeepslateBricks,
    &BlueGlazedTerracotta,
    &LightBlueGlazedTerracotta,
    &RoseBush,
    &FloweringAzalea,
    &OxidizedCutCopper,
    &BlueWool,
    &PaleOakHangingSign,
    &WeepingVines,
    &ChorusPlant,
    &Water,
    &MudBrickStairs,
    &UnpoweredRepeater,
    &StoneBrickWall,
    &SmoothRedSandstoneStairs,
    &Element100,
    &Element101,
    &Element102,
    &Element103,
    &Element104,
    &Element105,
    &Element106,
    &Element107,
    &Element108,
    &Element109,
    &Element113,
    &Element112,
    &Element111,
    &Element110,
    &Element117,
    &Element116,
    &Element115,
    &Element114,
    &Element118,
    &AndesiteWall,
    &WhiteGlazedTerracotta,
    &StrippedWarpedHyphae,
    &MovingBlock,
    &TrappedChest,
    &AcaciaTrapdoor,
    &WeatheredCopperChest,
    &BrainCoralBlock,
    &WeatheredCopperChain,
    &StandingSign,
    &BambooPlanks,
    &GlowLichen,
    &PurpurPillar,
    &WallBanner,
    &TwistingVines,
    &ChiseledCopper,
    &AcaciaDoubleSlab,
    &DarkOakDoor,
    &OakFence,
    &PaleMossBlock,
    &SoulLantern,
    &Dirt,
    &BlueStainedGlass,
    &Deny,
    &BeeNest,
    &BubbleColumn,
    &Campfire,
    &SmoothStoneDoubleSlab,
    &LightBlueStainedGlass,
    &SoulSoil,
    &SoulSand,
    &GraniteWall,
    &SpruceHangingSign,
    &PolishedDiorite,
    &ReinforcedDeepslate,
    &FletchingTable,
    &CherryLeaves,
    &CreeperHead,
    &BlackGlazedTerracotta,
    &WaxedOxidizedCutCopperStairs,
    &WaxedWeatheredCopperBulb,
    &DragonHead,
    &WaxedWeatheredCopperBars,
    &CalibratedSculkSensor,
    &DarkPrismarineSlab,
    &CopperTrapdoor,
    &StrippedAcaciaLog,
    &CobbledDeepslateDoubleSlab,
    &WarpedFence,
    &CraftingTable,
    &SeaPickle,
    &CherryStandingSign,
    &PaleOakShelf,
    &BrownConcretePowder,
    &MangroveHangingSign,
    &WaxedExposedCopperTrapdoor,
    &BrownCandle,
    &MossyStoneBrickStairs,
    &EndRod,
    &CrimsonStem,
    &GreenConcrete,
    &TuffBrickDoubleSlab,
    &CrimsonSlab,
    &WarpedHyphae,
    &WarpedWartBlock,
    &LightGrayShulkerBox,
    &ResinBricks,
    &Carrots,
    &TuffStairs,
    &YellowCarpet,
    &CyanStainedGlass,
    &BlackStainedGlass,
    &WaxedOxidizedCopperDoor,
    &DeadHornCoral,
    &AndesiteDoubleSlab,
    &GrassBlock,
    &TripwireHook,
    &CaveVinesBodyWithBerries,
    &DarkOakPressurePlate,
    &CopperDoor,
    &HardBlackStainedGlass,
    &StrippedBirchLog,
    &TintedGlass,
    &BigDripleaf,
    &CutSandstone,
    &WarpedHangingSign,
    &LimeWool,
    &BlueCandleCake,
    &SweetBerryBush,
    &PolishedBlackstoneSlab,
    &Reeds,
    &BlackShulkerBox,
    &WeatheredCopperGolemStatue,
    &JungleSapling,
    &ChiseledSandstone,
    &Barrier,
    &TorchflowerCrop,
    &BlackCarpet,
    &PaleOakLog,
    &JungleStandingSign,
    &CherryDoubleSlab,
    &WeatheredCutCopperSlab,
    &OxidizedCopperLantern,
    &DarkOakLeaves,
    &NetherBrickSlab,
    &Fire,
    &Fern,
    &PurpurDoubleSlab,
    &Torchflower,
    &ShortDryGrass,
    &InfestedStone,
    &PaleHangingMoss,
    &PaleMossCarpet,
    &EndPortalFrame,
    &BambooPressurePlate,
    &Prismarine,
    &MagentaCandleCake,
    &ExposedCopperTrapdoor,
    &MushroomStem,
    &BlackTerracotta,
    &ResinBrickStairs,
    &DeepslateGoldOre,
    &AncientDebris,
    &Vault,
    &Beehive,
    &HardOrangeStainedGlass,
    &JungleDoor,
    &Glass,
    &WitherRose,
    &NetherBrickDoubleSlab,
    &ExposedCutCopper,
    &WaxedWeatheredCutCopperStairs,
    &MangroveRoots,
    &YellowCandle,
    &AcaciaStairs,
    &BambooMosaicStairs,
    &BrownConcrete,
    &CherrySlab,
    &ChiseledResinBricks,
    &BubbleCoral,
    &OrangeShulkerBox,
    &LightGrayCandle,
    &PolishedBlackstonePressurePlate,
    &AcaciaStandingSign,
    &PolishedGraniteSlab,
    &SmoothRedSandstoneDoubleSlab,
    &TuffBrickStairs,
    &BlueShulkerBox,
    &ExposedCopperBulb,
    &ExposedCopperBars,
    &DeadFireCoral,
    &StoneBrickSlab,
    &CrimsonStairs,
    &WaxedOxidizedCopperBars,
    &StrippedSpruceLog,
    &WaxedOxidizedCopperBulb,
    &PumpkinStem,
    &AzaleaLeavesFlowered,
    &HardMagentaStainedGlassPane,
    &StickyPistonArmCollision,
    &WarpedNylium,
    &DeepslateEmeraldOre,
    &AcaciaSapling,
    &QuartzBricks,
    &AndesiteSlab,
    &UnpoweredComparator,
    &LimeCandle,
    &StructureBlock,
    &EndBrickStairs,
    &PurpleTerracotta,
    &Target,
    &WoodenButton,
    &MangroveDoor,
    &EndStoneBrickDoubleSlab,
    &HardLimeStainedGlass,
    &WeatheredCopperDoor,
    &PearlescentFroglight,
    &BambooButton,
    &TallGrass,
    &WeatheredCopperLantern,
    &LightBlock12,
    &LightBlock13,
    &LightBlock10,
    &LightBlock11,
    &LightBlock14,
    &LightBlock15,
    &NetherSprouts,
    &CyanStainedGlassPane,
    &DeadHornCoralBlock,
    &VerdantFroglight,
    &HardGrayStainedGlassPane,
    &ResinBlock,
    &WarpedSlab,
    &WarpedStem,
    &HornCoralFan,
    &GreenShulkerBox,
    &LargeFern,
    &StrippedCrimsonHyphae,
    &Cocoa,
    &Lever,
    &BambooSlab,
    &HardGreenStainedGlass,
    &BrickStairs,
    &ColoredTorchGreen,
    &WeatheredCopperTrapdoor,
    &SmoothRedSandstoneSlab,
    &MossBlock,
    &PurpleConcretePowder,
    &PinkGlazedTerracotta,
    &ShortGrass,
    &WaxedWeatheredCutCopperSlab,
    &FireCoralFan,
    &SpruceTrapdoor,
    &ChainCommandBlock,
    &RedSandstone,
    &RedNetherBrickSlab,
    &ExposedChiseledCopper,
    &SpruceFenceGate,
    &ExposedCutCopperSlab,
    &RedNetherBrickStairs,
    &GreenGlazedTerracotta,
    &JunglePlanks,
    &DeepslateRedstoneOre,
    &DeadBrainCoralBlock,
    &MangroveFence,
    &OxidizedCopperGrate,
    &Anvil,
    &BirchTrapdoor,
    &TuffBricks,
    &MangroveLeaves,
    &CobbledDeepslate,
    &QuartzSlab,
    &Bookshelf,
    &Mud,
    &LitPumpkin,
    &Ice,
    &Air,
    &Bed,
    &BlackConcrete,
    &Tnt,
    &PurpleCandleCake,
    &Web,
    &DeadTubeCoralFan,
    &OxidizedCopperChest,
    &OxidizedCopperChain,
    &PaleOakStandingSign,
    &PolishedDioriteStairs,
    &BlueConcretePowder,
    &OrangeConcrete,
    &CryingObsidian,
    &LimeCarpet,
    &ClosedEyeblossom,
    &DeadFireCoralFan,
    &DecoratedPot,
    &GraniteDoubleSlab,
    &EnchantingTable,
    &PolishedBlackstoneWall,
    &WaxedExposedDoubleCutCopperSlab,
    &BubbleCoralWallFan,
    &OrangeTulip,
    &BrownShulkerBox,
    &Azalea,
    &MudBricks,
    &BirchWallSign,
    &BambooWallSign,
    &AcaciaWood,
    &GrayStainedGlassPane,
    &Hopper,
    &HardRedStainedGlass,
    &Bell,
    &Lectern,
    &Bush,
    &StrippedCrimsonStem,
    &StandingBanner,
    &LightBlueShulkerBox,
    &JungleStairs,
    &MangrovePropagule,
    &Cactus,
    &BuddingAmethyst,
    &SnifferEgg,
    &PolishedDioriteDoubleSlab,
    &BirchStairs,
    &NetherBrickWall,
    &PurpleGlazedTerracotta,
    &GreenConcretePowder,
    &Bedrock,
    &SpruceSlab,
    &BlackstoneStairs,
    &BlueIce,
    &CyanShulkerBox,
    &HardRedStainedGlassPane,
    &PolishedAndesiteStairs,
    &DeadHornCoralWallFan,
    &PiglinHead,
    &Sculk,
    &HardPurpleStainedGlassPane,
    &Netherrack,
    &PurpleCandle,
    &SpruceStandingSign,
    &MangroveButton,
    &OrangeCarpet,
    &DeadHornCoralFan,
    &Lantern,
    &CrimsonShelf,
    &WaxedWeatheredCopperDoor,
    &RedStainedGlassPane,
    &LitBlastFurnace,
    &WaxedOxidizedLightningRod,
    &PinkStainedGlassPane,
    &LightBlueWool,
    &Allow,
    &DarkOakFence,
    &DeprecatedPurpurBlock2,
    &DeprecatedPurpurBlock1,
    &BirchDoor,
    &CherryShelf,
    &Chest,
    &CherryWood,
    &Clay,
    &CherryStairs,
    &Cake,
    &CrimsonHangingSign,
    &SculkVein,
    &DeadBrainCoral,
    &DeepslateCoalOre,
    &WeatheredCutCopper,
    &WarpedStandingSign,
    &PolishedAndesiteDoubleSlab,
    &CrackedPolishedBlackstoneBricks,
    &BambooStandingSign,
    &FlowingLava,
    &WitherSkeletonSkull,
    &PolishedTuff,
    &MagentaStainedGlass,
    &HardWhiteStainedGlassPane,
    &AcaciaButton,
    &HardCyanStainedGlassPane,
    &LitFurnace,
    &ChiseledNetherBricks,
    &WarpedButton,
    &RedConcretePowder,
    &LightGrayConcretePowder,
    &DeepslateLapisOre,
    &DeadBubbleCoral,
    &CherrySapling,
    &CherryLog,
    &PrismarineStairs,
    &WhiteCarpet,
    &CyanConcrete,
    &PolishedTuffStairs,
    &DragonEgg,
    &BlueConcrete,
    &NetherBrick,
    &DeepslateIronOre,
    &Element1,
    &Element0,
    &Element3,
    &Element2,
    &Element5,
    &Element4,
    &Element7,
    &Element6,
    &Element9,
    &Element8,
    &OxeyeDaisy,
    &Camera,
    &Wheat,
    &WaxedCutCopper,
    &IronChain,
    &ResinBrickSlab,
    &HeavyCore,
    &CobbledDeepslateSlab,
    &Lilac,
    &PaleOakTrapdoor,
    &ChiseledQuartzBlock,
    &SporeBlossom,
    &WaxedExposedCopperLantern,
    &CrimsonStandingSign,
    &DarkoakStandingSign,
    &WeatheredDoubleCutCopperSlab,
    &PaleOakStairs,
    &EmeraldOre,
    &BrownMushroomBlock,
    &GrayConcretePowder,
    &PetrifiedOakSlab,
    &GrayConcrete,
    &PinkCandle,
    &RedNetherBrickWall,
    &PurpleShulkerBox,
    &CarvedPumpkin,
    &Dropper,
    &SpruceWallSign,
    &StrippedWarpedStem,
    &Candle,
    &PolishedAndesiteSlab,
    &PointedDripstone,
    &RedCarpet,
    &Netherreactor,
    &CutRedSandstoneSlab,
    &DeepslateBrickStairs,
    &DarkPrismarineStairs,
    &CreakingHeart,
    &PaleOakButton,
    &ChiseledTuffBricks,
    &LightBlueConcrete,
    &ExposedCopperGolemStatue,
    &RedTulip,
    &ChemicalHeat,
    &TripWire,
    &Cauldron,
    &CaveVinesHeadWithBerries,
    &TubeCoralBlock,
    &ChiseledRedSandstone,
    &DeadTubeCoralWallFan,
    &BirchSapling,
    &DarkOakTrapdoor,
    &HardPinkStainedGlassPane,
    &OrangeTerracotta,
    &BrickSlab,
    &WaxedOxidizedCopper,
    &OakPlanks,
    &StrippedOakLog,
    &SmoothStoneSlab,
    &PolishedAndesite,
    &SeaLantern,
    &BrewingStand,
    &BambooSapling,
    &WeatheredCopperBulb,
    &WeatheredCopperBars,
    &BlastFurnace,
    &CrimsonRoots,
    &AcaciaSlab,
    &StonecutterBlock,
    &SmoothQuartzSlab,
    &YellowConcretePowder,
    &WhiteCandleCake,
    &CandleCake,
    &LimeStainedGlassPane,
    &EndPortal,
    &YellowStainedGlass,
    &JungleDoubleSlab,
    &PolishedGraniteDoubleSlab,
    &SpruceWood,
    &Blackstone,
    &AcaciaFenceGate,
    &LitDeepslateRedstoneOre,
    &Wildflowers,
    &Element10,
    &Element11,
    &Element12,
    &Element13,
    &Element14,
    &Element15,
    &Element16,
    &Element17,
    &Element18,
    &Element19,
    &Element36,
    &Element37,
    &Element34,
    &Element35,
    &Element32,
    &Element33,
    &Element30,
    &Element31,
    &Element38,
    &Element39,
    &Element29,
    &Element28,
    &Element21,
    &Element20,
    &Element23,
    &Element22,
    &Element25,
    &Element24,
    &Element27,
    &Element26,
    &Element58,
    &Element59,
    &Element54,
    &Element55,
    &Element56,
    &Element57,
    &Element50,
    &Element51,
    &Element52,
    &Element53,
    &Element49,
    &Element48,
    &Element47,
    &Element46,
    &Element45,
    &Element44,
    &Element43,
    &Element42,
    &Element41,
    &Element40,
    &Element72,
    &Element73,
    &Element70,
    &Element71,
    &Element76,
    &Element77,
    &Element74,
    &Element75,
    &Element78,
    &Element79,
    &Element65,
    &Element64,
    &Element67,
    &Element66,
    &Element61,
    &Element60,
    &Element63,
    &Element62,
    &Element69,
    &Element68,
    &Element98,
    &Element99,
    &Element90,
    &Element91,
    &Element92,
    &Element93,
    &Element94,
    &Element95,
    &Element96,
    &Element97,
    &Element89,
    &Element88,
    &Element83,
    &Element82,
    &Element81,
    &Element80,
    &Element87,
    &Element86,
    &Element85,
    &Element84,
    &LitSmoker,
    &LapisOre,
    &RedConcrete,
    &PinkCarpet,
    &SmoothQuartzStairs,
    &RedCandleCake,
    &WaxedCopperLantern,
    &AzaleaLeaves,
    &PurpurBlock,
    &CherryWallSign,
    &CyanCandle,
    &WaxedCopper,
    &RepeatingCommandBlock,
    &ColoredTorchPurple,
    &NetherWart,
    &PurpleCarpet,
    &WaxedOxidizedDoubleCutCopperSlab,
    &CrimsonFungus,
    &CherryPlanks,
    &PolishedDeepslate,
    &TuffDoubleSlab,
    &SmoothRedSandstone,
    &PurpurStairs,
    &TubeCoral,
    &WaxedCopperDoor,
    &Portal,
    &BirchButton,
    &Peony,
    &CommandBlock,
    &PolishedBlackstoneButton,
    &Crafter,
    &SprucePlanks,
    &MossyCobblestoneDoubleSlab,
    &Furnace,
    &InfoUpdate2,
    &AmethystCluster,
    &WaxedChiseledCopper,
    &WaxedCutCopperSlab,
    &PolishedDeepslateWall,
    &PrismarineBrickDoubleSlab,
    &DriedKelpBlock,
    &HardLightGrayStainedGlass,
    &CrimsonFence,
    &ChiseledTuff,
    &LimeConcretePowder,
    &TurtleEgg,
    &Magma,
    &Dispenser,
    &BrownTerracotta,
    &CobblestoneDoubleSlab,
    &DeepslateDiamondOre,
    &Grindstone,
    &WaxedCopperGolemStatue,
    &LightGrayWool,
    &SoulCampfire,
    &PrismarineBricks,
    &WoodenPressurePlate,
    &SandstoneWall,
    &BirchFence,
    &LimeCandleCake,
    &WaxedOxidizedCopperGrate,
    &DamagedAnvil,
    &BirchDoubleSlab,
    &WhiteConcrete,
    &MaterialReducer,
    &TrialSpawner,
    &AcaciaFence,
    &GrassPath,
    &ResinBrickWall,
    &CobbledDeepslateWall,
    &WaxedWeatheredLightningRod,
    &OrangeConcretePowder,
    &OrangeCandleCake,
    &WeatheredCopper,
    &MossyStoneBrickWall,
    &UnlitRedstoneTorch,
    &PaleOakDoubleSlab,
    &LimeTerracotta,
    &CherryFenceGate,
    &GrayGlazedTerracotta,
    &ColoredTorchRed,
    &Lodestone,
    &BambooMosaic,
    &HardBlueStainedGlassPane,
    &RawIronBlock,
    &LightGrayCarpet,
    &PurpleWool,
    &IronBlock,
    &Ladder,
    &CrimsonPressurePlate,
    &StrippedMangroveLog,
    &CopperLantern,
    &Gravel,
    &CartographyTable,
    &OxidizedCopperDoor,
    &TubeCoralWallFan,
    &Dandelion,
];

/// Number of vanilla blocks.
pub const BLOCK_COUNT: usize = 1321;
