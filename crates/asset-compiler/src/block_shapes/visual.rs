use super::{UNIT, selection::bounds, state::State};
use assets::facts::BlockProperty;
use assets::{RegistryRecord, block_shapes::ShapeBox};
use assets::{facts::shape::ShapeTraits, registry::BlockTraits};

/// Compiles class visual shapes that differ from movement boxes or their quantization.
pub(super) fn shape(record: &RegistryRecord, shapes: ShapeTraits) -> Option<ShapeBox> {
    if record.model_family == assets::ModelFamily::Chest {
        return Some(bounds([0.025, 0.0, 0.025], [0.975, 0.95, 0.975]));
    }
    Some(match () {
        _ if shapes.intersects(
            ShapeTraits::VISUAL_UNIT
                | ShapeTraits::ANVIL
                | ShapeTraits::CACTUS
                | ShapeTraits::BREWING_STAND,
        ) || record.traits.contains(BlockTraits::WATER)
            || record.traits.contains(BlockTraits::LAVA)
            || shapes.intersects(ShapeTraits::BUBBLE_COLUMN | ShapeTraits::CHORUS_PLANT) =>
        {
            UNIT
        }
        _ if shapes.contains(ShapeTraits::HONEY) => {
            bounds([0.0625, 0.0, 0.0625], [0.9375, 0.9375, 0.9375])
        }
        _ if record.traits.contains(BlockTraits::FARMLAND)
            || shapes.contains(ShapeTraits::PATH) =>
        {
            bounds([0.0; 3], [1.0, 0.9375, 1.0])
        }
        _ if shapes.contains(ShapeTraits::LECTERN) => bounds([0.0; 3], [1.0, 0.9, 1.0]),
        _ if shapes.contains(ShapeTraits::CONDUIT) => bounds([0.25, 0.0, 0.25], [0.75, 0.5, 0.75]),
        _ if shapes.contains(ShapeTraits::END_PORTAL_FRAME) => bounds([0.0; 3], [1.0, 0.8125, 1.0]),
        _ if (record.traits.contains(BlockTraits::PORTAL)
            && !record.traits.contains(BlockTraits::END_PORTAL)) =>
        {
            match State::read(record)?.text(BlockProperty::PortalAxis)? {
                "x" => bounds([0.0, 0.0, 0.375], [1.0, 1.0, 0.625]),
                "z" | "unknown" => bounds([0.375, 0.0, 0.0], [0.625, 1.0, 1.0]),
                _ => return None,
            }
        }
        _ if record.traits.contains(BlockTraits::DRIPSTONE_OFFSET) => {
            let state = State::read(record)?;
            let (inset, bottom, top) = match state.text(BlockProperty::DripstoneThickness)? {
                "tip" => (
                    0.3125_f32,
                    if state.bit(BlockProperty::Hanging)? {
                        0.3125
                    } else {
                        0.0
                    },
                    if state.bit(BlockProperty::Hanging)? {
                        1.0
                    } else {
                        0.6875
                    },
                ),
                "merge" => (0.3125, 0.0, 1.0),
                "frustum" => (0.25, 0.0, 1.0),
                "middle" => (0.1875, 0.0, 1.0),
                "base" => (0.125, 0.0, 1.0),
                _ => return None,
            };
            let [x, _, z] = block_transform::random_offset::POINTED_DRIPSTONE.offset([0; 3]);
            bounds(
                [inset + x, bottom, inset + z],
                [1.0 - inset + x, top, 1.0 - inset + z],
            )
        }
        _ if shapes.contains(ShapeTraits::BAMBOO) => {
            let state = assets::bamboo::BambooState::from_record(record)?;
            let edge = if state.thick { 0.6875_f32 } else { 0.625_f32 };
            let [x, _, z] = block_transform::bamboo::ORIGIN_OFFSET;
            bounds([0.5 + x, 0.0, 0.5 + z], [edge + x, 1.0, edge + z])
        }
        _ if shapes.contains(ShapeTraits::TALL_DRY_GRASS) => {
            bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375])
        }
        _ if shapes.intersects(
            ShapeTraits::WHEAT
                | ShapeTraits::CARROTS
                | ShapeTraits::ROOT_CROP
                | ShapeTraits::NETHER_WART
                | ShapeTraits::TORCHFLOWER_CROP,
        ) =>
        {
            bounds([0.0; 3], [1.0, 0.25, 1.0])
        }
        _ if shapes.contains(ShapeTraits::TURTLE_EGG) => bounds(
            [0.5_f32 - 0.3, 0.0, f32::from_bits(0x3e4ccccc)],
            [0.8, 0.45, 0.8],
        ),
        _ if shapes.contains(ShapeTraits::BIG_DRIPLEAF) => {
            let state = State::read(record)?;
            if state.bit(BlockProperty::BigDripleafHead)? {
                bounds([0.0; 3], [1.0, 0.9375, 1.0])
            } else {
                let (min, max) = match state.direction()? {
                    0 => ([0.3125, 0.0, 0.125], [0.6875, 1.0, 0.5]),
                    1 => ([0.5, 0.0, 0.3125], [0.875, 1.0, 0.6875]),
                    2 => ([0.3125, 0.0, 0.5], [0.6875, 1.0, 0.875]),
                    3 => ([0.125, 0.0, 0.3125], [0.5, 1.0, 0.6875]),
                    _ => return None,
                };
                bounds(min, max)
            }
        }
        _ if record.traits.contains(BlockTraits::LIGHT_BLOCK) => UNIT,
        _ if record.model_family == assets::ModelFamily::Trapdoor => {
            let state = State::read(record)?;
            let thickness = assets::door::THICKNESS;
            if state.bit(BlockProperty::OpenBit)? {
                let (min, max) = match state.direction()? {
                    0 => ([0.0; 3], [thickness, 1.0, 1.0]),
                    1 => ([1.0 - thickness, 0.0, 0.0], [1.0; 3]),
                    2 => ([0.0; 3], [1.0, 1.0, thickness]),
                    3 => ([0.0, 0.0, 1.0 - thickness], [1.0; 3]),
                    _ => return None,
                };
                bounds(min, max)
            } else if state.bit(BlockProperty::UpsideDownBit)? {
                bounds([0.0, 1.0 - thickness, 0.0], [1.0; 3])
            } else {
                bounds([0.0; 3], [1.0, thickness, 1.0])
            }
        }
        _ => return None,
    })
}
