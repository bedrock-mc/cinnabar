use super::{selection::bounds, state::State};
use assets::facts::BlockProperty;
use assets::facts::shape::ShapeTraits;
use assets::{RegistryRecord, block_shapes::ShapeBox};

/// Compiles visual plant boxes; their movement geometry remains independent.
pub(super) fn shape(record: &RegistryRecord, shapes: ShapeTraits) -> Option<ShapeBox> {
    let shape = match () {
        _ if shapes.intersects(
            ShapeTraits::PLANT_UNIT
                | ShapeTraits::CAVE_VINES
                | ShapeTraits::TWISTING_VINES
                | ShapeTraits::WEEPING_VINES
                | ShapeTraits::HANGING_MOSS,
        ) =>
        {
            super::UNIT
        }
        _ if shapes.contains(ShapeTraits::FROG_SPAWN) => bounds([0.0; 3], [1.0, 0.015625, 1.0]),
        _ if shapes.contains(ShapeTraits::PETALS) => bounds([0.0; 3], [1.0, 0.1875, 1.0]),
        _ if shapes.contains(ShapeTraits::LEAF_LITTER) => bounds([0.0; 3], [1.0, 0.0625, 1.0]),
        _ if shapes.contains(ShapeTraits::SEA_PICKLE) => bounds([0.0; 3], [1.0, 0.5, 1.0]),
        _ if shapes.intersects(ShapeTraits::CRIMSON_FUNGUS | ShapeTraits::WARPED_FUNGUS) => bounds(
            [0.5_f32 - 0.23, 0.0, 0.27],
            [
                0.73,
                if shapes.contains(ShapeTraits::CRIMSON_FUNGUS) {
                    0.75
                } else {
                    0.58
                },
                0.73,
            ],
        ),
        _ if shapes.contains(ShapeTraits::KELP) => bounds([0.125, 0.0, 0.125], [0.875, 1.0, 0.875]),
        _ if shapes.contains(ShapeTraits::GRASS) => bounds([0.1, 0.0, 0.1], [0.9, 0.8, 0.9]),
        _ if shapes.contains(ShapeTraits::SHELF_MUSHROOM) => shelf(record)?,
        _ if shapes.intersects(ShapeTraits::SMALL_DRIPLEAF | ShapeTraits::FLOWER) => {
            bounds([0.15, 0.0, 0.15], [0.7, 0.6, 0.7])
        }
        _ if shapes.contains(ShapeTraits::PITCHER_PLANT) => bounds(
            [0.3, 0.0, 0.3],
            [
                0.7,
                if State::read(record)?.bit(BlockProperty::UpperBlockBit)? {
                    0.6
                } else {
                    1.0
                },
                0.7,
            ],
        ),
        _ if shapes.contains(ShapeTraits::HANGING_ROOTS) => {
            bounds([0.15, 0.7, 0.15], [0.85, 1.0, 0.85])
        }
        _ if shapes.contains(ShapeTraits::SPORE_BLOSSOM) => {
            bounds([0.15, 0.75, 0.15], [0.85, 1.0, 0.85])
        }
        _ if shapes.contains(ShapeTraits::BERRY_BUSH) => {
            if State::read(record)?.number(BlockProperty::Growth)? == 0 {
                bounds([0.5_f32 - 0.2, 0.0, 0.5_f32 - 0.2], [0.7, 0.8, 0.7])
            } else {
                bounds([0.5_f32 - 0.4, 0.0, 0.5_f32 - 0.4], [0.9, 1.0, 0.9])
            }
        }
        _ if shapes.contains(ShapeTraits::PROPAGULE) => {
            let state = State::read(record)?;
            if state.bit(BlockProperty::Hanging)? {
                let growth = state.number(BlockProperty::PropaguleStage)?.clamp(0, 4);
                let length =
                    (3 * (growth + 1) + i64::from(growth == 0 || growth >= 3)) as f32 / 16.0;
                bounds([0.375, 1.0 - length, 0.375], [0.625, 1.0, 0.625])
            } else {
                bounds([0.375, 0.0, 0.375], [0.625, 1.0, 0.625])
            }
        }
        _ if shapes.contains(ShapeTraits::STEM) => stem(record)?,
        _ if shapes.contains(ShapeTraits::PITCHER_CROP) => {
            let state = State::read(record)?;
            let growth = state.number(BlockProperty::Growth)?;
            if state.bit(BlockProperty::UpperBlockBit)? {
                bounds(
                    [0.1875, 0.0, 0.1875],
                    [0.8125, if growth == 3 { 0.6875 } else { 0.9375 }, 0.8125],
                )
            } else if growth == 0 {
                bounds([0.3125, -0.0625, 0.3125], [0.6875, 0.1875, 0.6875])
            } else {
                bounds(
                    [0.1875, -0.0625, 0.1875],
                    [0.8125, if growth == 1 { 0.875 } else { 1.0 }, 0.8125],
                )
            }
        }
        _ if shapes.contains(ShapeTraits::CORAL) => bounds([0.1, 0.0, 0.1], [0.9, 0.8, 0.9]),
        _ if shapes.contains(ShapeTraits::CORAL_FAN) => {
            bounds([0.5_f32 - 0.3, 0.0, 0.5_f32 - 0.3], [0.8, 0.4, 0.8])
        }
        _ if shapes.contains(ShapeTraits::CORAL_WALL_FAN) => {
            bounds([0.5_f32 - 0.3, 0.3, 0.5_f32 - 0.3], [0.8, 0.7, 0.8])
        }
        _ => return None,
    };
    Some(shape)
}

/// Computes a stem's growth height and attachment extension.
fn stem(record: &RegistryRecord) -> Option<ShapeBox> {
    let state = State::read(record)?;
    let facing = state.number(BlockProperty::FacingDirection)?;
    let height = if facing == 0 {
        (2 * state.number(BlockProperty::Growth)? + 2) as f32 / 16.0
    } else {
        0.25
    };
    let mut shape = bounds([0.375, 0.0, 0.375], [0.625, height, 0.625]);
    match facing {
        2 => shape.min[2] = 0.0,
        3 => shape.max[2] = 1.0,
        4 => shape.min[0] = 0.0,
        5 => shape.max[0] = 1.0,
        _ => {}
    }
    Some(shape)
}

/// Selects a growth-sized shelf cap against its supporting face, independently of collision.
fn shelf(record: &RegistryRecord) -> Option<ShapeBox> {
    let state = State::read(record)?;
    let (inset, lower, depth) = match state.number(BlockProperty::Growth)? {
        0 => (0.1875, 0.5, 0.4375),
        1 => (0.0625, 0.375, 0.625),
        _ => return None,
    };
    let (min, max) = match state.direction()? {
        0 => ([inset, lower, 0.0], [1.0 - inset, 0.6875, depth]),
        1 => ([1.0 - depth, lower, inset], [1.0, 0.6875, 1.0 - inset]),
        2 => ([inset, lower, 1.0 - depth], [1.0 - inset, 0.6875, 1.0]),
        3 => ([0.0, lower, inset], [depth, 0.6875, 1.0 - inset]),
        _ => return None,
    };
    Some(bounds(min, max))
}
