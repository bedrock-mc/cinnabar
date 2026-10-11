//! Native selection bounds are independent of movement collision geometry.
//!
//! The interaction ray and outline both consume these bounds. Passable foliage
//! and the first snow layer remain selectable without a movement collider.
//! Unreviewed blocks retain their existing registry bounds.

use assets::{ModelFamily, ModelStateField, RegistryRecord, TOP_SNOW_LAYER_COUNT};
use sim::{Aabb, Vec3};

impl super::PhysicsCollisionRegistries {
    /// Native gateways hide their overlay; barriers expose it only in Creative.
    pub fn selection_overlay_visible(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> bool {
        let identifier = self.block_identifier(mode, runtime_id);
        identifier != Some("minecraft:end_gateway")
            && (identifier != Some("minecraft:barrier")
                || game_mode == Some(protocol::PlayerGameMode::Creative))
    }
}

/// Visual bounds used by ray clipping and outlines, independently of movement collision.
pub(super) fn shape(record: &RegistryRecord) -> Option<Aabb> {
    let name = record.name.strip_prefix("minecraft:")?;
    if record.model_family == ModelFamily::Wall {
        return wall_shape(record);
    }
    if name.ends_with("_fence") {
        let boxes = super::connected::shapes(record)?;
        let first = boxes.first()?;
        let mut min = first.min;
        let mut max = first.max;
        for shape in &boxes[1..] {
            min.x = min.x.min(shape.min.x);
            min.z = min.z.min(shape.min.z);
            max.x = max.x.max(shape.max.x);
            max.z = max.z.max(shape.max.z);
        }
        max.y = 1.0;
        return Some(Aabb::new(min, max));
    }
    // EndGatewayBlock inherits the native unit visual AABB and mayPick=true,
    // independently of its empty End-dimension movement collision shapes.
    if matches!(name, "web" | "end_gateway") {
        return Some(bounds([0.0; 3], [1.0; 3]));
    }
    if name.ends_with("standing_sign") {
        return Some(bounds([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]));
    }
    if name.ends_with("wall_sign") {
        return wall_sign_shape(record);
    }
    if name == "standing_banner" {
        return Some(bounds([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]));
    }
    if name == "wall_banner" {
        return wall_banner_shape(record);
    }
    if name.ends_with("hanging_sign") {
        let state = serde_json::from_str::<serde_json::Value>(&record.canonical_state).ok()?;
        return match state["facing_direction"]["value"].as_u64()? {
            4 | 5 => Some(bounds([0.375, 0.0, 0.0], [0.625, 1.0, 1.0])),
            0..=3 => Some(bounds([0.0, 0.0, 0.375], [1.0, 1.0, 0.625])),
            _ => None,
        };
    }
    if name == "snow_layer" {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
        let height = state.get("height")?.get("value")?.as_u64()?;
        if height >= u64::from(TOP_SNOW_LAYER_COUNT) {
            return None;
        }
        // Snow layers: full X/Z,
        // visual/outline height (height + 1)/8, independently of collision.
        return Some(Aabb::new(
            Vec3::ZERO,
            Vec3::new(
                1.0,
                (height + 1) as f64 / f64::from(TOP_SNOW_LAYER_COUNT),
                1.0,
            ),
        ));
    }
    if matches!(
        name,
        "torch" | "soul_torch" | "redstone_torch" | "unlit_redstone_torch"
    ) {
        return torch_shape(record);
    }
    let (inset, height, edge): (f32, f32, f32) = match name {
        "short_grass" | "fern" => (0.1, 0.8, 0.9),
        "short_dry_grass" => (0.125, 0.625, 0.875),
        "brown_mushroom" | "red_mushroom" => (0.3, 0.4, 0.7),
        "nether_sprouts" => (0.15, 0.3, 0.85),
        "cactus_flower" => (0.0625, 0.875, 0.9375),
        "reeds" => (0.125, 1.0, 0.875),
        // Crops and nether wart span the cell width and are .25 blocks tall.
        "wheat" | "carrots" | "potatoes" | "beetroot" | "nether_wart" => (0.0, 0.25, 1.0),
        "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "red_tulip"
        | "orange_tulip" | "white_tulip" | "pink_tulip" | "oxeye_daisy" | "cornflower"
        | "lily_of_the_valley" | "wither_rose" | "torchflower" => (0.15, 0.6, 0.7),
        "oak_sapling" | "spruce_sapling" | "birch_sapling" | "jungle_sapling"
        | "acacia_sapling" | "dark_oak_sapling" | "cherry_sapling" | "pale_oak_sapling" => {
            (0.1, 0.8, 0.9)
        }
        // Dead bushes use grass-sized bounds rather than flower bounds,
        // including maxY=.8.
        "deadbush" => (0.1, 0.8, 0.9),
        // Bushes span minXYZ=(0,0,0) to (1, .8, 1).
        "bush" => (0.0, 0.8, 1.0),
        "tall_grass" | "large_fern" | "sunflower" | "lilac" | "rose_bush" | "peony" => {
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
            let upper = state.get("upper_block_bit")?.get("value")?.as_i64()? != 0;
            (0.3, if upper { 0.6 } else { 1.0 }, 0.7)
        }
        _ => return None,
    };
    Some(bounds([inset, 0.0, inset], [edge, height, edge]))
}

/// Walls outline the visual post and arms, independently of their taller collider.
fn wall_shape(record: &RegistryRecord) -> Option<Aabb> {
    let connections = record.model_state.get(ModelStateField::Connections)?;
    let [north, east, south, west] =
        std::array::from_fn::<_, 4, _>(|axis| (connections >> (axis * 2)) & 3);
    let links = [north, east, south, west];
    if connections & !0x1ff != 0 || links.contains(&3) {
        return None;
    }
    let post = connections & 0x100 != 0;
    let mut min = [
        if west != 0 { 0.0 } else { 0.25 },
        0.0,
        if north != 0 { 0.0 } else { 0.25 },
    ];
    let mut max = [
        if east != 0 { 1.0 } else { 0.75 },
        if post || links.contains(&2) {
            1.0
        } else {
            0.875
        },
        if south != 0 { 1.0 } else { 0.75 },
    ];
    if !post && north != 0 && south != 0 && east == 0 && west == 0 {
        min[0] = 0.3125;
        max[0] = 0.6875;
    }
    if !post && east != 0 && west != 0 && north == 0 && south == 0 {
        min[2] = 0.3125;
        max[2] = 0.6875;
    }
    Some(bounds(min, max))
}

/// Torches choose the visual box by `torch_facing_direction`, independently
/// of its empty collision. Four wall boxes and one upright box cover the orientations.
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
    Some(bounds(min, max))
}

