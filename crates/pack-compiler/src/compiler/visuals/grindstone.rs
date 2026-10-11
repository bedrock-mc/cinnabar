//! State-selected grindstone supports, pivots and wheel with independent sprite regions.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Identifies the terrain-owned model before the provisional envelope route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:grindstone"
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Admits typed mounting selectors, projecting the legacy multiple state to standing.
fn pose(record: &RegistryRecord) -> Option<(u8, u8)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    let attachment = match super::state::exact_tagged_string(state.get("attachment")?)? {
        "standing" | "multiple" => 0,
        "hanging" => 1,
        "side" => 2,
        _ => return None,
    };
    let direction = super::state::exact_tagged_int(state.get("direction")?, 3)? as u8;
    Some((attachment, direction))
}

/// Compiles every mounting pose while retaining pack-resolved material ownership.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], u8, u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((attachment, direction)) = pose(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, attachment, direction);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            quads(materials, attachment, direction),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Maps an oriented face onto an explicit pixel rectangle, with local UV rotation.
fn sprite_region(quad: &mut ModelQuad, rect: [u16; 4], mut rotation: u8, face: BlockFace) {
    if matches!(face, BlockFace::North | BlockFace::East) {
        rotation = match rotation {
            1 => 2,
            2 => 1,
            other => other,
        };
    }
    let min: [u16; 2] =
        std::array::from_fn(|axis| quad.uvs.iter().map(|uv| uv[axis]).min().unwrap());
    let max: [u16; 2] =
        std::array::from_fn(|axis| quad.uvs.iter().map(|uv| uv[axis]).max().unwrap());
    for uv in &mut quad.uvs {
        let [u, v] =
            std::array::from_fn(|axis| u16::from(uv[axis] == max[axis] && max[axis] != min[axis]));
        let local = match rotation {
            1 => [v, 1 - u],
            2 => [1 - v, u],
            3 => [1 - u, 1 - v],
            _ => [u, v],
        };
        *uv = std::array::from_fn(|axis| rect[axis + usize::from(local[axis]) * 2] * 256);
    }
}

/// Emits five closed boxes with direction-selected bounds and authored face slots.
fn quads(materials: [u32; 6], attachment: u8, direction: u8) -> Vec<ModelQuad> {
    let odd = direction & 1 != 0;
    let high = attachment == 1 || (attachment == 2 && matches!(direction, 1 | 2));
    let (leg_y, pivot_y, wheel_y) = if high {
        ([9, 16], [3, 9], [0, 12])
    } else {
        ([0, 7], [7, 13], [4, 16])
    };
    let boxes = [
        ([12, leg_y[0], 6], [14, leg_y[1], 10], 0),
        ([2, leg_y[0], 6], [4, leg_y[1], 10], 0),
        ([12, pivot_y[0], 5], [14, pivot_y[1], 11], 1),
        ([2, pivot_y[0], 5], [4, pivot_y[1], 11], 1),
        ([4, wheel_y[0], 2], [12, wheel_y[1], 14], 2),
    ];
    let mut result = Vec::with_capacity(30);
    for (mut min, mut max, part) in boxes {
        if odd {
            min.swap(0, 2);
            max.swap(0, 2);
        }
        if attachment == 2 {
            let axis = if odd { 0 } else { 2 };
            min.swap(1, axis);
            max.swap(1, axis);
        }
        let mut regions = match part {
            0 => [
                [6, 9, 10, 16],
                [10, 16, 6, 9],
                [12, 6, 14, 10],
                [12, 6, 14, 10],
                [2, 9, 4, 16],
                [10, 16, 6, 9],
            ],
            1 => [
                [0, 0, 6, 6],
                [0, 0, 6, 6],
                [8, 0, 10, 6],
                [8, 0, 10, 6],
                [6, 0, 8, 6],
                [6, 0, 8, 6],
            ],
            _ => [
                [0, 0, 12, 12],
                [0, 0, 12, 12],
                [0, 0, 8, 12],
                [0, 0, 8, 12],
                [0, 0, 8, 12],
                [0, 0, 8, 12],
            ],
        };
        let mut rotations = [0; 6];
        if odd {
            regions.swap(0, 4);
            regions.swap(1, 5);
            if part == 0 {
                regions.swap(0, 1);
            } else {
                rotations[2] = 1;
                rotations[3] = 1;
            }
        }
        if part == 0 && attachment == 2 {
            regions = if odd {
                [
                    [12, 6, 14, 10],
                    [12, 6, 14, 10],
                    [2, 9, 4, 16],
                    [10, 16, 6, 9],
                    [6, 9, 10, 16],
                    [10, 16, 6, 9],
                ]
            } else {
                [
                    [6, 9, 10, 16],
                    [10, 16, 6, 9],
                    [2, 9, 4, 16],
                    [10, 16, 6, 9],
                    [12, 6, 14, 10],
                    [12, 6, 14, 10],
                ]
            };
            rotations = match direction {
                0 => [1, 1, 0, 0, 0, 0],
                1 => [0, 0, 1, 1, 1, 1],
                2 => [1, 1, 3, 3, 0, 0],
                _ => [0, 0, 2, 2, 1, 1],
            };
        }
        let part_material = match part {
            0 => BlockFace::Down,
            1 => BlockFace::North,
            _ => BlockFace::Up,
        };
        let mut faces = [materials[part_material as usize]; 6];
        if part == 2 {
            for face in if odd {
                [BlockFace::North, BlockFace::South]
            } else {
                [BlockFace::West, BlockFace::East]
            } {
                faces[face as usize] = materials[BlockFace::South as usize];
            }
        }
        for (((mut quad, rect), rotation), face) in
            super::geometry::vanilla_cuboid_quads(faces, min.map(|p| p * 16), max.map(|p| p * 16))
                .into_iter()
                .zip(regions)
                .zip(rotations)
                .zip(BlockFace::ALL)
        {
            sprite_region(&mut quad, rect, rotation, face);
            result.push(quad);
        }
    }
    result
}
