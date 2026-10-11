//! End rods map a separate shaft and mounting plate onto one sparse sprite.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Identifies the terrain-owned rod family before generic fallback selection.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:end_rod"
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Accepts only the six integer-facing states supported by the rod model.
fn facing(record: &RegistryRecord) -> Option<usize> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 1 {
        return None;
    }
    super::state::exact_tagged_int(state.get("facing_direction")?, 5).map(|v| v as usize)
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

// Signed coordinate axes for each face's U/V projection, in BlockFace order.
// A negative axis reverses the coordinate across that cuboid's bounds.
const SHAFT_UV_AXES: [[[i8; 2]; 6]; 6] = [
    [[-3, 2], [3, 2], [1, 3], [1, 3], [1, 2], [-1, 2]],
    [[3, -2], [-3, -2], [1, 3], [1, 3], [-1, -2], [1, -2]],
    [[-2, -3], [-2, -3], [-1, -3], [-1, -3], [2, -1], [1, 2]],
    [[2, 3], [2, 3], [1, 3], [1, 3], [-2, 1], [-1, -2]],
    [[2, 3], [2, 3], [3, 1], [3, 1], [2, -1], [2, 1]],
    [[-2, 3], [-2, 3], [3, -1], [3, -1], [-2, 1], [-2, -1]],
];
const BASE_UV_AXES: [[[i8; 2]; 6]; 6] = [
    [[3, -2], [-3, -2], [-1, -3], [-1, -3], [-1, -2], [1, -2]],
    [[3, -2], [-3, -2], [1, 3], [1, 3], [-1, -2], [1, -2]],
    [[-2, -3], [-2, -3], [-1, -3], [-1, -3], [2, -1], [1, 2]],
    [[2, 3], [2, 3], [1, 3], [1, 3], [-2, 1], [-1, -2]],
    [[-2, 3], [-2, 3], [-1, 3], [-1, 3], [2, -1], [2, 1]],
    [[2, 3], [2, 3], [1, 3], [1, 3], [-2, 1], [-2, -1]],
];

/// Emits both closed cuboids without full-face neighbour occlusion boundaries.
fn rod_quads(material: u32, direction: usize) -> Vec<ModelQuad> {
    let axis = [1, 1, 2, 2, 0, 0][direction];
    let base_at_high_end = [true, false, false, true, false, true][direction];
    let mut quads = Vec::with_capacity(12);
    for base in [false, true] {
        let (mut min, mut max) = if base {
            ([96; 3], [160; 3])
        } else {
            ([112; 3], [144; 3])
        };
        let (low, high) = match (base, base_at_high_end) {
            (false, false) => (16, 256),
            (false, true) => (0, 240),
            (true, false) => (0, 16),
            (true, true) => (240, 256),
        };
        min[axis] = low;
        max[axis] = high;
        let projections = if base {
            BASE_UV_AXES[direction]
        } else {
            SHAFT_UV_AXES[direction]
        };
        for (face, mut quad) in
            BlockFace::ALL
                .into_iter()
                .zip(cuboid_quads([material; 6], min, max))
        {
            let face_axis = match face {
                BlockFace::West | BlockFace::East => 0,
                BlockFace::Down | BlockFace::Up => 1,
                BlockFace::North | BlockFace::South => 2,
            };
            let (uv_min, uv_max) = match (base, face_axis == axis) {
                (false, false) => ([0, 0], [2, 15]),
                (false, true) => ([2, 0], [4, 2]),
                (true, false) => ([2, 6], [6, 7]),
                (true, true) => ([2, 2], [6, 6]),
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
                    (uv_min[component] * 256
                        + i32::from(offset) * (uv_max[component] - uv_min[component]) * 256
                            / i32::from(max[coordinate] - min[coordinate]))
                        as u16
                })
            });
            quads.push(quad);
        }
    }
    quads
}
