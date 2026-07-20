use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 7], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_stair(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some(orientation @ 0..=3) = record.model_state.get(ModelStateField::Orientation)
        && let Some(upside @ 0..=1) = record.model_state.get(ModelStateField::Half)
    {
        let rotation = (orientation + 2) & 3;
        let canonical = canonical_stair_materials(materials, rotation);
        let key = [
            canonical[0],
            canonical[1],
            canonical[2],
            canonical[3],
            canonical[4],
            canonical[5],
            upside,
        ];
        let base = if let Some(&base) = templates.get(&key) {
            base
        } else {
            let base = u32::try_from(storage.templates.len()).map_err(|_| {
                AssetError::BlobSizeOverflow {
                    section: "model template",
                }
            })?;
            for shape in 0..5 {
                push_model_template(
                    stair_quads(canonical, 2, upside != 0, shape),
                    MODEL_TEMPLATE_FLAG_STAIR,
                    storage.templates,
                    storage.quads,
                )?;
            }
            templates.insert(key, base);
            base
        };
        set_model_visual(&mut visual, materials, base);
        visual.variant = rotation | (upside << 2);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

pub(in crate::compiler) fn stair_quads(
    materials: [u32; 6],
    orientation: u32,
    upside_down: bool,
    shape: u32,
) -> Vec<ModelQuad> {
    debug_assert!(orientation < 4 && shape < 5);
    let mut occupied = [false; 8];
    let base_y = usize::from(upside_down);
    let step_y = 1 - base_y;
    for x in 0..2 {
        for z in 0..2 {
            occupied[cell_index(x, base_y, z)] = true;
            let facing = toward(orientation, x, z);
            let right = toward((orientation + 1) & 3, x, z);
            let left = toward((orientation + 3) & 3, x, z);
            let opposite = toward((orientation + 2) & 3, x, z);
            let step = match shape {
                0 => facing,
                1 => facing || (opposite && right),
                2 => facing || (opposite && left),
                3 => facing && left,
                4 => facing && right,
                _ => false,
            };
            if step {
                occupied[cell_index(x, step_y, z)] = true;
            }
        }
    }
    let mut quads = Vec::with_capacity(32);
    for x in 0..2 {
        for y in 0..2 {
            for z in 0..2 {
                if !occupied[cell_index(x, y, z)] {
                    continue;
                }
                for face in BlockFace::ALL {
                    let neighbour = match face {
                        BlockFace::West => x.checked_sub(1).map(|nx| [nx, y, z]),
                        BlockFace::East => (x + 1 < 2).then_some([x + 1, y, z]),
                        BlockFace::Down => y.checked_sub(1).map(|ny| [x, ny, z]),
                        BlockFace::Up => (y + 1 < 2).then_some([x, y + 1, z]),
                        BlockFace::North => z.checked_sub(1).map(|nz| [x, y, nz]),
                        BlockFace::South => (z + 1 < 2).then_some([x, y, z + 1]),
                    };
                    if neighbour.is_none_or(|[nx, ny, nz]| !occupied[cell_index(nx, ny, nz)]) {
                        quads.push(stair_cell_quad(materials, face, x, y, z));
                    }
                }
            }
        }
    }
    debug_assert!(!quads.is_empty() && quads.len() <= 32);
    quads
}

pub(in crate::compiler) const fn canonical_stair_materials(
    materials: [u32; 6],
    rotation: u32,
) -> [u32; 6] {
    let mut canonical = materials;
    match rotation {
        0 => {}
        1 => {
            canonical[BlockFace::West as usize] = materials[BlockFace::North as usize];
            canonical[BlockFace::East as usize] = materials[BlockFace::South as usize];
            canonical[BlockFace::North as usize] = materials[BlockFace::East as usize];
            canonical[BlockFace::South as usize] = materials[BlockFace::West as usize];
        }
        2 => {
            canonical[BlockFace::West as usize] = materials[BlockFace::East as usize];
            canonical[BlockFace::East as usize] = materials[BlockFace::West as usize];
            canonical[BlockFace::North as usize] = materials[BlockFace::South as usize];
            canonical[BlockFace::South as usize] = materials[BlockFace::North as usize];
        }
        3 => {
            canonical[BlockFace::West as usize] = materials[BlockFace::South as usize];
            canonical[BlockFace::East as usize] = materials[BlockFace::North as usize];
            canonical[BlockFace::North as usize] = materials[BlockFace::West as usize];
            canonical[BlockFace::South as usize] = materials[BlockFace::East as usize];
        }
        _ => {}
    }
    canonical
}

pub(in crate::compiler) const fn cell_index(x: usize, y: usize, z: usize) -> usize {
    x | (y << 1) | (z << 2)
}

pub(in crate::compiler) const fn toward(orientation: u32, x: usize, z: usize) -> bool {
    match orientation {
        0 => z == 1, // south
        1 => x == 0, // west
        2 => z == 0, // north
        3 => x == 1, // east
        _ => false,
    }
}

pub(in crate::compiler) fn stair_cell_quad(
    materials: [u32; 6],
    face: BlockFace,
    x: usize,
    y: usize,
    z: usize,
) -> ModelQuad {
    let x0 = (x * 128) as i16;
    let x1 = x0 + 128;
    let y0 = (y * 128) as i16;
    let y1 = y0 + 128;
    let z0 = (z * 128) as i16;
    let z1 = z0 + 128;
    let (positions, face_id, boundary) = match face {
        BlockFace::West => (
            [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
            3,
            x == 0,
        ),
        BlockFace::East => (
            [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
            4,
            x == 1,
        ),
        BlockFace::Down => (
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
            1,
            y == 0,
        ),
        BlockFace::Up => (
            [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
            2,
            y == 1,
        ),
        BlockFace::North => (
            [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
            5,
            z == 0,
        ),
        BlockFace::South => (
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            6,
            z == 1,
        ),
    };
    let uvs = positions.map(|[px, py, pz]| match face {
        BlockFace::West | BlockFace::East => [(pz as u16) * 16, (4096 - i32::from(py) * 16) as u16],
        BlockFace::North | BlockFace::South => {
            [(px as u16) * 16, (4096 - i32::from(py) * 16) as u16]
        }
        BlockFace::Down | BlockFace::Up => [(px as u16) * 16, (pz as u16) * 16],
    });
    ModelQuad {
        positions,
        uvs,
        material: materials[face as usize],
        flags: face_id | (u32::from(boundary) * (face_id << 4)),
    }
}
