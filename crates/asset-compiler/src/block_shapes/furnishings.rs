use super::{selection::bounds, state::State};
use assets::{ModelFamily, RegistryRecord, block_shapes::ShapeBox};

/// Compiles stored class bounds and state-only furniture overrides.
pub(super) fn shape(record: &RegistryRecord) -> Option<ShapeBox> {
    let name = record.name.strip_prefix("minecraft:")?;
    if record.model_family == ModelFamily::Slab {
        if super::state::is_double_slab(record) {
            return Some(super::UNIT);
        }
        let state = State::read(record)?;
        let top = state
            .text("minecraft:vertical_half")
            .map(|half| half == "top")
            .or_else(|| state.bit("top_slot_bit"))?;
        return Some(bounds(
            [0.0, if top { 0.5 } else { 0.0 }, 0.0],
            [1.0, if top { 1.0 } else { 0.5 }, 1.0],
        ));
    }
    if name.ends_with("_shelf") {
        let mut min = [0.0; 3];
        let mut max = [1.0; 3];
        match State::read(record)?.direction()? {
            0 => max[2] = 0.3125,
            1 => min[0] = 0.6875,
            2 => min[2] = 0.6875,
            3 => max[0] = 0.3125,
            _ => return None,
        }
        return Some(bounds(min, max));
    }
    Some(match name {
        "bell" => bell(record)?,
        "cocoa" => cocoa(record)?,
        "bed" | "stonecutter_block" => bounds([0.0; 3], [1.0, 0.5625, 1.0]),
        "flower_pot" => bounds([0.3125, 0.0, 0.3125], [0.6875, 0.375, 0.6875]),
        "sniffer_egg" => bounds([0.0625, 0.0, 0.125], [0.9375, 1.0, 0.875]),
        "waterlily" => bounds([0.0625, 0.0, 0.0625], [0.9375, 0.09375, 0.9375]),
        "heavy_core" => bounds([0.25, 0.0, 0.25], [0.75, 0.5, 0.75]),
        "end_portal" => bounds([0.0; 3], [1.0, 0.25, 1.0]),
        "enchanting_table" => bounds([0.0; 3], [1.0, 0.75, 1.0]),
        "daylight_detector" | "daylight_detector_inverted" => bounds([0.0; 3], [1.0, 0.375, 1.0]),
        "decorated_pot" => bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375]),
        "dried_ghast" => bounds([0.1875, 0.0, 0.1875], [0.8125, 0.625, 0.8125]),
        "sculk_sensor" | "calibrated_sculk_sensor" | "sculk_shrieker" => {
            bounds([0.0; 3], [1.0, 0.5, 1.0])
        }
        "campfire" | "soul_campfire" => bounds([0.0; 3], [1.0, 0.4375, 1.0]),
        "cake" => {
            let bites = State::read(record)?.number("bite_counter")? as f32;
            bounds(
                [(bites * 2.0 + 1.0) / 16.0, 0.0, 0.0625],
                [0.9375, 0.5, 0.9375],
            )
        }
        _ if record.model_family == ModelFamily::Carpet => bounds([0.0; 3], [1.0, 0.0625, 1.0]),
        _ if record.model_family == ModelFamily::Statue => {
            bounds([0.1875, 0.0, 0.1875], [0.8125, 0.875, 0.8125])
        }
        _ if name.ends_with("_repeater") || name.ends_with("_comparator") => {
            bounds([0.0; 3], [1.0, 0.125, 1.0])
        }
        _ if name == "candle_cake" || name.ends_with("_candle_cake") => {
            bounds([0.0625, 0.0, 0.0625], [0.9375, 0.5, 0.9375])
        }
        _ if name == "candle" || name.ends_with("_candle") => {
            let (min, max) = match State::read(record)?.number("candles")? {
                0 => ([0.4375, 0.0, 0.4375], [0.5625, 0.375, 0.5625]),
                1 => ([0.3125, 0.0, 0.4375], [0.6875, 0.375, 0.625]),
                2 => ([0.3125, 0.0, 0.375], [0.625, 0.375, 0.6875]),
                3 => ([0.3125, 0.0, 0.3125], [0.6875, 0.375, 0.625]),
                _ => return None,
            };
            bounds(min, max)
        }
        _ => return None,
    })
}

/// Compiles bell attachment bounds in the four horizontal directions.
fn bell(record: &RegistryRecord) -> Option<ShapeBox> {
    let state = State::read(record)?;
    let direction = state.direction()?;
    let (min, max) = match state.text("attachment")? {
        "standing" if direction % 2 == 0 => ([0.0, 0.0, 0.25], [1.0, 0.8125, 0.75]),
        "standing" => ([0.25, 0.0, 0.0], [0.75, 0.8125, 1.0]),
        "hanging" => ([0.25; 3], [0.75, 1.0, 0.75]),
        "multiple" if direction % 2 == 0 => ([0.25, 0.25, 0.0], [0.75, 1.0, 1.0]),
        "multiple" => ([0.0, 0.25, 0.25], [1.0, 1.0, 0.75]),
        "side" => match direction {
            0 => ([0.25, 0.25, 0.0], [0.75, 0.9375, 0.8125]),
            1 => ([0.1875, 0.25, 0.25], [1.0, 0.9375, 0.75]),
            2 => ([0.25, 0.25, 0.1875], [0.75, 0.9375, 1.0]),
            3 => ([0.0, 0.25, 0.25], [0.8125, 0.9375, 0.75]),
            _ => return None,
        },
        _ => return None,
    };
    Some(bounds(min, max))
}

/// Compiles age-sized cocoa pods against their horizontal supporting face.
fn cocoa(record: &RegistryRecord) -> Option<ShapeBox> {
    let state = State::read(record)?;
    let age = state.number("age")? as f32;
    let width = (age * 2.0 + 4.0) / 16.0;
    let bottom = 0.75 - (age * 2.0 + 5.0) / 16.0;
    let inset = 0.5 - width * 0.5;
    let edge = 0.5 + width * 0.5;
    let (min, max) = match state.direction()? {
        0 => ([inset, bottom, 0.9375 - width], [edge, 0.75, 0.9375]),
        1 => ([0.0625, bottom, inset], [0.0625 + width, 0.75, edge]),
        2 => ([inset, bottom, 0.0625], [edge, 0.75, 0.0625 + width]),
        3 => ([0.9375 - width, bottom, inset], [0.9375, 0.75, edge]),
        _ => return None,
    };
    Some(bounds(min, max))
}
