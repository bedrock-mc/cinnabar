use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, bool), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !matches!(
        record.name.as_ref(),
        "minecraft:lantern" | "minecraft:soul_lantern"
    ) || record.contributor_role != ContributorRole::Primary
        || record.flags.contains(BlockFlags::AIR)
    {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(hanging @ 0..=1) = canonical_state_u32(&record.canonical_state, "hanging") else {
        return Ok(CompileRuleResult::Reject);
    };
    // Every surface uses the same North-face sprite, including pack overrides.
    let material = inputs
        .material(record, BlockFace::North)
        .unwrap_or(inputs.vanilla_fallback_material);
    let key = (material, hanging == 1);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            lantern_quads(material, key.1),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, [material; 6], template);
    visual.support = VisualSupport::VanillaFallback;
    Ok(CompileRuleResult::Compiled(visual))
}

fn lantern_quads(material: u32, hanging: bool) -> Vec<ModelQuad> {
    let base = if hanging { 2 } else { 0 };
    let mut quads = Vec::with_capacity(16);
    quads.extend(sprite_cuboid(
        material,
        [5 * 16, base * 16, 5 * 16],
        [11 * 16, (base + 7) * 16, 11 * 16],
        [[0, 2], [6, 9]],
        [[0, 9], [6, 15]],
    ));
    quads.extend(sprite_cuboid(
        material,
        [6 * 16, (base + 7) * 16, 6 * 16],
        [10 * 16, (base + 9) * 16, 10 * 16],
        [[1, 0], [5, 2]],
        [[1, 10], [5, 14]],
    ));
    let (bottom, top, first_v, second_v) = if hanging {
        (11, 16, 6, 6)
    } else {
        (9, 12, 3, 9)
    };
    let planes = [
        (
            [[7, top, 7], [9, top, 9], [9, bottom, 9], [7, bottom, 7]],
            [[11, 0], [14, 0], [14, first_v], [11, first_v]],
        ),
        (
            [[9, top, 7], [7, top, 9], [7, bottom, 9], [9, bottom, 7]],
            [[14, second_v], [11, second_v], [11, 12], [14, 12]],
        ),
    ];
    for (positions, uvs) in planes {
        let mut quad = ModelQuad {
            positions: positions.map(|position| position.map(|value| value * 16)),
            uvs: uvs.map(|uv| uv.map(|value| value * 256)),
            material,
            flags: 0,
        };
        quads.push(quad);
        quad.positions.reverse();
        quad.uvs.reverse();
        quads.push(quad);
    }
    quads
}

fn sprite_cuboid(
    material: u32,
    min: [i16; 3],
    max: [i16; 3],
    side: [[u16; 2]; 2],
    horizontal: [[u16; 2]; 2],
) -> [ModelQuad; 6] {
    let mut quads = super::geometry::vanilla_cuboid_quads([material; 6], min, max);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        let region = match face {
            BlockFace::Down | BlockFace::Up => horizontal,
            _ => side,
        };
        for axis in 0..2 {
            let source_min = quad.uvs.iter().map(|uv| uv[axis]).min().unwrap();
            let source_max = quad.uvs.iter().map(|uv| uv[axis]).max().unwrap();
            let target_min = u32::from(region[0][axis]) * 256;
            let target_span = u32::from(region[1][axis] - region[0][axis]) * 256;
            for uv in &mut quad.uvs {
                uv[axis] = (target_min
                    + u32::from(uv[axis] - source_min) * target_span
                        / u32::from(source_max - source_min)) as u16;
            }
        }
    }
    quads
}
