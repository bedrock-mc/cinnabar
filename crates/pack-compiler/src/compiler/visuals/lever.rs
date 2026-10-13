//! Separate lever bases and pitched handles for wall, floor and ceiling mounts.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct TemplateKey {
    base: u32,
    handle: u32,
    direction: u8,
    powered: bool,
}

/// Identifies terrain-owned levers independently of their provisional envelopes.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:lever"
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Reads the complete typed mounting and power selectors, rejecting malformed states.
fn pose(record: &RegistryRecord) -> Option<(u8, bool)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    let direction = match super::state::exact_tagged_string(state.get("lever_direction")?)? {
        "down_east_west" => 0,
        "east" => 1,
        "west" => 2,
        "south" => 3,
        "north" => 4,
        "up_north_south" => 5,
        "up_east_west" => 6,
        "down_north_south" => 7,
        _ => return None,
    };
    Some((
        direction,
        super::state::exact_tagged_byte(state.get("open_bit")?, 1)? != 0,
    ))
}

/// Resolves the base from the pack's cobblestone top sprite, separately from lever art.
pub(in crate::compiler) fn base_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<(Descriptor, Box<str>)> {
    let mut base = record.clone();
    base.name = "minecraft:cobblestone".into();
    base.canonical_state = "{}".into();
    let key = resolve_texture_key(&pack.blocks, &base, BlockFace::Up).key?;
    let path = pack.terrain.get(&key)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            flags: pack.blocks.isotropic_face_flags(&base)[BlockFace::Up as usize],
            state_variant: 0,
            flipbook_selector: None,
        },
        key,
    ))
}

/// Compiles independently resolved base and handle materials for every admitted pose.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((direction, powered)) = pose(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(handle) = inputs.material(record, BlockFace::Down) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some((descriptor, _)) = base_descriptor(inputs.pack, record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some(&base) = inputs.material_by_descriptor.get(&descriptor) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = TemplateKey {
        base,
        handle,
        direction,
        powered,
    };
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            quads(base, handle, direction, powered),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, [handle; 6], template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Rotates a north/south floor mount around the block centre with outward winding.
fn mount([x, y, z]: [f32; 3], direction: u8) -> [f32; 3] {
    match direction {
        0 => [z, 1.0 - y, x],
        1 => [y, z, x],
        2 => [1.0 - y, z, 1.0 - x],
        3 => [1.0 - x, z, y],
        4 => [x, z, 1.0 - y],
        5 => [x, y, z],
        6 => [z, y, 1.0 - x],
        7 => [1.0 - x, 1.0 - y, z],
        _ => unreachable!("validated lever direction"),
    }
}

/// Quantizes block positions to the carrier's model coordinates.
fn position(point: [f32; 3]) -> [i16; 3] {
    point.map(|value| (value * 256.0).round() as i16)
}

/// Builds a cropped base and a closed handle tilted toward its selected power pose.
fn quads(base: u32, handle: u32, direction: u8, powered: bool) -> Vec<ModelQuad> {
    let ends = [[0.3125, 0.0, 0.25], [0.6875, 0.1875, 0.75]]
        .map(|point| position(mount(point, direction)));
    let min = std::array::from_fn(|axis| ends[0][axis].min(ends[1][axis]));
    let max = std::array::from_fn(|axis| ends[0][axis].max(ends[1][axis]));
    let mut result = super::geometry::vanilla_cuboid_quads([base; 6], min, max).to_vec();
    let (sin, cos) = 40.0_f32.to_radians().sin_cos();
    let points = [
        [-0.0625, 0.0, -0.0625],
        [0.0625, 0.0, -0.0625],
        [0.0625, 0.0, 0.0625],
        [-0.0625, 0.0, 0.0625],
        [-0.0625, 0.625, -0.0625],
        [0.0625, 0.625, -0.0625],
        [0.0625, 0.625, 0.0625],
        [-0.0625, 0.625, 0.0625],
    ]
    .map(|[x, y, z]| {
        let sign = if powered { -1.0 } else { 1.0 };
        let z = z + sign * 0.0625;
        position(mount(
            [
                x + 0.5,
                y * cos - z * sin * sign + 0.125,
                z * cos + y * sin * sign + 0.5,
            ],
            direction,
        ))
    });
    for (index, corners) in [
        [0, 1, 2, 3],
        [7, 6, 5, 4],
        [1, 0, 4, 5],
        [2, 1, 5, 6],
        [3, 2, 6, 7],
        [0, 3, 7, 4],
    ]
    .into_iter()
    .enumerate()
    {
        let bottom = if index < 2 { 8 } else { 16 };
        result.push(ModelQuad {
            positions: corners.map(|i| points[i]),
            uvs: [[7, bottom], [9, bottom], [9, 6], [7, 6]].map(|uv| uv.map(|value| value * 256)),
            material: handle,
            // The handle samples white flat lighting from the lever cell.
            flags: 0,
        });
    }
    result
}
