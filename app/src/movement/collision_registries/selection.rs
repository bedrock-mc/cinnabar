//! Vanilla plants have visual pick bounds even when their movement collision is empty.

use assets::RegistryRecord;
use sim::{Aabb, Vec3};

/// Visual bounds from R:TallGrassBlock:59, R:FlowerBlock:45, R:SaplingBlock:120,
/// R:BushBlock:55 and R:DoublePlantBaseBlock:56; `BlockType::clip` picks these
/// independently of movement collision (R:BlockType:20090,20728).
pub(super) fn shape(record: &RegistryRecord) -> Option<Aabb> {
    let name = record.name.strip_prefix("minecraft:")?;
    if matches!(
        name,
        "torch" | "soul_torch" | "redstone_torch" | "unlit_redstone_torch"
    ) {
        return torch_shape(record);
    }
    let (inset, height, edge): (f32, f32, f32) = match name {
        "short_grass" | "fern" => (0.1, 0.8, 0.9),
        // R:ShortDryGrassBlock:53, R:MushroomBlock:240, R:NetherSproutsBlock:41,
        // R:CactusFlowerBlock:53 and R:SugarCaneBlock:40 (Lens reads the referenced f32s).
        "short_dry_grass" => (0.125, 0.625, 0.875),
        "brown_mushroom" | "red_mushroom" => (0.3, 0.4, 0.7),
        "nether_sprouts" => (0.15, 0.3, 0.85),
        "cactus_flower" => (0.0625, 0.875, 0.9375),
        "reeds" => (0.125, 1.0, 0.875),
        // R:CropBlock:250 and R:NetherWartBlock:102; Lens 0x10dab6d30 is (1, .25).
        "wheat" | "carrots" | "potatoes" | "beetroot" | "nether_wart" => (0.0, 0.25, 1.0),
        "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "red_tulip"
        | "orange_tulip" | "white_tulip" | "pink_tulip" | "oxeye_daisy" | "cornflower"
        | "lily_of_the_valley" | "wither_rose" | "torchflower" => (0.15, 0.6, 0.7),
        "oak_sapling" | "spruce_sapling" | "birch_sapling" | "jungle_sapling"
        | "acacia_sapling" | "dark_oak_sapling" | "cherry_sapling" | "pale_oak_sapling" => {
            (0.1, 0.8, 0.9)
        }
        "deadbush" => (0.1, 1.0, 0.9),
        "bush" => (0.0, 1.0, 1.0),
        "tall_grass" | "large_fern" | "sunflower" | "lilac" | "rose_bush" | "peony" => {
            let upper = serde_json::from_str::<serde_json::Value>(&record.canonical_state).ok()?["upper_block_bit"]
                ["value"]
                == 1;
            (0.3, if upper { 0.6 } else { 1.0 }, 0.7)
        }
        _ => return None,
    };
    Some(Aabb::new(
        Vec3::new(f64::from(inset), 0.0, f64::from(inset)),
        Vec3::new(f64::from(edge), f64::from(height), f64::from(edge)),
    ))
}

/// R:TorchBlock:880 chooses the visual box by `torch_facing_direction`, independently
/// of its empty collision (R:TorchBlock:918). Lens `read_data` verifies the four
/// wall arrays at 0x10e265d40 and the upright vector at 0x10e265880.
fn torch_shape(record: &RegistryRecord) -> Option<Aabb> {
    let state = serde_json::from_str::<serde_json::Value>(&record.canonical_state).ok()?;
    let facing = state["torch_facing_direction"]["value"].as_str()?;
    let (min, max) = match facing {
        "west" => ([0.0, 0.2, 0.35], [0.3, 0.8, 0.65]),
        "east" => ([0.7, 0.2, 0.35], [1.0, 0.8, 0.65]),
        "north" => ([0.35, 0.2, 0.0], [0.65, 0.8, 0.3]),
        "south" => ([0.35, 0.2, 0.7], [0.65, 0.8, 1.0]),
        _ => ([0.4, 0.0, 0.4], [0.6, 0.6, 0.6]),
    };
    let vector = |point: [f32; 3]| {
        Vec3::new(
            f64::from(point[0]),
            f64::from(point[1]),
            f64::from(point[2]),
        )
    };
    Some(Aabb::new(vector(min), vector(max)))
}
