//! Collision, selection, friction and transformation components of a custom block.

use super::*;

pub(super) fn collision_enabled(component: Option<&Nbt>) -> bool {
    match component {
        Some(Nbt::Byte(enabled)) => *enabled != 0,
        Some(compound @ Nbt::Compound(_)) if compound.field("boxes").is_some() => {
            matches!(compound.field("enabled"), Some(Nbt::Byte(enabled)) if *enabled != 0)
        }
        Some(compound @ Nbt::Compound(_)) => {
            !matches!(compound.field("enabled"), Some(Nbt::Byte(0)))
        }
        _ => true,
    }
}

pub(super) fn selection_component(component: Option<&Nbt>) -> CustomSelection {
    match component {
        Some(Nbt::Byte(0)) => CustomSelection::Disabled,
        Some(Nbt::Byte(_)) => CustomSelection::Box(CustomBox::FULL),
        Some(compound @ Nbt::Compound(_))
            if matches!(compound.field("enabled"), Some(Nbt::Byte(0))) =>
        {
            CustomSelection::Disabled
        }
        Some(compound @ Nbt::Compound(_)) => box_component(compound)
            .or_else(|| {
                matches!(compound.field("enabled"), Some(Nbt::Byte(_))).then_some(CustomBox::FULL)
            })
            .map_or(CustomSelection::Default, CustomSelection::Box),
        _ => CustomSelection::Default,
    }
}

/// Retains physical overrides and reports total omissions and mining omissions separately.
pub(super) fn physical_components(
    components: Option<&Nbt>,
) -> (CustomPhysicalComponents, usize, usize) {
    let (mining, mining_skips) =
        mining::component(components.and_then(|set| set.field("minecraft:destructible_by_mining")));
    let collision = components.and_then(|set| set.field("minecraft:collision_box"));
    let (boxes, collision_skips) = collision_components(collision);
    let (friction, friction_skips) =
        friction_component(components.and_then(|set| set.field("minecraft:friction")));
    (
        CustomPhysicalComponents {
            mining,
            random_offset: components
                .and_then(|set| set.field("minecraft:random_offset"))
                .and_then(random_offset_component),
            collision: collision.map(|component| CustomCollision {
                enabled: collision_enabled(Some(component)),
                boxes,
            }),
            selection: components
                .and_then(|set| set.field("minecraft:selection_box"))
                .map(|component| selection_component(Some(component))),
            transformation: transformation_component(components),
            friction,
        },
        mining_skips
            + collision_skips
            + friction_skips
            + usize::from(
                components
                    .and_then(|set| set.field("minecraft:random_offset"))
                    .is_some_and(|value| random_offset_component(value).is_none()),
            )
            + usize::from(
                components
                    .and_then(|set| set.field("minecraft:transformation"))
                    .is_some_and(|_| transformation_component(components).is_none()),
            ),
        mining_skips,
    )
}

/// Reads the network `value`, which defaults to zero when absent or mistyped; a value that
/// leaves no positive friction is counted and ignored.
pub(super) fn friction_component(component: Option<&Nbt>) -> (Option<f32>, usize) {
    match component {
        None => (None, 0),
        Some(compound @ Nbt::Compound(_)) => {
            let value = match compound.field("value") {
                Some(Nbt::Float(value)) => *value as f32,
                _ => 0.0,
            };
            if value.is_finite() && value < 1.0 {
                (Some(value), 0)
            } else {
                (None, 1)
            }
        }
        Some(_) => (None, 1),
    }
}

/// Decodes authored pixel ranges once, keeping absence distinct from an explicit zero component.
pub(super) fn random_offset_component(
    value: &Nbt,
) -> Option<block_transform::random_offset::RandomOffsetComponent> {
    use block_transform::random_offset::{RandomOffsetAxis, RandomOffsetComponent};
    if !matches!(value, Nbt::Compound(_)) {
        return None;
    }
    let mut result = RandomOffsetComponent::default();
    for (index, key) in ["x", "y", "z"].into_iter().enumerate() {
        let Some(axis) = value.field(key) else {
            continue;
        };
        if !matches!(axis, Nbt::Compound(_)) {
            return None;
        }
        let range = match axis.field("range") {
            None => [0.0; 2],
            Some(range @ Nbt::Compound(_)) => {
                let read = |key| range.field(key).map_or(Some(0.0), Nbt::number);
                [read("min")? as f32, read("max")? as f32]
            }
            Some(_) => return None,
        };
        let steps = axis.field("steps").map_or(Some(0.0), Nbt::number)?;
        if !steps.is_finite() || steps < 0.0 || steps > f64::from(u32::MAX) || steps.fract() != 0.0
        {
            return None;
        }
        result.axes[index] = RandomOffsetAxis::from_pixels(range, steps as u32);
    }
    result.is_valid().then_some(result)
}

