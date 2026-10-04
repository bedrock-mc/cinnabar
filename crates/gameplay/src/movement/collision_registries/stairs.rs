use assets::RegistryRecord;
use sim::{Aabb, Vec3};

/// Builds native stair pieces from the registry's direction, half and corner state.
pub(super) fn shapes(record: &RegistryRecord) -> Option<Vec<Aabb>> {
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
    let mut boxes = vec![Aabb::new(Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0))];
    let (z_min, z_max) = if corner.starts_with("outer_") {
        side
    } else {
        (0.0, 1.0)
    };
    boxes.push(Aabb::new(
        Vec3::new(0.5, 0.5, z_min),
        Vec3::new(1.0, 1.0, z_max),
    ));
    if corner.starts_with("inner_") {
        boxes.push(Aabb::new(
            Vec3::new(0.0, 0.5, side.0),
            Vec3::new(0.5, 1.0, side.1),
        ));
    }
    for shape in &mut boxes {
        for _ in 0..turns {
            *shape = Aabb::new(
                Vec3::new(1.0 - shape.max.z, shape.min.y, shape.min.x),
                Vec3::new(1.0 - shape.min.z, shape.max.y, shape.max.x),
            );
        }
        if upside_down {
            (shape.min.y, shape.max.y) = (1.0 - shape.max.y, 1.0 - shape.min.y);
        }
    }
    Some(boxes)
}
