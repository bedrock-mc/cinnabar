use super::{selection::bounds, state::State};
use assets::{RegistryRecord, block_shapes::ShapeBox};

/// Compiles visual attachment and signal bounds independently of movement boxes.
pub(super) fn shape(record: &RegistryRecord) -> Option<ShapeBox> {
    let name = record.name.strip_prefix("minecraft:")?;
    let state = State::read(record)?;
    if matches!(
        name,
        "rail" | "golden_rail" | "detector_rail" | "activator_rail"
    ) {
        let direction = state.number("rail_direction")?;
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
    if name == "redstone_wire" {
        return Some(bounds([0.0; 3], [1.0, 0.0625, 1.0]));
    }
    if name.ends_with("_pressure_plate") {
        let height = if state.number("redstone_signal")? > 0 {
            0.03125
        } else {
            0.0625
        };
        return Some(bounds([0.0625, 0.0, 0.0625], [0.9375, height, 0.9375]));
    }
    if name.ends_with("_button") {
        let thickness = if state.bit("button_pressed_bit")? {
            0.0625
        } else {
            0.125
        };
        let (min, max) = match state.number("facing_direction")? {
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
    if name == "lever" {
        let (min, max) = match state.text("lever_direction")? {
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
    if name == "trip_wire" {
        let attached = state.bit("attached_bit")?;
        let suspended = state.bit("suspended_bit")?;
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
    if name == "tripwire_hook" {
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
