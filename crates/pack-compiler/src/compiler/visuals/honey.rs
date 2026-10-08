//! Honey's inset core and full-size shell, as drawn by the native block tessellator.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::vanilla_cuboid_quads;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 6], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if record.name.as_ref() != "minecraft:honey_block"
        || record.contributor_role != ContributorRole::Primary
        || record.flags.contains(BlockFlags::AIR)
    {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let template = if let Some(&template) = templates.get(&materials) {
        template
    } else {
        // BlockTessellator::tessellateHoneyBlockInWorld draws the inset core first,
        // then the full cube with face 0's texture and an explicit all-faces mask.
        let quads = vanilla_cuboid_quads(materials, [16; 3], [240; 3])
            .into_iter()
            .chain(vanilla_cuboid_quads(
                [materials[BlockFace::Down as usize]; 6],
                [0; 3],
                [256; 3],
            ))
            .collect();
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(materials, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}
