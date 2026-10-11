//! Lightning rods map their shaft and tip onto separate regions of one sprite.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Identifies the terrain-owned family before generic fallback selection.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    crate::lightning_rod::is_record(record)
}

/// Accepts the complete lightning-rod selector before geometry generation.
fn facing(record: &RegistryRecord) -> Option<usize> {
    crate::lightning_rod::state(record).map(|(direction, _)| direction)
}

/// Compiles complete rod geometry using the face-zero sprite, including pack overrides.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, usize), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let Some(direction) = facing(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let Some(material) = inputs.material(record, BlockFace::Down) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (material, direction);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            rod_quads(material, direction),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, [material; 6], template);
    Ok(CompileRuleResult::Compiled(visual))
}

const SHAFT_UV_AXES: [[[i8; 2]; 6]; 6] = [
    [[-3, 2], [3, 2], [3, 1], [-3, -1], [1, 2], [-1, 2]],
    [[3, -2], [-3, -2], [-3, 1], [3, 1], [-1, -2], [1, -2]],
    [[2, 3], [-2, 3], [-1, 3], [1, 3], [1, -2], [-2, 1]],
    [[-2, -3], [2, -3], [1, -3], [-1, -3], [1, -2], [-2, 1]],
    [[-3, -2], [-3, 2], [3, 1], [-3, 1], [-2, 1], [2, 1]],
    [[-3, -2], [-3, 2], [-3, -1], [3, -1], [2, -1], [-2, -1]],
];
const TIP_UV_AXES: [[[i8; 2]; 6]; 6] = [
    [[2, -3], [2, 3], [3, 1], [3, 1], [2, 1], [2, -1]],
    [[-2, 3], [-2, -3], [1, -3], [-3, -1], [-2, -1], [-2, 1]],
    [[-2, 3], [-2, -3], [-1, 3], [-3, -1], [-2, -1], [-2, 1]],
    [[-2, 3], [-2, -3], [1, -3], [3, 1], [-2, -1], [-2, 1]],
    [[-2, 3], [-2, -3], [3, 1], [-1, 3], [-2, -1], [-2, 1]],
    [[-2, 3], [-2, -3], [-3, -1], [1, -3], [-2, -1], [-2, 1]],
];

/// Emits a closed tip and shaft with the vertical internal junction face omitted.
fn rod_quads(material: u32, direction: usize) -> Vec<ModelQuad> {
    let axis = [1, 1, 2, 2, 0, 0][direction];
    let tip_at_high_end = direction % 2 == 1;
    let mut quads = Vec::with_capacity(12);
    for tip in [false, true] {
        let (mut min, mut max) = if tip {
            ([96; 3], [160; 3])
        } else {
            ([112; 3], [144; 3])
        };
        let (low, high) = match (tip, tip_at_high_end) {
            (false, false) => (64, 256),
            (false, true) => (0, 192),
            (true, false) => (0, 64),
            (true, true) => (192, 256),
        };
        min[axis] = low;
        max[axis] = high;
        let projections = if tip {
            TIP_UV_AXES[direction]
        } else {
            SHAFT_UV_AXES[direction]
        };
        for (face, mut quad) in
            BlockFace::ALL
                .into_iter()
                .zip(cuboid_quads([material; 6], min, max))
        {
            if !tip
                && ((direction == 0 && face == BlockFace::Down)
                    || (direction == 1 && face == BlockFace::Up))
            {
                continue;
            }
            let face_axis = match face {
                BlockFace::West | BlockFace::East => 0,
                BlockFace::Down | BlockFace::Up => 1,
                BlockFace::North | BlockFace::South => 2,
            };
            // Fixed-point sprite bounds retain the authored inset from transparent texels.
            let (uv_min, uv_max) = if tip {
                ([10, 10], [1014, 1014])
            } else if face_axis == axis {
                ([10, 10], [502, 502])
            } else {
                ([10, 1034], [502, 4086])
            };
            quad.uvs = quad.positions.map(|position| {
                std::array::from_fn(|component| {
                    let projection = projections[face as usize][component];
                    let coordinate = usize::from(projection.unsigned_abs() - 1);
                    let offset = if projection > 0 {
                        position[coordinate] - min[coordinate]
                    } else {
                        max[coordinate] - position[coordinate]
                    };
                    (uv_min[component]
                        + i32::from(offset) * (uv_max[component] - uv_min[component])
                            / i32::from(max[coordinate] - min[coordinate]))
                        as u16
                })
            });
            quads.push(quad);
        }
    }
    quads
}
