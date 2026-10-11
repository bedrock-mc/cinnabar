use assets::RegistryRecord;
use assets::block_shapes::ShapeBox;

/// Compiles a full-width slab and its step without artificial half-cell seams.
pub(super) fn selection(record: &RegistryRecord) -> Option<Box<[ShapeBox]>> {
    if record.model_family != assets::ModelFamily::Stair {
        return None;
    }
    let state = super::state::State::read(record)?;
    let direction = state.number("weirdo_direction")?;
    let upper = state.bit("upside_down_bit")?;
    let (slab_min, slab_max, step_min, step_max) = if upper {
        (0.5, 1.0, 0.0, 0.5)
    } else {
        (0.0, 0.5, 0.5, 1.0)
    };
    let mut min = [0.0, step_min, 0.0];
    let mut max = [1.0, step_max, 1.0];
    match direction {
        0 => min[0] = 0.5,
        1 => max[0] = 0.5,
        2 => min[2] = 0.5,
        3 => max[2] = 0.5,
        _ => return None,
    }
    Some(Box::new([
        ShapeBox::new([0.0, slab_min, 0.0], [1.0, slab_max, 1.0]),
        ShapeBox::new(min, max),
    ]))
}

/// Builds native stair pieces from the registry's direction, half and corner state.
pub(super) fn shapes(record: &RegistryRecord) -> Option<Vec<ShapeBox>> {
    if !record.name.ends_with("_stairs") {
        return None;
    }
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
    let value = |name: &str| state.get(name)?.get("value");
    let turns = assets::StairDirection::from_raw(
        u32::try_from(value("weirdo_direction")?.as_u64()?).ok()?,
    )?
    .turns_from_east();
    let upside_down = value("upside_down_bit")?.as_i64()? != 0;
    let corner = value("minecraft:corner")?.as_str()?;
    let side = match corner {
        "inner_left" | "outer_left" => (0.0, 0.5),
        "inner_right" | "outer_right" => (0.5, 1.0),
        "none" => (0.0, 1.0),
        _ => return None,
    };
    // Vanilla emits slab, step, then the optional inner piece.
    let mut boxes = vec![ShapeBox::new([0.0; 3], [1.0, 0.5, 1.0])];
    let (z_min, z_max) = if corner.starts_with("outer_") {
        side
    } else {
        (0.0, 1.0)
    };
    boxes.push(ShapeBox::new([0.5, 0.5, z_min], [1.0, 1.0, z_max]));
    if corner.starts_with("inner_") {
        boxes.push(ShapeBox::new([0.0, 0.5, side.0], [0.5, 1.0, side.1]));
    }
    for shape in &mut boxes {
        for _ in 0..turns {
            *shape = ShapeBox::new(
                [1.0 - shape.max[2], shape.min[1], shape.min[0]],
                [1.0 - shape.min[2], shape.max[1], shape.max[0]],
            );
        }
        if upside_down {
            (shape.min[1], shape.max[1]) = (1.0 - shape.max[1], 1.0 - shape.min[1]);
        }
    }
    Some(boxes)
}
