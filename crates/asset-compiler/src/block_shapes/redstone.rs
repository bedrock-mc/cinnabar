use super::{selection::bounds, state::State};
use assets::facts::BlockProperty;
use assets::facts::shape::ShapeTraits;
use assets::{RegistryRecord, block_shapes::ShapeBox};

/// Compiles visual attachment and signal bounds independently of movement boxes.
pub(super) fn shape(record: &RegistryRecord, shapes: ShapeTraits) -> Option<ShapeBox> {
    let state = State::read(record)?;
    if record.model_family == assets::ModelFamily::Rail {
        let direction = state.number(BlockProperty::RailDirection)?;
        return Some(bounds(
            [0.0; 3],
            [
                1.0,
                if (2..=5).contains(&direction) {
                    0.5
                } else {
                    0.0625
                },
                1.0,
            ],
        ));
    }
    if shapes.contains(ShapeTraits::REDSTONE_WIRE) {
        return Some(bounds([0.0; 3], [1.0, 0.0625, 1.0]));
    }
    if record.model_family == assets::ModelFamily::PressurePlate {
        let height = if state.number(BlockProperty::RedstoneSignal)? > 0 {
            0.03125
        } else {
            0.0625
        };
        return Some(bounds([0.0625, 0.0, 0.0625], [0.9375, height, 0.9375]));
    }
    if record.model_family == assets::ModelFamily::Button {
        let thickness = if state.bit(BlockProperty::ButtonPressedBit)? {
            0.0625
        } else {
            0.125
        };
        let (min, max) = match state.number(BlockProperty::FacingDirection)? {
            0 => ([0.3125, 1.0 - thickness, 0.375], [0.6875, 1.0, 0.625]),
            1 => ([0.3125, 0.0, 0.375], [0.6875, thickness, 0.625]),
            2 => ([0.3125, 0.375, 1.0 - thickness], [0.6875, 0.625, 1.0]),
            3 => ([0.3125, 0.375, 0.0], [0.6875, 0.625, thickness]),
            4 => ([1.0 - thickness, 0.375, 0.3125], [1.0, 0.625, 0.6875]),
            5 => ([0.0, 0.375, 0.3125], [thickness, 0.625, 0.6875]),
            _ => return None,
        };
        return Some(bounds(min, max));
    }
    if record.model_family == assets::ModelFamily::Lever {
        let (min, max) = match state.text(BlockProperty::LeverDirection)? {
            "east" => ([0.0, 0.25, 0.3125], [0.375, 0.75, 0.6875]),
            "west" => ([0.625, 0.25, 0.3125], [1.0, 0.75, 0.6875]),
            "south" => ([0.3125, 0.25, 0.0], [0.6875, 0.75, 0.375]),
            "north" => ([0.3125, 0.25, 0.625], [0.6875, 0.75, 1.0]),
            "up_north_south" | "up_east_west" => ([0.25, 0.0, 0.25], [0.75, 0.6, 0.75]),
            "down_north_south" | "down_east_west" => ([0.25, 0.4, 0.25], [0.75, 1.0, 0.75]),
            _ => return None,
        };
        return Some(bounds(min, max));
    }
    if shapes.contains(ShapeTraits::TRIP_WIRE) {
        let attached = state.bit(BlockProperty::AttachedBit)?;
        let suspended = state.bit(BlockProperty::SuspendedBit)?;
        let (min, max) = if suspended {
            if attached {
                (0.0625, 0.15625)
            } else {
                (0.0, 0.5)
            }
        } else {
            (0.0, 0.09375)
        };
        return Some(bounds([0.0, min, 0.0], [1.0, max, 1.0]));
    }
    if shapes.contains(ShapeTraits::TRIPWIRE_HOOK) {
        let (min, max) = match state.direction()? {
            0 => ([0.375, 0.0625, 0.0], [0.625, 0.5625, 0.375]),
            1 => ([0.625, 0.0625, 0.375], [1.0, 0.5625, 0.625]),
            2 => ([0.375, 0.0625, 0.625], [0.625, 0.5625, 1.0]),
            3 => ([0.0, 0.0625, 0.375], [0.375, 0.5625, 0.625]),
            _ => return None,
        };
        return Some(bounds(min, max));
    }
    None
}
