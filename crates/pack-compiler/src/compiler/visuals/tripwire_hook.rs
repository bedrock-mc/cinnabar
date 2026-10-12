//! Wall-mounted hook parts and state-selected shaft, eye and attached string.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Identifies the terrain model independently of its provisional envelope.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:tripwire_hook"
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Reads only the registered direction, attachment and powered state product.
fn pose(record: &RegistryRecord) -> Option<(u8, bool, bool)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 3 {
        return None;
    }
    Some((
        super::state::exact_tagged_int(state.get("direction")?, 3)? as u8,
        super::state::exact_tagged_byte(state.get("attached_bit")?, 1)? != 0,
        super::state::exact_tagged_byte(state.get("powered_bit")?, 1)? != 0,
    ))
}

/// Compiles independent materials and retains every selector in the template key.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], u8, bool, bool), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((direction, attached, powered)) = pose(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, direction, attached, powered);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            quads(materials, direction, attached, powered),
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

/// Turns the south-facing pose around the block centre and encodes model coordinates.
fn orient([x, y, z]: [f32; 3], direction: u8) -> [i16; 3] {
    let point = match direction {
        0 => [x, y, z],
        1 => [1.0 - z, y, x],
        2 => [1.0 - x, y, 1.0 - z],
        _ => [z, y, 1.0 - x],
    };
    point.map(|p| (p * 256.0).round() as i16)
}

/// Emits the mount, two pitched parts, and the string leading to the block boundary.
fn quads(materials: [u32; 6], direction: u8, attached: bool, powered: bool) -> Vec<ModelQuad> {
    let mut result = Vec::with_capacity(if attached { 20 } else { 18 });
    let base = materials[BlockFace::Down as usize];
    let (min, max) = match direction {
        0 => ([96, 13, 0], [160, 141, 32]),
        1 => ([224, 13, 96], [256, 141, 160]),
        2 => ([96, 13, 224], [160, 141, 256]),
        _ => ([0, 13, 96], [32, 141, 160]),
    };
    result.extend(super::geometry::vanilla_cuboid_quads([base; 6], min, max));
    let shaft_angle: f32 = if powered {
        -30.0
    } else if attached {
        -5.0
    } else {
        40.0
    };
    let (sin, cos) = shaft_angle.to_radians().sin_cos();
    let shaft_depth = if attached || powered { 0.0625 } else { 0.125 };
    let shaft = |[x, y, z]: [f32; 3]| {
        let z = z + 0.0625;
        orient(
            [
                x + 0.5,
                y * sin + z * cos + 0.3125,
                y * cos - z * sin + shaft_depth,
            ],
            direction,
        )
    };
    let eye = |[x, y, z]: [f32; 3]| {
        let z = z + 0.21875;
        let (height, depth) = if powered {
            (y - 0.09375, z - 0.1625)
        } else if attached {
            let (sin, cos) = 10.0_f32.to_radians().sin_cos();
            let y = y + 0.015625;
            let z = z - 0.171875;
            (y * cos + z * sin, z * cos - y * sin)
        } else {
            (y * sin + z * cos, z * sin - y * cos)
        };
        orient([x + 0.5, height + 0.3125, 0.5 - depth], direction)
    };
    let material = materials[BlockFace::Up as usize];
    pitched_part(
        &mut result,
        material,
        0.046875,
        0.3125,
        [7, 9, 9, 11],
        [7, 9, 9, 16],
        shaft,
    );
    pitched_part(
        &mut result,
        material,
        0.09375,
        0.03125,
        [5, 3, 11, 9],
        [5, 3, 11, 5],
        eye,
    );
    if attached {
        let tip_height = f32::from(eye([-0.09375, 0.0, -0.09375])[1]) / 256.0;
        for (near, far, near_height) in [(0.5, 0.75, tip_height), (0.75, 1.0, 0.21875)] {
            result.push(ModelQuad {
                positions: [
                    [0.484375, 0.21875, far],
                    [0.515625, 0.21875, far],
                    [0.515625, near_height, near],
                    [0.484375, near_height, near],
                ]
                .map(|p| orient(p, direction)),
                uvs: [[0, 512], [0, 1024], [4096, 1024], [4096, 512]],
                material: materials[BlockFace::South as usize],
                flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
            });
        }
    }
    result
}

/// Gives each closed pitched box its cap and side sprite regions with outward winding.
fn pitched_part(
    result: &mut Vec<ModelQuad>,
    material: u32,
    half_width: f32,
    height: f32,
    cap: [u16; 4],
    side: [u16; 4],
    transform: impl Fn([f32; 3]) -> [i16; 3],
) {
    let points = [
        [-half_width, 0.0, -half_width],
        [half_width, 0.0, -half_width],
        [half_width, 0.0, half_width],
        [-half_width, 0.0, half_width],
        [-half_width, height, -half_width],
        [half_width, height, -half_width],
        [half_width, height, half_width],
        [-half_width, height, half_width],
    ]
    .map(transform);
    for (index, corners) in [
        [1, 0, 3, 2],
        [6, 7, 4, 5],
        [0, 1, 5, 4],
        [3, 0, 4, 7],
        [2, 3, 7, 6],
        [1, 2, 6, 5],
    ]
    .into_iter()
    .enumerate()
    {
        let [u0, v0, u1, v1] = if index < 2 { cap } else { side };
        result.push(ModelQuad {
            positions: corners.map(|i| points[i]),
            uvs: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]].map(|uv| uv.map(|p| p * 256)),
            material,
            // The metal parts take light from the hook cell without cuboid AO.
            flags: 0,
        });
    }
}
