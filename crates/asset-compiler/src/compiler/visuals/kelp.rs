use super::super::*;
use super::context::{ModelStorage, RuleInputs, diagnostic_visual, push_model_template};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 6], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_kelp(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some([west, east, down, up, north, south]) = inputs.materials(record) {
        let ordered = [north, south, up, down, east, west];
        let template = if let Some(&template) = templates.get(&ordered) {
            template
        } else {
            let template = push_model_template(
                kelp_quads(ordered).to_vec(),
                MODEL_TEMPLATE_FLAG_KELP,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(ordered, template);
            template
        };
        visual.faces = [west, east, down, up, north, south];
        visual.kind = VisualKind::Model;
        visual.model_template = template;
    }
    Ok(CompileRuleResult::Compiled(visual))
}

pub(in crate::compiler) fn kelp_quads(materials: [u32; 6]) -> [ModelQuad; 6] {
    let uvs = [[0, 4096], [4096, 4096], [4096, 0], [0, 0]];
    let diagonal_a = [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]];
    let diagonal_b = [[256, 0, 0], [0, 0, 256], [0, 256, 256], [256, 256, 0]];
    let reverse_a = [diagonal_a[1], diagonal_a[0], diagonal_a[3], diagonal_a[2]];
    let reverse_b = [diagonal_b[1], diagonal_b[0], diagonal_b[3], diagonal_b[2]];
    [
        ModelQuad {
            positions: diagonal_a,
            uvs,
            material: materials[0],
            flags: 0,
        },
        ModelQuad {
            positions: diagonal_b,
            uvs,
            material: materials[1],
            flags: 0,
        },
        ModelQuad {
            positions: reverse_a,
            uvs,
            material: materials[2],
            flags: 0,
        },
        ModelQuad {
            positions: reverse_b,
            uvs,
            material: materials[3],
            flags: 0,
        },
        ModelQuad {
            positions: diagonal_a,
            uvs,
            material: materials[4],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
        ModelQuad {
            positions: diagonal_b,
            uvs,
            material: materials[5],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
    ]
}
