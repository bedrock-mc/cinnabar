//! Dragon eggs use eight stacked cuboids with cropped block-face artwork.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

const STEPS: [([i16; 3], [i16; 3]); 8] = [
    ([96, 240, 96], [160, 256, 160]),
    ([80, 224, 80], [176, 240, 176]),
    ([80, 208, 80], [176, 224, 176]),
    ([48, 176, 48], [208, 208, 208]),
    ([32, 128, 32], [224, 176, 224]),
    ([16, 48, 16], [240, 128, 240]),
    ([32, 16, 32], [224, 48, 224]),
    ([48, 0, 48], [208, 16, 208]),
];

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 6], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if record.name.as_ref() != "minecraft:dragon_egg"
        || record.contributor_role != ContributorRole::Primary
        || record.flags.contains(BlockFlags::AIR)
    {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state);
    if !state.is_ok_and(|state| state.is_empty()) {
        return Ok(CompileRuleResult::Compiled(visual));
    }
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let template = if let Some(&template) = templates.get(&materials) {
        template
    } else {
        let mut sections = STEPS.chunks_exact(STEPS.len() / 2);
        let mut emit = |sections: &[([i16; 3], [i16; 3])], flags| {
            push_model_template(
                sections
                    .iter()
                    .flat_map(|&(min, max)| egg_cuboid(materials, min, max))
                    .collect(),
                flags,
                storage.templates,
                storage.quads,
            )
        };
        let head = emit(sections.next().unwrap(), MODEL_TEMPLATE_FLAG_COMPOUND_NEXT)?;
        let tail = emit(sections.next().unwrap(), 0)?;
        debug_assert_eq!(tail, head + 1);
        templates.insert(materials, head);
        head
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

fn egg_cuboid(materials: [u32; 6], min: [i16; 3], max: [i16; 3]) -> [ModelQuad; 6] {
    let mut quads = super::geometry::vanilla_cuboid_quads(materials, min, max);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        let axis = match face {
            BlockFace::West | BlockFace::East => 0,
            BlockFace::Down | BlockFace::Up => 1,
            _ => 2,
        };
        if quad
            .positions
            .iter()
            .all(|position| position[axis] == 0 || position[axis] == 256)
        {
            quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
        }
    }
    quads
}
