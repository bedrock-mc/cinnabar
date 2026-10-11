use super::state::State;
use assets::{RegistryRecord, block_shapes::ShapeBox};

/// Compiles the hopper's three clipping pieces rather than its hollow movement bowl.
pub(super) fn selection(record: &RegistryRecord) -> Option<Box<[ShapeBox]>> {
    if record.name.as_ref() != "minecraft:hopper" {
        return None;
    }
    let (min, max) = match State::read(record)?.number("facing_direction")? {
        2 => ([0.375, 0.25, 0.0], [0.625, 0.5, 0.25]),
        3 => ([0.375, 0.25, 0.75], [0.625, 0.5, 1.0]),
        4 => ([0.0, 0.25, 0.375], [0.25, 0.5, 0.625]),
        5 => ([0.75, 0.25, 0.375], [1.0, 0.5, 0.625]),
        0 | 1 => ([0.375, 0.0, 0.375], [0.625, 0.25, 0.625]),
        _ => return None,
    };
    Some(Box::new([
        ShapeBox::new([0.0, 0.625, 0.0], [1.0; 3]),
        ShapeBox::new([0.25; 3], [0.75, 0.625, 0.75]),
        ShapeBox::new(min, max),
    ]))
}
