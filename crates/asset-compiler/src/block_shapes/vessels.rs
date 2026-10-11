use super::state::State;
use assets::{RegistryRecord, block_shapes::ShapeBox};

/// Compiles the separate floor and rim pieces used by vessel clipping.
pub(super) fn selection(record: &RegistryRecord) -> Option<Box<[ShapeBox]>> {
    let height = match record.name.as_ref() {
        "minecraft:cauldron" => 0.3125,
        "minecraft:composter" => match State::read(record)?.number("composter_fill_level")? {
            0 => 0.125,
            8 => 0.9375,
            level => (level as f64 * 2.0 + 1.0) / 16.0,
        },
        _ => return None,
    };
    Some(Box::new([
        ShapeBox::new([0.0; 3], [1.0, height, 1.0]),
        ShapeBox::new([0.0; 3], [0.125, 1.0, 1.0]),
        ShapeBox::new([0.0; 3], [1.0, 1.0, 0.125]),
        ShapeBox::new([0.875, 0.0, 0.0], [1.0; 3]),
        ShapeBox::new([0.0, 0.0, 0.875], [1.0; 3]),
    ]))
}
