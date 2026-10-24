//! Native selection bounds are independent of movement collision geometry.
//!
//! The interaction ray and outline both consume these bounds. Passable foliage
//! and the first snow layer remain selectable without a movement collider.
//! Classes without a visual override use the unit visual box.

use assets::block_shapes::ShapeBox;
use assets::facts::BlockProperty;
use assets::{ModelFamily, ModelStateField, RegistryRecord, TOP_SNOW_LAYER_COUNT};
use assets::{facts::shape::ShapeTraits, registry::BlockTraits};

/// Visual bounds used by ray clipping and outlines, independently of movement collision.
pub(super) fn shape(record: &RegistryRecord, shapes: ShapeTraits) -> Option<ShapeBox> {
    if record.model_family == ModelFamily::Pane {
        let state = super::state::State::read(record)?;
        let links = [
            BlockProperty::MinecraftConnectionWest,
            BlockProperty::MinecraftConnectionEast,
            BlockProperty::MinecraftConnectionNorth,
            BlockProperty::MinecraftConnectionSouth,
        ]
        .map(|key| state.bit(key));
        let [Some(west), Some(east), Some(north), Some(south)] = links else {
            return None;
        };
        return Some(bounds(
            [
                if west { 0.0 } else { 0.4375 },
                0.0,
                if north { 0.0 } else { 0.4375 },
            ],
            [
                if east { 1.0 } else { 0.5625 },
                1.0,
                if south { 1.0 } else { 0.5625 },
            ],
        ));
    }
    if record.model_family == ModelFamily::Wall {
        return wall_shape(record);
    }
    if record.traits.contains(BlockTraits::CONNECTS_FENCE) {
        let boxes = super::connected::shapes(record)?;
        let first = boxes.first()?;
        let mut min = first.min;
        let mut max = first.max;
        for shape in &boxes[1..] {
            min[0] = min[0].min(shape.min[0]);
            min[2] = min[2].min(shape.min[2]);
            max[0] = max[0].max(shape.max[0]);
            max[2] = max[2].max(shape.max[2]);
        }
        max[1] = 1.0;
        return Some(ShapeBox::new(min, max));
    }
    // EndGatewayBlock inherits the native unit visual AABB and mayPick=true,
    // independently of its empty End-dimension movement collision shapes.
    if shapes.intersects(ShapeTraits::VISUAL_UNIT | ShapeTraits::END_GATEWAY) {
        return Some(bounds([0.0; 3], [1.0; 3]));
    }
    if record.traits.contains(BlockTraits::STANDING_SIGN) {
        return Some(bounds([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]));
    }
    if record.traits.contains(BlockTraits::WALL_SIGN) {
        return wall_sign_shape(record);
    }
    if record.traits.contains(BlockTraits::BANNER) && !shapes.contains(ShapeTraits::WALL_BANNER) {
        return Some(bounds([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]));
    }
    if shapes.contains(ShapeTraits::WALL_BANNER) {
        return wall_banner_shape(record);
    }
    if record.traits.contains(BlockTraits::HANGING_SIGN) {
        let state = serde_json::from_str::<serde_json::Value>(&record.canonical_state).ok()?;
        return match state["facing_direction"]["value"].as_u64()? {
            4 | 5 => Some(bounds([0.375, 0.0, 0.0], [0.625, 1.0, 1.0])),
            0..=3 => Some(bounds([0.0, 0.0, 0.375], [1.0, 1.0, 0.625])),
            _ => None,
        };
    }
    if record.traits.contains(BlockTraits::SNOW_LAYER) {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
        let height = state.get("height")?.get("value")?.as_u64()?;
        if height >= u64::from(TOP_SNOW_LAYER_COUNT) {
            return None;
        }
        // Snow layers: full X/Z,
        // visual/outline height (height + 1)/8, independently of collision.
        return Some(ShapeBox::new(
            [0.0; 3],
            [
                1.0,
                (height + 1) as f64 / f64::from(TOP_SNOW_LAYER_COUNT),
                1.0,
            ],
        ));
    }
    if record.traits.contains(BlockTraits::TORCH) {
        return torch_shape(record);
    }
    if matches!(
        assets::facts::block_render_fact(&record.name).kind,
        assets::facts::BlockRenderKind::EndRod | assets::facts::BlockRenderKind::LightningRod
    ) {
        return rod_shape(record);
    }
    let (inset, height, edge): (f32, f32, f32) = match () {
        _ if shapes.contains(ShapeTraits::GRASS) => (0.1, 0.8, 0.9),
        _ if shapes.contains(ShapeTraits::SHORT_DRY_GRASS) => (0.125, 0.625, 0.875),
        _ if shapes.contains(ShapeTraits::MUSHROOM) => (0.3, 0.4, 0.7),
        _ if shapes.contains(ShapeTraits::NETHER_SPROUTS) => (0.15, 0.3, 0.85),
        _ if shapes.contains(ShapeTraits::CACTUS_FLOWER) => (0.0625, 0.875, 0.9375),
        _ if shapes.contains(ShapeTraits::REEDS) => (0.125, 1.0, 0.875),
        _ if shapes.intersects(
            ShapeTraits::WHEAT
                | ShapeTraits::CARROTS
                | ShapeTraits::ROOT_CROP
                | ShapeTraits::NETHER_WART
                | ShapeTraits::TORCHFLOWER_CROP,
        ) =>
        {
            (0.0, 0.25, 1.0)
        }
        _ if shapes.contains(ShapeTraits::FLOWER) => (0.15, 0.6, 0.7),
        _ if shapes.contains(ShapeTraits::BUSH) => (0.0, 0.8, 1.0),
        _ if shapes.contains(ShapeTraits::DOUBLE_PLANT) => {
            let upper = super::state::State::read(record)?.bit(BlockProperty::UpperBlockBit)?;
            (0.3, if upper { 0.6 } else { 1.0 }, 0.7)
        }
        _ => return None,
    };
    Some(bounds([inset, 0.0, inset], [edge, height, edge]))
}

/// Walls outline the visual post and arms, independently of their taller collider.
fn wall_shape(record: &RegistryRecord) -> Option<ShapeBox> {
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
            0.9999_f32
        } else {
            0.8125
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
fn torch_shape(record: &RegistryRecord) -> Option<ShapeBox> {
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

/// End and lightning rods use a four-pixel square and fill the block along their facing axis.
fn rod_shape(record: &RegistryRecord) -> Option<ShapeBox> {
    let facing = super::state::State::read(record)?.number(BlockProperty::FacingDirection)?;
    let axis = match facing {
        0 | 1 => 1,
        2 | 3 => 2,
        4 | 5 => 0,
        _ => return None,
    };
    let mut min = [0.375; 3];
    let mut max = [0.625; 3];
    min[axis] = 0.0;
    max[axis] = 1.0;
    Some(bounds(min, max))
}

/// Selectable wall-sign bounds sit against the supporting face without movement collision.
fn wall_sign_shape(record: &RegistryRecord) -> Option<ShapeBox> {
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
fn wall_banner_shape(record: &RegistryRecord) -> Option<ShapeBox> {
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

/// Converts native f32 selection coordinates to the compiled bounds type.
pub(super) fn bounds(min: [f32; 3], max: [f32; 3]) -> ShapeBox {
    let point = |[x, y, z]: [f32; 3]| [f64::from(x), f64::from(y), f64::from(z)];
    ShapeBox::new(point(min), point(max))
}
