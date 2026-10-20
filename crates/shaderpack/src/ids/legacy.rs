//! Iris's built-in block ids for packs without block.properties.

/// Iris's `LegacyIdMap` (Iris 1.11) as the block.properties it amounts to: the
/// pre-flattening numeric ids old packs test `mc_Entity.x` against (SEUS Renewed's leaves
/// are `18.0`, its water `9.0`). Iris uses it only when the pack has no block.properties
/// at all. Names Iris lists that Minecraft has since renamed (`grass`) are kept as Iris
/// keeps them and match nothing.
pub(super) const BLOCK_PROPERTIES: &str = "\
block.1=stone granite diorite andesite\n\
block.2=grass_block\n\
block.4=cobblestone\n\
block.50=torch\n\
block.89=glowstone\n\
block.124=redstone_lamp\n\
block.12=sand\n\
block.24=sandstone\n\
block.41=gold_block\n\
block.42=iron_block\n\
block.57=diamond_block\n\
block.-123=emerald_block\n\
block.35=white_wool orange_wool magenta_wool light_blue_wool yellow_wool lime_wool pink_wool gray_wool light_gray_wool cyan_wool purple_wool blue_wool brown_wool green_wool red_wool black_wool\n\
block.9=water\n\
block.11=lava\n\
block.79=ice\n\
block.18=oak_leaves birch_leaves jungle_leaves spruce_leaves acacia_leaves dark_oak_leaves\n\
block.95=white_stained_glass orange_stained_glass magenta_stained_glass light_blue_stained_glass yellow_stained_glass lime_stained_glass pink_stained_glass gray_stained_glass light_gray_stained_glass cyan_stained_glass purple_stained_glass blue_stained_glass brown_stained_glass green_stained_glass red_stained_glass black_stained_glass\n\
block.160=white_stained_glass_pane orange_stained_glass_pane magenta_stained_glass_pane light_blue_stained_glass_pane yellow_stained_glass_pane lime_stained_glass_pane pink_stained_glass_pane gray_stained_glass_pane light_gray_stained_glass_pane cyan_stained_glass_pane purple_stained_glass_pane blue_stained_glass_pane brown_stained_glass_pane green_stained_glass_pane red_stained_glass_pane black_stained_glass_pane\n\
block.31=grass seagrass sweet_berry_bush\n\
block.59=wheat carrots potatoes\n\
block.37=dandelion poppy blue_orchid allium azure_bluet red_tulip pink_tulip white_tulip orange_tulip oxeye_daisy cornflower lily_of_the_valley wither_rose\n\
block.175=sunflower lilac tall_grass large_fern rose_bush peony tall_seagrass\n\
block.51=fire\n\
block.111=lily_pad\n";