/// Selectable wall-sign bounds sit against the supporting face without movement collision.
fn wall_sign_shape(record: &RegistryRecord) -> Option<Aabb> {
    let state = serde_json::from_str::<serde_json::Value>(&record.canonical_state).ok()?;
    let facing = state["facing_direction"]["value"].as_u64()?;
    let (min, max) = match facing {
        2 => ([0.0, 0.28125, 0.875], [1.0, 0.78125, 1.0]),
        3 => ([0.0, 0.28125, 0.0], [1.0, 0.78125, 0.125]),
        4 => ([0.875, 0.28125, 0.0], [1.0, 0.78125, 1.0]),
        5 => ([0.0, 0.28125, 0.0], [0.125, 0.78125, 1.0]),
        0 | 1 => ([0.0; 3], [1.0; 3]),
        _ => return None,
    };
    Some(bounds(min, max))
}

/// Wall banners select a thin slab against the supporting face, from the cell
/// floor to the hanging rod, without movement collision.
fn wall_banner_shape(record: &RegistryRecord) -> Option<Aabb> {
    let state = serde_json::from_str::<serde_json::Value>(&record.canonical_state).ok()?;
    let (min, max) = match state["facing_direction"]["value"].as_u64()? {
        2 => ([0.0, 0.0, 0.875], [1.0, 0.78125, 1.0]),
        3 => ([0.0, 0.0, 0.0], [1.0, 0.78125, 0.125]),
        4 => ([0.875, 0.0, 0.0], [1.0, 0.78125, 1.0]),
        5 => ([0.0, 0.0, 0.0], [0.125, 0.78125, 1.0]),
        _ => return None,
    };
    Some(bounds(min, max))
}

/// Converts native f32 selection coordinates to the simulator bounds type.
fn bounds(min: [f32; 3], max: [f32; 3]) -> Aabb {
    let point = |[x, y, z]: [f32; 3]| Vec3::new(f64::from(x), f64::from(y), f64::from(z));
    Aabb::new(point(min), point(max))
}

#[cfg(test)]
mod tests;
