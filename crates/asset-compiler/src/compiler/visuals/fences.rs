use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 2], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_fence(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(material) = inputs.material(record, BlockFace::South) {
        let flag = if record.name.as_ref() == "minecraft:nether_brick_fence" {
            MODEL_TEMPLATE_FLAG_FENCE_NETHER
        } else {
            MODEL_TEMPLATE_FLAG_FENCE_WOOD
        };
        let key = [material, flag];
        let base = if let Some(&base) = templates.get(&key) {
            base
        } else {
            let base = u32::try_from(storage.templates.len()).map_err(|_| {
                AssetError::BlobSizeOverflow {
                    section: "model template",
                }
            })?;
            push_model_template(
                cuboid_quads([material; 6], [96, 0, 96], [160, 256, 160]).to_vec(),
                flag,
                storage.templates,
                storage.quads,
            )?;
            for mask in 0..16 {
                push_model_template(
                    fence_arm_quads(material, mask),
                    flag,
                    storage.templates,
                    storage.quads,
                )?;
            }
            templates.insert(key, base);
            base
        };
        set_model_visual(&mut visual, [material; 6], base);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

pub(in crate::compiler) fn fence_arm_quads(material: u32, mask: u32) -> Vec<ModelQuad> {
    debug_assert!(mask <= 15);
    let mut quads = Vec::with_capacity(mask.count_ones() as usize * 8);
    let directions = [
        (1, [112, 0, 0], [144, 0, 128]),
        (2, [128, 0, 112], [256, 0, 144]),
        (4, [112, 0, 128], [144, 0, 256]),
        (8, [0, 0, 112], [128, 0, 144]),
    ];
    for (bit, mut min, mut max) in directions {
        if mask & bit == 0 {
            continue;
        }
        let extension_axis = if bit == 1 || bit == 4 { 2 } else { 0 };
        for (min_y, max_y) in [(96, 144), (192, 240)] {
            min[1] = min_y;
            max[1] = max_y;
            for (face, quad) in cuboid_quads([material; 6], min, max)
                .into_iter()
                .enumerate()
            {
                let is_extension_cap = match extension_axis {
                    0 => matches!(face, 0 | 1),
                    _ => matches!(face, 4 | 5),
                };
                if !is_extension_cap {
                    quads.push(quad);
                }
            }
        }
    }
    quads
}
