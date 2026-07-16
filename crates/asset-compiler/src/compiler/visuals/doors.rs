use super::super::*;
use super::context::{
    CuboidTemplateKey, ModelStorage, RuleInputs, diagnostic_visual, intern_cuboid_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<CuboidTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_door(record) && !is_trapdoor(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if is_door(record) {
        const UPPER: u32 = 1 << 7;
        if let (Some(orientation @ 0..=3), Some(open @ 0..=1), Some(hinge @ 0..=1), Some(flags)) = (
            record.model_state.get(ModelStateField::Orientation),
            record.model_state.get(ModelStateField::Open),
            record.model_state.get(ModelStateField::Hinge),
            record.model_state.get(ModelStateField::Flags),
        ) && flags & !UPPER == 0
        {
            let face = if flags & UPPER == 0 {
                BlockFace::Down
            } else {
                BlockFace::South
            };
            if let Some(material) = inputs.material(record, face) {
                let materials = [material; 6];
                let (min, max) = door_bounds(orientation, open, hinge);
                let template = intern_cuboid_template(
                    materials,
                    min,
                    max,
                    templates,
                    storage.templates,
                    storage.quads,
                )?;
                set_model_visual(&mut visual, materials, template);
            }
        }
    } else if let Some(materials) = inputs.materials(record)
        && let (Some(orientation @ 0..=3), Some(open @ 0..=1), Some(half @ 0..=1)) = (
            record.model_state.get(ModelStateField::Orientation),
            record.model_state.get(ModelStateField::Open),
            record.model_state.get(ModelStateField::Half),
        )
    {
        let (min, max) = trapdoor_bounds(orientation, open, half);
        let template = intern_cuboid_template(
            materials,
            min,
            max,
            templates,
            storage.templates,
            storage.quads,
        )?;
        set_model_visual(&mut visual, materials, template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

pub(in crate::compiler) fn door_bounds(
    orientation: u32,
    open: u32,
    hinge: u32,
) -> ([i16; 3], [i16; 3]) {
    const THICKNESS: i16 = 3 * 16;
    const HIGH: i16 = 256 - THICKNESS;
    // Dragonfly writes `Door.Facing.RotateRight()` into the Bedrock cardinal
    // state. Decode that stored orientation back to the logical closed facing
    // before applying model.Door's open/hinge rotations.
    const NORTH: u32 = 0;
    const SOUTH: u32 = 1;
    const WEST: u32 = 2;
    const EAST: u32 = 3;
    let facing = match orientation {
        0 => EAST,  // encoded south
        1 => SOUTH, // encoded west
        2 => WEST,  // encoded north
        3 => NORTH, // encoded east
        _ => unreachable!("door selectors are checked before geometry generation"),
    };
    let rotate_right = |facing| match facing {
        NORTH => EAST,
        EAST => SOUTH,
        SOUTH => WEST,
        WEST => NORTH,
        _ => unreachable!(),
    };
    let rotate_left = |facing| match facing {
        NORTH => WEST,
        WEST => SOUTH,
        SOUTH => EAST,
        EAST => NORTH,
        _ => unreachable!(),
    };
    let effective = match (open, hinge) {
        (0, 0 | 1) => facing,
        (1, 0) => rotate_right(facing),
        (1, 1) => rotate_left(facing),
        _ => unreachable!("door selectors are checked before geometry generation"),
    };
    match effective {
        NORTH => ([0, 0, HIGH], [256, 256, 256]),
        SOUTH => ([0, 0, 0], [256, 256, THICKNESS]),
        WEST => ([HIGH, 0, 0], [256, 256, 256]),
        EAST => ([0, 0, 0], [THICKNESS, 256, 256]),
        _ => unreachable!(),
    }
}

pub(in crate::compiler) fn trapdoor_bounds(
    orientation: u32,
    open: u32,
    half: u32,
) -> ([i16; 3], [i16; 3]) {
    const THICKNESS: i16 = 3 * 16;
    const HIGH: i16 = 256 - THICKNESS;
    match (open, orientation, half) {
        (0, _, 0) => ([0, 0, 0], [256, THICKNESS, 256]),
        (0, _, 1) => ([0, HIGH, 0], [256, 256, 256]),
        (1, 0, _) => ([0, 0, 0], [THICKNESS, 256, 256]),
        (1, 1, _) => ([HIGH, 0, 0], [256, 256, 256]),
        (1, 2, _) => ([0, 0, 0], [256, 256, THICKNESS]),
        (1, 3, _) => ([0, 0, HIGH], [256, 256, 256]),
        _ => unreachable!("trapdoor selectors are checked before geometry generation"),
    }
}
