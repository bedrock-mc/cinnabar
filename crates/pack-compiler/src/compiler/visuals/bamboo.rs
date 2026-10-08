use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::cuboid_quads;

pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:bamboo"
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, u32, i16, u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record)
        || record.contributor_role != ContributorRole::Primary
        || record.flags.contains(BlockFlags::AIR)
    {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Ok(state) = serde_json::from_str::<serde_json::Value>(&record.canonical_state) else {
        return Ok(CompileRuleResult::Reject);
    };
    let Some(state) = state.as_object().filter(|state| state.len() == 3) else {
        return Ok(CompileRuleResult::Reject);
    };
    if state
        .get("age_bit")
        .and_then(|value| super::state::exact_tagged_byte(value, 1))
        .is_none()
    {
        return Ok(CompileRuleResult::Reject);
    }
    let width = match state
        .get("bamboo_stalk_thickness")
        .and_then(super::state::exact_tagged_string)
    {
        Some("thin") => 32,
        Some("thick") => 48,
        _ => return Ok(CompileRuleResult::Reject),
    };
    let (leaf_size, selector) = match state
        .get("bamboo_leaf_size")
        .and_then(super::state::exact_tagged_string)
    {
        Some("no_leaves") => (0, BlockFace::South),
        Some("small_leaves") => (1, BlockFace::South),
        Some("large_leaves") => (2, BlockFace::Up),
        _ => return Ok(CompileRuleResult::Reject),
    };
    let stem = inputs
        .material(record, BlockFace::North)
        .unwrap_or(inputs.vanilla_fallback_material);
    let leaf = inputs
        .material(record, selector)
        .unwrap_or(inputs.vanilla_fallback_material);
    let key = (stem, leaf, width, leaf_size);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            bamboo_quads(stem, leaf, width, leaf_size),
            assets::MODEL_TEMPLATE_FLAG_BAMBOO,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, [stem; 6], template);
    visual.support = VisualSupport::VanillaFallback;
    Ok(CompileRuleResult::Compiled(visual))
}

fn bamboo_quads(stem: u32, leaf: u32, width: i16, leaf_size: u8) -> Vec<ModelQuad> {
    let mut quads =
        cuboid_quads([stem; 6], [128, 0, 128], [128 + width, 256, 128 + width]).to_vec();
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        quad.uvs = quad.positions.map(|[x, y, z]| {
            let u = match face {
                BlockFace::West => z - 128,
                BlockFace::East => 128 + width - z,
                BlockFace::North => 128 + width - x,
                _ => x - 128,
            } as u16
                * 16;
            match face {
                BlockFace::Down => [3328 + u, 1024 + (128 + width - z) as u16 * 16],
                BlockFace::Up => [3328 + u, (z - 128) as u16 * 16],
                _ => [u, (256 - y) as u16 * 16],
            }
        });
    }
    if leaf_size == 0 {
        return quads;
    }
    let center = 128 + width / 2;
    let (length, outer_uv) = if leaf_size == 1 {
        (80, [512, 3584])
    } else {
        (112, [0, 4096])
    };
    for axis in 0..2 {
        for positive in [true, false] {
            let start = if positive { 128 + width } else { 128 };
            let end = start + if positive { length } else { -length };
            let (inner_u, outer_u) = if positive {
                (1792, outer_uv[0])
            } else {
                (2304, outer_uv[1])
            };
            let positions = if axis == 0 {
                [
                    [start, 256, center],
                    [end, 256, center],
                    [end, 0, center],
                    [start, 0, center],
                ]
            } else {
                [
                    [center, 256, start],
                    [center, 256, end],
                    [center, 0, end],
                    [center, 0, start],
                ]
            };
            quads.push(ModelQuad {
                positions,
                uvs: [[inner_u, 0], [outer_u, 0], [outer_u, 4096], [inner_u, 4096]],
                material: leaf,
                flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
            });
        }
    }
    quads
}

#[cfg(test)]
#[path = "bamboo_tests.rs"]
mod tests;