pub(super) fn collision_components(component: Option<&Nbt>) -> (Option<Arc<[CustomBox]>>, usize) {
    let Some(component) = component else {
        return (None, 0);
    };
    match component.field("boxes") {
        Some(Nbt::List(entries)) if !entries.is_empty() => {
            let mut skipped = entries.len().saturating_sub(MAX_COLLISION_BOXES);
            let boxes = entries
                .iter()
                .take(MAX_COLLISION_BOXES)
                .filter_map(|entry| {
                    let shape = native_collision_box(entry);
                    skipped += usize::from(shape.is_none());
                    shape
                })
                .collect::<Arc<[CustomBox]>>();
            ((!boxes.is_empty()).then_some(boxes), skipped)
        }
        Some(Nbt::List(_)) => (Some(Arc::from([CustomBox::FULL])), 0),
        Some(_) => (None, 1),
        None => (
            origin_size_box(component, 1.5)
                .or_else(|| {
                    (matches!(component, Nbt::Byte(_))
                        || matches!(component.field("enabled"), Some(Nbt::Byte(_))))
                    .then_some(CustomBox::FULL)
                })
                .map(|shape| Arc::from([shape])),
            0,
        ),
    }
}

/// Wire bounds use corner coordinates in sixteenths; X faces opposite world X.
fn native_collision_box(entry: &Nbt) -> Option<CustomBox> {
    if !matches!(entry, Nbt::Compound(_)) {
        return None;
    }
    let mut coordinates = [0.0_f32; 6];
    for (coordinate, name) in coordinates
        .iter_mut()
        .zip(["minX", "minY", "minZ", "maxX", "maxY", "maxZ"])
    {
        *coordinate = match entry.field(name) {
            Some(Nbt::Float(value)) => *value as f32,
            _ => 0.0,
        };
        if !coordinate.is_finite() {
            return None;
        }
    }
    let mut min = [0.0; 3];
    let mut max = [0.0; 3];
    for axis in 0..3 {
        let ceiling = if axis == 1 { 24.0 } else { 16.0 };
        let first = coordinates[axis].clamp(0.0, ceiling) / 16.0;
        let second = coordinates[axis + 3].clamp(0.0, ceiling) / 16.0;
        min[axis] = first.min(second);
        max[axis] = first.max(second);
    }
    (min[0], max[0]) = (1.0 - max[0], 1.0 - min[0]);
    Some(CustomBox { min, max })
}

/// Reads a `{origin, size}` box given in sixteenths from the block's bottom
/// centre, clamped to the block; empty bounds remain explicit.
pub(super) fn box_component(component: &Nbt) -> Option<CustomBox> {
    origin_size_box(component, 1.0)
}

/// Normalizes finite pixel endpoints, preserving zero volume and reflecting authored X.
fn origin_size_box(component: &Nbt, max_y: f32) -> Option<CustomBox> {
    let triple = |name: &str| -> Option<[f32; 3]> {
        let values = component.list(name);
        if values.len() != 3 {
            return None;
        }
        let mut out = [0.0_f32; 3];
        for (slot, value) in out.iter_mut().zip(values) {
            let number = value.number()?;
            let narrowed = number as f32;
            if !narrowed.is_finite() {
                return None;
            }
            *slot = narrowed;
        }
        Some(out)
    };
    let (origin, size) = (triple("origin")?, triple("size")?);
    let shift = [8.0, 0.0, 8.0];
    let mut min = [0.0_f32; 3];
    let mut max = [0.0_f32; 3];
    for axis in 0..3 {
        let low = origin[axis] + shift[axis];
        let high = low + size[axis];
        if !low.is_finite() || !high.is_finite() {
            return None;
        }
        let ceiling = if axis == 1 { max_y } else { 1.0 };
        let first = (low / 16.0).clamp(0.0, ceiling);
        let second = (high / 16.0).clamp(0.0, ceiling);
        min[axis] = first.min(second);
        max[axis] = first.max(second);
    }
    (min[0], max[0]) = (1.0 - max[0], 1.0 - min[0]);
    Some(CustomBox { min, max })
}

/// Reads `minecraft:transformation`: quarter turns about each axis, a scale and a
/// translation in block units, all applied about the block centre.
pub(super) fn transformation_component(components: Option<&Nbt>) -> Option<CustomTransformation> {
    let transform = components?.field("minecraft:transformation")?;
    let number = |key: &str, default: f64| {
        transform
            .field(key)
            .and_then(Nbt::number)
            .unwrap_or(default)
    };
    let quarter = |key: &str| (number(key, 0.0).round() as i64).rem_euclid(4) as i32;
    let transformation = CustomTransformation {
        rotation: [quarter("RX"), quarter("RY"), quarter("RZ")],
        scale: [
            number("SX", 1.0) as f32,
            number("SY", 1.0) as f32,
            number("SZ", 1.0) as f32,
        ],
        translation: [
            number("TX", 0.0) as f32,
            number("TY", 0.0) as f32,
            number("TZ", 0.0) as f32,
        ],
    };
    transformation
        .scale
        .iter()
        .chain(&transformation.translation)
        .all(|value| value.is_finite())
        .then_some(transformation)
}
