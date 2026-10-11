use super::{UNIT, state::State};
use assets::{RegistryRecord, block_shapes::ShapeBox};

/// Compiles independent clipping pieces for six-face attachment classes.
pub(super) fn multiface(record: &RegistryRecord) -> Option<Box<[ShapeBox]>> {
    if !matches!(
        record.name.as_ref(),
        "minecraft:glow_lichen" | "minecraft:sculk_vein" | "minecraft:resin_clump"
    ) {
        return None;
    }
    let bits = State::read(record)?.number("multi_face_direction_bits")?;
    if !(0..64).contains(&bits) {
        return None;
    }
    Some(
        (0..6)
            .filter(|face| bits & (1 << face) != 0)
            .map(|face| slab(face, 1.0 / 16.0))
            .collect(),
    )
}

/// Compiles the union of the enabled side planes for a nonzero vine state.
pub(super) fn vine(record: &RegistryRecord) -> Option<Box<[ShapeBox]>> {
    if record.name.as_ref() != "minecraft:vine" {
        return None;
    }
    let bits = State::read(record)?.number("vine_direction_bits")?;
    if !(1..16).contains(&bits) {
        return None;
    }
    let mut min = [1.0_f64; 3];
    let mut max = [0.0_f64; 3];
    for (bit, face) in [(1, 2), (2, 3), (4, 4), (8, 5)] {
        if bits & bit == 0 {
            continue;
        }
        let shape = slab(face, 1.0 / 16.0);
        for axis in 0..3 {
            min[axis] = min[axis].min(shape.min[axis]);
            max[axis] = max[axis].max(shape.max[axis]);
        }
    }
    Some(Box::new([ShapeBox::new(min, max)]))
}

/// Builds a slab in down, up, south, west, north, east order.
fn slab(face: u8, thickness: f64) -> ShapeBox {
    let mut shape = UNIT;
    let (axis, upper) = [
        (1, false),
        (1, true),
        (2, true),
        (0, false),
        (2, false),
        (0, true),
    ][usize::from(face)];
    if upper {
        shape.min[axis] = 1.0 - thickness;
    } else {
        shape.max[axis] = thickness;
    }
    shape
}

/// Compiles the pale moss floor and its tallest side into the visual box.
pub(super) fn pale_moss(record: &RegistryRecord) -> Option<Box<[ShapeBox]>> {
    if record.name.as_ref() != "minecraft:pale_moss_carpet" {
        return None;
    }
    let state = State::read(record)?;
    let mut height: f64 = if state.bit("upper_block_bit")? {
        0.0
    } else {
        1.0 / 16.0
    };
    for side in ["north", "east", "south", "west"] {
        let name = format!("pale_moss_carpet_side_{side}");
        height = height.max(match state.text(&name)? {
            "none" => 0.0,
            "short" => 0.625,
            "tall" => 1.0,
            _ => return None,
        });
    }
    Some(if height == 0.0 {
        Box::new([])
    } else {
        Box::new([ShapeBox::new([0.0; 3], [1.0, height, 1.0])])
    })
}
