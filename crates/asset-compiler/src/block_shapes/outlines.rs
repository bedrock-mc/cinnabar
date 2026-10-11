use super::{UNIT, selection::bounds, state::State};
use assets::{
    RegistryRecord,
    block_shapes::{OutlineContext, ShapeBox},
};

/// Compiles outline overrides and the two boxes used by continued vine outlines.
pub(super) fn compile(
    record: &RegistryRecord,
) -> (Option<ShapeBox>, Option<ShapeBox>, OutlineContext) {
    if record.model_family == assets::ModelFamily::Stair {
        return (Some(UNIT), None, OutlineContext::Static);
    }
    let Some(name) = record.name.strip_prefix("minecraft:") else {
        return (None, None, OutlineContext::Static);
    };
    match name {
        "chorus_plant" => {
            return (
                Some(bounds([0.125; 3], [0.875; 3])),
                None,
                OutlineContext::Chorus,
            );
        }
        "twisting_vines" => {
            return (
                Some(bounds([0.25, 0.0, 0.25], [0.75, 0.9375, 0.75])),
                Some(bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375])),
                OutlineContext::TwistingVines,
            );
        }
        "weeping_vines" => {
            return (
                Some(bounds([0.25, 0.5625, 0.25], [0.75, 1.0, 0.75])),
                Some(bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375])),
                OutlineContext::WeepingVines,
            );
        }
        _ => {}
    }
    (static_outline(name, record), None, OutlineContext::Static)
}

/// Resolves a state-only outline independently of its interaction clipping shape.
fn static_outline(name: &str, record: &RegistryRecord) -> Option<ShapeBox> {
    Some(match name {
        "cave_vines" | "cave_vines_head_with_berries" | "cave_vines_body_with_berries" => {
            bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375])
        }
        "small_dripleaf_block"
        | "kelp"
        | "portal"
        | "piston_arm_collision"
        | "sticky_piston_arm_collision"
        | "hopper" => UNIT,
        "wheat" => bounds(
            [0.0; 3],
            [
                1.0,
                (State::read(record)?.number("growth")?.clamp(0, 7) + 1) as f32 / 7.0,
                1.0,
            ],
        ),
        "carrots" | "potatoes" | "beetroot" => {
            let growth = State::read(record)?.number("growth")?.clamp(0, 7);
            let scale = if name == "carrots" { 0.7_f32 } else { 0.6_f32 };
            bounds(
                [0.0; 3],
                [1.0, (growth + 1) as f32 * (1.0_f32 / 7.0) * scale, 1.0],
            )
        }
        "nether_wart" => bounds(
            [0.0; 3],
            [
                1.0,
                (State::read(record)?.number("age")? + 1) as f32 * 0.25,
                1.0,
            ],
        ),
        _ if name.ends_with("_coral_wall_fan") => {
            let mut shape = super::plants::shape(record)?;
            let (axis, offset) = match State::read(record)?.number("coral_direction")? {
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
        "cactus" => bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375]),
        "brewing_stand" => bounds([0.125, 0.0, 0.125], [0.875, 1.0, 0.875]),
        "pale_hanging_moss" => bounds(
            [
                0.0,
                if State::read(record)?.bit("tip")? {
                    0.125
                } else {
                    0.0
                },
                0.0,
            ],
            [1.0; 3],
        ),
        "anvil" | "chipped_anvil" | "damaged_anvil" | "deprecated_anvil" => {
            if State::read(record)?.direction()? % 2 == 0 {
                bounds([0.125, 0.0, 0.0], [0.875, 1.0, 1.0])
            } else {
                bounds([0.0, 0.0, 0.125], [1.0, 1.0, 0.875])
            }
        }
        _ if name.ends_with("_fence_gate") || name == "fence_gate" => {
            if State::read(record)?.direction()? % 2 == 0 {
                bounds([0.0, 0.0, 0.375], [1.0, 1.0, 0.625])
            } else {
                bounds([0.375, 0.0, 0.0], [0.625, 1.0, 1.0])
            }
        }
        _ => return None,
    })
}
