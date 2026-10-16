use super::{UNIT, selection::bounds, state::State};
use assets::facts::BlockProperty;
use assets::{
    RegistryRecord,
    block_shapes::{OutlineContext, ShapeBox},
};
use assets::{facts::shape::ShapeTraits, registry::BlockTraits};

/// Compiles outline overrides and the two boxes used by continued vine outlines.
pub(super) fn compile(
    record: &RegistryRecord,
    shapes: ShapeTraits,
) -> (Option<ShapeBox>, Option<ShapeBox>, OutlineContext) {
    if record.model_family == assets::ModelFamily::Stair {
        return (Some(UNIT), None, OutlineContext::Static);
    }
    match () {
        _ if shapes.contains(ShapeTraits::CHORUS_PLANT) => {
            return (
                Some(bounds([0.125; 3], [0.875; 3])),
                None,
                OutlineContext::Chorus,
            );
        }
        _ if shapes.contains(ShapeTraits::TWISTING_VINES) => {
            return (
                Some(bounds([0.25, 0.0, 0.25], [0.75, 0.9375, 0.75])),
                Some(bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375])),
                OutlineContext::TwistingVines,
            );
        }
        _ if shapes.contains(ShapeTraits::WEEPING_VINES) => {
            return (
                Some(bounds([0.25, 0.5625, 0.25], [0.75, 1.0, 0.75])),
                Some(bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375])),
                OutlineContext::WeepingVines,
            );
        }
        _ => {}
    }
    (static_outline(record, shapes), None, OutlineContext::Static)
}

/// Resolves a state-only outline independently of its interaction clipping shape.
fn static_outline(record: &RegistryRecord, shapes: ShapeTraits) -> Option<ShapeBox> {
    Some(match () {
        _ if shapes.contains(ShapeTraits::CAVE_VINES) => {
            bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375])
        }
        _ if shapes.intersects(ShapeTraits::SMALL_DRIPLEAF | ShapeTraits::KELP)
            || (record.traits.contains(BlockTraits::PORTAL)
                && !record.traits.contains(BlockTraits::END_PORTAL))
            || shapes.contains(ShapeTraits::PISTON_ARM)
            || record.traits.contains(BlockTraits::HOPPER) =>
        {
            UNIT
        }
        _ if shapes.contains(ShapeTraits::WHEAT) => bounds(
            [0.0; 3],
            [
                1.0,
                (State::read(record)?
                    .number(BlockProperty::Growth)?
                    .clamp(0, 7)
                    + 1) as f32
                    / 7.0,
                1.0,
            ],
        ),
        _ if shapes.intersects(ShapeTraits::CARROTS | ShapeTraits::ROOT_CROP) => {
            let growth = State::read(record)?
                .number(BlockProperty::Growth)?
                .clamp(0, 7);
            let scale = if shapes.contains(ShapeTraits::CARROTS) {
                0.7_f32
            } else {
                0.6_f32
            };
            bounds(
                [0.0; 3],
                [1.0, (growth + 1) as f32 * (1.0_f32 / 7.0) * scale, 1.0],
            )
        }
        _ if shapes.contains(ShapeTraits::NETHER_WART) => bounds(
            [0.0; 3],
            [
                1.0,
                (State::read(record)?.number(BlockProperty::Age)? + 1) as f32 * 0.25,
                1.0,
            ],
        ),
        _ if shapes.contains(ShapeTraits::CORAL_WALL_FAN) => {
            let mut shape = super::plants::shape(record, shapes)?;
            let (axis, offset) = match State::read(record)?.number(BlockProperty::CoralDirection)? {
                0 => (0, 0.2_f64),
                1 => (0, -0.2_f64),
                2 => (2, 0.2_f64),
                3 => (2, -0.2_f64),
                _ => return None,
            };
            // Add in the native coordinate precision before widening to f64.
            shape.min[axis] = f64::from(shape.min[axis] as f32 + offset as f32);
            shape.max[axis] = f64::from(shape.max[axis] as f32 + offset as f32);
            shape
        }
        _ if shapes.contains(ShapeTraits::CACTUS) => {
            bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375])
        }
        _ if shapes.contains(ShapeTraits::BREWING_STAND) => {
            bounds([0.125, 0.0, 0.125], [0.875, 1.0, 0.875])
        }
        _ if shapes.contains(ShapeTraits::HANGING_MOSS) => bounds(
            [
                0.0,
                if State::read(record)?.bit(BlockProperty::Tip)? {
                    0.125
                } else {
                    0.0
                },
                0.0,
            ],
            [1.0; 3],
        ),
        _ if shapes.contains(ShapeTraits::ANVIL) => {
            if State::read(record)?.direction()? % 2 == 0 {
                bounds([0.125, 0.0, 0.0], [0.875, 1.0, 1.0])
            } else {
                bounds([0.0, 0.0, 0.125], [1.0, 1.0, 0.875])
            }
        }
        _ if record.model_family == assets::ModelFamily::Gate => gate_shape(record)?,
        _ => return None,
    })
}

/// Gate picking and outlines share a thin plane, including open and in-wall states.
pub(super) fn gate_shape(record: &RegistryRecord) -> Option<ShapeBox> {
    if record.model_family != assets::ModelFamily::Gate {
        return None;
    }
    Some(if State::read(record)?.direction()? % 2 == 0 {
        bounds([0.0, 0.0, 0.375], [1.0, 1.0, 0.625])
    } else {
        bounds([0.375, 0.0, 0.0], [0.625, 1.0, 1.0])
    })
}
