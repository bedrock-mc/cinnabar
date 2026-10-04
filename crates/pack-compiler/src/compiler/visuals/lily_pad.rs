use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

// Vanilla draws the plane at 1/64 block. Collision thickness is not art height.
const PLANE_HEIGHT: i16 = 4;

pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:waterlily"
        && record.canonical_state.as_ref() == "{}"
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<u32, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let Some((descriptor, _)) = descriptor_for(inputs.fallback, inputs.pack, record, BlockFace::Up)
    else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some((path, _)) = inputs
        .pack
        .terrain
        .fixed_tint_source(&descriptor.texture_key, descriptor.state_variant)
    else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    if path != descriptor.path.as_ref()
        || inputs.pack.flipbooks.iter().any(|flipbook| {
            flipbook.atlas_tile == descriptor.texture_key || flipbook.texture_path.as_ref() == path
        })
    {
        return Ok(CompileRuleResult::Compiled(visual));
    }
    if let Some(&material) = inputs.material_by_descriptor.get(&descriptor) {
        let template = if let Some(&template) = templates.get(&material) {
            template
        } else {
            let template = push_model_template(
                planes(material).to_vec(),
                assets::MODEL_TEMPLATE_FLAG_LILY_PAD,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(material, template);
            template
        };
        set_model_visual(&mut visual, [material; 6], template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

fn planes(material: u32) -> [ModelQuad; 2] {
    // Native rotation zero starts at SW and associates it with texture (0,0).
    // Its +Y winding matches our ordinary top faces. The explicit reverse
    // plane has distinct colour, but preserves every position/UV association.
    let positions = [
        [0, PLANE_HEIGHT, 256],
        [256, PLANE_HEIGHT, 256],
        [256, PLANE_HEIGHT, 0],
        [0, PLANE_HEIGHT, 0],
    ];
    let uvs = [[0, 0], [4096, 0], [4096, 4096], [0, 4096]];
    [
        ModelQuad {
            positions,
            uvs,
            material,
            flags: 2,
        },
        ModelQuad {
            positions: [positions[3], positions[2], positions[1], positions[0]],
            uvs: [uvs[3], uvs[2], uvs[1], uvs[0]],
            material,
            flags: 1,
        },
    ]
}

#[cfg(test)]
#[path = "lily_pad_tests.rs"]
mod tests;
